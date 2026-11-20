// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Audio backends behind one trait.
//!
//! * `JackBackend`: a JACK client. On pilab it runs under `pw-jack`, so the synth is a
//!   normal PipeWire node ("pilab-synth") with a MIDI input per source slot and a stereo
//!   output, patchable next to SuperCollider, Faust, Csound or AirPlay.
//! * `NullBackend`: renders on a timer with no device (development, CI, screenshots).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pilab_synth::{Cmd, Engine, Garbage, Stats};
use serde::Serialize;

pub const CLIENT_NAME: &str = "pilab-synth";
/// JACK MIDI input ports, by input slot. Slot 0 is the virtual port other apps connect to.
pub const MIDI_PORTS: [&str; 8] = ["midi_in", "in_1", "in_2", "in_3", "in_4", "in_5", "in_6", "in_7"];

#[derive(Clone, Copy)]
pub struct MidiEv {
    pub port: u8,
    pub len: u8,
    pub b: [u8; 3],
}

/// Rings between the realtime thread and the rest of the service.
pub struct RtLinks {
    pub cmd_tx: rtrb::Producer<Cmd>,
    pub gc_rx: rtrb::Consumer<Garbage>,
    pub mon_rx: rtrb::Consumer<MidiEv>,
    pub rec_rx: rtrb::Consumer<f32>,
}

/// Shared flags/counters written by the realtime side.
#[derive(Default)]
pub struct RtShared {
    pub rec_armed: AtomicBool,
    pub rec_dropped: AtomicU64,
    pub xruns: AtomicU64,
    /// 1 = pinned to the audio CPU, 0 = not yet, <0 = -errno.
    pub pinned: AtomicI32,
    /// Scheduling policy of the audio thread (SCHED_FIFO = 1).
    pub policy: AtomicI32,
    pub rt_prio: AtomicI32,
}

/// Owns the engine inside the realtime callback.
pub struct RtHost {
    engine: Engine,
    cmd_rx: rtrb::Consumer<Cmd>,
    gc_tx: rtrb::Producer<Garbage>,
    mon_tx: rtrb::Producer<MidiEv>,
    rec_tx: rtrb::Producer<f32>,
    shared: Arc<RtShared>,
    audio_cpu: usize,
    pinned: bool,
}

pub fn rt_pair(engine: Engine, shared: Arc<RtShared>, audio_cpu: usize, sr: f32) -> (RtHost, RtLinks) {
    let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(2048);
    let (gc_tx, gc_rx) = rtrb::RingBuffer::new(256);
    let (mon_tx, mon_rx) = rtrb::RingBuffer::new(1024);
    let (rec_tx, rec_rx) = rtrb::RingBuffer::new((sr as usize) * 2 * 4); // 4 s stereo
    (
        RtHost { engine, cmd_rx, gc_tx, mon_tx, rec_tx, shared, audio_cpu, pinned: false },
        RtLinks { cmd_tx, gc_rx, mon_rx, rec_rx },
    )
}

impl RtHost {
    #[inline]
    fn begin(&mut self) {
        if !self.pinned {
            self.pinned = true;
            self.pin();
        }
        while let Ok(c) = self.cmd_rx.pop() {
            if let Some(g) = self.engine.apply(c) {
                let _ = self.gc_tx.push(g);
            }
        }
    }

    /// Pins the calling (audio) thread to the reserved CPU once, and records its policy.
    fn pin(&mut self) {
        unsafe {
            let mut set: libc::cpu_set_t = core::mem::zeroed();
            libc::CPU_SET(self.audio_cpu, &mut set);
            let r = libc::sched_setaffinity(0, core::mem::size_of::<libc::cpu_set_t>(), &set);
            let v = if r == 0 { 1 } else { -*libc::__errno_location() };
            self.shared.pinned.store(v, Ordering::Relaxed);
            let pol = libc::sched_getscheduler(0);
            self.shared.policy.store(pol, Ordering::Relaxed);
            let mut sp: libc::sched_param = core::mem::zeroed();
            libc::sched_getparam(0, &mut sp);
            self.shared.rt_prio.store(sp.sched_priority, Ordering::Relaxed);
        }
    }

    #[inline]
    fn midi(&mut self, port: u8, bytes: &[u8]) {
        if bytes.is_empty() || bytes.len() > 3 || bytes[0] < 0x80 || bytes[0] >= 0xF0 {
            return;
        }
        self.engine.midi(port, bytes);
        let mut b = [0u8; 3];
        b[..bytes.len()].copy_from_slice(bytes);
        let _ = self.mon_tx.push(MidiEv { port, len: bytes.len() as u8, b });
    }

