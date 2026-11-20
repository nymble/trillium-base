// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Service state: the live scene, settings, MIDI input assignment, machine snapshot,
//! event log and the background loops that keep them current.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use pilab_core::scene::Slot;
use pilab_core::{compile_routes, glob, PortNames, Route, Scene, SceneStore, Settings};
use pilab_synth::osc::Osc;
use pilab_synth::tuning::gain_to_db;
use pilab_synth::{Bands, Cmd, CompiledRoute, Garbage, RouteTable, SlotParams, Stats, Waveform, MAX_PORTS, MAX_SLOTS, PORT_AUDITION, PORT_CONSOLE, PORT_VIRTUAL};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::audio::{AudioBackend, MidiEv, RtLinks, RtShared};
use crate::library::Library;
use crate::paths::{self, Paths};
use crate::record::{self, Recorder};
use crate::sys::{self, CpuSampler, Listen, Service, Sink, UsbDev};
use crate::timefmt;

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub t: i64,
    pub time: String,
    pub level: &'static str,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MidiLog {
    pub t: f64,
    pub port: u8,
    pub source: String,
    pub kind: &'static str,
    pub ch: u8,
    pub text: String,
    pub bytes: Vec<u8>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Persist {
    active_scene: String,
}

struct Inner {
    scene: Scene,
    dirty: bool,
    settings: Settings,
    ports: PortNames,
    events: VecDeque<Event>,
    midi_pending: Vec<MidiLog>,
    midi_counts: [u64; MAX_PORTS],
    services: Vec<Service>,
    listen: Vec<Listen>,
    usb: Vec<UsbDev>,
    sinks: Vec<Sink>,
    cores: Vec<f32>,
    temp: Option<f32>,
    mem: f32,
    load: String,
    unfiled: usize,
    oldest_unfiled: Option<i64>,
    wd_last: Option<i64>,
    wd_ticks: u32,
    xr_prev: u64,
    xr_rate: f32,
    audition: u8,
}

pub struct App {
    pub paths: Paths,
    pub device_id: String,
    pub hostname: String,
    pub port: u16,
    pub backend: Arc<dyn AudioBackend>,
    pub stats: Arc<Stats>,
    pub shared: Arc<RtShared>,
    pub recorder: Recorder,
    pub library: Library,
    pub store: SceneStore,
    pub token: RwLock<String>,
    pub tx: broadcast::Sender<String>,
    pub started: Instant,
    cmd: Mutex<rtrb::Producer<Cmd>>,
    gc_rx: Mutex<rtrb::Consumer<Garbage>>,
    mon_rx: Mutex<rtrb::Consumer<MidiEv>>,
    rec_rx: Mutex<rtrb::Consumer<f32>>,
    inner: Mutex<Inner>,
}

pub fn short_source(name: &str) -> String {
    let s = name.strip_prefix("Midi-Bridge:").unwrap_or(name);
    let s = s.strip_suffix(" (capture)").unwrap_or(s);
    s.to_string()
}

fn note_name(n: u8) -> String {
    let (name, oct) = zpod_music_core::harmony::note_name_parts(n);
    format!("{name}{oct}")
}

pub fn describe_midi(b: &[u8]) -> (&'static str, u8, String) {
    if b.is_empty() {
        return ("?", 0, String::new());
    }
    let ch = (b[0] & 0x0F) + 1;
    let d1 = b.get(1).copied().unwrap_or(0);
    let d2 = b.get(2).copied().unwrap_or(0);
    match b[0] & 0xF0 {
        0x90 if d2 > 0 => ("note-on", ch, format!("{} v{d2}", note_name(d1))),
        0x90 | 0x80 => ("note-off", ch, note_name(d1)),
        0xB0 => ("cc", ch, format!("CC {d1} = {d2}")),
        0xE0 => ("bend", ch, format!("bend {:+}", ((d2 as i32) << 7 | d1 as i32) - 8192)),
        0xD0 => ("pressure", ch, format!("pressure {d1}")),
        0xA0 => ("poly-pressure", ch, format!("{} pressure {d2}", note_name(d1))),
        0xC0 => ("program", ch, format!("program {d1}")),
        _ => ("other", ch, format!("{b:02X?}")),
    }
}

