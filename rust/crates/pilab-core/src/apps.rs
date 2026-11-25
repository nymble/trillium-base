// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! App manifests: what an installable Trillium Base app declares about itself (name, version,
//! licence, service unit, ports, audio and MIDI ports, resource class, hardening), and the
//! checks a manifest must pass before the console or `trillium-base apps` will manage it.
//!
//! Prototype of the app manager described in `docs/app-manager.md`. Pure data: reading a
//! manifest never starts, stops or installs anything.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub app: AppInfo,
    #[serde(default)]
    pub service: Option<Service>,
    #[serde(default)]
    pub resources: Resources,
    #[serde(default)]
    pub network: Vec<Port>,
    #[serde(default)]
    pub audio: AudioPorts,
    #[serde(default)]
    pub midi: MidiPorts,
    #[serde(default)]
    pub config: Option<ConfigRef>,
    #[serde(default)]
    pub security: Security,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppInfo {
    /// Stable id: lower-case letters, digits and dashes.
    pub id: String,
    pub name: String,
    pub version: String,
    /// SPDX expression of the app itself.
    pub license: String,
    pub summary: String,
    /// audio, midi, control, network, analysis, sdr, generative, host.
    pub kind: String,
    /// Where it came from: `base` (shipped in the image), `apt`, `oci`, `local-build`.
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub homepage: String,
}

