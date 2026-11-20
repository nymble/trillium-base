// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Machine state: temperature, CPU, memory, DAC sink state, services, listening ports,
//! USB devices. Read-only; nothing here changes the system.

use std::fs;
use std::process::Command;

use serde::Serialize;

use crate::brand::DEFAULT_PORT;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if out.status.success() || !out.stdout.is_empty() {
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        None
    }
}

pub fn temperature_c() -> Option<f32> {
    fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok()?.trim().parse::<f32>().ok().map(|m| m / 1000.0)
}

#[derive(Default, Clone)]
pub struct CpuSampler {
    prev: Vec<(u64, u64)>,
}

impl CpuSampler {
    /// Per-core busy percentage since the previous call.
    pub fn sample(&mut self) -> Vec<f32> {
        let s = fs::read_to_string("/proc/stat").unwrap_or_default();
        let mut cur = vec![];
        for line in s.lines().filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(|c| c.is_ascii_digit())) {
            let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
            let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
            let total: u64 = v.iter().take(8).sum();
            cur.push((total, idle));
        }
        let out = cur
            .iter()
            .enumerate()
            .map(|(i, &(t, id))| match self.prev.get(i) {
                Some(&(pt, pid)) if t > pt => 100.0 * (1.0 - (id - pid) as f32 / (t - pt) as f32),
                _ => 0.0,
            })
            .collect();
        self.prev = cur;
        out
    }
}

pub fn mem_used_pct() -> f32 {
    let s = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let get = |k: &str| s.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let (t, a) = (get("MemTotal:"), get("MemAvailable:"));
    if t > 0.0 {
        (100.0 * (1.0 - a / t)) as f32
    } else {
        0.0
    }
}

pub fn loadavg() -> String {
    fs::read_to_string("/proc/loadavg").unwrap_or_default().split_whitespace().take(3).collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Sink {
    pub id: String,
    pub name: String,
    pub format: String,
    pub state: String,
}

pub fn sinks() -> Vec<Sink> {
    run("pactl", &["list", "short", "sinks"])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            if c.len() >= 5 {
                Some(Sink { id: c[0].into(), name: c[1].into(), format: c[3].into(), state: c[4].into() })
            } else {
                None
            }
        })
        .collect()
}

