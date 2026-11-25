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
use pilab_core::zero_map;
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
    /// Last commanded microphone test-stream state (ACTION 7/8). Not reported by GET_STATUS.
    pub mic: bool,
    /// The touch strip reported Running at some point since this device was found.
    #[serde(skip)]
    trill_seen: bool,
    /// Last sensor inventory summary, so a change is logged once and never repeated.
    #[serde(skip)]
    inv_sig: String,
    #[serde(skip)]
    last_hello: Option<Instant>,
}

pub struct Manager {
    dir: PathBuf,
    zeros: Mutex<HashMap<String, ZeroState>>,
    io: Mutex<()>,
    auto_due: Mutex<Vec<String>>,
    /// Live telemetry frames (newest at the back), filled while a Zero is connected.
    frames: Mutex<HashMap<String, VecDeque<Telemetry>>>,
    /// Long-lived sequencer sessions used only to harvest telemetry (port 1).
    telem: Mutex<HashMap<String, Session>>,
    lease_until: Mutex<HashMap<String, Instant>>,
    /// Events raised outside `poll` (sensor inventory changes), returned by the next `poll`.
    pending_ev: Mutex<Vec<(&'static str, String)>>,
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
        Manager { dir: config_dir.join("devices"), zeros: Mutex::new(HashMap::new()), io: Mutex::new(()), auto_due: Mutex::new(Vec::new()), frames: Mutex::new(HashMap::new()), telem: Mutex::new(HashMap::new()), lease_until: Mutex::new(HashMap::new()), pending_ev: Mutex::new(Vec::new()) }
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
        if reconnected {
            // Reset or replug: the firmware drives the mic power pin low.
            st.mic = false;
        }
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
                        if !d.is_empty() && (reconnected || adopted) && p.manual {
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
                        // Firmware stops the PCM stream on unplug; the live flag must not
                        // hide a later profile re-apply after a replug.
                        s.mic = false;
                        s.trill_seen = false;
                        s.inv_sig.clear();
                    }
                    self.frames.lock().unwrap().remove(&k);
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
                self.auto_due.lock().unwrap().push(f.key.clone());
            } else if !same_client {
                ev.push(("info", format!("Trillium Zero ({}) replugged (new ALSA client {})", f.usb_path, f.client)));
                ev.extend(self.refresh(f, true));
                self.auto_due.lock().unwrap().push(f.key.clone());
            } else if due {
                let boot = {
                    let _io = self.io.lock().unwrap();
                    Session::open(f.client).and_then(|s| s.ask(&zero::hello(0x10)))
                };
                match boot {
                    Ok(Reply::Hello(h)) if Some(h.boot_id) != last_boot => {
                        ev.push(("info", format!("Trillium Zero ({}) reset (boot id {} → {})", f.usb_path, last_boot.unwrap_or(0), h.boot_id)));
                        ev.extend(self.refresh(f, true));
                        self.auto_due.lock().unwrap().push(f.key.clone());
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
        ev.extend(self.pending_ev.lock().unwrap().drain(..));
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
        let mut p = Profile::from_params(&f.key, &f.label, &f.usb_id, &fw, st.read_at as u64, &st.params);
        p.mic = st.mic || self.load_profile(key).map(|o| o.mic).unwrap_or(false);
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
                let inv = zero::sensors(s.hello.as_ref(), s.status.as_ref(), &live, s.mic, s.trill_seen);
                let running = |id: &str| inv.iter().any(|x| x.id == id && x.state == "running");
                let note_path = zero::note_path(&s.params, running("trill"), running("imu"));
                json!({
                    "sensors": inv,
                    "note_path": note_path,
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
                    "mic": s.mic,
                    "summary": zero_map::describe(&s.params),
                    "manual": profile.as_ref().map(|p| p.manual).unwrap_or(false),
                    "preset": profile.as_ref().map(|p| p.preset.clone()).unwrap_or_default(),
                    "mic_profile": profile.as_ref().map(|p| p.mic).unwrap_or(false),
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


    /// Profiles due for an automatic re-apply (device just connected, replugged or reset).
    /// Sends the stored settings unless the profile is marked manual. Returns events.
    pub fn auto_apply_due(&self) -> Vec<(&'static str, String)> {
        let keys: Vec<String> = std::mem::take(&mut *self.auto_due.lock().unwrap());
        let mut ev = vec![];
        for key in keys {
            let Some(p) = self.load_profile(&key) else { continue };
            if p.manual {
                continue;
            }
            let differs = self.zeros.lock().unwrap().get(&key).map(|s| s.profile_state == "differs").unwrap_or(false);
            if !differs {
                continue;
            }
            match self.reapply(&key, true) {
                Ok(v) => ev.push(("info", format!("Trillium Zero ({key}): restored {} stored setting(s) after connect{}", v["changed"].as_array().map(|a| a.len()).unwrap_or(0), if p.preset.is_empty() { String::new() } else { format!(" (mapping \"{}\")", p.preset) }))),
                Err(e) => ev.push(("warn", format!("Trillium Zero ({key}): could not restore its stored settings: {e}"))),
            }
        }
        ev
    }

    /// Validated changes sent to the device, read back, and (with `save`) merged into the
    /// stored profile. Parameters the firmware does not have are skipped and reported.
    pub fn set_values(&self, key: &str, changes: &[(u8, u16)], save: bool, preset: Option<&str>) -> Result<Value, String> {
        for &(id, v) in changes {
            zero_map::validate(id, v)?;
        }
        let f = self.found(key)?;
        let _ = self.refresh(&f, false);
        let (caps, current) = {
            let z = self.zeros.lock().unwrap();
            let st = z.get(key).ok_or("device not read yet")?;
            (st.hello.as_ref().map(|h| h.caps).unwrap_or(0), st.params.clone())
        };
        if caps & zero::CAP_SET_CONFIG == 0 {
            return Err("this firmware does not accept settings changes".into());
        }
        let skipped: Vec<u8> = changes.iter().filter(|(id, _)| !zero_map::supported(*id, caps)).map(|c| c.0).collect();
        let send: Vec<(u8, u16)> = changes
            .iter()
            .copied()
            .filter(|(id, v)| zero_map::supported(*id, caps) && current.iter().find(|x| x.0 == *id).map(|x| x.1) != Some(*v))
            .collect();
        let mut errors = vec![];
        if !send.is_empty() {
            let _io = self.io.lock().unwrap();
            let s = Session::open(f.client)?;
            for fr in zero::set_config_frames(0x20, &send) {
                match s.ask(&fr) {
                    Ok(Reply::Config(_)) => {}
                    Ok(Reply::Error { code, detail }) => errors.push(format!("{} (parameter {detail} {})", zero::error_name(code), zero::param_name(detail))),
                    Ok(_) => errors.push("unexpected reply".into()),
                    Err(e) => errors.push(e),
                }
            }
        }
        let _ = self.refresh(&f, false);
        let live = self.zeros.lock().unwrap().get(key).map(|s| s.params.clone()).unwrap_or_default();
        let not_taken: Vec<u8> = send.iter().filter(|(id, v)| live.iter().find(|x| x.0 == *id).map(|x| x.1) != Some(*v)).map(|c| c.0).collect();
        if save {
            let st = self.zeros.lock().unwrap().get(key).cloned().unwrap_or_default();
            let fw = st.hello.as_ref().map(|h| h.firmware.clone()).unwrap_or_default();
            let mut p = self.load_profile(key).unwrap_or_else(|| Profile::from_params(&f.key, &f.label, &f.usb_id, &fw, st.read_at as u64, &live));
            for &(id, v) in changes {
                if !zero_map::supported(id, caps) || not_taken.contains(&id) {
                    continue;
                }
                match p.params.iter_mut().find(|x| x.id == id) {
                    Some(x) => x.value = v,
                    None => p.params.push(zero::Param { id, value: v }),
                }
            }
            p.params.sort_by_key(|x| x.id);
            p.firmware = fw;
            p.captured = timefmt::now_secs() as u64;
            p.preset = preset.unwrap_or("").to_string();
            self.save_profile(&p)?;
            let d = zero::diff(&p, &live);
            if let Some(s) = self.zeros.lock().unwrap().get_mut(key) {
                s.differs = d.iter().map(|&(id, want)| (id, want, live.iter().find(|x| x.0 == id).map(|x| x.1).unwrap_or(0))).collect();
                s.profile_state = if d.is_empty() { "matches".into() } else { "differs".into() };
                s.reconnected = false;
            }
        }
        Ok(json!({
            "ok": errors.is_empty() && not_taken.is_empty(),
            "sent": send.iter().map(|(id, v)| json!({ "id": id, "name": zero::param_name(*id), "value": v })).collect::<Vec<_>>(),
            "skipped_for_firmware": skipped,
            "not_taken": not_taken,
            "errors": errors,
            "saved": save,
            "summary": zero_map::describe(&live),
        }))
    }

    /// Mark a profile as re-applied automatically on connect (default) or only by hand.
    pub fn set_manual(&self, key: &str, manual: bool) -> Result<Value, String> {
        let mut p = self.load_profile(key).ok_or("no stored profile for this device")?;
        p.manual = manual;
        self.save_profile(&p)?;
        Ok(json!({ "ok": true, "manual": manual }))
    }

    /// One device action. Bluetooth actions (10–13) and the bootloader (6) are not sent
    /// from here: radio settings stay with the owner, and the bootloader belongs to the
    /// firmware update path.
    pub fn action(&self, key: &str, name: &str) -> Result<Value, String> {
        let code = match name {
            "panic" => 1,
            "recalibrate" => 2,
            "led-test" => 3,
            "restore-defaults" => 4,
            "zero-orientation" => 5,
            _ => return Err("action: panic | recalibrate | led-test | restore-defaults | zero-orientation".into()),
        };
        let f = self.found(key)?;
        let reply = {
            let _io = self.io.lock().unwrap();
            let s = Session::open(f.client)?;
            s.ask(&zero::action(0x58, code))?
        };
        let ok = match reply {
            Reply::Ack { action } => action == code,
            Reply::Config(_) => code == 4,
            Reply::Error { code: c, detail } => return Err(format!("{name}: {} ({detail})", zero::error_name(c))),
            _ => false,
        };
        let _ = self.refresh(&f, false);
        Ok(json!({ "ok": ok, "action": name }))
    }

    fn presets_dir(&self) -> PathBuf {
        self.dir.parent().map(|p| p.join("zero-presets")).unwrap_or_else(|| self.dir.join("presets"))
    }

    /// Built-in mapping presets, then the owner's (stored as trill-preset JSON files).
    pub fn presets(&self) -> Vec<zero_map::MapPreset> {
        let mut v = zero_map::builtin_presets();
        if let Ok(rd) = std::fs::read_dir(self.presets_dir()) {
            let mut user: Vec<zero_map::MapPreset> = rd
                .flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
                .filter_map(|e| {
                    let id = e.path().file_stem()?.to_str()?.to_string();
                    let j: Value = serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
                    let (name, values) = zero_map::from_trill_preset(&j).ok()?;
                    let scene = j.get("base_scene").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    Some(zero_map::MapPreset { id, summary: zero_map::describe(&values), name, scene, builtin: false, values })
                })
                .collect();
            user.sort_by(|a, b| a.name.cmp(&b.name));
            v.extend(user);
        }
        v
    }

    pub fn save_preset(&self, name: &str, values: &[(u8, u16)], scene: &str) -> Result<Value, String> {
        let id: String = name.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().trim_matches('-').to_string();
        if id.is_empty() || id.len() > 48 || name.len() > 64 {
            return Err("preset name: 1-64 characters, letters or digits".into());
        }
        if zero_map::builtin_presets().iter().any(|p| p.id == id) {
            return Err("that name is a built-in preset".into());
        }
        for &(i, v) in values {
            zero_map::validate(i, v)?;
        }
        let mut j = zero_map::to_trill_preset(&id, name, values);
        if !scene.is_empty() {
            j["base_scene"] = json!(scene);
        }
        let dir = self.presets_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{id}.json"));
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&j).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        Ok(json!({ "ok": true, "id": id }))
    }

    pub fn delete_preset(&self, id: &str) -> Result<Value, String> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err("bad preset id".into());
        }
        std::fs::remove_file(self.presets_dir().join(format!("{id}.json"))).map_err(|e| e.to_string())?;
        Ok(json!({ "ok": true }))
    }

    /// Live values of a connected (or last-seen) device.
    pub fn live_values(&self, key: &str) -> Vec<(u8, u16)> {
        self.zeros.lock().unwrap().get(key).map(|s| s.params.clone()).unwrap_or_default()
    }

    /// HELLO of a device, for the firmware view.
    pub fn hello_of(&self, key: &str) -> Option<Hello> {
        self.zeros.lock().unwrap().get(key).and_then(|s| s.hello.clone())
    }

    pub fn client_of(&self, key: &str) -> Option<u32> {
        self.zeros.lock().unwrap().get(key).and_then(|s| s.found.as_ref().map(|f| f.client))
    }

    /// Sends ACTION 6 (enter the UF2 bootloader). Only the firmware update path calls this,
    /// after the owner confirmed an update. Writes no flash by itself.
    pub fn enter_bootloader(&self, key: &str) -> Result<(), String> {
        let f = self.found(key)?;
        let caps = self.hello_of(key).map(|h| h.caps).unwrap_or(0);
        if caps & zero::CAP_BOARD == 0 {
            return Err("this firmware cannot restart into the bootloader; double-press RESET on the Zero instead".into());
        }
        let _io = self.io.lock().unwrap();
        let s = Session::open(f.client)?;
        match s.ask(&zero::action(0x5E, 6))? {
            Reply::Ack { action: 6 } => Ok(()),
            other => Err(format!("unexpected reply to bootloader action: {other:?}")),
        }
    }

    pub fn first_connected(&self) -> Option<String> {
        let mut k = self.connected_keys_live();
        k.sort();
        k.into_iter().next()
    }

    /// Stored or live request for the microphone test stream on a connected device.
    pub fn mic_wanted(&self) -> bool {
        let z = self.zeros.lock().unwrap();
        z.values().any(|s| s.found.is_some() && (s.mic || self.load_profile(&s.key).map(|p| p.mic).unwrap_or(false)))
    }

    /// Start or stop the onboard 16 kHz microphone test stream (ACTION 7 / 8).
    /// Only that action is sent. Settings are read before and after; if they
    /// change, the stream is stopped and the previous settings are restored.
    /// Turning the microphone on needs `confirm`. The flag is stored on the profile
    /// so a reconnect can start the stream again.
    pub fn set_mic(&self, key: &str, on: bool, confirm: bool) -> Result<Value, String> {
        if on && !confirm {
            return Err("starting the microphone test stream writes to the device; send confirm: true".into());
        }
        let f = self.found(key)?;
        let can = self.zeros.lock().unwrap().get(key).and_then(|s| s.hello.as_ref().map(|h| h.caps & zero::CAP_ACTION != 0 && h.mic())).unwrap_or(false);
        if !can {
            return Err("this firmware does not offer the microphone test stream".into());
        }
        let before = {
            let _ = self.refresh(&f, false);
            self.zeros.lock().unwrap().get(key).map(|s| s.params.clone()).unwrap_or_default()
        };
        let tap_before = before.iter().find(|(id, _)| *id == 11).map(|(_, v)| *v).unwrap_or(0);
        if tap_before != 0 {
            // Leave percussion alone unless we can see it is off; we will not turn it on.
        }
        let code = if on { zero::ACTION_START_MIC } else { zero::ACTION_STOP_MIC };
        {
            let _io = self.io.lock().unwrap();
            let s = Session::open(f.client)?;
            match s.ask(&zero::action(0x50, code))? {
                Reply::Ack { action } if action == code => {}
                Reply::Error { code: c, detail } => return Err(format!("ACTION {code}: {} ({detail})", zero::error_name(c))),
                other => return Err(format!("unexpected reply to microphone ACTION: {other:?}")),
            }
        }
        let _ = self.refresh(&f, false);
        let after = self.zeros.lock().unwrap().get(key).map(|s| s.params.clone()).unwrap_or_default();
        let tap = after.iter().find(|(id, _)| *id == 11).map(|(_, v)| *v).unwrap_or(0);
        if after != before || tap != 0 && tap_before == 0 {
            let _ = self.restore_params(&f, &before);
            let _io = self.io.lock().unwrap();
            if let Ok(s) = Session::open(f.client) {
                let _ = s.ask(&zero::action(0x51, zero::ACTION_STOP_MIC));
            }
            return Err("settings changed after the microphone action; the previous settings were restored and the stream was stopped".into());
        }
        if let Some(st) = self.zeros.lock().unwrap().get_mut(key) {
            st.mic = on;
        }
        if let Some(mut p) = self.load_profile(key) {
            p.mic = on;
            let _ = self.save_profile(&p);
        } else {
            let st = self.zeros.lock().unwrap().get(key).cloned().unwrap_or_default();
            let fw = st.hello.as_ref().map(|h| h.firmware.clone()).unwrap_or_default();
            let mut p = Profile::from_params(&f.key, &f.label, &f.usb_id, &fw, st.read_at as u64, &st.params);
            p.mic = on;
            let _ = self.save_profile(&p);
        }
        Ok(json!({
            "ok": true,
            "mic": on,
            "params_unchanged": true,
            "tap_percussion": tap,
            "settings": after.len(),
        }))
    }

    fn restore_params(&self, f: &Found, params: &[(u8, u16)]) -> Result<(), String> {
        let current = self.zeros.lock().unwrap().get(&f.key).map(|s| s.params.clone()).unwrap_or_default();
        let changes: Vec<(u8, u16)> = params.iter().copied().filter(|(id, want)| current.iter().find(|(i, _)| i == id).map(|(_, v)| v) != Some(want)).collect();
        if changes.is_empty() {
            return Ok(());
        }
        let _io = self.io.lock().unwrap();
        let s = Session::open(f.client)?;
        let mut req = 0x60u8;
        for frame in zero::set_config_frames(req, &changes) {
            let _ = s.ask(&frame);
            req = req.wrapping_add(1) & 0x7F;
        }
        Ok(())
    }

    /// True when the stored profile wants the microphone on and it is not yet running.
    /// The caller must open `/dev/trillium-zero-pcm` first, then send ACTION 7.
    pub fn profile_wants_mic(&self, key: &str) -> bool {
        let want = self.load_profile(key).map(|p| p.mic).unwrap_or(false);
        let already = self.zeros.lock().unwrap().get(key).map(|s| s.mic).unwrap_or(false);
        want && !already
    }

    pub fn connected_keys_live(&self) -> Vec<String> {
        self.zeros.lock().unwrap().values().filter(|s| s.found.is_some()).map(|s| s.key.clone()).collect()
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
        if last.is_some() {
            self.note_sensors(&f.key);
        }
        last
    }

    /// Updates the touch-strip memory and logs a sensor inventory change once.
    fn note_sensors(&self, key: &str) {
        let frames: Vec<Telemetry> = self.frames.lock().unwrap().get(key).map(|q| q.iter().cloned().collect()).unwrap_or_default();
        let mut z = self.zeros.lock().unwrap();
        let Some(s) = z.get_mut(key) else { return };
        if frames.iter().any(|t| t.trill_state == 2) {
            s.trill_seen = true;
        }
        let inv = zero::sensors(s.hello.as_ref(), s.status.as_ref(), &frames, s.mic, s.trill_seen);
        // Only the settled sensors: the mic and battery change on their own and are not news here.
        let sig: String = inv.iter().filter(|x| x.id == "imu" || x.id == "trill").map(|x| format!("{}={}", x.id, x.state)).collect::<Vec<_>>().join(" ");
        if sig != s.inv_sig && !inv.iter().any(|x| (x.id == "imu" || x.id == "trill") && x.state == "checking") {
            s.inv_sig = sig;
            let label = s.found.as_ref().map(|f| f.usb_path.clone()).unwrap_or_default();
            let line: Vec<String> = inv.iter().filter(|x| x.id == "imu" || x.id == "trill").map(|x| format!("{}: {}", x.label, x.state.replace('_', " "))).collect();
            self.pending_ev.lock().unwrap().push(("info", format!("Trillium Zero ({label}) sensors: {}", line.join(", "))));
        }
    }

    /// What `measure strike` needs: the first connected Zero's inventory, settings and note path.
    pub fn strike_plan(&self) -> Option<StrikePlan> {
        let key = self.zeros.lock().unwrap().values().find(|s| s.found.is_some()).map(|s| s.key.clone())?;
        let frames: Vec<Telemetry> = self.frames.lock().unwrap().get(&key).map(|q| q.iter().cloned().collect()).unwrap_or_default();
        let z = self.zeros.lock().unwrap();
        let s = z.get(&key)?;
        let inv = zero::sensors(s.hello.as_ref(), s.status.as_ref(), &frames, s.mic, s.trill_seen);
        let running = |id: &str| inv.iter().any(|x| x.id == id && x.state == "running");
        let path = zero::note_path(&s.params, running("trill"), running("imu"));
        Some(StrikePlan { key, trill_fitted: running("trill"), imu_running: running("imu"), mic: s.mic, path })
    }
}

/// Facts `measure strike` uses to word its cue and explain a missing note.
#[derive(Clone, Debug, Serialize)]
pub struct StrikePlan {
    pub key: String,
    pub trill_fitted: bool,
    pub imu_running: bool,
    pub mic: bool,
    pub path: zero::NotePath,
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
