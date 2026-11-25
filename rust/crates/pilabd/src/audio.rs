// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Audio backends behind one trait.
//!
//! * `JackBackend`: a JACK client. On pilab it runs under `pw-jack`, so the synth is a
//!   normal PipeWire node ("Trillium Synth") with a MIDI input per source slot and a stereo
//!   output, patchable next to SuperCollider, Faust, Csound or AirPlay. It also has
//!   hidden meter inputs (`meter_N`) that pilabd links to outputs it wants to meter.
//! * `NullBackend`: renders on a timer with no device (development, CI, screenshots).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use pilab_synth::{Cmd, Engine, Garbage, Stats};
use serde::Serialize;

pub const CLIENT_NAME: &str = crate::brand::SYNTH_NAME;
/// Hidden meter inputs (4 stereo pairs). Never shown in the graph.
pub const METER_PORTS: usize = 8;
pub const METER_PREFIX: &str = "meter_";

/// Port name with the pre-rename client name mapped to the current one.
pub fn canon_port(name: &str) -> String {
    match name.strip_prefix(crate::brand::LEGACY_SYNTH_CLIENT).and_then(|r| r.strip_prefix(':')) {
        Some(rest) => format!("{CLIENT_NAME}:{rest}"),
        None => name.to_string(),
    }
}

fn is_meter_port(name: &str) -> bool {
    name.strip_prefix(CLIENT_NAME).and_then(|r| r.strip_prefix(':')).is_some_and(|p| p.starts_with(METER_PREFIX))
}
/// JACK MIDI input ports, by input slot. Slot 0 is the virtual port other apps connect to.
/// Audio input strip ports (an external source such as a USB microphone).
pub const INPUT_PORTS: [&str; 2] = ["input_l", "input_r"];
pub const MIDI_PORTS: [&str; 8] = ["midi_in", "in_1", "in_2", "in_3", "in_4", "in_5", "in_6", "in_7"];
/// MIDI output: live Trillium Zero motion and touch, for the patch panel (not auto-routed).
pub const ZERO_MOTION_PORT: &str = "zero_motion";
/// MIDI outputs driven by the MIDI router (FluidSynth, ZynAddSubFX, USB MIDI devices).
pub const ROUTER_OUT_PREFIX: &str = "midi_out_";
/// Stereo input for external engines (FluidSynth, ZynAddSubFX); mixed before the master
/// level and limiter, so the master and the headphone-safety ceiling apply to them too.
pub const ENGINE_PORTS: [&str; 2] = ["engine_l", "engine_r"];
use pilab_core::midi_router::{Table as RouterTable, DST_SYNTH, MAX_OUTS as ROUTER_OUTS, SRC_ZERO_MOTION};

#[derive(Clone, Copy)]
pub struct MidiEv {
    pub port: u8,
    pub len: u8,
    pub b: [u8; 3],
    /// CLOCK_MONOTONIC nanoseconds when the realtime thread saw the event.
    pub t_ns: u64,
    /// Router destinations it reached (bit 0 the synth, bit k external output k); 0 = dropped.
    pub to: u32,
}

/// Rings between the realtime thread and the rest of the service.
pub struct RtLinks {
    pub cmd_tx: rtrb::Producer<Cmd>,
    pub gc_rx: rtrb::Consumer<Garbage>,
    pub mon_rx: rtrb::Consumer<MidiEv>,
    pub rec_rx: rtrb::Consumer<f32>,
    pub router_tx: rtrb::Producer<Box<RouterTable>>,
    pub router_gc: rtrb::Consumer<Box<RouterTable>>,
    /// Direct sends to a router output: [dst, len, b0, b1, b2] (engine program changes).
    pub direct_tx: rtrb::Producer<[u8; 5]>,
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
    /// Peak (f32 bits, max-hold until read) of each meter input.
    pub meters: [AtomicU32; METER_PORTS],
    /// Input strip: gain (linear, f32 bits), monitor to the output, include in recordings.
    pub in_gain: AtomicU32,
    pub in_monitor: AtomicBool,
    pub in_record: AtomicBool,
    /// Input peak per side (f32 bits, max-hold until read), after gain.
    pub in_peak: [AtomicU32; 2],
    /// Packed Trillium Zero motion CCs for the `zero_motion` MIDI port: pitch, roll, touch, flags.
    pub zero_cc: AtomicU32,
    pub zero_cc_gen: AtomicU32,
}

impl RtShared {
    /// Reads and resets the meter peaks (linear).
    /// Reads and resets the input strip peaks (linear, after gain).
    pub fn take_input_peaks(&self) -> [f32; 2] {
        [f32::from_bits(self.in_peak[0].swap(0, Ordering::Relaxed)), f32::from_bits(self.in_peak[1].swap(0, Ordering::Relaxed))]
    }