fn compile_all(routes: &[Route], ports: &PortNames, audition: u8) -> RouteTable {
    let mut t = compile_routes(routes, ports);
    if audition >= 1 {
        t.push(CompiledRoute { port_mask: 1 << PORT_AUDITION, slot: audition - 1, ..Default::default() });
    }
    t
}

impl App {
    pub fn new(paths: Paths, port: u16, backend: Arc<dyn AudioBackend>, links: RtLinks, stats: Arc<Stats>, shared: Arc<RtShared>) -> Result<Arc<App>, String> {
        std::fs::create_dir_all(&paths.config).map_err(|e| e.to_string())?;
        let token = paths::ensure_token(&paths.token()).map_err(|e| format!("token: {e}"))?;
        let store = SceneStore::new(paths.scenes()).map_err(|e| e.to_string())?;
        if !store.exists("default") {
            store.save(&Scene::factory(), "default")?;
        }
        let settings = match std::fs::read_to_string(paths.settings()) {
            Ok(s) => Settings::from_toml(&s).unwrap_or_else(|e| {
                eprintln!("settings.toml invalid ({e}); using defaults");
                Settings::default()
            }),
            Err(_) => {
                let s = Settings::default();
                let _ = std::fs::write(paths.settings(), s.to_toml().unwrap_or_default());
                s
            }
        };
        let persist: Persist = std::fs::read_to_string(paths.state()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();
        let active = if persist.active_scene.is_empty() { "default".to_string() } else { persist.active_scene };
        let scene = store.load(&active).or_else(|_| store.load("default")).unwrap_or_else(|_| Scene::factory());
        let mut ports: PortNames = Default::default();
        ports[PORT_VIRTUAL as usize] = Some("virtual".into());
        ports[PORT_CONSOLE as usize] = Some("console".into());
        ports[PORT_AUDITION as usize] = Some("audition".into());
        let (tx, _) = broadcast::channel(64);
        let sr = backend.sample_rate() as u32;
        let app = Arc::new(App {
            device_id: paths::device_id(),
            hostname: paths::hostname(),
            library: Library::new(paths.akwf_root()),
            paths,
            port,
            backend,
            stats,
            recorder: Recorder::new(shared.clone(), sr),
            shared,
            store,
            token: RwLock::new(token),
            tx,
            started: Instant::now(),
            cmd: Mutex::new(links.cmd_tx),
            gc_rx: Mutex::new(links.gc_rx),
            mon_rx: Mutex::new(links.mon_rx),
            rec_rx: Mutex::new(links.rec_rx),
            inner: Mutex::new(Inner {
                scene,
                dirty: false,
                settings,
                ports,
                events: VecDeque::new(),
                midi_pending: vec![],
                midi_counts: [0; MAX_PORTS],
                services: vec![],
                listen: vec![],
                usb: vec![],
                sinks: vec![],
                cores: vec![],
                temp: None,
                mem: 0.0,
                load: String::new(),
                unfiled: 0,
                oldest_unfiled: None,
                wd_last: None,
                wd_ticks: 0,
                xr_prev: 0,
                xr_rate: 0.0,
                audition: 0,
            }),
        });
        Ok(app)
    }

    pub fn send(&self, c: Cmd) -> Result<(), String> {
        self.cmd.lock().unwrap().push(c).map_err(|_| "command queue full".to_string())
    }

    pub fn event(&self, level: &'static str, text: impl Into<String>) {
        let text = text.into();
        eprintln!("[{level}] {text}");
        let now = timefmt::now_secs();
        let mut g = self.inner.lock().unwrap();
        g.events.push_front(Event { t: now, time: timefmt::hms(now), level, text });
        g.events.truncate(50);
    }

    pub fn settings(&self) -> Settings {
        self.inner.lock().unwrap().settings.clone()
    }

    pub fn scene(&self) -> (Scene, bool) {
        let g = self.inner.lock().unwrap();
        (g.scene.clone(), g.dirty)
    }

    pub fn recordings_dir(&self) -> PathBuf {
        paths::expand(&self.settings().recording.location)
    }

    fn persist_active(&self, name: &str) {
        let p = Persist { active_scene: name.to_string() };
        let _ = std::fs::write(self.paths.state(), toml::to_string(&p).unwrap_or_default());
    }

    /// Pushes a whole scene into the engine. `startup` caps the master at -18 dBFS.
    pub fn apply_scene(&self, sc: &Scene, startup: bool) -> Result<(), String> {
        sc.validate()?;
        let (ports, audition) = {
            let g = self.inner.lock().unwrap();
            (g.ports.clone(), g.audition)
        };
        for i in 0..MAX_SLOTS {
            let id = (i + 1) as u8;
            match sc.slot(id) {
                Some(s) => {
                    self.send(Cmd::SetSlot(i as u8, s.sound))?;
                    let table = if s.sound.wave == Waveform::Table && !s.table.is_empty() {
                        match self.library.load(&s.table) {
                            Ok(t) => Some(t),
                            Err(e) => {
                                self.event("warn", format!("slot {id} \"{}\": wavetable not loaded ({e}); fetch AKWF with scripts/fetch-akwf.sh", s.name));
                                None
                            }
                        }
                    } else {
                        None
                    };
                    self.send(Cmd::SetTable(i as u8, table))?;
                }
                None => self.send(Cmd::SetSlot(i as u8, SlotParams { enabled: false, ..SlotParams::default() }))?,
            }
        }
        self.send(Cmd::SetRoutes(Box::new(compile_all(&sc.routes, &ports, audition))))?;
        self.send(Cmd::SetTuning { a4_hz: sc.a4_hz, bend_range: sc.bend_range })?;
        let db = if startup { sc.master_db.min(pilab_synth::engine::START_MASTER_DB) } else { sc.master_db };
        self.send(Cmd::SetMaster { db })?;
        Ok(())
    }

    pub fn replace_scene(&self, sc: Scene) -> Result<(), String> {
        self.apply_scene(&sc, false)?;
        let mut g = self.inner.lock().unwrap();
        g.scene = sc;
        g.dirty = true;
        Ok(())
    }

    pub fn edit_scene(&self, f: impl FnOnce(&mut Scene) -> Result<(), String>) -> Result<Scene, String> {
        let mut sc = self.scene().0;
        f(&mut sc)?;
        sc.validate()?;
        self.replace_scene(sc.clone())?;
        Ok(sc)
    }

    pub fn upsert_slot(&self, slot: Slot) -> Result<Scene, String> {
        self.edit_scene(|sc| {
            match sc.slots.iter_mut().find(|s| s.id == slot.id) {
                Some(s) => *s = slot,
                None => {
                    sc.slots.push(slot);
                    sc.slots.sort_by_key(|s| s.id);
                }
            }
            Ok(())
        })
    }

    pub fn delete_slot(&self, id: u8) -> Result<Scene, String> {
        self.edit_scene(|sc| {
            sc.slots.retain(|s| s.id != id);
            sc.routes.retain(|r| r.slot != id);
            Ok(())
        })
    }

    pub fn set_routes(&self, routes: Vec<Route>) -> Result<Scene, String> {
        self.edit_scene(|sc| {
            sc.routes = routes;
            Ok(())
        })
    }

    pub fn load_scene(&self, name: &str) -> Result<Scene, String> {
        let sc = self.store.load(name)?;
        // Loading a scene never raises the level above what is playing now.
        let cur = gain_to_db(self.stats_master_gain());
        let mut applied = sc.clone();
        applied.master_db = sc.master_db.min(cur.max(pilab_synth::engine::START_MASTER_DB));
        self.apply_scene(&applied, false)?;
        {
            let mut g = self.inner.lock().unwrap();
            g.scene = applied.clone();
            g.dirty = applied != sc;
        }
        self.persist_active(name);
        self.event("info", format!("scene \"{name}\" loaded"));
        Ok(applied)
    }

    fn stats_master_gain(&self) -> f32 {
        pilab_synth::tuning::db_to_gain(Stats::f32(&self.stats.master_db))
    }

    pub fn save_scene(&self, name: &str) -> Result<(), String> {
        let mut sc = self.scene().0;
        sc.name = name.to_string();
        self.store.save(&sc, name)?;
        let mut g = self.inner.lock().unwrap();
        g.scene.name = name.to_string();
        g.dirty = false;
        drop(g);
        self.persist_active(name);
        self.event("info", format!("scene \"{name}\" saved"));
        Ok(())
    }

    pub fn set_master(&self, db: f32) -> Result<f32, String> {
        let db = db.clamp(-80.0, pilab_synth::engine::MAX_MASTER_DB);
        self.send(Cmd::SetMaster { db })?;
        let mut g = self.inner.lock().unwrap();
        g.scene.master_db = db;
        g.dirty = true;
        Ok(db)
    }

    pub fn put_settings(&self, mut s: Settings) -> Result<Settings, String> {
        let mut g = self.inner.lock().unwrap();
        // "configured" is set only once the NAS account has been verified, not from the UI.
        s.archive.configured = g.settings.archive.configured;
        if s.recording.location.trim().is_empty() {
            return Err("recording location is empty".into());
        }
        let was = g.settings.clone();
        g.settings = s.clone();
        drop(g);
        std::fs::write(self.paths.settings(), s.to_toml()?).map_err(|e| e.to_string())?;
        if was.archive.enabled != s.archive.enabled {
            let (_, text) = s.archive_status();
            self.event(if s.archive.enabled { "warn" } else { "info" }, format!("archival switched {}: {text}", if s.archive.enabled { "on" } else { "off" }));
        }
        if was.recording.enabled != s.recording.enabled {
            self.event("info", format!("local recording {}", if s.recording.enabled { "allowed" } else { "disabled" }));
        }
        if was.midi != s.midi {
            self.refresh_midi_inputs(true);
        }
        Ok(s)
    }

    fn recompile(&self) {
        let (routes, ports, audition) = {
            let g = self.inner.lock().unwrap();
            (g.scene.routes.clone(), g.ports.clone(), g.audition)
        };
        let _ = self.send(Cmd::SetRoutes(Box::new(compile_all(&routes, &ports, audition))));
    }

    /// Selects the slot the "Selected sound" keyboard mode plays (0 = none).
    pub fn set_audition(&self, slot: u8) {
        self.inner.lock().unwrap().audition = slot.min(MAX_SLOTS as u8);
        self.recompile();
    }

    pub fn console_midi_on(&self, port: u8, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() || bytes.len() > 3 || bytes[0] < 0x80 || bytes[0] >= 0xF0 {
            return Err("one MIDI channel message (1-3 bytes) expected".into());
        }
        let mut b = [0u8; 3];
        b[..bytes.len()].copy_from_slice(bytes);
        let port = if port == PORT_AUDITION { PORT_AUDITION } else { PORT_CONSOLE };
        self.send(Cmd::Midi { port, len: bytes.len() as u8, bytes: b })?;
        self.log_midi(MidiEv { port, len: bytes.len() as u8, b });
        Ok(())
    }

    fn log_midi(&self, ev: MidiEv) {
        let bytes = ev.b[..ev.len as usize].to_vec();
        let (kind, ch, text) = describe_midi(&bytes);
        let mut g = self.inner.lock().unwrap();
        let source = g.ports.get(ev.port as usize).cloned().flatten().map(|s| short_source(&s)).unwrap_or_default();
        if (ev.port as usize) < MAX_PORTS {
            g.midi_counts[ev.port as usize] += 1;
        }
        if g.midi_pending.len() < 256 {
            g.midi_pending.push(MidiLog { t: self.started.elapsed().as_secs_f64(), port: ev.port, source, kind, ch, text, bytes });
        }
    }

    pub fn engine_run(&self, on: bool) -> Result<(), String> {
        self.send(Cmd::Run(on))?;
        self.event("info", if on { "synth started (fade-in)" } else { "synth stopped (fade-out)" });
        Ok(())
    }

    pub fn record(&self, on: bool) -> Result<Value, String> {
        if on {
            let s = self.settings();
            if !s.recording.enabled {
                return Err("local recording is switched off in Settings".into());
            }
            let dir = paths::expand(&s.recording.location);
            let free = sys::free_mb(&dir).unwrap_or(0);
            if free < s.recording.min_free_mb {
                return Err(format!("only {free} MB free at {} (minimum {} MB)", dir.display(), s.recording.min_free_mb));
            }
            let id = self.recorder.start(&dir, &self.scene().0.name)?;
            self.event("info", format!("recording {id} started in {}", dir.display()));
            Ok(json!({ "recording": true, "id": id }))
        } else {
            let card = self.recorder.stop(&mut self.rec_rx.lock().unwrap())?;
            self.event("info", format!("recording {} saved ({:.1} s); not filed to the NAS yet", card.id, card.duration_s));
            self.scan_unfiled();
            Ok(json!({ "recording": false, "card": card }))
        }
    }

    fn scan_unfiled(&self) {
        let cards = record::list(&self.recordings_dir());
        let unf: Vec<&record::RecordCard> = cards.iter().filter(|c| !c.filed).collect();
        let mut g = self.inner.lock().unwrap();
        g.unfiled = unf.len();
        g.oldest_unfiled = unf.iter().map(|c| c.created_unix).min();
    }

    /// Server-rendered preview of one slot's oscillator shape (one cycle at 110 Hz).
    pub fn slot_preview(&self, id: u8, points: usize) -> Vec<f32> {
        let sc = self.scene().0;
        let Some(slot) = sc.slot(id) else { return vec![] };
        let bands = Bands::shared();
        let dt = 110.0 / self.backend.sample_rate();
        let bl = bands.saw.level_for_dt(dt);
        let table = if slot.sound.wave == Waveform::Table && !slot.table.is_empty() { self.library.load(&slot.table).ok() } else { None };
        let lv = table.as_ref().map(|t| t.level_for_dt(dt)).unwrap_or(0);
        (0..points)
            .map(|i| Osc::shape_table(slot.sound.wave, i as f64 / points as f64, slot.sound.pulse_width, &bands, bl, table.as_deref(), lv))
            .collect()
    }

    /// Assigns MIDI sources in the graph to input slots 1..=7, connects them, and
    /// recompiles routes when the assignment changes.
    pub fn refresh_midi_inputs(&self, force: bool) {
        let settings = self.settings();
        let sources: Vec<String> = self
            .backend
            .midi_sources()
            .into_iter()
            .filter(|s| !settings.midi.ignore.iter().any(|g| glob::matches(g, s)))
            .collect();
        let mut changed = force;
        let mut connects = vec![];
        let mut msgs = vec![];
        {
            let mut g = self.inner.lock().unwrap();
            for k in 1..8 {
                if let Some(n) = g.ports[k].clone() {
                    if !sources.contains(&n) {
                        g.ports[k] = None;
                        changed = true;
                        msgs.push(format!("MIDI source gone: {}", short_source(&n)));
                    }
                }
            }
            if settings.midi.autoconnect {
                for s in &sources {
                    if g.ports[1..8].iter().any(|p| p.as_deref() == Some(s.as_str())) {
                        continue;
                    }
                    if let Some(k) = (1..8).find(|&k| g.ports[k].is_none()) {
                        g.ports[k] = Some(s.clone());
                        changed = true;
                        connects.push((s.clone(), k));
                        msgs.push(format!("MIDI source connected: {} -> input {k}", short_source(s)));
                    }
                }
            }
        }
        for (s, k) in connects {
            if let Some(dst) = self.backend.input_port(k) {
                if let Err(e) = self.backend.connect(&s, &dst) {
                    if !e.contains("Exist") {
                        msgs.push(format!("could not connect {s}: {e}"));
                    }
                }
            }
        }
        for m in msgs {
            self.event("info", m);
        }
        if changed {
            self.recompile();
        }
    }

    pub fn connect_outputs(&self) {
        let s = self.settings();
        let ours = self.backend.output_ports();
        for (i, dst) in s.audio.output_ports.iter().take(2).enumerate() {
            match self.backend.connect(&ours[i], dst) {
                Ok(()) => {}
                Err(e) if e.contains("Exist") => {}
                Err(e) => self.event("warn", format!("output {} -> {dst}: {e}", ours[i])),
            }
        }
    }

    pub fn midi_inputs_json(&self) -> Value {
        let g = self.inner.lock().unwrap();
        let mut v = vec![];
        for k in 0..MAX_PORTS {
            if let Some(n) = &g.ports[k] {
                v.push(json!({
                    "slot": k,
                    "source": n,
                    "label": short_source(n),
                    "port": self.backend.input_port(k),
                    "events": g.midi_counts[k],
                }));
            }
        }
        Value::Array(v)
    }

    pub fn status_json(&self) -> Value {
        let (pl, pr) = self.stats.take_peaks();
        let rec = self.recorder.state();
        let mut g = self.inner.lock().unwrap();
        let midi = std::mem::take(&mut g.midi_pending);
        json!({
            "t": "status",
            "voices": self.stats.voices.load(Ordering::Relaxed),
            "peak_db": [gain_to_db(pl), gain_to_db(pr)],
            "limiter_db": Stats::f32(&self.stats.limiter_db),
            "master_db": Stats::f32(&self.stats.master_db),
            "running": self.stats.running.load(Ordering::Relaxed),
            "xruns": self.shared.xruns.load(Ordering::Relaxed),
            "xrun_rate": g.xr_rate,
            "dsp_load": self.backend.cpu_load(),
            "rec": rec,
            "midi": midi,
            "dirty": g.dirty,
            "scene": g.scene.name,
        })
    }

    pub fn system_json(&self) -> Value {
        let s = self.settings();
        let (arch_state, arch_text) = s.archive_status();
        let g = self.inner.lock().unwrap();
        let dac = g.sinks.iter().find(|x| x.name == s.audio.dac_sink).cloned();
        let sr = self.backend.sample_rate();
        let buf = self.backend.buffer_size();
        let unfiled_age_h = g.oldest_unfiled.map(|t| (timefmt::now_secs() - t) as f64 / 3600.0);
        json!({
            "t": "sys",
            "hostname": self.hostname,
            "device_id": self.device_id,
            "addresses": sys::lan_addresses(),
            "port": self.port,
            "uptime_s": self.started.elapsed().as_secs(),
            "temp_c": g.temp,
            "cores": g.cores,
            "mem_pct": g.mem,
            "load": g.load,
            "dac": dac,
            "sinks": g.sinks,
            "backend": {
                "name": self.backend.name(),
                "sample_rate": sr,
                "buffer": buf,
                "latency_ms": 1000.0 * buf as f32 / sr.max(1.0),
                "pinned": self.shared.pinned.load(Ordering::Relaxed),
                "policy": match self.shared.policy.load(Ordering::Relaxed) { 1 => "SCHED_FIFO", 2 => "SCHED_RR", 0 => "SCHED_OTHER", _ => "unknown" },
                "rt_prio": self.shared.rt_prio.load(Ordering::Relaxed),
            },
            "services": g.services,
            "listening": g.listen,
            "usb": g.usb,
            "events": g.events.iter().take(30).collect::<Vec<_>>(),
            "recording": {
                "enabled": s.recording.enabled,
                "location": s.recording.location,
                "free_mb": sys::free_mb(&paths::expand(&s.recording.location)),
                "unfiled": g.unfiled,
                "oldest_unfiled_hours": unfiled_age_h,
                "warn": g.unfiled > 0 && unfiled_age_h.unwrap_or(0.0) >= s.recording.unfiled_warn_hours as f64,
            },
            "archive": { "state": arch_state, "text": arch_text, "enabled": s.archive.enabled, "target": s.archive.target, "account": s.archive.account },
            "watchdog": { "enabled": s.watchdog.enabled, "last_action": g.wd_last.map(timefmt::iso_local) },
            "library": { "akwf_root": self.library.root.display().to_string(), "available": self.library.available() },
        })
    }

    pub fn broadcast(&self, v: &Value) {
        let _ = self.tx.send(v.to_string());
    }
}

/// Background loops: realtime ring draining + status at 15 Hz; machine state every 2 s.
pub fn spawn_loops(app: Arc<App>) {
    let a = app.clone();
    std::thread::Builder::new()
        .name("status".into())
        .spawn(move || loop {
            {
                while let Ok(g) = a.gc_rx.lock().unwrap().pop() {
                    drop(g);
                }
                let evs: Vec<MidiEv> = {
                    let mut rx = a.mon_rx.lock().unwrap();
                    let mut v = vec![];
                    while let Ok(e) = rx.pop() {
                        v.push(e);
                    }
                    v
                };
                for e in evs {
                    a.log_midi(e);
                }
                if a.recorder.is_recording() {
                    a.recorder.drain(&mut a.rec_rx.lock().unwrap());
                } else {
                    // Not recording: discard anything left in the ring.
                    let mut rx = a.rec_rx.lock().unwrap();
                    let n = rx.slots();
                    if n > 0 {
                        if let Ok(c) = rx.read_chunk(n) {
                            c.commit_all();
                        }
                    }
                }
                if a.tx.receiver_count() > 0 {
                    let v = a.status_json();
                    a.broadcast(&v);
                } else {
                    a.inner.lock().unwrap().midi_pending.clear();
                    let _ = a.stats.take_peaks();
                }
            }
            std::thread::sleep(Duration::from_millis(66));
        })
        .expect("spawn status loop");

    std::thread::Builder::new()
        .name("system".into())
        .spawn(move || {
            let mut cpu = CpuSampler::default();
            let mut tick: u64 = 0;
            loop {
                let s = app.settings();
                // Token rotation by `pilabctl pair --rotate` is picked up here.
                if let Ok(t) = std::fs::read_to_string(app.paths.token()) {
                    let t = t.trim().to_string();
                    if t.len() >= 32 && *app.token.read().unwrap() != t {
                        *app.token.write().unwrap() = t;
                        app.event("info", "pairing token changed; paired browsers must re-enter it");
                    }
                }
                let cores = cpu.sample();
                let sinks = sys::sinks();
                let services = if tick % 3 == 0 { Some(sys::services(app.stats.running.load(Ordering::Relaxed))) } else { None };
                let listen = if tick % 5 == 0 { Some(sys::listening()) } else { None };
                let usb = if tick % 5 == 0 { Some(sys::usb_devices()) } else { None };
                {
                    let mut g = app.inner.lock().unwrap();
                    g.cores = cores;
                    g.temp = sys::temperature_c();
                    g.mem = sys::mem_used_pct();
                    g.load = sys::loadavg();
                    g.sinks = sinks;
                    if let Some(v) = services {
                        g.services = v;
                    }
                    if let Some(v) = listen {
                        g.listen = v;
                    }
                    if let Some(v) = usb {
                        g.usb = v;
                    }
                }
                app.refresh_midi_inputs(false);
                if tick % 15 == 0 {
                    app.scan_unfiled();
                }
                // Recording guard rails.
                if app.recorder.is_recording() {
                    let free = sys::free_mb(&paths::expand(&s.recording.location)).unwrap_or(0);
                    if free < s.recording.min_free_mb / 2 || app.recorder.too_big() {
                        let why = if app.recorder.too_big() { "file size limit".to_string() } else { format!("only {free} MB free") };
                        let _ = app.record(false);
                        app.recorder.set_error(format!("stopped automatically: {why}"));
                        app.event("warn", format!("recording stopped automatically: {why}"));
                    }
                }
                // Dropout watchdog.
                let xr = app.shared.xruns.load(Ordering::Relaxed);
                let fire = {
                    let mut g = app.inner.lock().unwrap();
                    let d = xr.saturating_sub(g.xr_prev);
                    g.xr_prev = xr;
                    g.xr_rate = d as f32 / 2.0;
                    if g.xr_rate >= s.watchdog.xruns_per_s {
                        g.wd_ticks += 1;
                    } else {
                        g.wd_ticks = 0;
                    }
                    let now = timefmt::now_secs();
                    let cool = g.wd_last.is_none_or(|t| now - t >= s.watchdog.cooldown_s as i64);
                    if s.watchdog.enabled && g.wd_ticks * 2 >= s.watchdog.hold_s && cool {
                        g.wd_last = Some(now);
                        g.wd_ticks = 0;
                        Some(g.xr_rate)
                    } else {
                        None
                    }
                };
                if let Some(rate) = fire {
                    app.event("warn", format!("watchdog: {rate:.0} dropouts/s; suspending and resuming {}", s.audio.dac_sink));
                    match sys::bounce_sink(&s.audio.dac_sink) {
                        Ok(()) => app.event("info", "watchdog: DAC sink resumed"),
                        Err(e) => app.event("error", format!("watchdog: {e}")),
                    }
                }
                if app.tx.receiver_count() > 0 {
                    let v = app.system_json();
                    app.broadcast(&v);
                }
                tick += 1;
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("spawn system loop");
}