/// Suspends and resumes a sink (the documented recovery for the DAC dropout state).
pub fn bounce_sink(name: &str) -> Result<(), String> {
    let a = Command::new("pactl").args(["suspend-sink", name, "1"]).status().map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let b = Command::new("pactl").args(["suspend-sink", name, "0"]).status().map_err(|e| e.to_string())?;
    if a.success() && b.success() {
        Ok(())
    } else {
        Err("pactl failed".into())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Listen {
    pub proto: String,
    pub addr: String,
    pub port: u16,
    pub scope: &'static str,
    pub service: String,
}

pub fn listening() -> Vec<Listen> {
    let mut v = vec![];
    for l in run("ss", &["-Htlnu"]).unwrap_or_default().lines() {
        let c: Vec<&str> = l.split_whitespace().collect();
        if c.len() < 5 {
            continue;
        }
        let local = c[4];
        let Some((addr, port)) = local.rsplit_once(':') else { continue };
        let Ok(port) = port.parse::<u16>() else { continue };
        let addr = addr.trim_matches(|ch| ch == '[' || ch == ']').to_string();
        let scope = if addr.starts_with("127.") || addr == "::1" {
            "localhost"
        } else if addr.starts_with("fe80") || addr.contains("%") {
            "link-local"
        } else {
            "lan"
        };
        let proto = c[0].to_string();
        if proto == "udp" && port > 32767 && scope != "localhost" {
            // ephemeral client sockets (mDNS queriers, RTP sessions); keep the list readable
            continue;
        }
        let service = port_owner(&proto, port).to_string();
        if !v.iter().any(|x: &Listen| x.proto == proto && x.port == port && x.scope == scope) {
            v.push(Listen { proto, addr, port, scope, service });
        }
    }
    v.sort_by_key(|x| (x.port, x.proto.clone()));
    v
}

fn port_owner(proto: &str, port: u16) -> &'static str {
    match (proto, port) {
        ("tcp", 22) => "ssh",
        ("udp", 5353) => "mdns",
        ("tcp", 7000) => "airplay-rx",
        ("udp", 319) | ("udp", 320) => "airplay-rx",
        ("udp", 9000) => "airplay-rx",
        ("tcp", 5900) => "vnc",
        ("tcp", 5510) | ("udp", 5510..=5512) => "faust-gui",
        ("udp", 57110) | ("tcp", 57110) => "supercollider",
        ("udp", 546) => "dhcpv6",
        (_, p) if p == DEFAULT_PORT => "pilab-console",
        _ => "undeclared",
    }
}

/// Known services with a risk note shown on the console's toggles.
#[derive(Clone, Debug, Serialize)]
pub struct Service {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub ports: &'static str,
    pub risk: &'static str,
    /// "info" | "caution" | "gated"
    pub severity: &'static str,
    /// "console" = switchable here; "read-only" = shown, switched elsewhere
    pub control: &'static str,
    pub how: &'static str,
    pub active: bool,
    pub detail: String,
}

struct Def {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    ports: &'static str,
    risk: &'static str,
    severity: &'static str,
    control: &'static str,
    how: &'static str,
    probe: Probe,
}

enum Probe {
    Engine,
    Console,
    UserUnit(&'static str),
    SystemUnit(&'static str),
    Process(&'static str),
}

const SERVICES: &[Def] = &[
    Def { id: "pilab-synth", name: "Waveform synth", kind: "audio", ports: "none (PipeWire node)", risk: "Plays through the DAC, including its headphone output. Starts at -18 dBFS with a fade-in and is capped at -6 dBFS.", severity: "info", control: "console", how: "", probe: Probe::Engine },
    Def { id: "pilab-console", name: "Console", kind: "control", ports: "TCP 8410 (LAN)", risk: "Anyone on this network who has the pairing token can change the sound. Plain HTTP on the LAN, so the token is not encrypted in transit.", severity: "caution", control: "read-only", how: "systemctl --user {start|stop} pilabd", probe: Probe::Console },
    Def { id: "airplay-rx", name: "AirPlay receiver", kind: "audio", ports: "TCP 7000, UDP 319-320", risk: "Anyone on this network can play audio to the DAC unless a password is set.", severity: "caution", control: "read-only", how: "pilab-airplay receiver on|off (on the Pi)", probe: Probe::UserUnit("shairport-sync.service") },
    Def { id: "airplay-tx", name: "AirPlay sender", kind: "audio", ports: "outbound only", risk: "Sends audio to AirPlay speakers on this network.", severity: "info", control: "read-only", how: "pilab-airplay sender on|off (on the Pi)", probe: Probe::UserUnit("pilab-airplay-sender.service") },
    Def { id: "faust-gui", name: "Faust web controls", kind: "audio", ports: "TCP 5510, UDP 5510-5512", risk: "Anyone on this network can change the sound (no login). Often the point at an installation.", severity: "info", control: "read-only", how: "faust-gui <patch.dsp> (on the Pi)", probe: Probe::Process("synth-gui") },
    Def { id: "supercollider", name: "SuperCollider server", kind: "audio", ports: "UDP 57110 while running", risk: "scsynth accepts OSC commands from the network while it runs.", severity: "info", control: "read-only", how: "sc-play <patch.scd> (on the Pi)", probe: Probe::Process("scsynth") },
    Def { id: "vnc", name: "Desktop (VNC)", kind: "access", ports: "TCP 5900 (localhost only)", risk: "Reachable only through the SSH tunnel.", severity: "info", control: "read-only", how: "mac/pilab-desktop.command", probe: Probe::UserUnit("wayvnc-lab.service") },
    Def { id: "ssh", name: "SSH", kind: "access", ports: "TCP 22", risk: "Key-only login for user lab.", severity: "info", control: "read-only", how: "system service", probe: Probe::SystemUnit("ssh.service") },
    Def { id: "mdns", name: "mDNS discovery (avahi)", kind: "network", ports: "UDP 5353", risk: "Announces this device's name and services to everyone on the network.", severity: "info", control: "read-only", how: "system service", probe: Probe::SystemUnit("avahi-daemon.service") },
];

fn unit_active(user: bool, unit: &str) -> bool {
    let mut a = vec![];
    if user {
        a.push("--user");
    }
    a.extend(["is-active", "--quiet", unit]);
    Command::new("systemctl").args(&a).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

fn process_running(name: &str) -> bool {
    Command::new("pgrep").args(["-x", name]).output().map(|o| o.status.success()).unwrap_or(false)
}

pub fn services(engine_running: bool) -> Vec<Service> {
    SERVICES
        .iter()
        .map(|d| {
            let active = match d.probe {
                Probe::Engine => engine_running,
                Probe::Console => true,
                Probe::UserUnit(u) => unit_active(true, u),
                Probe::SystemUnit(u) => unit_active(false, u),
                Probe::Process(p) => process_running(p),
            };
            Service { id: d.id, name: d.name, kind: d.kind, ports: d.ports, risk: d.risk, severity: d.severity, control: d.control, how: d.how, active, detail: String::new() }
        })
        .collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct UsbDev {
    pub id: String,
    pub vendor: String,
    pub product: String,
    pub midi: bool,
    pub audio: bool,
}

pub fn usb_devices() -> Vec<UsbDev> {
    let mut v = vec![];
    let Ok(rd) = fs::read_dir("/sys/bus/usb/devices") else { return v };
    for e in rd.flatten() {
        let p = e.path();
        let rd = |f: &str| fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
        let vid = rd("idVendor");
        if vid.is_empty() || rd("bDeviceClass") == "09" {
            continue; // not a device, or a hub
        }
        let (mut midi, mut audio) = (false, false);
        if let Ok(ifs) = fs::read_dir(&p) {
            for i in ifs.flatten() {
                let ip = i.path();
                let c = fs::read_to_string(ip.join("bInterfaceClass")).unwrap_or_default();
                let s = fs::read_to_string(ip.join("bInterfaceSubClass")).unwrap_or_default();
                if c.trim() == "01" {
                    audio = true;
                    if s.trim() == "03" {
                        midi = true;
                    }
                }
            }
        }
        v.push(UsbDev { id: format!("{vid}:{}", rd("idProduct")), vendor: rd("manufacturer"), product: rd("product"), midi, audio });
    }
    v.sort_by(|a, b| a.product.cmp(&b.product));
    v
}

pub fn free_mb(path: &std::path::Path) -> Option<u64> {
    let mut p = path.to_path_buf();
    while !p.exists() {
        if !p.pop() {
            return None;
        }
    }
    let c = std::ffi::CString::new(p.to_string_lossy().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { core::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64) / (1024 * 1024))
}

pub fn lan_addresses() -> Vec<String> {
    run("hostname", &["-I"]).unwrap_or_default().split_whitespace().filter(|a| !a.starts_with("fe80")).map(|s| s.to_string()).collect()
}
