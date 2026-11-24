// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Devices module: USB and Bluetooth devices the Base can see, and Trillium Zero
//! controllers it can talk to over their configuration port (USB MIDI cable 1).
//!
//! What this does on its own: reads identity (HELLO), status and settings (GET_*)
//! whenever a Zero appears, after a replug or reset (boot id change), and every 30 s.
//! The first read of a device is stored as its profile (a settings snapshot on the
//! Base; the firmware keeps settings in RAM only). It never writes settings by itself:
//! sending a stored profile back to a device happens only through `reapply`, which the
//! console calls from an explicit button with `confirm`.
//!
//! Transport: the ALSA sequencer through `aseqdump` and `aseqsend` (alsa-utils), so the
//! configuration port is used alongside PipeWire's MIDI bridge without opening the raw
//! MIDI device. The diagnostics serial port is never opened.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pilab_core::zero::{self, Hello, Message, Profile, Reply, Status, Telemetry};
use serde::Serialize;
use serde_json::{json, Value};

use crate::identity;
use crate::timefmt;

/// A Trillium Zero found in the ALSA sequencer.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Found {
    /// Stable for the same USB socket: "zero-<usb port path>".
    pub key: String,
    pub label: String,
    pub client: u32,
    pub card: u32,
    pub usb_id: String,
    pub usb_path: String,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct ZeroState {
    pub key: String,
    pub found: Option<Found>,
    pub hello: Option<Hello>,
    pub status: Option<Status>,
    pub generation: Option<u16>,
    pub params: Vec<(u8, u16)>,
    /// Unix seconds of the last full read.
    pub read_at: i64,
    pub error: String,
    /// "none" | "matches" | "differs" | "unknown"
    pub profile_state: String,
    /// (id, stored value, device value)
    pub differs: Vec<(u8, u16, u16)>,
    /// The device restarted or was replugged since the Base last read it.
    pub reconnected: bool,
    #[serde(skip)]
    last_hello: Option<Instant>,
}

pub struct Manager {
    dir: PathBuf,
    zeros: Mutex<HashMap<String, ZeroState>>,
    io: Mutex<()>,
    /// Live telemetry frames (newest at the back), filled while a Zero is connected.
    frames: Mutex<HashMap<String, VecDeque<Telemetry>>>,
    /// Long-lived sequencer sessions used only to harvest telemetry (port 1).
    telem: Mutex<HashMap<String, Session>>,
    lease_until: Mutex<HashMap<String, Instant>>,
}

/// One aseqdump/aseqsend session on a device's configuration port.
struct Session {
    port: String,
    child: Child,
    rx: Receiver<Vec<u8>>,
}