    /// pitch/roll in degrees, touch 0..1. Written from the devices thread; read in the process callback.
    pub fn set_zero_motion(&self, pitch: f32, roll: f32, touch: f32, touching: bool) {
        let p = ((pitch + 90.0) / 180.0 * 127.0).clamp(0.0, 127.0) as u32;
        let r = ((roll + 90.0) / 180.0 * 127.0).clamp(0.0, 127.0) as u32;
        let t = if touching { (touch.clamp(0.0, 1.0) * 127.0) as u32 } else { 0 };
        let f = u32::from(touching);
        let packed = p | (r << 8) | (t << 16) | (f << 24);
        self.zero_cc.store(packed, Ordering::Relaxed);
        self.zero_cc_gen.fetch_add(1, Ordering::Relaxed);
    }

    pub fn take_meters(&self) -> [f32; METER_PORTS] {
        let mut v = [0.0; METER_PORTS];
        for (i, m) in self.meters.iter().enumerate() {
            v[i] = f32::from_bits(m.swap(0, Ordering::Relaxed));
        }
        v
    }
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
    router: Box<RouterTable>,
    router_rx: rtrb::Consumer<Box<RouterTable>>,
    router_gc: rtrb::Producer<Box<RouterTable>>,
    direct_rx: rtrb::Consumer<[u8; 5]>,
}

pub fn rt_pair(engine: Engine, shared: Arc<RtShared>, audio_cpu: usize, sr: f32) -> (RtHost, RtLinks) {
    let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(2048);
    let (gc_tx, gc_rx) = rtrb::RingBuffer::new(256);
    let (mon_tx, mon_rx) = rtrb::RingBuffer::new(1024);
    let (rec_tx, rec_rx) = rtrb::RingBuffer::new((sr as usize) * 2 * 4); // 4 s stereo
    let (router_tx, router_rx) = rtrb::RingBuffer::new(8);
    let (router_gc_tx, router_gc) = rtrb::RingBuffer::new(16);
    let (direct_tx, direct_rx) = rtrb::RingBuffer::new(256);
    (
        RtHost { engine, cmd_rx, gc_tx, mon_tx, rec_tx, shared, audio_cpu, pinned: false, router: Box::default(), router_rx, router_gc: router_gc_tx, direct_rx },
        RtLinks { cmd_tx, gc_rx, mon_rx, rec_rx, router_tx, router_gc, direct_tx },
    )
}

impl RtHost {
    #[inline]
    fn begin(&mut self) {
        self.begin_with(&mut |_, _, _| {});
    }

