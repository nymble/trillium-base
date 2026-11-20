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
use crate::identity;
use pilab_core::{classify, compile_routes_with, glob, PortDevices, PortNames, Route, Scene, SceneStore, Settings, SourceIdentity};
use pilab_synth::osc::Osc;
use pilab_synth::tuning::gain_to_db;
use pilab_synth::{Bands, Cmd, Engine, PREVIEW_SLOT, CompiledRoute, Garbage, RouteTable, SlotParams, Stats, Waveform, MAX_PORTS, MAX_SLOTS, PORT_AUDITION, PORT_CONSOLE, PORT_VIRTUAL};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::audio::{AudioBackend, MidiEv, RtLinks, RtShared};
use crate::library::Library;
use crate::samples::SampleLib;
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
    /// Device class id matched per input slot (routes `device:<id>`).
    devices: PortDevices,
    /// Identity, matched class label and rule for each source port seen.
    idents: std::collections::HashMap<String, (SourceIdentity, Option<(String, &'static str)>)>,
    /// Sources left unconnected by a device class (port, reason).
    skipped: Vec<(String, String)>,
    ble_cache: Option<(std::time::Instant, Vec<String>)>,
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
    /// Bank id last sent to the engine per slot (sampler instruments).
    sent_inst: [String; MAX_SLOTS + 1],
}

/// What an audition plays: a scene slot, or a library item in the hidden preview slot.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditionSrc {
    Slot(u8),
    Bank(String),
    Table(String),
    Preset(String),
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
    pub samples: SampleLib,
    aud_gen: std::sync::atomic::AtomicU64,
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

fn is_pad(p: &SlotParams) -> bool {
    p.amp_env.a >= 0.25 || p.amp_env.r >= 1.2
}

/// Audition phrase as (ms, MIDI bytes): drums play kick, hat, snare, hat on channel 10;
/// pads hold C4; everything else plays C3, C4, G4. About 1.5 s.
pub fn phrase(p: &SlotParams, drum: bool) -> Vec<(u64, [u8; 3])> {
    let on = |ch: u8, n: u8| [0x90 | (ch - 1), n, 96];
    let off = |ch: u8, n: u8| [0x80 | (ch - 1), n, 0];
    if drum {
        let mut v = vec![];
        for (i, n) in [36u8, 42, 38, 42, 36, 42].iter().enumerate() {
            let t = i as u64 * 250;
            v.push((t, on(10, *n)));
            v.push((t + 120, off(10, *n)));
        }
        v.sort_by_key(|s| s.0);
        v
    } else if is_pad(p) {
        vec![(0, on(1, 60)), (1500, off(1, 60))]
    } else {
        vec![(0, on(1, 48)), (350, on(1, 60)), (700, on(1, 67)), (1500, off(1, 48)), (1500, off(1, 60)), (1500, off(1, 67))]
    }
}

pub fn wav16(x: &[f32], sr: u32) -> Vec<u8> {
    let mut b = Vec::with_capacity(44 + x.len() * 2);
    let n = (x.len() * 2) as u32;
    b.extend(b"RIFF");
    b.extend((36 + n).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(sr.to_le_bytes());
    b.extend((sr * 2).to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(n.to_le_bytes());
    for v in x {
        b.extend(((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    b
}

fn compile_all(routes: &[Route], ports: &PortNames, devices: &PortDevices, audition: u8) -> RouteTable {
    let mut t = compile_routes_with(routes, ports, devices);
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
        if !store.exists("samples") {
            store.save(&Scene::samples_demo(), "samples")?;
        }
        if !store.exists("trillium split") {
            store.save(&Scene::trillium_sample(), "trillium split")?;
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
            samples: SampleLib::new(paths.samples_root()),
            aud_gen: std::sync::atomic::AtomicU64::new(0),
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
                devices: Default::default(),
                idents: Default::default(),
                skipped: vec![],
                ble_cache: None,
                sent_inst: Default::default(),
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
        let (ports, devices, audition) = {
            let g = self.inner.lock().unwrap();
            (g.ports.clone(), g.devices.clone(), g.audition)
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
                    let want = if s.sound.wave == Waveform::Sampler { s.sample.clone() } else { String::new() };
                    self.set_instrument(i as u8, &want, id, &s.name)?;
                }
                None => {
                    self.send(Cmd::SetSlot(i as u8, SlotParams { enabled: false, ..SlotParams::default() }))?;
                    self.set_instrument(i as u8, "", id, "")?;
                }
            }
        }
        self.send(Cmd::SetRoutes(Box::new(compile_all(&sc.routes, &ports, &devices, audition))))?;
        self.send(Cmd::SetTuning { a4_hz: sc.a4_hz, bend_range: sc.bend_range })?;
        let db = if startup { sc.master_db.min(pilab_synth::engine::START_MASTER_DB) } else { sc.master_db };
        self.send(Cmd::SetMaster { db })?;
        Ok(())
    }

    /// Sends a sampler instrument to engine slot `i` when the bank changed.
    fn set_instrument(&self, i: u8, bank: &str, id: u8, name: &str) -> Result<(), String> {
        if self.inner.lock().unwrap().sent_inst[i as usize] == bank {
            return Ok(());
        }
        let inst = if bank.is_empty() {
            None
        } else {
            match self.samples.load(bank) {
                Ok(t) => Some(t),
                Err(e) => {
                    self.event("warn", format!("slot {id} \"{name}\": samples not loaded ({e})"));
                    None
                }
            }
        };
        let key = if inst.is_some() { bank.to_string() } else { String::new() };
        self.send(Cmd::SetInstrument(i, inst))?;
        self.inner.lock().unwrap().sent_inst[i as usize] = key;
        Ok(())
    }

    /// Resolves an audition source to slot params plus its table or instrument.
    fn audition_sound(&self, src: &AuditionSrc) -> Result<(String, SlotParams, Option<Arc<pilab_synth::Wavetable>>, Option<Arc<pilab_synth::Instrument>>, bool), String> {
        match src {
            AuditionSrc::Slot(id) => {
                let sc = self.scene().0;
                let s = sc.slot(*id).ok_or("no such slot")?.clone();
                let table = if s.sound.wave == Waveform::Table && !s.table.is_empty() { self.library.load(&s.table).ok() } else { None };
                let (inst, drum) = if s.sound.wave == Waveform::Sampler && !s.sample.is_empty() {
                    (Some(self.samples.load(&s.sample)?), self.samples.bank(&s.sample).is_some_and(|b| b.drum))
                } else {
                    (None, false)
                };
                Ok((format!("slot {id} \"{}\"", s.name), s.sound, table, inst, drum))
            }
            AuditionSrc::Bank(b) => {
                let bank = self.samples.bank(b).ok_or("bank not installed")?;
                let p = pilab_core::presets::for_bank(&bank.category);
                Ok((bank.title.clone(), p, None, Some(self.samples.load(b)?), bank.drum))
            }
            AuditionSrc::Preset(id) => {
                let p = pilab_core::presets::find(id).ok_or("no such preset")?;
                let inst = if p.bank.is_empty() { None } else { Some(self.samples.load(p.bank)?) };
                Ok((p.name.to_string(), p.sound, None, inst, p.drum))
            }
            AuditionSrc::Table(path) => {
                let t = self.library.load(path)?;
                let p = SlotParams { wave: Waveform::Table, ..SlotParams::default() };
                Ok((path.rsplit('/').next().unwrap_or(path).to_string(), p, Some(t), None, false))
            }
        }
    }

    /// Loads the bank or table behind `src` (slow the first time) so play starts promptly.
    pub fn audition_sound_preload(&self, src: &AuditionSrc) -> Result<(), String> {
        self.audition_sound(src).map(|_| ())
    }

    /// Makes slot `id` a sampler with a preset or a bare bank. Drum sounds get a channel-10
    /// route ("*" input) when no route plays the slot yet.
    pub fn apply_preset(&self, id: u8, preset: Option<&str>, bank: Option<&str>, add_route: bool) -> Result<Scene, String> {
        let (mut slot, drum) = if let Some(p) = preset {
            let p = pilab_core::presets::find(p).ok_or("no such preset")?;
            (p.slot(id), p.drum)
        } else if let Some(b) = bank {
            let bk = self.samples.bank(b).ok_or("bank not installed")?;
            let mut sound = pilab_core::presets::for_bank(&bk.category);
            if let Ok(inst) = self.samples.load(b) {
                if let Some(r) = inst.env.release {
                    sound.amp_env.r = r.clamp(0.02, 4.0);
                }
                if let Some(a) = inst.env.attack {
                    sound.amp_env.a = a.clamp(0.001, 4.0);
                }
            }
            (Slot { id, name: bk.title.clone(), table: String::new(), sample: b.to_string(), sound }, bk.drum)
        } else {
            return Err("preset or bank required".into());
        };
        if !slot.sample.is_empty() {
            self.samples.load(&slot.sample)?;
        }
        let sc = self.scene().0;
        if let Some(old) = sc.slot(id) {
            slot.sound.octave = 0;
            slot.sound.pan = old.sound.pan;
        }
        self.edit_scene(|sc| {
            match sc.slots.iter_mut().find(|s| s.id == id) {
                Some(s) => *s = slot,
                None => {
                    sc.slots.push(slot);
                    sc.slots.sort_by_key(|s| s.id);
                }
            }
            if drum && add_route && !sc.routes.iter().any(|r| r.slot == id) {
                sc.routes.push(Route { label: "Drums (GM ch 10)".into(), input: "*".into(), channel: 10, slot: id, ..Route::default() });
            }
            Ok(())
        })
    }

    /// Plays a short phrase of `src` on the DAC through the audition port, at the current
    /// master level (never louder) and through the same output stage.
    pub fn audition_play(self: &Arc<Self>, src: AuditionSrc) -> Result<Value, String> {
        let (label, sound, table, inst, drum) = self.audition_sound(&src)?;
        match &src {
            AuditionSrc::Slot(id) => self.set_audition(*id),
            _ => {
                let p = PREVIEW_SLOT;
                self.send(Cmd::SetSlot(p, SlotParams { enabled: true, ..sound }))?;
                self.send(Cmd::SetTable(p, table))?;
                self.send(Cmd::SetInstrument(p, inst))?;
                self.inner.lock().unwrap().audition = p + 1;
                self.recompile();
            }
        }
        let steps = phrase(&sound, drum);
        let gen = self.aud_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let app = self.clone();
        tokio::spawn(async move {
            let t0 = tokio::time::Instant::now();
            let mut held: Vec<[u8; 3]> = vec![];
            for (ms, b) in steps {
                tokio::time::sleep_until(t0 + Duration::from_millis(ms)).await;
                if app.aud_gen.load(Ordering::SeqCst) != gen {
                    break;
                }
                let _ = app.send(Cmd::Midi { port: PORT_AUDITION, len: 3, bytes: b });
                if b[0] & 0xF0 == 0x90 && b[2] > 0 {
                    held.push(b);
                } else {
                    held.retain(|h| !(h[0] & 0x0F == b[0] & 0x0F && h[1] == b[1]));
                }
            }
            for h in held {
                let _ = app.send(Cmd::Midi { port: PORT_AUDITION, len: 3, bytes: [0x80 | (h[0] & 0x0F), h[1], 0] });
            }
        });
        let s = self.settings();
        Ok(json!({
            "output": "dac",
            "label": "DAC (Pi headphone / line out)",
            "sink": s.audio.dac_sink,
            "playing": label,
            "master_db": Stats::f32(&self.stats.master_db),
            "phrase": if drum { "kick, hat, snare, hat (ch 10)" } else if is_pad(&sound) { "held C4" } else { "C3, C4, G4" },
        }))
    }

    /// Renders the audition phrase offline to a mono 16-bit WAV at the live master level.
    pub fn audition_render(&self, src: AuditionSrc) -> Result<Vec<u8>, String> {
        let (_, sound, table, inst, drum) = self.audition_sound(&src)?;
        let sr = 48_000u32;
        let mut e = Engine::new(sr as f32);
        e.apply(Cmd::SetSlot(0, SlotParams { enabled: true, ..sound }));
        e.apply(Cmd::SetTable(0, table));
        e.apply(Cmd::SetInstrument(0, inst));
        let mut rt = RouteTable::new();
        rt.push(CompiledRoute { port_mask: u32::MAX, slot: 0, ..Default::default() });
        e.apply(Cmd::SetRoutes(Box::new(rt)));
        let live = Stats::f32(&self.stats.master_db);
        let db = live.min(self.scene().0.master_db).min(pilab_synth::engine::MAX_MASTER_DB);
        e.apply(Cmd::SetMaster { db });
        e.apply(Cmd::Run(true));
        let steps = phrase(&sound, drum);
        let total = (steps.last().map(|s| s.0).unwrap_or(0) as f32 / 1000.0 + sound.amp_env.r.min(1.5) + 0.2) * sr as f32;
        let total = total as usize;
        let mut out = Vec::with_capacity(total);
        let (mut l, mut r) = (vec![0f32; 256], vec![0f32; 256]);
        let mut next = 0;
        while out.len() < total {
            let now_ms = out.len() as u64 * 1000 / sr as u64;
            while next < steps.len() && steps[next].0 <= now_ms {
                e.midi(0, &steps[next].1);
                next += 1;
            }
            e.process(&mut l, &mut r);
            out.extend(l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)));
        }
        Ok(wav16(&out, sr))
    }

    /// Re-reads service state from systemd now (after a switch).
    pub fn refresh_services(&self) {
        let v = sys::services(self.stats.running.load(Ordering::Relaxed));
        self.inner.lock().unwrap().services = v;
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
        let (routes, ports, devices, audition) = {
            let g = self.inner.lock().unwrap();
            (g.scene.routes.clone(), g.ports.clone(), g.devices.clone(), g.audition)
        };
        let _ = self.send(Cmd::SetRoutes(Box::new(compile_all(&routes, &ports, &devices, audition))));
    }

    /// Selects the slot the "Selected sound" keyboard mode plays (0 = none).
    pub fn set_audition(&self, slot: u8) {
        self.inner.lock().unwrap().audition = slot.min(MAX_SLOTS as u8);
        self.aud_gen.fetch_add(1, Ordering::SeqCst);
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
        if slot.sound.wave == Waveform::Sampler {
            return self.samples.load(&slot.sample).map(|i| i.preview(60, points)).unwrap_or_else(|_| vec![0.0; points]);
        }
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
        let raw: Vec<String> = self
            .backend
            .midi_sources()
            .into_iter()
            .filter(|s| !settings.midi.ignore.iter().any(|g| glob::matches(g, s)))
            .collect();
        // BLE peers can connect without the graph changing, so poll BlueZ (at most every 6 s).
        let aliases = if raw.iter().any(|s| identity::is_ble(s)) {
            let mut g = self.inner.lock().unwrap();
            match &g.ble_cache {
                Some((t, v)) if t.elapsed().as_secs() < 6 => v.clone(),
                _ => {
                    drop(g);
                    let v = identity::ble_connected_aliases();
                    g = self.inner.lock().unwrap();
                    g.ble_cache = Some((std::time::Instant::now(), v.clone()));
                    v
                }
            }
        } else {
            vec![]
        };
        let alsa = identity::read_alsa();
        let classes = &settings.midi.devices;
        let mut sources = vec![];
        let mut skipped = vec![];
        let mut idents = std::collections::HashMap::new();
        let mut class_of = std::collections::HashMap::new();
        for id in identity::resolve(&raw, &alsa, &aliases) {
            let m = classify(classes, &id);
            let tag = m.as_ref().map(|m| (classes[m.class].label.clone(), m.rule));
            match &m {
                Some(m) if !m.routable => skipped.push((id.port.clone(), m.reason.clone())),
                _ => {
                    sources.push(id.port.clone());
                    class_of.insert(id.port.clone(), m.as_ref().map(|m| classes[m.class].id.clone()));
                }
            }
            idents.insert(id.port.clone(), (id, tag));
        }
        let mut disconnects = vec![];
        let mut notes = vec![];
        {
            let mut g = self.inner.lock().unwrap();
            for (p, why) in &skipped {
                if !g.skipped.iter().any(|(q, _)| q == p) {
                    notes.push(format!("MIDI source not connected: {} ({why})", short_source(p)));
                }
            }
            g.skipped = skipped.clone();
            g.idents = idents;
            // Drop slots whose source is still present but no longer routable.
            for k in 1..8 {
                if let Some(n) = g.ports[k].clone() {
                    if skipped.iter().any(|(p, _)| *p == n) {
                        disconnects.push((n, k));
                    }
                }
            }
        }
        for m in notes {
            self.event("info", m);
        }
        for (n, k) in &disconnects {
            if let Some(dst) = self.backend.input_port(*k) {
                let _ = self.backend.disconnect(n, &dst);
            }
        }
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
                        let dev = match class_of.get(s).cloned().flatten() {
                            Some(d) => format!(" [device:{d}]"),
                            None => String::new(),
                        };
                        msgs.push(format!("MIDI source connected: {} -> input {k}{dev}", short_source(s)));
                    }
                }
            }
        }
        {
            let mut g = self.inner.lock().unwrap();
            let mut devs: PortDevices = Default::default();
            for k in 1..8 {
                devs[k] = g.ports[k].as_ref().and_then(|n| class_of.get(n).cloned().flatten());
            }
            if devs != g.devices {
                g.devices = devs;
                changed = true;
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

    pub fn device_classes_json(&self) -> Value {
        let g = self.inner.lock().unwrap();
        Value::Array(g.settings.midi.devices.iter().map(|c| json!({ "id": c.id, "label": c.label, "note": c.note })).collect())
    }

    pub fn midi_inputs_json(&self) -> Value {
        let g = self.inner.lock().unwrap();
        let mut v = vec![];
        for k in 0..MAX_PORTS {
            if let Some(n) = &g.ports[k] {
                let (identity, device, rule) = match g.idents.get(n) {
                    Some((id, tag)) => (id.describe(), tag.as_ref().map(|t| t.0.clone()), tag.as_ref().map(|t| t.1)),
                    None => (String::new(), None, None),
                };
                v.push(json!({
                    "slot": k,
                    "source": n,
                    "label": short_source(n),
                    "port": self.backend.input_port(k),
                    "events": g.midi_counts[k],
                    "identity": identity,
                    "device": device,
                    "device_id": g.devices[k],
                    "rule": rule,
                }));
            }
        }
        for (p, why) in &g.skipped {
            let (identity, device) = match g.idents.get(p) {
                Some((id, tag)) => (id.describe(), tag.as_ref().map(|t| t.0.clone())),
                None => (String::new(), None),
            };
            v.push(json!({ "slot": null, "source": p, "label": short_source(p), "identity": identity, "device": device, "skipped": why, "events": 0 }));
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