    #[inline]
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.engine.process(l, r);
        if self.shared.rec_armed.load(Ordering::Relaxed) {
            if self.rec_tx.slots() >= l.len() * 2 {
                for i in 0..l.len() {
                    let _ = self.rec_tx.push(l[i]);
                    let _ = self.rec_tx.push(r[i]);
                }
            } else {
                self.shared.rec_dropped.fetch_add(l.len() as u64, Ordering::Relaxed);
            }
        }
    }

    pub fn stats(&self) -> Arc<Stats> {
        self.engine.stats.clone()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphPort {
    pub name: String,
    pub client: String,
    pub short: String,
    /// "audio" | "midi" | other type string
    pub kind: String,
    /// "out" | "in"
    pub dir: &'static str,
    pub physical: bool,
    pub connections: Vec<String>,
}

pub trait AudioBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn sample_rate(&self) -> f32;
    fn buffer_size(&self) -> u32;
    fn cpu_load(&self) -> f32;
    fn graph(&self) -> Vec<GraphPort>;
    fn connect(&self, from: &str, to: &str) -> Result<(), String>;
    fn disconnect(&self, from: &str, to: &str) -> Result<(), String>;
    /// MIDI output ports in the graph that are not ours.
    fn midi_sources(&self) -> Vec<String>;
    /// Full name of our input port for slot `k`.
    fn input_port(&self, k: usize) -> Option<String>;
    fn output_ports(&self) -> [String; 2];
}

// ---------------------------------------------------------------- JACK

struct Proc {
    host: RtHost,
    midi: Vec<jack::Port<jack::MidiIn>>,
    out_l: jack::Port<jack::AudioOut>,
    out_r: jack::Port<jack::AudioOut>,
}

impl jack::ProcessHandler for Proc {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        self.host.begin();
        for (k, p) in self.midi.iter().enumerate() {
            for ev in p.iter(ps) {
                self.host.midi(k as u8, ev.bytes);
            }
        }
        let l = self.out_l.as_mut_slice(ps);
        let r = self.out_r.as_mut_slice(ps);
        self.host.render(l, r);
        jack::Control::Continue
    }
}

struct Notes {
    shared: Arc<RtShared>,
}

impl jack::NotificationHandler for Notes {
    fn xrun(&mut self, _: &jack::Client) -> jack::Control {
        self.shared.xruns.fetch_add(1, Ordering::Relaxed);
        jack::Control::Continue
    }
}

pub struct JackBackend {
    client: Mutex<Option<jack::AsyncClient<Notes, Proc>>>,
    sr: f32,
    bufsize: u32,
}

impl JackBackend {
    /// Opens the client (never starts a JACK server) and builds the engine at the graph rate.
    pub fn start(shared: Arc<RtShared>, audio_cpu: usize) -> Result<(JackBackend, RtLinks, Arc<Stats>), String> {
        let (client, _st) = jack::Client::new(CLIENT_NAME, jack::ClientOptions::NO_START_SERVER)
            .map_err(|e| format!("JACK/PipeWire client: {e:?}"))?;
        let sr = client.sample_rate() as f32;
        let bufsize = client.buffer_size();
        let mut midi = Vec::new();
        for n in MIDI_PORTS {
            midi.push(client.register_port(n, jack::MidiIn::default()).map_err(|e| format!("{n}: {e:?}"))?);
        }
        let out_l = client.register_port("out_l", jack::AudioOut::default()).map_err(|e| format!("{e:?}"))?;
        let out_r = client.register_port("out_r", jack::AudioOut::default()).map_err(|e| format!("{e:?}"))?;
        let engine = Engine::new(sr);
        let (host, links) = rt_pair(engine, shared.clone(), audio_cpu, sr);
        let stats = host.stats();
        let active = client
            .activate_async(Notes { shared }, Proc { host, midi, out_l, out_r })
            .map_err(|e| format!("activate: {e:?}"))?;
        Ok((JackBackend { client: Mutex::new(Some(active)), sr, bufsize }, links, stats))
    }

    fn with<T>(&self, f: impl FnOnce(&jack::Client) -> T) -> Option<T> {
        let g = self.client.lock().unwrap();
        g.as_ref().map(|a| f(a.as_client()))
    }

    /// Deactivates and closes the client (removes our node and its links).
    pub fn close(&self) {
        if let Some(a) = self.client.lock().unwrap().take() {
            let _ = a.deactivate();
        }
    }
}

fn kind_of(t: &str) -> String {
    if t.contains("audio") {
        "audio".into()
    } else if t.contains("midi") {
        "midi".into()
    } else {
        t.to_string()
    }
}

