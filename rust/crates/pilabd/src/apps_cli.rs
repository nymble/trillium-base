// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! `trillium-base apps ...`: list, inspect, enable and disable the apps that have a manifest
//! (see `docs/app-manager.md`). Included into pilabctl; it never installs software.

use std::path::PathBuf;
use std::process::Command;

use pilab_core::apps::{load_dir, Manifest};

fn dirs() -> Vec<PathBuf> {
    if let Ok(d) = std::env::var("PILAB_APPS_DIR") {
        return d.split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    vec![PathBuf::from("/etc/trillium/apps.d"), PathBuf::from("/usr/share/trillium/apps.d"), PathBuf::from(home).join(".config/pilab/apps.d")]
}

fn all() -> Vec<(PathBuf, Manifest)> {
    let mut v: Vec<(PathBuf, Manifest)> = vec![];
    for d in dirs() {
        for r in load_dir(&d) {
            match r {
                Ok((p, m)) => {
                    // a later directory (the user's) overrides an earlier one with the same id
                    v.retain(|(_, o)| o.app.id != m.app.id);
                    v.push((p, m));
                }
                Err((p, e)) => eprintln!("[warn] {}: {e}", p.display()),
            }
        }
    }
    v.sort_by(|a, b| a.1.app.id.cmp(&b.1.app.id));
    v
}

fn systemctl(scope: &str, args: &[&str]) -> Command {
    let mut c;
    if scope == "system" {
        c = Command::new("sudo");
        c.args(["-n", "systemctl"]);
    } else {
        c = Command::new("systemctl");
        c.arg("--user");
    }
    c.args(args);
    c
}

fn query(scope: &str, verb: &str, unit: &str) -> String {
    let mut c = Command::new("systemctl");
    if scope != "system" {
        c.arg("--user");
    }
    c.args([verb, unit]);
    c.output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn state(m: &Manifest) -> (String, String) {
    match &m.service {
        Some(s) => (query(&s.scope, "is-enabled", &s.unit), query(&s.scope, "is-active", &s.unit)),
        None => ("-".into(), "-".into()),
    }
}

fn ports(m: &Manifest) -> String {
    let p: Vec<String> = m.network.iter().filter(|n| n.exposure != "outbound").map(|n| format!("{}/{}{}", n.proto, n.port, if n.exposure == "localhost" { " (local)" } else { "" })).collect();
    if p.is_empty() { "-".into() } else { p.join(" ") }
}

fn find(id: &str) -> Result<(PathBuf, Manifest), String> {
    all().into_iter().find(|(_, m)| m.app.id == id).ok_or_else(|| format!("no app {id:?} (try `apps list`)"))
}

pub fn run(args: &[&str]) -> Result<(), String> {
    match args {
        [] | ["list"] => {
            println!("{:<18} {:<22} {:<12} {:<9} {:<8} {:<5} {}", "ID", "NAME", "VERSION", "ENABLED", "ACTIVE", "RT", "LAN PORTS");
            for (_, m) in all() {
                let (en, ac) = state(&m);
                println!("{:<18} {:<22} {:<12} {:<9} {:<8} {:<5} {}", m.app.id, m.app.name, m.app.version, en, ac, m.resources.rt_class, ports(&m));
            }
            Ok(())
        }
        ["show", id] => {
            let (path, m) = find(id)?;
            let (en, ac) = state(&m);
            println!("{} {} ({})\n{}\nlicence {}   kind {}   source {}\nmanifest {}", m.app.name, m.app.version, m.app.id, m.app.summary, m.app.license, m.app.kind, m.app.source, path.display());
            match &m.service {
                Some(s) => println!("service  {} ({} scope), enabled: {en}, active: {ac}{}", s.unit, s.scope, if s.requires.is_empty() { String::new() } else { format!(", needs {}", s.requires.iter().map(|r| r.unit.as_str()).collect::<Vec<_>>().join(", ")) }),
                None => println!("service  none (started by hand; enable and disable do not apply)"),
            }
            println!("ports    {}\nRT class {}   CPUs {}   hardening {}   radio {}", ports(&m), m.resources.rt_class, m.resources.cpus, m.security.hardening, m.security.radio);
            if !m.security.risk.is_empty() {
                println!("risk     {}", m.security.risk);
            }
            println!("\nsystemd hardening drop-in (review before installing):\n{}", m.hardening_dropin());
            Ok(())
        }
        ["validate", file] => {
            let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
            let m: Manifest = toml::from_str(&text).map_err(|e| format!("{file}: {e}"))?;
            let p = m.problems();
            if p.is_empty() {
                println!("{file}: ok ({} {})", m.app.id, m.app.version);
                Ok(())
            } else {
                Err(p.iter().map(|x| format!("{file}: {x}")).collect::<Vec<_>>().join("\n"))
            }
        }
        [verb @ ("enable" | "disable"), id, flags @ ..] => {
            let (dry, radio_ok) = (flags.contains(&"--dry-run"), flags.contains(&"--confirm-radio"));
            let (_, m) = find(id)?;
            let Some(s) = &m.service else {
                return Err(format!("{} has no service unit: it is started by hand, so there is nothing to {verb}", m.app.id));
            };
            let on = *verb == "enable";
            if on && m.security.radio && !radio_ok {
                return Err(format!("{} transmits radio. It is never switched on by default or by a script: re-run with --confirm-radio once the owner has approved it", m.app.id));
            }
            if on && m.resources.rt_class == "rt" {
                for (_, o) in all() {
                    if o.app.id != m.app.id && o.resources.rt_class == "rt" && state(&o).0 == "enabled" {
                        eprintln!("[warn] {} is also an RT app: two RT apps compete for the isolated CPU", o.app.id);
                    }
                }
            }
            // requirements start first on the way up and stop last on the way down
            let mut steps: Vec<(String, String)> = s.requires.iter().map(|r| (r.scope.clone(), r.unit.clone())).collect();
            steps.push((s.scope.clone(), s.unit.clone()));
            if !on {
                steps.reverse();
                // only the app's own unit goes down; a shared requirement may serve another app
                steps.truncate(1);
            }
            for (scope, unit) in steps {
                let args: [&str; 3] = if on { ["enable", "--now", &unit] } else { ["disable", "--now", &unit] };
                let shown = format!("{}systemctl {}{}", if scope == "system" { "sudo -n " } else { "" }, if scope == "system" { "" } else { "--user " }, args.join(" "));
                if dry {
                    println!("would run: {shown}");
                    continue;
                }
                println!("{shown}");
                let out = systemctl(&scope, &args).output().map_err(|e| format!("{shown}: {e}"))?;
                if !out.status.success() {
                    return Err(format!("{shown}: {}", String::from_utf8_lossy(&out.stderr).trim()));
                }
            }
            Ok(())
        }
        _ => Err("usage: apps [list | show ID | validate FILE | enable ID [--dry-run] [--confirm-radio] | disable ID [--dry-run]]".into()),
    }
}