impl Session {
    fn open(client: u32) -> Result<Session, String> {
        let port = format!("{client}:1");
        let mut child = Command::new("stdbuf")
            .args(["-oL", "aseqdump", "-p", &port])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("aseqdump: {e}"))?;
        let out = child.stdout.take().ok_or("aseqdump: no stdout")?;
        let (tx, rx) = channel::<Vec<u8>>();
        let (ready_tx, ready_rx) = channel::<()>();
        std::thread::Builder::new()
            .name("zero-rx".into())
            .spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if line.starts_with("Waiting") {
                        let _ = ready_tx.send(());
                    } else if let Some(i) = line.find("F0 ") {
                        if let Some(b) = zero::parse_hex(&line[i..]) {
                            if tx.send(b).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let mut s = Session { port, child, rx };
        if ready_rx.recv_timeout(Duration::from_millis(800)).is_err() {
            s.close();
            return Err("configuration port did not open".into());
        }
        std::thread::sleep(Duration::from_millis(30));
        Ok(s)
    }

    fn send(&self, frame: &[u8]) -> Result<(), String> {
        let st = Command::new("aseqsend")
            .args(["-p", &self.port, &zero::to_hex(frame)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("aseqsend: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!("aseqsend failed ({st})"))
        }
    }

    /// Replies until `want` have arrived or `timeout` passes.
    fn try_drain(&self) -> Vec<Vec<u8>> {
        let mut v = vec![];
        while let Ok(b) = self.rx.try_recv() {
            v.push(b);
        }
        v
    }

    fn collect(&self, want: usize, timeout: Duration) -> Vec<Message> {
        let end = Instant::now() + timeout;
        let mut v = vec![];
        while v.len() < want {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match self.rx.recv_timeout(left) {
                Ok(b) => {
                    if let Some(m) = zero::parse(&b) {
                        v.push(m);
                    }
                }
                Err(_) => break,
            }
        }
        v
    }

    /// One request, its reply (matched by request id).
    fn ask(&self, frame: &[u8]) -> Result<Reply, String> {
        let req = frame[6];
        self.send(frame)?;
        let end = Instant::now() + Duration::from_millis(700);
        while Instant::now() < end {
            for m in self.collect(1, end.saturating_duration_since(Instant::now())) {
                if m.req == req && !matches!(m.reply, Reply::Telemetry(_)) {
                    return Ok(m.reply);
                }
            }
        }
        Err("no reply from the device".into())
    }

    fn close(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

/// "1-1" from /sys/class/sound/cardN/device -> .../usb1/1-1/1-1:1.2
fn usb_path(card: u32) -> String {
    std::fs::canonicalize(format!("/sys/class/sound/card{card}/device"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
        .map(|f| f.split(':').next().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// Trillium Zero controllers in the ALSA sequencer, matched by the device classes
/// whose id is "trillium" (USB ids or product-name prefixes).
pub fn find(classes: &[pilab_core::DeviceClass]) -> Vec<Found> {
    let clients = identity::parse_seq_clients(&std::fs::read_to_string("/proc/asound/seq/clients").unwrap_or_default());
    let cards = identity::parse_cards(&std::fs::read_to_string("/proc/asound/cards").unwrap_or_default());
    find_in(classes, &clients, &cards, |n| std::fs::read_to_string(format!("/proc/asound/card{n}/usbid")).ok().map(|s| s.trim().to_lowercase()), usb_path)
}

pub fn find_in(
    classes: &[pilab_core::DeviceClass],
    clients: &[(u32, String, Vec<(u32, String)>)],
    cards: &HashMap<String, u32>,
    usbid: impl Fn(u32) -> Option<String>,
    path: impl Fn(u32) -> String,
) -> Vec<Found> {
    let Some(class) = classes.iter().find(|c| c.id == "trillium") else { return vec![] };
    let mut v = vec![];
    for (num, name, ports) in clients {
        if ports.len() < 2 {
            continue;
        }
        let card = cards.get(name).copied().or_else(|| (*num >= 16).then(|| (*num - 16) / 4));
        let Some(card) = card else { continue };
        let id = usbid(card).unwrap_or_default();
        let by_id = class.usb_ids.iter().any(|u| u.eq_ignore_ascii_case(&id));
        let by_name = class.name_prefixes.iter().any(|p| !p.is_empty() && name.to_lowercase().starts_with(&p.to_lowercase()));
        if !(by_id || by_name) {
            continue;
        }
        let up = path(card);
        let key = if up.is_empty() { format!("zero-card{card}") } else { format!("zero-{up}") };
        v.push(Found { key, label: "Trillium Zero".into(), client: *num, card, usb_id: id, usb_path: up });
    }
    v
}

fn read_full(f: &Found) -> Result<(Hello, Option<Status>, u16, Vec<(u8, u16)>), String> {
    let s = Session::open(f.client)?;
    let Reply::Hello(h) = s.ask(&zero::hello(0x10))? else { return Err("unexpected reply to HELLO".into()) };
    let status = if h.has_status() {
        match s.ask(&zero::get_status(0x11))? {
            Reply::Status(st) => Some(st),
            _ => None,
        }
    } else {
        None
    };
    let mut params = vec![];
    let mut generation = 0;
    for (i, page) in h.pages().into_iter().enumerate() {
        match s.ask(&zero::get_config(0x12 + i as u8, page))? {
            Reply::Config(c) => {
                generation = c.generation;
                params.extend(c.params);
            }
            Reply::Error { code, detail } => return Err(format!("GET_CONFIG page {page}: {} ({detail})", zero::error_name(code))),
            _ => return Err(format!("unexpected reply to GET_CONFIG page {page}")),
        }
    }
    params.sort_by_key(|p| p.0);
    Ok((h, status, generation, params))
}

impl Manager {
    pub fn new(config_dir: &std::path::Path) -> Manager {
        Manager { dir: config_dir.join("devices"), zeros: Mutex::new(HashMap::new()), io: Mutex::new(()), frames: Mutex::new(HashMap::new()), telem: Mutex::new(HashMap::new()), lease_until: Mutex::new(HashMap::new()) }
    }

    fn profile_path(&self, key: &str) -> PathBuf {
        let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
        self.dir.join(format!("{safe}.toml"))
    }

    pub fn load_profile(&self, key: &str) -> Option<Profile> {
        std::fs::read_to_string(self.profile_path(key)).ok().and_then(|s| Profile::from_toml(&s).ok())
    }

    fn save_profile(&self, p: &Profile) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.profile_path(&p.key);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, p.to_toml()?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    /// Full read of one device into the state, and profile bookkeeping. Returns events.
    fn refresh(&self, f: &Found, reconnected: bool) -> Vec<(&'static str, String)> {
        let mut ev = vec![];
        let r = {
            let _io = self.io.lock().unwrap();
            read_full(f)
        };
        let adopted = self.adopt_profile_if_needed(f, &mut ev);
        let mut z = self.zeros.lock().unwrap();
        let st = z.entry(f.key.clone()).or_default();
        st.key = f.key.clone();
        st.found = Some(f.clone());
        st.last_hello = Some(Instant::now());
        match r {
            Ok((h, status, generation, params)) => {
                st.error.clear();
                st.read_at = timefmt::now_secs();
                st.generation = Some(generation);
                st.status = status;
                st.params = params;
                st.reconnected = st.reconnected || reconnected;
                let fw = h.firmware.clone();
                st.hello = Some(h);
                match self.load_profile(&f.key) {
                    None => {
                        let p = Profile::from_params(&f.key, &f.label, &f.usb_id, &fw, st.read_at as u64, &st.params);
                        match self.save_profile(&p) {
                            Ok(()) => ev.push(("info", format!("Trillium Zero ({}): stored a settings snapshot ({} settings, firmware {fw})", f.usb_path, p.params.len()))),
                            Err(e) => ev.push(("warn", format!("Trillium Zero: could not store its profile: {e}"))),
                        }
                        st.profile_state = "matches".into();
                        st.differs.clear();
                    }
                    Some(p) => {
                        let d = zero::diff(&p, &st.params);
                        st.differs = d.iter().map(|&(id, want)| (id, want, st.params.iter().find(|x| x.0 == id).map(|x| x.1).unwrap_or(0))).collect();
                        st.profile_state = if d.is_empty() { "matches".into() } else { "differs".into() };
                        if !d.is_empty() && (reconnected || adopted) {
                            ev.push(("warn", format!("Trillium Zero ({}): {} setting(s) differ from its stored profile; re-apply from Devices", f.usb_path, d.len())));
                        }
                    }
                }
            }
            Err(e) => {
                if st.error != e {
                    ev.push(("warn", format!("Trillium Zero ({}): {e}", f.usb_path)));
                }
                st.error = e;
                st.profile_state = "unknown".into();
            }
        }
        ev
    }

    /// Background check, every few seconds: new devices and replugs get a full read;
    /// known ones a HELLO every 30 s to catch a reset (boot id change).
    pub fn poll(&self, classes: &[pilab_core::DeviceClass]) -> Vec<(&'static str, String)> {
        let found = find(classes);
        let mut ev = vec![];
        let keys: Vec<String> = found.iter().map(|f| f.key.clone()).collect();
        {
            let mut z = self.zeros.lock().unwrap();
            let gone: Vec<String> = z.keys().filter(|k| !keys.contains(k)).cloned().collect();
            for k in gone {
                if z.get(&k).is_some_and(|s| s.found.is_some()) {
                    ev.push(("info", format!("Trillium Zero unplugged ({k})")));
                    if let Some(s) = z.get_mut(&k) {
                        s.found = None;
                        s.reconnected = false;
                    }
                    self.close_telem(&k);
                }
            }
        }
        for f in &found {
            let (known, was_connected, same_client, due, last_boot) = {
                let z = self.zeros.lock().unwrap();
                match z.get(&f.key) {
                    Some(s) => (
                        true,
                        s.found.is_some(),
                        s.found.as_ref().is_some_and(|o| o.client == f.client),
                        s.last_hello.map_or(true, |t| t.elapsed() >= Duration::from_secs(30)),
                        s.hello.as_ref().map(|h| h.boot_id),
                    ),
                    None => (false, false, false, true, None),
                }
            };
            if !known || !was_connected {
                ev.push(("info", format!("Trillium Zero connected ({}, ALSA client {})", f.usb_path, f.client)));
                // First sight in this process, including after a daemon restart, is not a reconnect.
                ev.extend(self.refresh(f, false));
            } else if !same_client {
                ev.push(("info", format!("Trillium Zero ({}) replugged (new ALSA client {})", f.usb_path, f.client)));
                ev.extend(self.refresh(f, true));
            } else if due {
                let boot = {
                    let _io = self.io.lock().unwrap();
                    Session::open(f.client).and_then(|s| s.ask(&zero::hello(0x10)))
                };
                match boot {
                    Ok(Reply::Hello(h)) if Some(h.boot_id) != last_boot => {
                        ev.push(("info", format!("Trillium Zero ({}) reset (boot id {} → {})", f.usb_path, last_boot.unwrap_or(0), h.boot_id)));
                        ev.extend(self.refresh(f, true));
                    }
                    Ok(_) => {
                        if let Some(s) = self.zeros.lock().unwrap().get_mut(&f.key) {
                            s.last_hello = Some(Instant::now());
                        }
                    }
                    Err(e) => {
                        if let Some(s) = self.zeros.lock().unwrap().get_mut(&f.key) {
                            s.last_hello = Some(Instant::now());
                            s.error = e;
                        }
                    }
                }
            }
        }
        ev
    }

    fn found(&self, key: &str) -> Result<Found, String> {
        self.zeros.lock().unwrap().get(key).and_then(|s| s.found.clone()).ok_or_else(|| format!("no connected device {key}"))
    }

    /// Re-reads the device and replaces its stored profile with what it has now.
    /// Read-only towards the device.
    pub fn snapshot(&self, key: &str) -> Result<Value, String> {
        let f = self.found(key)?;
        let mut ev = self.refresh(&f, false);
        let st = self.zeros.lock().unwrap().get(key).cloned().unwrap_or_default();
        if !st.error.is_empty() {
            return Err(st.error);
        }
        let fw = st.hello.as_ref().map(|h| h.firmware.clone()).unwrap_or_default();
        let p = Profile::from_params(&f.key, &f.label, &f.usb_id, &fw, st.read_at as u64, &st.params);
        self.save_profile(&p)?;
        if let Some(s) = self.zeros.lock().unwrap().get_mut(key) {
            s.profile_state = "matches".into();
            s.differs.clear();
            s.reconnected = false;
        }
        ev.push(("info", format!("Trillium Zero ({}): settings snapshot stored ({} settings)", f.usb_path, p.params.len())));
        Ok(json!({ "ok": true, "stored": p.params.len(), "events": ev.into_iter().map(|(_, t)| t).collect::<Vec<_>>() }))
    }

    /// Sends the stored profile's values that differ from the device (one SET_CONFIG per
    /// page), then reads the device back. Changes the device's settings: only called
    /// from an explicit console action with `confirm`.
    pub fn reapply(&self, key: &str, confirm: bool) -> Result<Value, String> {
        if !confirm {
            return Err("re-applying changes the device's settings; send confirm: true from the console button".into());
        }
        let f = self.found(key)?;
        let profile = self.load_profile(key).ok_or("no stored profile for this device")?;
        let _ = self.refresh(&f, false);
        let current = self.zeros.lock().unwrap().get(key).map(|s| s.params.clone()).unwrap_or_default();
        let changes = zero::diff(&profile, &current);
        if changes.is_empty() {
            return Ok(json!({ "ok": true, "changed": [], "note": "device already matches its profile" }));
        }
        let can = self.zeros.lock().unwrap().get(key).and_then(|s| s.hello.as_ref().map(|h| h.caps & zero::CAP_SET_CONFIG != 0)).unwrap_or(false);
        if !can {
            return Err("this firmware does not accept SET_CONFIG".into());
        }
        let mut errors = vec![];
        {
            let _io = self.io.lock().unwrap();
            let s = Session::open(f.client)?;
            for fr in zero::set_config_frames(0x30, &changes) {
                match s.ask(&fr)? {
                    Reply::Config(_) => {}
                    Reply::Error { code, detail } => errors.push(format!("{} (parameter {detail})", zero::error_name(code))),
                    _ => errors.push("unexpected reply".into()),
                }
            }
        }
        let _ = self.refresh(&f, false);
        if let Some(s) = self.zeros.lock().unwrap().get_mut(key) {
            s.reconnected = false;
        }
        let left = self.zeros.lock().unwrap().get(key).map(|s| s.differs.len()).unwrap_or(0);
        Ok(json!({
            "ok": errors.is_empty() && left == 0,
            "changed": changes.iter().map(|(id, v)| json!({ "id": id, "name": zero::param_name(*id), "value": v })).collect::<Vec<_>>(),
            "errors": errors,
            "still_different": left,
        }))
    }

    /// Sensor snapshot: a telemetry lease at `hz` for `ms`, then stop. Read-only.
    pub fn telemetry(&self, key: &str, hz: u8, ms: u64) -> Result<Vec<Telemetry>, String> {
        let f = self.found(key)?;
        let _io = self.io.lock().unwrap();
        let s = Session::open(f.client)?;
        s.send(&zero::telemetry_start(0x40, hz.clamp(1, 50)))?;
        let msgs = s.collect(usize::MAX, Duration::from_millis(ms.clamp(100, 3000)));
        let _ = s.send(&zero::telemetry_stop(0x41));
        let _ = s.collect(1, Duration::from_millis(200));
        let mut v = vec![];
        for m in msgs {
            match m.reply {
                Reply::Telemetry(t) => v.push(*t),
                Reply::Error { code, .. } => return Err(format!("telemetry: {}", zero::error_name(code))),
                _ => {}
            }
        }
        Ok(v)
    }

    pub fn zeros_json(&self) -> Vec<Value> {
        let z = self.zeros.lock().unwrap();
        let mut v: Vec<&ZeroState> = z.values().collect();
        v.sort_by_key(|s| s.key.clone());
        let frames = self.frames.lock().unwrap();
        v.into_iter()
            .map(|s| {
                let key = if s.key.is_empty() { s.found.as_ref().map(|f| f.key.clone()).unwrap_or_default() } else { s.key.clone() };
                let profile = self.load_profile(&key);
                let live = frames.get(&key).map(|q| q.iter().cloned().collect::<Vec<_>>()).unwrap_or_default();
                json!({
                    "key": key,
                    "found": s.found,
                    "hello": s.hello,
                    "status": s.status,
                    "generation": s.generation,
                    "read_at": s.read_at,
                    "read_at_text": if s.read_at > 0 { timefmt::iso_local(s.read_at) } else { String::new() },
                    "error": s.error,
                    "profile_state": s.profile_state,
                    "reconnected": s.reconnected,
                    "profile": profile.as_ref().map(|p| json!({ "captured": p.captured, "captured_text": timefmt::iso_local(p.captured as i64), "firmware": p.firmware, "count": p.params.len() })),
                    "params": s.params.iter().map(|(id, v)| {
                        let stored = profile.as_ref().and_then(|p| p.params.iter().find(|x| x.id == *id).map(|x| x.value));
                        json!({ "id": id, "page": zero::page_of(*id), "name": zero::param_name(*id), "value": v, "stored": stored })
                    }).collect::<Vec<_>>(),
                    "differs": s.differs.iter().map(|(id, want, have)| json!({ "id": id, "name": zero::param_name(*id), "stored": want, "device": have })).collect::<Vec<_>>(),
                    "frames": live,
                })
            })
            .collect()
    }

    /// If this USB socket has no profile yet, reuse the only stored profile for the same
    /// USB id whose device is not currently connected (the Zero was moved to another port).
    fn adopt_profile_if_needed(&self, f: &Found, ev: &mut Vec<(&'static str, String)>) -> bool {
        if self.load_profile(&f.key).is_some() {
            return false;
        }
        let Some(src) = self.adopt_candidate(&f.key, &f.usb_id) else { return false };
        let Some(mut p) = self.load_profile(&src) else { return false };
        let from = p.key.clone();
        p.key = f.key.clone();
        if let Err(e) = self.save_profile(&p) {
            ev.push(("warn", format!("Trillium Zero: could not adopt the profile from {from}: {e}")));
            return false;
        }
        ev.push(("info", format!("Trillium Zero moved from {from} to {} (same USB id {}); using the stored profile", f.key, f.usb_id)));
        true
    }

    fn adopt_candidate(&self, new_key: &str, usb_id: &str) -> Option<String> {
        adopt_candidate(new_key, usb_id, &self.list_profile_keys_and_ids(), &self.connected_keys())
    }

    fn list_profile_keys_and_ids(&self) -> Vec<(String, String)> {
        let mut v = vec![];
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return v };
        for e in rd.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) != Some("toml") {
                continue;
            }
            if let Ok(s) = std::fs::read_to_string(e.path()) {
                if let Ok(p) = Profile::from_toml(&s) {
                    v.push((p.key, p.usb_id));
                }
            }
        }
        v
    }

    fn connected_keys(&self) -> Vec<String> {
        self.zeros.lock().unwrap().values().filter(|s| s.found.is_some()).map(|s| if s.key.is_empty() { s.found.as_ref().unwrap().key.clone() } else { s.key.clone() }).collect()
    }

    fn close_telem(&self, key: &str) {
        if let Some(mut s) = self.telem.lock().unwrap().remove(key) {
            s.close();
        }
        self.lease_until.lock().unwrap().remove(key);
    }

    /// Keep a 20 Hz telemetry lease on each connected Zero, drain new frames into the
    /// live ring, and return the latest frame for the motion MIDI port.
    pub fn harvest_motion(&self) -> Option<Telemetry> {
        let connected: Vec<Found> = self.zeros.lock().unwrap().values().filter_map(|s| s.found.clone()).collect();
        let live: Vec<String> = connected.iter().map(|f| f.key.clone()).collect();
        {
            let stale: Vec<String> = self.telem.lock().unwrap().keys().filter(|k| !live.contains(k)).cloned().collect();
            for k in stale {
                self.close_telem(&k);
            }
        }
        let mut latest = None;
        for f in &connected {
            if let Some(t) = self.harvest_one(f) {
                latest = Some(t);
            }
        }
        latest
    }

    fn harvest_one(&self, f: &Found) -> Option<Telemetry> {
        let _io = self.io.lock().ok()?;
        let need_open = !self.telem.lock().unwrap().contains_key(&f.key);
        if need_open {
            match Session::open(f.client) {
                Ok(s) => { self.telem.lock().unwrap().insert(f.key.clone(), s); }
                Err(_) => return None,
            }
        }
        let due = self.lease_until.lock().unwrap().get(&f.key).map(|t| Instant::now() >= *t).unwrap_or(true);
        if due {
            let ok = {
                let map = self.telem.lock().unwrap();
                map.get(&f.key).and_then(|s| s.send(&zero::telemetry_start(0x40, 20)).ok()).is_some()
            };
            if ok {
                self.lease_until.lock().unwrap().insert(f.key.clone(), Instant::now() + Duration::from_secs(4));
            }
        }
        let raw = {
            let map = self.telem.lock().unwrap();
            map.get(&f.key).map(|s| s.try_drain()).unwrap_or_default()
        };
        let mut last = None;
        {
            let mut rings = self.frames.lock().unwrap();
            let q = rings.entry(f.key.clone()).or_default();
            for b in raw {
                if let Some(Message { reply: Reply::Telemetry(t), .. }) = zero::parse(&b) {
                    let t = (*t).clone();
                    q.push_back(t.clone());
                    last = Some(t);
                }
            }
            while q.len() > 120 {
                q.pop_front();
            }
        }
        last
    }
}

/// Picks a stored profile to reuse when a Zero appears on a new USB socket.
/// Only when there is exactly one other profile with the same USB id whose device is not connected.
pub fn adopt_candidate(new_key: &str, usb_id: &str, profiles: &[(String, String)], connected: &[String]) -> Option<String> {
    if profiles.iter().any(|(k, _)| k == new_key) {
        return None;
    }
    let mut cands: Vec<&str> = profiles
        .iter()
        .filter(|(k, id)| k != new_key && id.eq_ignore_ascii_case(usb_id) && !connected.iter().any(|c| c == k))
        .map(|(k, _)| k.as_str())
        .collect();
    cands.sort();
    cands.dedup();
    if cands.len() == 1 { Some(cands[0].to_string()) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_zero_by_usb_id_and_keys_it_by_socket() {
        let clients = identity::parse_seq_clients(
            "Client  14 : \"Midi Through\" [Kernel Legacy]\n  Port   0 : \"Midi Through Port-0\" (RWe-) [In/Out]\nClient  36 : \"Trill XIAO MIDI+PCM (dev)\" [Kernel Legacy]\n  Port   0 : \"Trill XIAO MIDI+PCM (dev) MIDI \" (RWeX) [In/Out]\n  Port   1 : \"Trill XIAO MIDI+PCM (dev) MIDI \" (RWeX) [In/Out]\nClient  20 : \"Keystation 49\" [Kernel Legacy]\n  Port   0 : \"Keystation 49 MIDI 1\" (RWeX) [In/Out]\n  Port   1 : \"Keystation 49 MIDI 2\" (RWeX) [In/Out]\n",
        );
        let cards = identity::parse_cards(" 5 [MIDIPCMdev     ]: USB-Audio - Trill XIAO MIDI+PCM (dev)\n 1 [K49            ]: USB-Audio - Keystation 49\n");
        let ids = |n: u32| Some(if n == 5 { "1209:0003".to_string() } else { "0763:2027".to_string() });
        let f = find_in(&pilab_core::devices::default_classes(), &clients, &cards, ids, |n| if n == 5 { "1-1".into() } else { "3-2".into() });
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].client, 36);
        assert_eq!(f[0].card, 5);
        assert_eq!(f[0].key, "zero-1-1");
        assert_eq!(f[0].usb_id, "1209:0003");
    }

    #[test]
    fn profile_files_are_named_safely() {
        let m = Manager::new(std::path::Path::new("/tmp/x"));
        assert_eq!(m.profile_path("zero-1-1.2"), PathBuf::from("/tmp/x/devices/zero-1-1.2.toml"));
        assert_eq!(m.profile_path("../evil"), PathBuf::from("/tmp/x/devices/.._evil.toml"));
    }

    #[test]
    fn adopts_the_only_disconnected_profile_for_the_same_usb_id() {
        let profiles = vec![("zero-1-1".into(), "1209:0003".into())];
        assert_eq!(adopt_candidate("zero-3-1", "1209:0003", &profiles, &[]).as_deref(), Some("zero-1-1"));
        assert_eq!(adopt_candidate("zero-1-1", "1209:0003", &profiles, &[]), None);
        assert_eq!(adopt_candidate("zero-3-1", "1209:0003", &profiles, &["zero-1-1".into()]), None);
        let two = vec![("zero-1-1".into(), "1209:0003".into()), ("zero-3-2".into(), "1209:0003".into())];
        assert_eq!(adopt_candidate("zero-4-1", "1209:0003", &two, &[]), None);
        assert_eq!(adopt_candidate("zero-4-1", "1209:0003", &two, &["zero-3-2".into()]).as_deref(), Some("zero-1-1"));
    }
}
