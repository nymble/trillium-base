// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Service state: the live scene, settings, MIDI input assignment, machine snapshot,
//! event log and the background loops that keep them current.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
    #[serde(skip)]
    pub t_ns: u64,
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
    /// Links the input strip made itself (source port, our port), so only those are undone.
    input_links: Vec<(String, String)>,
    /// Which output port each meter input is linked to (client label, port name).
    meter_map: Vec<(String, String)>,
    meter_poll: Option<Instant>,
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
    /// Wavetable group last sent per slot.
    sent_set: [String; MAX_SLOTS + 1],
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

struct AudSound {
    label: String,
    sound: SlotParams,
    table: Option<Arc<pilab_synth::Wavetable>>,
    set: Option<Arc<pilab_synth::TableSet>>,
    inst: Option<Arc<pilab_synth::Instrument>>,
    drum: bool,
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
    pub mixer: crate::mixer::Mixer,
    pub devmgr: crate::devices::Manager,
    pub zeromic: Mutex<Option<crate::zeromic::Bridge>>,
    /// Held true while ACTION 7 is in flight so sync_zero_mic cannot drop the just-opened port.
    mic_hold: AtomicBool,
    /// Set for the duration of `measure_strike` so `log_midi` keeps note-ons/offs.
    measuring: AtomicBool,
    measure_notes: Mutex<Vec<MidiLog>>,
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
                let kept = paths::keep_bad_copy(&paths.settings());
                eprintln!("settings.toml invalid ({e}); using defaults{}", kept.map(|k| format!(", the broken file is kept as {}", k.display())).unwrap_or_default());
                Settings::default()
            }),
            Err(_) => {
                let s = Settings::default();
                let _ = paths::write_atomic(&paths.settings(), s.to_toml().unwrap_or_default().as_bytes());
                s
            }
        };
        let mut settings = settings;
        // Rename: patterns that named the old synth node now name "Trillium Synth".
        for p in settings.midi.ignore.iter_mut().chain(settings.audio.output_ports.iter_mut()) {
            *p = crate::audio::canon_port(p);
        }
        let persist: Persist = std::fs::read_to_string(paths.state()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();
        let active = if persist.active_scene.is_empty() { "default".to_string() } else { persist.active_scene };
        let scene = store.load(&active).or_else(|_| store.load("default")).unwrap_or_else(|_| Scene::factory());
        let mut ports: PortNames = Default::default();
        ports[PORT_VIRTUAL as usize] = Some("virtual".into());
        ports[PORT_CONSOLE as usize] = Some("console".into());
        ports[PORT_AUDITION as usize] = Some("audition".into());
        let (tx, _) = broadcast::channel(64);
        let sr = backend.sample_rate() as u32;
        let devmgr = crate::devices::Manager::new(&paths.config);
        let app = Arc::new(App {
            device_id: paths::device_id(),
            hostname: paths::hostname(),
            library: Library::new(paths.akwf_root()),
            samples: SampleLib::new(paths.samples_root()),
            mixer: crate::mixer::Mixer::new(),
            devmgr,
            zeromic: Mutex::new(None),
            mic_hold: AtomicBool::new(false),
            measuring: AtomicBool::new(false),
            measure_notes: Mutex::new(Vec::new()),
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
                meter_map: Vec::new(),
                meter_poll: None,
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
                sent_set: Default::default(),
                input_links: vec![],
            }),
        });
        app.apply_input_rt(&app.settings().input);
        Ok(app)
    }

    /// Pushes the input strip settings to the realtime side.
    fn apply_input_rt(&self, i: &pilab_core::settings::Input) {
        let g = if i.gain_db <= -60.0 { 0.0 } else { 10f32.powf(i.gain_db.clamp(-60.0, 24.0) / 20.0) };
        self.shared.in_gain.store(g.to_bits(), Ordering::Relaxed);
        self.shared.in_monitor.store(i.monitor, Ordering::Relaxed);
        self.shared.in_record.store(i.record, Ordering::Relaxed);
    }

    /// Connects the input strip to its configured source when it is in the graph.
    /// Mono sources feed both sides. Only links made here are ever removed here.
    pub fn connect_input(&self) {
        let s = self.settings().input;
        let Some(ours) = self.backend.input_strip_ports() else { return };
        let graph = self.backend.graph();
        let want: Vec<(String, String)> = if s.autoconnect && !s.source.trim().is_empty() {
            let mut src: Vec<String> = graph
                .iter()
                .filter(|p| p.kind == "audio" && p.dir == "out" && p.client != crate::brand::SYNTH_NAME && !p.short.starts_with("monitor_"))
                .filter(|p| glob::matches(s.source.trim(), &p.name))
                .map(|p| p.name.clone())
                .collect();
            src.sort();
            match src.len() {
                0 => vec![],
                1 => vec![(src[0].clone(), ours[0].clone()), (src[0].clone(), ours[1].clone())],
                _ => vec![(src[0].clone(), ours[0].clone()), (src[1].clone(), ours[1].clone())],
            }
        } else {
            vec![]
        };
        let have: Vec<(String, String)> = graph.iter().filter(|p| p.dir == "out").flat_map(|p| p.connections.iter().filter(|c| ours.contains(c)).map(move |c| (p.name.clone(), c.clone()))).collect();
        let made = self.inner.lock().unwrap().input_links.clone();
        let mut now = vec![];
        for l in &made {
            if !want.contains(l) && have.contains(l) {
                let _ = self.backend.disconnect(&l.0, &l.1);
            }
        }
        let mut connected = vec![];
        for l in &want {
            if have.contains(l) || self.backend.connect(&l.0, &l.1).is_ok() {
                if !have.contains(l) {
                    connected.push(l.0.clone());
                }
                now.push(l.clone());
            }
        }
        let lost = !made.is_empty() && now.is_empty();
        self.inner.lock().unwrap().input_links = now;
        if !connected.is_empty() {
            connected.dedup();
            self.event("info", format!("audio input connected: {} -> input strip (monitor {})", connected.join(", "), if s.monitor { "on" } else { "off" }));
        } else if lost {
            self.event("info", "audio input source gone");
        }
    }

    pub fn input_json(&self) -> Value {
        let s = self.settings().input;
        let links = self.inner.lock().unwrap().input_links.clone();
        let mut sources: Vec<String> = links.iter().map(|l| l.0.clone()).collect();
        sources.dedup();
        json!({
            "source": s.source, "autoconnect": s.autoconnect, "gain_db": s.gain_db,
            "monitor": s.monitor, "record": s.record, "connected": sources,
            "ports": self.backend.input_strip_ports(),
            "candidates": self.backend.graph().iter()
                .filter(|p| p.kind == "audio" && p.dir == "out" && p.client != crate::brand::SYNTH_NAME && !p.short.starts_with("monitor_"))
                .map(|p| p.name.clone()).collect::<Vec<_>>(),
        })
    }

    /// Changes the input strip. Turning monitoring on needs `confirm` (feedback risk).
    pub fn set_input(&self, source: Option<String>, gain_db: Option<f32>, monitor: Option<bool>, record: Option<bool>, autoconnect: Option<bool>, confirm: bool) -> Result<Value, String> {
        let mut st = self.settings();
        if monitor == Some(true) && !st.input.monitor && !confirm {
            return Err("monitoring sends the input to the DAC; with a microphone near the speakers this can feed back. Send confirm: true to turn it on".into());
        }
        if let Some(v) = source {
            st.input.source = v.trim().to_string();
        }
        if let Some(v) = gain_db {
            if !v.is_finite() {
                return Err("gain must be a number".into());
            }
            st.input.gain_db = v.clamp(-60.0, 24.0);
        }
        if let Some(v) = monitor {
            st.input.monitor = v;
        }
        if let Some(v) = record {
            st.input.record = v;
        }
        if let Some(v) = autoconnect {
            st.input.autoconnect = v;
        }
        let input = st.input.clone();
        self.put_settings(st)?;
        self.apply_input_rt(&input);
        self.connect_input();
        Ok(self.input_json())
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
        let _ = paths::write_atomic(&self.paths.state(), toml::to_string(&p).unwrap_or_default().as_bytes());
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
                    let wt = if s.sound.wave == Waveform::Table { s.wavetable.clone() } else { String::new() };
                    self.set_tableset(i as u8, &wt, id, &s.name)?;
                    let want = if s.sound.wave == Waveform::Sampler { s.sample.clone() } else { String::new() };
                    self.set_instrument(i as u8, &want, id, &s.name)?;
                }
                None => {
                    self.send(Cmd::SetSlot(i as u8, SlotParams { enabled: false, ..SlotParams::default() }))?;
                    self.set_instrument(i as u8, "", id, "")?;
                    self.set_tableset(i as u8, "", id, "")?;
                }
            }
        }
        self.send(Cmd::SetRoutes(Box::new(compile_all(&sc.routes, &ports, &devices, audition))))?;
        self.send(Cmd::SetTuning { a4_hz: sc.a4_hz, bend_range: sc.bend_range })?;
        let db = if startup { sc.master_db.min(pilab_synth::engine::START_MASTER_DB) } else { sc.master_db };
        self.send(Cmd::SetMaster { db })?;
        Ok(())
    }

    /// Sends a wavetable frame set to engine slot `i` when the group changed.
    fn set_tableset(&self, i: u8, group: &str, id: u8, name: &str) -> Result<(), String> {
        if self.inner.lock().unwrap().sent_set[i as usize] == group {
            return Ok(());
        }
        let set = if group.is_empty() {
            None
        } else {
            match self.library.load_group(group) {
                Ok(t) => Some(t),
                Err(e) => {
                    self.event("warn", format!("slot {id} \"{name}\": wavetable not loaded ({e})"));
                    None
                }
            }
        };
        let key = if set.is_some() { group.to_string() } else { String::new() };
        self.send(Cmd::SetTableSet(i, set))?;
        self.inner.lock().unwrap().sent_set[i as usize] = key;
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
    fn audition_sound(&self, src: &AuditionSrc) -> Result<AudSound, String> {
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
                let set = if s.sound.wave == Waveform::Table && !s.wavetable.is_empty() { self.library.load_group(&s.wavetable).ok() } else { None };
                Ok(AudSound { label: format!("slot {id} \"{}\"", s.name), sound: s.sound, table, set, inst, drum })
            }
            AuditionSrc::Bank(b) => {
                let bank = self.samples.bank(b).ok_or("bank not installed")?;
                let p = pilab_core::presets::for_bank(&bank.category);
                Ok(AudSound { label: bank.title.clone(), sound: p, table: None, set: None, inst: Some(self.samples.load(b)?), drum: bank.drum })
            }
            AuditionSrc::Preset(id) => {
                let p = pilab_core::presets::find(id).ok_or("no such preset")?;
                let inst = if p.bank.is_empty() { None } else { Some(self.samples.load(p.bank)?) };
                Ok(AudSound { label: p.name.to_string(), sound: p.sound, table: None, set: None, inst, drum: p.drum })
            }
            AuditionSrc::Table(path) => {
                let t = self.library.load(path)?;
                let p = SlotParams { wave: Waveform::Table, ..SlotParams::default() };
                Ok(AudSound { label: path.rsplit('/').next().unwrap_or(path).to_string(), sound: p, table: Some(t), set: None, inst: None, drum: false })
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
            let name = pilab_core::presets::all().into_iter().find(|p| p.bank == b).map(|p| p.name.to_string()).unwrap_or_else(|| bk.title.clone());
            (Slot { id, name, table: String::new(), sample: b.to_string(), wavetable: String::new(), name_locked: false, sound }, bk.drum)
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
            if old.name_locked {
                slot.name = old.name.clone();
                slot.name_locked = true;
            }
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
        let AudSound { label, sound, table, set, inst, drum } = self.audition_sound(&src)?;
        match &src {
            AuditionSrc::Slot(id) => self.set_audition(*id),
            _ => {
                let p = PREVIEW_SLOT;
                self.send(Cmd::SetSlot(p, SlotParams { enabled: true, ..sound }))?;
                self.send(Cmd::SetTable(p, table))?;
                self.send(Cmd::SetTableSet(p, set))?;
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
        let AudSound { sound, table, set, inst, drum, .. } = self.audition_sound(&src)?;
        let sr = 48_000u32;
        let mut e = Engine::new(sr as f32);
        e.apply(Cmd::SetSlot(0, SlotParams { enabled: true, ..sound }));
        e.apply(Cmd::SetTable(0, table));
        e.apply(Cmd::SetTableSet(0, set));
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

    pub fn upsert_slot(&self, mut slot: Slot) -> Result<Scene, String> {
        let old = self.scene().0.slot(slot.id).cloned();
        self.resolve_slot(&mut slot, old.as_ref())?;
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

    /// Makes a slot playable and names it. A wavetable slot without a group or file gets a
    /// group (the previous one, else a default) and its first audible wave, with `position`
    /// on that wave; a sampler slot without a bank gets one. The bank is loaded here, so
    /// the slot sounds as soon as the request returns. Unless the name is locked, a change
    /// of source (oscillator type, group, wave file, bank) or an empty name renames it.
    pub fn resolve_slot(&self, slot: &mut Slot, old: Option<&Slot>) -> Result<(), String> {
        match slot.sound.wave {
            Waveform::Table => {
                if slot.wavetable.is_empty() && slot.table.is_empty() {
                    let prev = old.map(|o| o.wavetable.clone()).filter(|g| !g.is_empty());
                    slot.wavetable = prev.or_else(|| self.default_group()).ok_or("no AKWF wavetables installed (scripts/fetch-akwf.sh)")?;
                }
                if !slot.wavetable.is_empty() && !slot.table.starts_with(&format!("{}/", slot.wavetable)) {
                    let frames = self.library.group_frames(&slot.wavetable, 64)?;
                    let i = first_audible(&frames).ok_or("that group has no audible waves")?;
                    slot.table = frames[i].0.clone();
                    slot.sound.position = if frames.len() > 1 { i as f32 / (frames.len() - 1) as f32 } else { 0.0 };
                }
                if !slot.wavetable.is_empty() {
                    self.library.load_group(&slot.wavetable)?;
                }
            }
            Waveform::Sampler => {
                if slot.sample.is_empty() {
                    let prev = old.map(|o| o.sample.clone()).filter(|b| !b.is_empty());
                    slot.sample = prev.or_else(|| self.default_bank()).ok_or("no sample banks installed (scripts/fetch-samples.sh)")?;
                }
                self.samples.load(&slot.sample)?;
            }
            _ => {}
        }
        let key = |s: &Slot| (s.sound.wave, s.wavetable.clone(), s.table.clone(), s.sample.clone());
        let changed = old.is_none_or(|o| key(o) != key(slot));
        if !slot.name_locked && (changed || slot.name.trim().is_empty()) {
            slot.name = self.auto_name(slot);
        }
        Ok(())
    }

    /// Readable name of a slot's source: "Saw", "FM synth #41", "Upright Piano".
    pub fn auto_name(&self, slot: &Slot) -> String {
        match slot.sound.wave {
            Waveform::Table if !slot.wavetable.is_empty() => format!("{} {}", crate::assets::akwf_name(&slot.wavetable), wave_num(&slot.table)).trim().to_string(),
            Waveform::Table if !slot.table.is_empty() => {
                let g = slot.table.split('/').next().unwrap_or("");
                format!("{} {}", crate::assets::akwf_name(g), wave_num(&slot.table)).trim().to_string()
            }
            Waveform::Table => "Wavetable".into(),
            Waveform::Sampler => {
                let preset = pilab_core::presets::all().into_iter().find(|p| p.bank == slot.sample);
                match (preset, self.samples.bank(&slot.sample)) {
                    (Some(p), _) => p.name.to_string(),
                    (None, Some(b)) => b.title,
                    (None, None) => "Sampler".into(),
                }
            }
            w => format!("{w:?}"),
        }
    }

    fn default_group(&self) -> Option<String> {
        let groups = self.library.groups();
        ["AKWF_bw_saw", "AKWF_fmsynth", "AKWF_bw_sin"]
            .iter()
            .find(|g| groups.iter().any(|x| x.name == **g))
            .map(|g| g.to_string())
            .or_else(|| groups.first().map(|g| g.name.clone()))
    }

    fn default_bank(&self) -> Option<String> {
        let banks = self.samples.banks();
        pilab_core::presets::all()
            .into_iter()
            .map(|p| p.bank.to_string())
            .find(|b| banks.iter().any(|x| &x.id == b))
            .or_else(|| banks.first().map(|b| b.id.clone()))
    }

    /// Audio nodes with volume and mute, for the patch panel. Also (re)links the meters.
    pub fn mixer_json(&self) -> Value {
        let nodes = self.mixer.nodes();
        self.ensure_meters(&nodes);
        json!({ "nodes": nodes, "cap": crate::mixer::VOLUME_CAP, "synth": crate::brand::SYNTH_NAME, "dac_sink": self.settings().audio.dac_sink })
    }

    /// Peak per metered client since the last call, in dBFS per channel.
    pub fn meters_json(&self) -> Value {
        let peaks = self.shared.take_meters();
        let mut g = self.inner.lock().unwrap();
        g.meter_poll = Some(Instant::now());
        let mut m: serde_json::Map<String, Value> = Default::default();
        for (k, (client, _)) in g.meter_map.iter().enumerate() {
            let e = m.entry(client.clone()).or_insert_with(|| json!([]));
            if let Some(a) = e.as_array_mut() {
                a.push(json!((gain_to_db(peaks[k]) * 10.0).round() / 10.0));
            }
        }
        Value::Object(m)
    }

    /// Links the hidden meter inputs to the outputs worth metering: running non-AirPlay
    /// sinks (their monitor ports), capture devices and app outputs. Never links to a
    /// suspended sink or an AirPlay sink, so metering never wakes a device or a speaker.
    fn ensure_meters(&self, nodes: &[crate::mixer::MixNode]) {
        if self.backend.meter_port(0).is_none() {
            return;
        }
        let graph = self.backend.graph();
        let synth = crate::audio::CLIENT_NAME;
        let mut want: Vec<(String, String)> = vec![];
        let mut clients: Vec<String> = vec![];
        for p in graph.iter().filter(|p| p.dir == "out" && p.kind == "audio" && p.client != synth) {
            if !clients.contains(&p.client) {
                clients.push(p.client.clone());
            }
        }
        let node = |c: &str| nodes.iter().find(|n| n.label == c);
        // sinks first, then capture and apps
        clients.sort_by_key(|c| match node(c).map(|n| n.role) {
            Some("sink") => 0,
            Some("source") => 1,
            _ => 2,
        });
        for c in clients {
            let n = node(&c);
            if n.is_some_and(|n| n.airplay) {
                continue;
            }
            let is_sink = n.is_some_and(|n| n.role == "sink");
            if is_sink && !n.is_some_and(|n| n.state == "running") {
                continue;
            }
            let ports: Vec<&crate::audio::GraphPort> = graph
                .iter()
                .filter(|p| p.client == c && p.dir == "out" && p.kind == "audio" && (p.short.starts_with("monitor_") == is_sink))
                .take(2)
                .collect();
            if want.len() + ports.len() > crate::audio::METER_PORTS {
                break;
            }
            want.extend(ports.into_iter().map(|p| (c.clone(), p.name.clone())));
        }
        let cur = self.inner.lock().unwrap().meter_map.clone();
        if cur == want {
            self.inner.lock().unwrap().meter_poll = Some(Instant::now());
            return;
        }
        self.detach_meters();
        for (k, (_, port)) in want.iter().enumerate() {
            if let Some(m) = self.backend.meter_port(k) {
                let _ = self.backend.connect(port, &m);
            }
        }
        let mut g = self.inner.lock().unwrap();
        g.meter_map = want;
        g.meter_poll = Some(Instant::now());
    }

    /// Unlinks all meter inputs (no one is looking at the meters).
    pub fn detach_meters(&self) {
        let map = std::mem::take(&mut self.inner.lock().unwrap().meter_map);
        for (k, (_, port)) in map.iter().enumerate() {
            if let Some(m) = self.backend.meter_port(k) {
                let _ = self.backend.disconnect(port, &m);
            }
        }
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
        paths::write_atomic(&self.paths.settings(), s.to_toml()?.as_bytes()).map_err(|e| e.to_string())?;
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
        self.log_midi(MidiEv { port, len: bytes.len() as u8, b, t_ns: crate::measure::now_ns() });
        Ok(())
    }

    fn log_midi(&self, ev: MidiEv) {
        let bytes = ev.b[..ev.len as usize].to_vec();
        let (kind, ch, text) = describe_midi(&bytes);
        let t = self.started.elapsed().as_secs_f64();
        let mut g = self.inner.lock().unwrap();
        let source = g.ports.get(ev.port as usize).cloned().flatten().map(|s| short_source(&s)).unwrap_or_default();
        if (ev.port as usize) < MAX_PORTS {
            g.midi_counts[ev.port as usize] += 1;
        }
        let log = MidiLog { t_ns: ev.t_ns, t, port: ev.port, source, kind, ch, text, bytes };
        if g.midi_pending.len() < 256 {
            g.midi_pending.push(log.clone());
        }
        drop(g);
        if self.measuring.load(Ordering::Relaxed) && (kind == "note-on" || kind == "note-off") {
            self.measure_notes.lock().unwrap().push(log);
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
            let rec_in = self.settings().input.record && self.shared.in_record.load(Ordering::Relaxed);
            let source = if rec_in {
                "Trillium Synth output (post master, limiter) and the audio input strip"
            } else {
                "Trillium Synth output (post master, limiter)"
            };
            let id = self.recorder.start(&dir, &self.scene().0.name, source)?;
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
        if slot.sound.wave == Waveform::Table && !slot.wavetable.is_empty() {
            if let Ok(set) = self.library.load_group(&slot.wavetable) {
                let lv = set.level_for_dt(dt);
                return (0..points).map(|i| set.sample(slot.sound.position, lv, i as f32 / points as f32)).collect();
            }
        }
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

    /// Connect the synth outputs to the configured sink ports. Returns true when every
    /// configured link exists afterwards. With `warn` false, failures are silent (used
    /// while waiting for the DAC sink to appear at boot).
    pub fn try_connect_outputs(&self, warn: bool) -> bool {
        let s = self.settings();
        let ours = self.backend.output_ports();
        let mut ok = true;
        for (i, dst) in s.audio.output_ports.iter().take(2).enumerate() {
            match self.backend.connect(&ours[i], dst) {
                Ok(()) => {}
                Err(e) if e.contains("Exist") => {}
                Err(e) => {
                    ok = false;
                    if warn {
                        self.event("warn", format!("output {} -> {dst}: {e}", ours[i]));
                    }
                }
            }
        }
        ok
    }

    pub fn connect_outputs(&self) -> bool {
        self.try_connect_outputs(true)
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
        let ip = self.shared.take_input_peaks();
        let mut g = self.inner.lock().unwrap();
        let midi = std::mem::take(&mut g.midi_pending);
        let input_on = !g.input_links.is_empty();
        json!({
            "t": "status",
            "input": { "connected": input_on, "peak_db": [gain_to_db(ip[0]), gain_to_db(ip[1])], "monitor": self.shared.in_monitor.load(Ordering::Relaxed) },
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

    /// Starts or stops the "Trillium Zero Mic" PipeWire source to match the profile
    /// and the last commanded microphone state. Off the synth's audio core.
    pub fn sync_zero_mic(&self) {
        if self.mic_hold.load(Ordering::SeqCst) {
            return;
        }
        let want = self.devmgr.mic_wanted();
        let mut g = self.zeromic.lock().unwrap();
        if want && g.is_none() {
            match crate::zeromic::Bridge::start(std::path::Path::new(crate::zeromic::DEVICE)) {
                Ok(b) => {
                    self.event("info", "Trillium Zero Mic source started (16 kHz → 48 kHz, monitor off)");
                    *g = Some(b);
                }
                Err(e) => self.event("warn", format!("Trillium Zero Mic source: {e}")),
            }
        } else if !want {
            if g.is_some() {
                *g = None;
                self.event("info", "Trillium Zero Mic source stopped");
            }
        }
    }

    /// ACTION 7/8 plus the PipeWire source. `confirm` is required to start.
    pub fn set_zero_mic(&self, key: &str, on: bool, confirm: bool) -> Result<Value, String> {
        if on {
            // Open the CDC port first so the firmware can write frames as soon as ACTION 7 acks.
            // Hold off sync_zero_mic so it cannot drop the port before ACTION 7 finishes.
            self.mic_hold.store(true, Ordering::SeqCst);
            let mut g = self.zeromic.lock().unwrap();
            if g.is_none() {
                match crate::zeromic::Bridge::start(std::path::Path::new(crate::zeromic::DEVICE)) {
                    Ok(b) => *g = Some(b),
                    Err(e) => {
                        self.mic_hold.store(false, Ordering::SeqCst);
                        return Err(e);
                    }
                }
            }
        }
        let result = self.devmgr.set_mic(key, on, confirm);
        self.mic_hold.store(false, Ordering::SeqCst);
        match result {
            Ok(v) => {
                if !on {
                    *self.zeromic.lock().unwrap() = None;
                }
                self.event("info", format!("Trillium Zero ({key}): microphone test stream {}", if on { "on" } else { "off" }));
                Ok(v)
            }
            Err(e) => {
                if on {
                    *self.zeromic.lock().unwrap() = None;
                }
                Err(e)
            }
        }
    }

    /// Arm a take, announce GO, and wait up to `timeout_s` (max 60) for ONE strike: the Zero's
    /// first note-on, the synth's first sound, and the acoustic onset (Zero mic and/or the
    /// input strip). Reports MIDI→synth and acoustic→synth. `rehearse` injects a note on the
    /// Zero's input instead of waiting for a person, to check the measurement end to end.
    /// Writes a JSON result under measurements/.
    pub fn measure_strike(&self, timeout_s: u64, rehearse: bool) -> Result<Value, String> {
        use crate::measure::{self, now_ns};
        use pilab_synth::tuning::gain_to_db;
        let plan = self.devmgr.strike_plan();
        let cue = measure::cue(plan.as_ref(), rehearse);
        let timeout = Duration::from_secs(if rehearse { 5 } else { timeout_s.clamp(5, 60) });
        let _ = self.stats.take_sticky_peaks();
        if let Some(b) = self.zeromic.lock().unwrap().as_ref() {
            let _ = b.take_peak();
        }
        let _ = self.shared.take_input_peaks();
        self.measure_notes.lock().unwrap().clear();
        self.measuring.store(true, Ordering::SeqCst);
        // Keep the input strip out of this take's WAV so it is synth-only; restored below.
        let rec_in_was = self.shared.in_record.load(Ordering::Relaxed);
        self.shared.in_record.store(false, Ordering::Relaxed);
        let rec = self.record(true);
        let recording = match &rec {
            Ok(v) => v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()),
            Err(e) => {
                self.event("warn", format!("strike measure: recording not started ({e}); using live peaks only"));
                None
            }
        };
        self.event("info", cue.clone());
        let go_ns = now_ns();
        let go = Instant::now();
        let has_zm = self.zeromic.lock().unwrap().is_some();
        let (mut base_zm, mut base_in, mut base_sy) = (0.0f32, 0.0f32, 0.0f32);
        let mut ac_zm: Option<(f32, f32)> = None;
        let mut ac_in: Option<(f32, f32)> = None;
        let mut synth: Option<(f32, f32)> = None;
        let mut midi: Option<(f32, &'static str, String, u8)> = None;
        let (mut inj_on, mut inj_off) = (false, false);
        while go.elapsed() < timeout {
            let zm = if has_zm { self.zeromic.lock().unwrap().as_ref().map(|b| b.take_peak()).unwrap_or(0.0) } else { 0.0 };
            let ip = self.shared.take_input_peaks();
            let ipk = ip[0].max(ip[1]);
            let (sl, sr) = self.stats.take_sticky_peaks();
            let sy = sl.max(sr);
            let ms = go.elapsed().as_secs_f32() * 1000.0;
            if rehearse && !inj_on && ms >= 600.0 {
                inj_on = true;
                let b = [0x90, 60, 100];
                self.send(Cmd::Midi { port: 1, len: 3, bytes: b })?;
                self.log_midi(MidiEv { port: 1, len: 3, b, t_ns: now_ns() });
            }
            if rehearse && inj_on && !inj_off && ms >= 1000.0 {
                inj_off = true;
                let b = [0x80, 60, 0];
                self.send(Cmd::Midi { port: 1, len: 3, bytes: b })?;
                self.log_midi(MidiEv { port: 1, len: 3, b, t_ns: now_ns() });
            }
            if ms < 300.0 {
                base_zm = base_zm.max(zm);
                base_in = base_in.max(ipk);
                base_sy = base_sy.max(sy);
            } else {
                if ac_zm.is_none() && has_zm && zm >= measure::threshold(base_zm, 12.0, -50.0) {
                    ac_zm = Some((ms, zm));
                }
                if ac_in.is_none() && ipk >= measure::threshold(base_in, 12.0, -40.0) {
                    ac_in = Some((ms, ipk));
                }
                if synth.is_none() && sy >= measure::threshold(base_sy, 12.0, -50.0) {
                    synth = Some((ms, sy));
                }
            }
            if midi.is_none() {
                let g = self.measure_notes.lock().unwrap();
                if let Some(e) = g.iter().find(|e| e.kind == "note-on" && e.t_ns >= go_ns && e.port != pilab_synth::PORT_CONSOLE && e.port != pilab_synth::PORT_AUDITION && e.port != pilab_synth::PORT_VIRTUAL) {
                    midi = Some(((e.t_ns - go_ns) as f32 / 1.0e6, "note-on", e.source.clone(), e.bytes.get(1).copied().unwrap_or(0)));
                }
            }
            if midi.is_some() && synth.is_some() {
                // Let the acoustic onset in (it can trail the note by the mic path's buffering).
                std::thread::sleep(Duration::from_millis(if rehearse { 100 } else { 400 }));
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        // One last read so an onset in the final sleep is not missed.
        if ac_zm.is_none() && has_zm {
            let zm = self.zeromic.lock().unwrap().as_ref().map(|b| b.take_peak()).unwrap_or(0.0);
            if zm >= measure::threshold(base_zm, 12.0, -50.0) {
                ac_zm = Some((go.elapsed().as_secs_f32() * 1000.0, zm));
            }
        }
        let ip = self.shared.take_input_peaks();
        if ac_in.is_none() && ip[0].max(ip[1]) >= measure::threshold(base_in, 12.0, -40.0) {
            ac_in = Some((go.elapsed().as_secs_f32() * 1000.0, ip[0].max(ip[1])));
        }
        let timed_out = midi.is_none() || synth.is_none();
        let rec_stop = if recording.is_some() { self.record(false).ok() } else { None };
        self.shared.in_record.store(rec_in_was, Ordering::Relaxed);
        self.measuring.store(false, Ordering::SeqCst);
        let rec_dir = rec_stop.as_ref().and_then(|v| v.get("card").and_then(|c| c.get("id")).and_then(|x| x.as_str()).map(|s| s.to_string()));
        let r1 = |x: f32| ((x * 10.0).round()) / 10.0;
        let db = |p: f32| r1(gain_to_db(p));
        let d = |a: Option<f32>, b: Option<f32>| match (a, b) {
            (Some(a), Some(b)) => Some(r1(b - a)),
            _ => None,
        };
        let midi_ms = midi.as_ref().map(|m| m.0);
        let sy_ms = synth.map(|s| s.0);
        // Primary acoustic reference: the Zero's own mic when it is on (next to the strike), else the input strip.
        let (ac_ms, ac_src, ac_peak) = match (ac_zm, ac_in) {
            (Some((t, p)), _) => (Some(t), Some("zero_mic"), Some(p)),
            (None, Some((t, p))) => (Some(t), Some("input_strip"), Some(p)),
            _ => (None, None, None),
        };
        let midi_notes: Vec<Value> = self
            .measure_notes
            .lock()
            .unwrap()
            .drain(..)
            .map(|e| {
                json!({
                    "ms": r1((e.t_ns as i64 - go_ns as i64) as f32 / 1.0e6),
                    "kind": e.kind, "ch": e.ch, "note": e.bytes.get(1).copied().unwrap_or(0),
                    "vel": e.bytes.get(2).copied().unwrap_or(0), "port": e.port, "source": e.source, "text": e.text,
                })
            })
            .collect();
        let midi_note_ons = midi_notes.iter().filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("note-on")).count();
        let diagnosis = if midi.is_none() {
            match &plan {
                None => "No Trillium Zero is connected, so no strike note can arrive.".to_string(),
                Some(p) if !p.path.strike_ready => format!("The Zero cannot play a note from a strike with its current settings: {}", p.path.hint),
                Some(p) if !p.imu_running => "The Zero's motion sensor is not running, so no strike is detected.".to_string(),
                Some(p) if !p.trill_fitted => "No note-on arrived from the Zero. Its motion tap needs the gyro calibrated and still for about 1 s, then a firm tap (default threshold 0.8 g, 120 ms apart).".to_string(),
                Some(_) => "No note-on arrived from the Zero.".to_string(),
            }
        } else if synth.is_none() {
            "A note-on reached the Base but the synth stayed silent: check the scene route, master level and that the synth is running.".to_string()
        } else {
            "ok".to_string()
        };
        let stamp = timefmt::stamp(timefmt::now_secs());
        let dir = self.paths.measurements();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("strike-{stamp}.json"));
        let result = json!({
            "ok": !timed_out,
            "rehearsal": rehearse,
            "go": cue,
            "diagnosis": diagnosis,
            "timeout_s": timeout.as_secs(),
            "timed_out": timed_out,
            "sensors": plan.as_ref().map(|p| json!({ "touch_strip": p.trill_fitted, "imu_running": p.imu_running, "zero_mic": p.mic, "note_path": p.path.summary })),
            "midi_ms": midi_ms.map(r1),
            "midi_source": midi.as_ref().map(|m| m.2.clone()),
            "midi_note": midi.as_ref().map(|m| m.3),
            "synth_ms": sy_ms.map(r1),
            "synth_peak_db": synth.map(|(_, p)| db(p)),
            "midi_to_synth_ms": d(midi_ms, sy_ms),
            "acoustic_ms": ac_ms.map(r1),
            "acoustic_source": ac_src,
            "acoustic_peak_db": ac_peak.map(db),
            "acoustic_to_synth_ms": d(ac_ms, sy_ms),
            "strike_to_midi_ms": d(ac_ms, midi_ms),
            "strike_to_sound_ms": d(ac_ms, sy_ms),
            "acoustic_zero_mic_ms": ac_zm.map(|a| r1(a.0)),
            "acoustic_zero_mic_to_synth_ms": d(ac_zm.map(|a| a.0), sy_ms),
            "acoustic_input_ms": ac_in.map(|a| r1(a.0)),
            "acoustic_input_to_synth_ms": d(ac_in.map(|a| a.0), sy_ms),
            "baseline_acoustic_db": db(if has_zm { base_zm } else { base_in }),
            "baseline_input_db": db(base_in),
            "baseline_synth_db": db(base_sy),
            "midi_notes": midi_notes,
            "midi_note_ons": midi_note_ons,
            "recording_includes_input": false,
            "recording": rec_dir,
            "path": path.display().to_string(),
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let summary = match (d(midi_ms, sy_ms), d(ac_ms, sy_ms)) {
            (Some(m), Some(a)) => format!("strike measured: MIDI→synth {m} ms, acoustic→synth {a} ms (result {})", path.display()),
            (Some(m), None) => format!("strike measured: MIDI→synth {m} ms, no acoustic onset (result {})", path.display()),
            _ => format!("strike measure: {diagnosis} (result {})", path.display()),
        };
        self.event("info", summary);
        Ok(result)
    }
}

/// Background loops: realtime ring draining + status at 15 Hz; machine state every 2 s.
pub fn spawn_loops(app: Arc<App>) {
    let d = app.clone();
    std::thread::Builder::new()
        .name("devices".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            let mut last_poll = Instant::now() - Duration::from_secs(5);
            loop {
                if last_poll.elapsed() >= Duration::from_secs(5) {
                    let classes = d.settings().midi.devices;
                    for (level, text) in d.devmgr.poll(&classes) {
                        d.event(level, text);
                    }
                    for key in d.devmgr.connected_keys_live() {
                        if d.devmgr.profile_wants_mic(&key) {
                            match d.set_zero_mic(&key, true, true) {
                                Ok(_) => d.event("info", format!("Trillium Zero ({key}): microphone test stream started from its stored profile")),
                                Err(e) => d.event("warn", format!("Trillium Zero ({key}): could not apply the stored microphone setting: {e}")),
                            }
                        }
                    }
                    last_poll = Instant::now();
                }
                if let Some(t) = d.devmgr.harvest_motion() {
                    d.shared.set_zero_motion(t.pitch, t.roll, t.touch_pos, t.touching);
                }
                d.sync_zero_mic();
                std::thread::sleep(Duration::from_millis(50));
            }
        })
        .expect("spawn devices loop");
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
                let idle = app.inner.lock().unwrap().meter_poll.is_some_and(|t| t.elapsed() > Duration::from_secs(15));
                if idle {
                    app.inner.lock().unwrap().meter_poll = None;
                    app.detach_meters();
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
                app.connect_input();
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

/// Index of the first wave whose cycle is not silent or flat (peak-to-peak above 0.1).
pub fn first_audible(frames: &[(String, Vec<f32>)]) -> Option<usize> {
    let pp = |p: &Vec<f32>| {
        let (lo, hi) = p.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        if p.is_empty() { 0.0 } else { hi - lo }
    };
    frames.iter().position(|(_, p)| pp(p) > 0.1).or(if frames.is_empty() { None } else { Some(0) })
}

/// "#41" from "AKWF_fmsynth/AKWF_fmsynth_0041.wav"; empty when the file has no number.
pub fn wave_num(file: &str) -> String {
    let stem = file.rsplit('/').next().unwrap_or("").trim_end_matches(".wav").trim_end_matches(".WAV");
    let digits: String = stem.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse::<u32>().map(|n| format!("#{n}")).unwrap_or_default()
}

#[cfg(test)]
mod select_tests {
    //! Select -> slot state -> engine: every selection must leave a playable slot, named
    //! after its source unless the name was typed, and audible when rendered.
    use super::*;
    use crate::audio::{NullBackend, RtShared};
    use crate::paths::Paths;

    fn wav16(samples: &[f32], sr: u32) -> Vec<u8> {
        let mut v = Vec::new();
        let data = samples.len() as u32 * 2;
        v.extend(b"RIFF");
        v.extend((36 + data).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(sr.to_le_bytes());
        v.extend((sr * 2).to_le_bytes());
        v.extend(2u16.to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"data");
        v.extend(data.to_le_bytes());
        for s in samples {
            v.extend(((s.clamp(-1.0, 1.0) * 32000.0) as i16).to_le_bytes());
        }
        v
    }

    fn rms_db(wav: &[u8]) -> f32 {
        let s: Vec<f32> = wav[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
        let e = s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32;
        10.0 * e.max(1e-18).log10()
    }

    fn app() -> (Arc<App>, PathBuf) {
        let root = std::env::temp_dir().join(format!("pilab-select-{}-{}", std::process::id(), rand_tag()));
        let akwf = root.join("data/library/akwf/AKWF_fmsynth");
        std::fs::create_dir_all(&akwf).unwrap();
        let tau = std::f32::consts::TAU;
        // wave 1 is silent: a fresh pick must skip it
        std::fs::write(akwf.join("AKWF_fmsynth_0001.wav"), wav16(&[0.0; 600], 44_100)).unwrap();
        let sine: Vec<f32> = (0..600).map(|i| (tau * i as f32 / 600.0).sin()).collect();
        let fm: Vec<f32> = (0..600).map(|i| (tau * i as f32 / 600.0 + 2.0 * (3.0 * tau * i as f32 / 600.0).sin()).sin()).collect();
        std::fs::write(akwf.join("AKWF_fmsynth_0002.wav"), wav16(&sine, 44_100)).unwrap();
        std::fs::write(akwf.join("AKWF_fmsynth_0041.wav"), wav16(&fm, 44_100)).unwrap();
        let sm = root.join("data/library/samples/test");
        std::fs::create_dir_all(&sm).unwrap();
        let tone: Vec<f32> = (0..48_000).map(|i| 0.5 * (tau * 261.63 * i as f32 / 48_000.0).sin()).collect();
        std::fs::write(sm.join("c4.wav"), wav16(&tone, 48_000)).unwrap();
        std::fs::write(sm.join("test.sfz"), "<region> sample=c4.wav lokey=0 hikey=127 pitch_keycenter=60\n").unwrap();
        std::fs::write(
            root.join("data/library/samples/index.json"),
            r#"{"items":[{"id":"test-bank","title":"Test Bank","category":"keys","sfz":"samples/test/test.sfz","license":"CC0"}]}"#,
        )
        .unwrap();
        let paths = Paths { config: root.join("config"), data: root.join("data") };
        let shared = Arc::new(RtShared::default());
        let (b, l, s) = NullBackend::start(shared.clone(), 48_000.0);
        (App::new(paths, 0, Arc::new(b), l, s, shared).unwrap(), root)
    }

    fn rand_tag() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64
    }

    fn slot(a: &App, id: u8) -> Slot {
        a.scene().0.slot(id).cloned().unwrap()
    }

    fn loud(a: &App, id: u8) -> f32 {
        rms_db(&a.audition_render(AuditionSrc::Slot(id)).unwrap())
    }

    #[test]
    fn select_sets_state_engine_and_name() {
        let (a, root) = app();
        let mut s = Slot { id: 1, name: "Warm Saw".into(), ..Slot::default() };
        a.upsert_slot(s.clone()).unwrap();
        assert_eq!(slot(&a, 1).name, "Warm Saw", "unchanged source keeps the name");
        // oscillator type change renames (name not locked)
        s.sound.wave = Waveform::Sine;
        a.upsert_slot(s.clone()).unwrap();
        assert_eq!(slot(&a, 1).name, "Sine");

        // wavetable type with no group: a group and its first audible wave are picked
        let mut s = slot(&a, 1);
        s.sound.wave = Waveform::Table;
        a.upsert_slot(s).unwrap();
        let r = slot(&a, 1);
        assert_eq!(r.wavetable, "AKWF_fmsynth");
        assert_eq!(r.table, "AKWF_fmsynth/AKWF_fmsynth_0002.wav", "skips the silent first wave");
        assert!((r.sound.position - 0.5).abs() < 1e-6);
        assert_eq!(r.name, "FM synth #2");
        assert_eq!(a.inner.lock().unwrap().sent_set[0], "AKWF_fmsynth", "engine got the frame set");
        assert!(loud(&a, 1) > -50.0, "wavetable slot is audible");

        // clicking a wave
        let mut s = slot(&a, 1);
        s.table = "AKWF_fmsynth/AKWF_fmsynth_0041.wav".into();
        s.sound.position = 1.0;
        a.upsert_slot(s).unwrap();
        assert_eq!(slot(&a, 1).name, "FM synth #41");
        assert_eq!(slot(&a, 1).sound.position, 1.0);

        // a parameter change does not rename
        let mut s = slot(&a, 1);
        s.sound.position = 0.7;
        a.upsert_slot(s).unwrap();
        assert_eq!(slot(&a, 1).name, "FM synth #41");

        // sampler with no bank: the bank is chosen and loaded
        let mut s = slot(&a, 1);
        s.sound.wave = Waveform::Sampler;
        a.upsert_slot(s).unwrap();
        let r = slot(&a, 1);
        assert_eq!(r.sample, "test-bank");
        assert_eq!(r.name, "Test Bank");
        assert!(a.samples.is_loaded("test-bank"));
        assert_eq!(a.inner.lock().unwrap().sent_inst[0], "test-bank", "engine got the instrument");
        assert!(loud(&a, 1) > -50.0, "sampler slot is audible");

        // typed names stick; reset to auto names it again
        let mut s = slot(&a, 1);
        s.name = "My keys".into();
        s.name_locked = true;
        a.upsert_slot(s).unwrap();
        let mut s = slot(&a, 1);
        s.sound.wave = Waveform::Saw;
        a.upsert_slot(s).unwrap();
        assert_eq!(slot(&a, 1).name, "My keys");
        assert!(a.apply_preset(1, None, Some("test-bank"), false).is_ok());
        assert_eq!(slot(&a, 1).name, "My keys", "presets respect a typed name");
        let mut s = slot(&a, 1);
        s.name = String::new();
        s.name_locked = false;
        a.upsert_slot(s).unwrap();
        assert_eq!(slot(&a, 1).name, "Test Bank");

        // back-compat: scenes without name_locked read as unlocked and do not write it
        let t = toml::to_string(&slot(&a, 1)).unwrap();
        assert!(!t.contains("name_locked"));
        let _ = std::fs::remove_dir_all(root);
    }
}