impl AudioBackend for JackBackend {
    fn name(&self) -> &'static str {
        "jack (pipewire-jack)"
    }
    fn sample_rate(&self) -> f32 {
        self.sr
    }
    fn buffer_size(&self) -> u32 {
        self.with(|c| c.buffer_size()).unwrap_or(self.bufsize)
    }
    fn cpu_load(&self) -> f32 {
        self.with(|c| c.cpu_load()).unwrap_or(0.0)
    }
    fn graph(&self) -> Vec<GraphPort> {
        self.with(|c| {
            let mut out = vec![];
            for name in c.ports(None, None, jack::PortFlags::empty()) {
                if let Some(p) = c.port_by_name(&name) {
                    let fl = p.flags();
                    let (client, short) = name.split_once(':').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((String::new(), name.clone()));
                    out.push(GraphPort {
                        kind: kind_of(&p.port_type().unwrap_or_default()),
                        dir: if fl.contains(jack::PortFlags::IS_OUTPUT) { "out" } else { "in" },
                        physical: fl.contains(jack::PortFlags::IS_PHYSICAL),
                        connections: if fl.contains(jack::PortFlags::IS_OUTPUT) { p.get_connections() } else { vec![] },
                        name,
                        client,
                        short,
                    });
                }
            }
            out
        })
        .unwrap_or_default()
    }
    fn connect(&self, from: &str, to: &str) -> Result<(), String> {
        self.with(|c| c.connect_ports_by_name(from, to).map_err(|e| format!("{e:?}"))).unwrap_or(Err("client closed".into()))
    }
    fn disconnect(&self, from: &str, to: &str) -> Result<(), String> {
        self.with(|c| c.disconnect_ports_by_name(from, to).map_err(|e| format!("{e:?}"))).unwrap_or(Err("client closed".into()))
    }
    fn midi_sources(&self) -> Vec<String> {
        self.with(|c| {
            c.ports(None, Some("midi"), jack::PortFlags::IS_OUTPUT)
                .into_iter()
                .filter(|n| !n.starts_with(&format!("{CLIENT_NAME}:")))
                .collect()
        })
        .unwrap_or_default()
    }
    fn input_port(&self, k: usize) -> Option<String> {
        MIDI_PORTS.get(k).map(|n| format!("{CLIENT_NAME}:{n}"))
    }
    fn output_ports(&self) -> [String; 2] {
        [format!("{CLIENT_NAME}:out_l"), format!("{CLIENT_NAME}:out_r")]
    }
}

// ---------------------------------------------------------------- Null

pub struct NullBackend {
    sr: f32,
    stop: Arc<AtomicBool>,
}

impl NullBackend {
    pub fn start(shared: Arc<RtShared>, sr: f32) -> (NullBackend, RtLinks, Arc<Stats>) {
        let engine = Engine::new(sr);
        let (mut host, links) = rt_pair(engine, shared, usize::MAX, sr);
        host.pinned = true; // never pin in the null backend
        let stats = host.stats();
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        std::thread::Builder::new()
            .name("null-audio".into())
            .spawn(move || {
                let n = 256usize;
                let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
                let period = std::time::Duration::from_secs_f64(n as f64 / sr as f64);
                let mut next = std::time::Instant::now();
                while !st.load(Ordering::Relaxed) {
                    host.begin();
                    host.render(&mut l, &mut r);
                    next += period;
                    let now = std::time::Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        next = now;
                    }
                }
            })
            .expect("spawn null audio");
        (NullBackend { sr, stop }, links, stats)
    }
}

impl Drop for NullBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl AudioBackend for NullBackend {
    fn name(&self) -> &'static str {
        "null (no audio device)"
    }
    fn sample_rate(&self) -> f32 {
        self.sr
    }
    fn buffer_size(&self) -> u32 {
        256
    }
    fn cpu_load(&self) -> f32 {
        0.0
    }
    fn graph(&self) -> Vec<GraphPort> {
        let mut v: Vec<GraphPort> = MIDI_PORTS
            .iter()
            .map(|n| GraphPort { name: format!("{CLIENT_NAME}:{n}"), client: CLIENT_NAME.into(), short: n.to_string(), kind: "midi".into(), dir: "in", physical: false, connections: vec![] })
            .collect();
        for n in ["out_l", "out_r"] {
            v.push(GraphPort { name: format!("{CLIENT_NAME}:{n}"), client: CLIENT_NAME.into(), short: n.into(), kind: "audio".into(), dir: "out", physical: false, connections: vec![] });
        }
        v
    }
    fn connect(&self, _: &str, _: &str) -> Result<(), String> {
        Err("no audio graph in the null backend".into())
    }
    fn disconnect(&self, _: &str, _: &str) -> Result<(), String> {
        Err("no audio graph in the null backend".into())
    }
    fn midi_sources(&self) -> Vec<String> {
        vec![]
    }
    fn input_port(&self, k: usize) -> Option<String> {
        MIDI_PORTS.get(k).map(|n| format!("{CLIENT_NAME}:{n}"))
    }
    fn output_ports(&self) -> [String; 2] {
        [format!("{CLIENT_NAME}:out_l"), format!("{CLIENT_NAME}:out_r")]
    }
}