fn default_source() -> String {
    "base".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Service {
    /// systemd unit to enable and start.
    pub unit: String,
    /// `user` (the lab user's manager) or `system` (needs sudo).
    #[serde(default = "default_scope")]
    pub scope: String,
    /// Units that must also be enabled and started with it (started first).
    #[serde(default)]
    pub requires: Vec<UnitRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitRef {
    pub unit: String,
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_scope() -> String {
    "user".into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Resources {
    /// `none` (no audio-thread work), `soft` (normal priority, CPUs 0-2) or `rt` (needs an
    /// RT priority; the manager will not enable it next to another `rt` app without a warning).
    pub rt_class: String,
    pub cpus: String,
    pub memory_max: String,
}

impl Default for Resources {
    fn default() -> Self {
        Resources { rt_class: "none".into(), cpus: "0-2".into(), memory_max: String::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Port {
    /// tcp or udp.
    pub proto: String,
    pub port: u16,
    /// `localhost`, `lan` or `outbound`.
    pub exposure: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioPorts {
    /// PipeWire node names the app creates.
    #[serde(default)]
    pub nodes: Vec<String>,
    /// Links the patch panel makes when the app starts (`from` and `to` are port globs).
    #[serde(default)]
    pub autoconnect: Vec<Link>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MidiPorts {
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfigRef {
    /// JSON Schema file (relative to the manifest) that the console renders as a settings form.
    pub schema: String,
    /// Where the app reads its settings.
    pub file: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Security {
    /// What a user should know before turning it on. Required when a port is `lan`.
    pub risk: String,
    /// Transmits radio (Bluetooth LE advertising, Wi-Fi AP). The CLI refuses to enable it
    /// without `--confirm-radio`, so it is never switched on by accident or by a script.
    pub radio: bool,
    pub needs_root: bool,
    /// `none`, `basic` (NoNewPrivileges, ProtectSystem=full) or `strict` (the full sandbox set).
    pub hardening: String,
}

impl Default for Security {
    fn default() -> Self {
        Security { risk: String::new(), radio: false, needs_root: false, hardening: "none".into() }
    }
}

fn id_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= 48 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !s.starts_with('-')
}

fn unit_ok(u: &str) -> bool {
    let Some(stem) = u.strip_suffix(".service") else { return false };
    // the first character must not be '-', or the unit name would read as an option to systemctl
    stem.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric()) && stem.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b))
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let m: Manifest = toml::from_str(text).map_err(|e| e.to_string())?;
        m.validate()?;
        Ok(m)
    }

    /// All problems at once, so an app author fixes them in one pass.
    pub fn problems(&self) -> Vec<String> {
        let mut p = vec![];
        let mut bad = |c: bool, m: String| {
            if c {
                p.push(m)
            }
        };
        bad(self.schema != SCHEMA, format!("schema must be {SCHEMA}, not {}", self.schema));
        bad(!id_ok(&self.app.id), "app.id: lower-case letters, digits and dashes, at most 48".into());
        bad(self.app.name.trim().is_empty(), "app.name is empty".into());
        bad(self.app.version.trim().is_empty(), "app.version is empty".into());
        bad(self.app.license.trim().is_empty(), "app.license must be an SPDX expression".into());
        bad(self.app.summary.trim().is_empty(), "app.summary is empty".into());
        bad(
            !["audio", "midi", "control", "network", "analysis", "sdr", "generative", "host"].contains(&self.app.kind.as_str()),
            format!("app.kind {:?} is not one of audio, midi, control, network, analysis, sdr, generative, host", self.app.kind),
        );
        bad(
            !["base", "apt", "oci", "local-build"].contains(&self.app.source.as_str()),
            format!("app.source {:?} is not one of base, apt, oci, local-build", self.app.source),
        );
        if let Some(s) = &self.service {
            bad(!unit_ok(&s.unit), format!("service.unit {:?} must be a plain *.service name", s.unit));
            bad(!["user", "system"].contains(&s.scope.as_str()), format!("service.scope {:?} must be user or system", s.scope));
            for r in &s.requires {
                bad(!unit_ok(&r.unit), format!("service.requires unit {:?} must be a plain *.service name", r.unit));
                bad(!["user", "system"].contains(&r.scope.as_str()), format!("service.requires scope {:?} must be user or system", r.scope));
            }
        }
        bad(!["none", "soft", "rt"].contains(&self.resources.rt_class.as_str()), format!("resources.rt_class {:?} must be none, soft or rt", self.resources.rt_class));
        bad(!["none", "basic", "strict"].contains(&self.security.hardening.as_str()), format!("security.hardening {:?} must be none, basic or strict", self.security.hardening));
        for n in &self.network {
            bad(!["tcp", "udp"].contains(&n.proto.as_str()), format!("network port {}: proto must be tcp or udp", n.port));
            bad(n.port == 0, "network port 0".into());
            bad(!["localhost", "lan", "outbound"].contains(&n.exposure.as_str()), format!("network port {}: exposure must be localhost, lan or outbound", n.port));
        }
        bad(
            self.network.iter().any(|n| n.exposure == "lan") && self.security.risk.trim().is_empty(),
            "security.risk is required when a port is exposed to the LAN".into(),
        );
        bad(self.security.needs_root && self.service.as_ref().is_some_and(|s| s.scope == "user"), "security.needs_root with a user service: use scope = \"system\"".into());
        if let Some(c) = &self.config {
            bad(c.schema.starts_with('/') || c.schema.contains(".."), "config.schema must be a relative path inside the app".into());
        }
        p
    }

    pub fn validate(&self) -> Result<(), String> {
        let p = self.problems();
        if p.is_empty() { Ok(()) } else { Err(p.join("; ")) }
    }
}

impl Manifest {
    /// systemd drop-in that applies the manifest's `security.hardening` level to its unit.
    /// Not installed automatically: `trillium-base apps show ID` prints it for review.
    pub fn hardening_dropin(&self) -> String {
        let mut o = format!("# Generated from the {} manifest (security.hardening = {}).\n[Service]\n", self.app.id, self.security.hardening);
        if self.security.hardening == "none" {
            o.push_str("# none: no sandbox options.\n");
            return o;
        }
        o.push_str("NoNewPrivileges=yes\nPrivateTmp=yes\nProtectSystem=full\nProtectControlGroups=yes\nLockPersonality=yes\n");
        if self.security.hardening == "strict" {
            o.push_str("ProtectKernelTunables=yes\nProtectKernelModules=yes\nProtectKernelLogs=yes\nRestrictNamespaces=yes\n");
            o.push_str("RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK\nSystemCallArchitectures=native\n");
            o.push_str("SystemCallFilter=@system-service\nCapabilityBoundingSet=\n");
            if self.resources.rt_class == "none" {
                // RestrictRealtime would block SCHED_FIFO, so it is only safe for non-RT apps.
                o.push_str("RestrictRealtime=yes\n");
            }
            o.push_str("# Not set: MemoryDenyWriteExecute (breaks JIT engines such as Csound and Faust).\n");
        }
        if self.resources.memory_max != "" {
            o.push_str(&format!("MemoryMax={}\n", self.resources.memory_max));
        }
        o
    }
}

/// Manifests found in `dir` (`*.toml`), with the file each came from. A broken manifest is
/// reported, not skipped silently.
pub fn load_dir(dir: &Path) -> Vec<Result<(PathBuf, Manifest), (PathBuf, String)>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect())
        .unwrap_or_default();
    files.sort();
    files
        .into_iter()
        .map(|p| match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| Manifest::parse(&t)) {
            Ok(m) => Ok((p, m)),
            Err(e) => Err((p, e)),
        })
        .collect()
}