    /// Applies queued commands; console MIDI goes through the router like device input.
    /// `out(dst, bytes, time)` writes to external router output `dst` (1-based).
    #[inline]
    fn begin_with(&mut self, out: &mut dyn FnMut(u8, &[u8], u32)) {
        if !self.pinned {
            self.pinned = true;
            self.pin();
        }
        // A new router table: swap it in and hand the old one back to be freed outside.
        while let Ok(t) = self.router_rx.pop() {
            let old = std::mem::replace(&mut self.router, t);
            let _ = self.router_gc.push(old);
        }
        while let Ok(d) = self.direct_rx.pop() {
            let n = (d[1] as usize).min(3);
            if d[0] >= 1 && n > 0 {
                out(d[0], &d[2..2 + n], 0);
            }
        }
        while let Ok(c) = self.cmd_rx.pop() {
            if let Cmd::Midi { port, len, bytes } = c {
                if port != pilab_synth::PORT_AUDITION && (len as usize) <= 3 {
                    self.midi(port, &bytes[..len as usize], 0, out);
                    continue;
                }
            }
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

    /// One channel message from source `port` through the router.
    #[inline]
    fn midi(&mut self, port: u8, bytes: &[u8], time: u32, out: &mut dyn FnMut(u8, &[u8], u32)) {
        if bytes.is_empty() || bytes.len() > 3 || bytes[0] < 0x80 || bytes[0] >= 0xF0 {
            return;
        }
        let (router, engine) = (&self.router, &mut self.engine);
        let to = router.route(port, bytes, |d, b, n| {
            if d == DST_SYNTH {
                engine.midi(port, &b[..n]);
            } else {
                out(d, &b[..n], time);
            }
        });
        let mut b = [0u8; 3];
        b[..bytes.len()].copy_from_slice(bytes);
        let _ = self.mon_tx.push(MidiEv { port, len: bytes.len() as u8, b, t_ns: crate::measure::now_ns(), to });
    }

    /// Zero motion CCs into the router (source `zero-motion`); not logged (they are continuous).
    #[inline]
    fn motion(&mut self, bytes: &[u8], out: &mut dyn FnMut(u8, &[u8], u32)) {
        let (router, engine) = (&self.router, &mut self.engine);
        router.route(SRC_ZERO_MOTION, bytes, |d, b, n| {
            if d == DST_SYNTH {
                // into the synth as its own (virtual) port
                engine.midi(pilab_synth::PORT_VIRTUAL, &b[..n]);
            } else {
                out(d, &b[..n], 0);
            }
        });
    }

    /// Renders one block. `input` is the input strip's source (left, right), if any:
    /// it is metered, mixed into the output only when monitoring is on, and into the
    /// recording when recording of the input is on. Sums are clamped to ±1.
    #[inline]
    fn render(&mut self, l: &mut [f32], r: &mut [f32], input: Option<(&[f32], &[f32])>) {
        self.render_ext(l, r, input, None);
    }

    #[inline]
    fn render_ext(&mut self, l: &mut [f32], r: &mut [f32], input: Option<(&[f32], &[f32])>, ext: Option<(&[f32], &[f32])>) {
        self.engine.process_ext(l, r, ext);
        let g = f32::from_bits(self.shared.in_gain.load(Ordering::Relaxed));
        let monitor = self.shared.in_monitor.load(Ordering::Relaxed);
        let rec_in = self.shared.in_record.load(Ordering::Relaxed);
        let input = input.filter(|(a, b)| a.len() >= l.len() && b.len() >= l.len() && g.is_finite() && g > 0.0);
        if let Some((il, ir)) = input {
            let (mut pl, mut pr) = (0.0f32, 0.0f32);
            for i in 0..l.len() {
                pl = pl.max((il[i] * g).abs());
                pr = pr.max((ir[i] * g).abs());
            }
            self.shared.in_peak[0].fetch_max(pl.to_bits(), Ordering::Relaxed);
            self.shared.in_peak[1].fetch_max(pr.to_bits(), Ordering::Relaxed);
            if monitor {
                for i in 0..l.len() {
                    l[i] = (l[i] + il[i] * g).clamp(-1.0, 1.0);
                    r[i] = (r[i] + ir[i] * g).clamp(-1.0, 1.0);
                }
            }
        }
        if self.shared.rec_armed.load(Ordering::Relaxed) {
            if self.rec_tx.slots() >= l.len() * 2 {
                let extra = if !monitor && rec_in { input } else { None };
                for i in 0..l.len() {
                    let (a, b) = match extra {
                        Some((il, ir)) => ((l[i] + il[i] * g).clamp(-1.0, 1.0), (r[i] + ir[i] * g).clamp(-1.0, 1.0)),
                        None => (l[i], r[i]),
                    };
                    let _ = self.rec_tx.push(a);
                    let _ = self.rec_tx.push(b);
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
    /// MIDI input ports in the graph that are not ours (router destinations).
    fn midi_sinks(&self) -> Vec<String> {
        vec![]
    }
    fn output_ports(&self) -> [String; 2];
    /// Full name of meter input `k` (0..METER_PORTS), if the backend has meters.
    fn meter_port(&self, _k: usize) -> Option<String> {
        None
    }
    /// Full names of the input strip's ports (left, right), if the backend has them.
    fn input_strip_ports(&self) -> Option<[String; 2]> {
        None
    }
}

// ---------------------------------------------------------------- JACK

struct Proc {
    host: RtHost,
    midi: Vec<jack::Port<jack::MidiIn>>,
    out_l: jack::Port<jack::AudioOut>,
    out_r: jack::Port<jack::AudioOut>,
    meters: Vec<jack::Port<jack::AudioIn>>,
    in_l: jack::Port<jack::AudioIn>,
    in_r: jack::Port<jack::AudioIn>,
    zero_motion: jack::Port<jack::MidiOut>,
    zero_cc_seen: u32,
    outs: Vec<jack::Port<jack::MidiOut>>,
    eng_l: jack::Port<jack::AudioIn>,
    eng_r: jack::Port<jack::AudioIn>,
}

impl jack::ProcessHandler for Proc {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        // Router outputs: one writer per port for this cycle (no allocation).
        let mut it = self.outs.iter_mut();
        let mut writers: [Option<jack::MidiWriter>; ROUTER_OUTS] = std::array::from_fn(|_| it.next().map(|p| p.writer(ps)));
        let mut out = |d: u8, b: &[u8], time: u32| {
            if let Some(Some(w)) = writers.get_mut(d as usize - 1) {
                let _ = w.write(&jack::RawMidi { time, bytes: b });
            }
        };
        self.host.begin_with(&mut out);
        for (k, p) in self.midi.iter().enumerate() {
            for ev in p.iter(ps) {
                self.host.midi(k as u8, ev.bytes, ev.time, &mut out);
            }
        }
        // Live Zero motion as CC 16/17/18 on a patch-panel MIDI output (only when the value
        // changes), and into the router as the `zero-motion` source.
        {
            let gen = self.host.shared.zero_cc_gen.load(Ordering::Relaxed);
            if gen != self.zero_cc_seen {
                self.zero_cc_seen = gen;
                let mut w = self.zero_motion.writer(ps);
                let packed = self.host.shared.zero_cc.load(Ordering::Relaxed);
                let (p, r, t) = ((packed & 0xff) as u8, ((packed >> 8) & 0xff) as u8, ((packed >> 16) & 0xff) as u8);
                for m in [[0xB0, 16, p], [0xB0, 17, r], [0xB0, 18, t]] {
                    let _ = w.write(&jack::RawMidi { time: 0, bytes: &m });
                    self.host.motion(&m, &mut out);
                }
            }
        }
        let l = self.out_l.as_mut_slice(ps);
        let r = self.out_r.as_mut_slice(ps);
        let (il, ir) = (self.in_l.as_slice(ps), self.in_r.as_slice(ps));
        let (el, er) = (self.eng_l.as_slice(ps), self.eng_r.as_slice(ps));
        self.host.render_ext(l, r, Some((il, ir)), Some((el, er)));
        // Meters: peak per block, max-held until pilabd reads it. No allocation, no locks.
        for (k, p) in self.meters.iter().enumerate() {
            let pk = p.as_slice(ps).iter().fold(0.0f32, |a, &x| a.max(x.abs()));
            if pk > 0.0 {
                self.host.shared.meters[k].fetch_max(pk.to_bits(), Ordering::Relaxed);
            }
        }
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
        let mut meters = Vec::new();
        for k in 0..METER_PORTS {
            meters.push(client.register_port(&format!("{METER_PREFIX}{}", k + 1), jack::AudioIn::default()).map_err(|e| format!("meter: {e:?}"))?);
        }
        let in_l = client.register_port(INPUT_PORTS[0], jack::AudioIn::default()).map_err(|e| format!("{e:?}"))?;
        let in_r = client.register_port(INPUT_PORTS[1], jack::AudioIn::default()).map_err(|e| format!("{e:?}"))?;
        let zero_motion = client.register_port(ZERO_MOTION_PORT, jack::MidiOut::default()).map_err(|e| format!("{e:?}"))?;
        let mut outs = Vec::new();
        for k in 0..ROUTER_OUTS {
            outs.push(client.register_port(&format!("{ROUTER_OUT_PREFIX}{}", k + 1), jack::MidiOut::default()).map_err(|e| format!("{e:?}"))?);
        }
        let eng_l = client.register_port(ENGINE_PORTS[0], jack::AudioIn::default()).map_err(|e| format!("{e:?}"))?;
        let eng_r = client.register_port(ENGINE_PORTS[1], jack::AudioIn::default()).map_err(|e| format!("{e:?}"))?;
        let engine = Engine::new(sr);
        let (host, links) = rt_pair(engine, shared.clone(), audio_cpu, sr);
        let stats = host.stats();
        let active = client
            .activate_async(Notes { shared }, Proc { host, midi, out_l, out_r, meters, in_l, in_r, zero_motion, zero_cc_seen: 0, outs, eng_l, eng_r })
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
                if is_meter_port(&name) {
                    continue;
                }
                if let Some(p) = c.port_by_name(&name) {
                    let fl = p.flags();
                    let (client, short) = name.split_once(':').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((String::new(), name.clone()));
                    out.push(GraphPort {
                        kind: kind_of(&p.port_type().unwrap_or_default()),
                        dir: if fl.contains(jack::PortFlags::IS_OUTPUT) { "out" } else { "in" },
                        physical: fl.contains(jack::PortFlags::IS_PHYSICAL),
                        connections: if fl.contains(jack::PortFlags::IS_OUTPUT) { p.get_connections().into_iter().filter(|c| !is_meter_port(c)).collect() } else { vec![] },
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
    fn meter_port(&self, k: usize) -> Option<String> {
        (k < METER_PORTS).then(|| format!("{CLIENT_NAME}:{METER_PREFIX}{}", k + 1))
    }
    fn input_strip_ports(&self) -> Option<[String; 2]> {
        Some(INPUT_PORTS.map(|n| format!("{CLIENT_NAME}:{n}")))
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
    fn midi_sinks(&self) -> Vec<String> {
        self.with(|c| {
            c.ports(None, Some("midi"), jack::PortFlags::IS_INPUT)
                .into_iter()
                .filter(|n| !n.starts_with(&format!("{CLIENT_NAME}:")))
                .collect()
        })
        .unwrap_or_default()
    }
    fn output_ports(&self) -> [String; 2] {
        [format!("{CLIENT_NAME}:out_l"), format!("{CLIENT_NAME}:out_r")]
    }
}

// ---------------------------------------------------------------- Null

pub struct NullBackend {
    sr: f32,
    stop: Arc<AtomicBool>,
    /// Development only: a recorded graph (`PILAB_DEMO_GRAPH=file.json`, the body of
    /// GET /graph from a real unit) that connect/disconnect edit in memory.
    demo: Option<Mutex<Vec<GraphPort>>>,
}

#[derive(serde::Deserialize)]
struct DemoPort {
    name: String,
    kind: String,
    dir: String,
    #[serde(default)]
    physical: bool,
    #[serde(default)]
    connections: Vec<String>,
}

fn load_demo_graph() -> Option<Vec<GraphPort>> {
    let path = std::env::var_os("PILAB_DEMO_GRAPH")?;
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let ports: Vec<DemoPort> = serde_json::from_value(v.get("ports")?.clone()).ok()?;
    Some(
        ports
            .into_iter()
            .map(|p| {
                let name = canon_port(&p.name);
                let (client, short) = name.split_once(':').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
                GraphPort {
                    name,
                    client,
                    short,
                    kind: p.kind,
                    dir: if p.dir == "out" { "out" } else { "in" },
                    physical: p.physical,
                    connections: p.connections.iter().map(|c| canon_port(c)).collect(),
                }
            })
            .collect(),
    )
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
                    host.render(&mut l, &mut r, None);
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
        (NullBackend { sr, stop, demo: load_demo_graph().map(Mutex::new) }, links, stats)
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
        if let Some(d) = &self.demo {
            return d.lock().unwrap().clone();
        }
        let mut v: Vec<GraphPort> = MIDI_PORTS
            .iter()
            .map(|n| GraphPort { name: format!("{CLIENT_NAME}:{n}"), client: CLIENT_NAME.into(), short: n.to_string(), kind: "midi".into(), dir: "in", physical: false, connections: vec![] })
            .collect();
        for n in ["out_l", "out_r"] {
            v.push(GraphPort { name: format!("{CLIENT_NAME}:{n}"), client: CLIENT_NAME.into(), short: n.into(), kind: "audio".into(), dir: "out", physical: false, connections: vec![] });
        }
        v.push(GraphPort { name: format!("{CLIENT_NAME}:{ZERO_MOTION_PORT}"), client: CLIENT_NAME.into(), short: ZERO_MOTION_PORT.into(), kind: "midi".into(), dir: "out", physical: false, connections: vec![] });
        v
    }
    fn connect(&self, from: &str, to: &str) -> Result<(), String> {
        let d = self.demo.as_ref().ok_or("no audio graph in the null backend")?;
        let mut g = d.lock().unwrap();
        let kind_in = g.iter().find(|p| p.name == to && p.dir == "in").map(|p| p.kind.clone()).ok_or("no such input")?;
        let src = g.iter_mut().find(|p| p.name == from && p.dir == "out").ok_or("no such output")?;
        if src.kind != kind_in {
            return Err("port types differ".into());
        }
        if src.connections.iter().any(|c| c == to) {
            return Err("Exists".into());
        }
        src.connections.push(to.to_string());
        Ok(())
    }
    fn disconnect(&self, from: &str, to: &str) -> Result<(), String> {
        let d = self.demo.as_ref().ok_or("no audio graph in the null backend")?;
        let mut g = d.lock().unwrap();
        let src = g.iter_mut().find(|p| p.name == from).ok_or("no such output")?;
        src.connections.retain(|c| c != to);
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_synth_port_names_map_to_the_new_node() {
        assert_eq!(canon_port("pilab-synth:out_l"), "Trillium Synth:out_l");
        assert_eq!(canon_port("pilab-synth:midi_in"), "Trillium Synth:midi_in");
        assert_eq!(canon_port("Built-in Audio Stereo:playback_FL"), "Built-in Audio Stereo:playback_FL");
        assert_eq!(canon_port("pilab-synthesizer:x"), "pilab-synthesizer:x");
        assert!(is_meter_port("Trillium Synth:meter_3"));
        assert!(!is_meter_port("Trillium Synth:midi_in"));
    }
}
