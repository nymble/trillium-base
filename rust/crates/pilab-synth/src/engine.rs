// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! The realtime engine: voices, MIDI handling, master gain, limiter and soft clip.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use zpod_voices::dsp::{sat, Adsr, Svf};

use crate::osc::{Osc, OscMode, Waveform};
use crate::params::SlotParams;
use crate::routing::{Hit, RouteTable};
use crate::tuning::{db_to_gain, note_to_hz};
use crate::wavetable::{Bands, Wavetable};
use crate::params::Warp;
use crate::sampler::{self, Instrument, LoopMode};
use crate::wavetable::TableSet;
use crate::{BLOCK, ENGINE_SLOTS, MAX_LAYERS, MAX_PORTS, MAX_VOICES, MAX_ZONE_LAYERS};

const NO_ZONE: u16 = u16::MAX;

/// Hard ceiling for the master level unless the host was built to allow more.
pub const MAX_MASTER_DB: f32 = -6.0;
/// The master never starts above this level (headphone safety).
pub const START_MASTER_DB: f32 = -18.0;
/// Fade time for start/stop.
pub const RUN_RAMP_S: f32 = 0.05;
/// Limiter ceiling (-1 dBFS).
pub const LIMIT: f32 = 0.891;
const CTL: u32 = 16;
const XFADE_S: f32 = 0.005;

/// Control messages from the host to the audio thread.
pub enum Cmd {
    SetSlot(u8, SlotParams),
    SetTable(u8, Option<Arc<Wavetable>>),
    /// Wavetable frame set (scanned by `position`) for a slot.
    SetTableSet(u8, Option<Arc<TableSet>>),
    /// Sampler instrument for a slot. Voices of that slot stop at once.
    SetInstrument(u8, Option<Arc<Instrument>>),
    SetRoutes(Box<RouteTable>),
    SetMaster { db: f32 },
    SetTuning { a4_hz: f32, bend_range: f32 },
    SetOscMode(OscMode),
    Midi { port: u8, len: u8, bytes: [u8; 3] },
    Run(bool),
    Panic,
}

/// Owned data the audio thread hands back to be freed elsewhere.
pub enum Garbage {
    Table(Arc<Wavetable>),
    Instrument(Arc<Instrument>),
    TableSet(Arc<TableSet>),
    Routes(Box<RouteTable>),
}

/// Lock-free counters the host reads for the console.
#[derive(Default)]
pub struct Stats {
    pub voices: AtomicU32,
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    /// Peaks that stay until a measurement reads them (not drained by the status loop).
    sticky_l: AtomicU32,
    sticky_r: AtomicU32,
    pub limiter_db: AtomicU32,
    pub clips: AtomicU64,
    pub running: AtomicBool,
    pub master_db: AtomicU32,
    pub blocks: AtomicU64,
}

impl Stats {
    #[inline]
    fn hold_peak(a: &AtomicU32, v: f32) {
        a.fetch_max(v.abs().to_bits(), Ordering::Relaxed);
    }
    /// Peak since the last call, per channel (linear).
    pub fn take_peaks(&self) -> (f32, f32) {
        (
            f32::from_bits(self.peak_l.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.peak_r.swap(0, Ordering::Relaxed)),
        )
    }
    /// Peak since the last measurement read, not affected by status-loop drains.
    pub fn take_sticky_peaks(&self) -> (f32, f32) {
        (
            f32::from_bits(self.sticky_l.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.sticky_r.swap(0, Ordering::Relaxed)),
        )
    }
    pub fn f32(a: &AtomicU32) -> f32 {
        f32::from_bits(a.load(Ordering::Relaxed))
    }
}

#[derive(Clone, Copy, Default)]
struct Chan {
    bend: f32,
    modw: f32,
    sustain: bool,
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    held: bool,
    sustained: bool,
    port: u8,
    ch: u8,
    src_note: u8,
    key: u8,
    slot: u8,
    vel: f32,
    age: u64,
    osc: Osc,
    amp: Adsr,
    fenv: Adsr,
    svf: Svf,
    wave: Waveform,
    prev_wave: Waveform,
    xf: u32,
    dt: f32,
    level: usize,
    blevel: usize,
    pw: f32,
    bend_s: f32,
    gain_l: f32,
    gain_r: f32,
    ctl: u32,
    fe: f32,
    /// Sampler zone index, or NO_ZONE for oscillator voices.
    zone: u16,
    spos: f64,
    sstep: f64,
    svf_r: Svf,
    /// Unison copy phases and the per-voice LFO phase (0..1) and position.
    uph: [f64; 2],
    lfo: f32,
    pos: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Voice {
            active: false,
            held: false,
            sustained: false,
            port: 0,
            ch: 0,
            src_note: 0,
            key: 0,
            slot: 0,
            vel: 0.0,
            age: 0,
            osc: Osc::default(),
            amp: Adsr::default(),
            fenv: Adsr::default(),
            svf: Svf::default(),
            wave: Waveform::Saw,
            prev_wave: Waveform::Saw,
            xf: 0,
            dt: 0.0,
            level: 0,
            blevel: 0,
            pw: 0.5,
            bend_s: 0.0,
            gain_l: 0.0,
            gain_r: 0.0,
            ctl: 0,
            fe: 0.0,
            zone: NO_ZONE,
            spos: 0.0,
            sstep: 0.0,
            svf_r: Svf::default(),
            uph: [0.33, 0.67],
            lfo: 0.0,
            pos: 0.0,
        }
    }
}

pub struct Engine {
    sr: f32,
    voices: [Voice; MAX_VOICES],
    slots: [SlotParams; ENGINE_SLOTS],
    tables: [Option<Arc<Wavetable>>; ENGINE_SLOTS],
    instruments: [Option<Arc<Instrument>>; ENGINE_SLOTS],
    tablesets: [Option<Arc<TableSet>>; ENGINE_SLOTS],
    rr: [[u8; 128]; ENGINE_SLOTS],
    rng: u32,
    routes: Box<RouteTable>,
    bands: Arc<Bands>,
    osc_mode: OscMode,
    chans: [[Chan; 16]; MAX_PORTS],
    a4: f32,
    bend_range: f32,
    master: f32,
    master_target: f32,
    master_k: f32,
    run: f32,
    run_target: f32,
    run_step: f32,
    lim_gain: f32,
    lim_rel: f32,
    dc_x: [f32; 2],
    dc_y: [f32; 2],
    age: u64,
    xfade_len: u32,
    buf_l: [f32; BLOCK],
    buf_r: [f32; BLOCK],
    hits: [Hit; MAX_LAYERS],
    pub stats: Arc<Stats>,
}

impl Engine {
    /// A stopped engine at `sr` Hz with master at [`START_MASTER_DB`].
    pub fn new(sr: f32) -> Engine {
        let stats = Arc::new(Stats::default());
        stats.master_db.store(START_MASTER_DB.to_bits(), Ordering::Relaxed);
        Engine {
            sr,
            voices: [Voice::default(); MAX_VOICES],
            slots: [SlotParams::default(); ENGINE_SLOTS],
            tables: Default::default(),
            instruments: Default::default(),
            tablesets: Default::default(),
            rr: [[0; 128]; ENGINE_SLOTS],
            rng: 0x9E37_79B9,
            routes: Box::new(RouteTable::new()),
            bands: Bands::shared(),
            osc_mode: OscMode::Table,
            chans: [[Chan::default(); 16]; MAX_PORTS],
            a4: 440.0,
            bend_range: 2.0,
            master: db_to_gain(START_MASTER_DB),
            master_target: db_to_gain(START_MASTER_DB),
            master_k: 1.0 - (-1.0 / (0.02 * sr)).exp(),
            run: 0.0,
            run_target: 0.0,
            run_step: 1.0 / (RUN_RAMP_S * sr),
            lim_gain: 1.0,
            lim_rel: 1.0 - (-1.0 / (0.1 * sr)).exp(),
            dc_x: [0.0; 2],
            dc_y: [0.0; 2],
            age: 0,
            xfade_len: (XFADE_S * sr) as u32,
            buf_l: [0.0; BLOCK],
            buf_r: [0.0; BLOCK],
            hits: [Hit::default(); MAX_LAYERS],
            stats,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
    pub fn master_gain_target(&self) -> f32 {
        self.master_target
    }
    pub fn is_running(&self) -> bool {
        self.run_target > 0.0
    }
    /// True once a stop ramp has fully reached silence.
    pub fn is_silent(&self) -> bool {
        self.run <= 0.0 && self.run_target <= 0.0
    }

    /// Applies one control message. Never allocates; returns anything to free.
    pub fn apply(&mut self, c: Cmd) -> Option<Garbage> {
        match c {
            Cmd::SetSlot(i, p) => {
                if (i as usize) < ENGINE_SLOTS {
                    let mut p = p;
                    p.amp_env = p.amp_env.clamped();
                    p.filter_env = p.filter_env.clamped();
                    self.slots[i as usize] = p;
                    if !p.enabled {
                        self.release_slot(i);
                    }
                }
                None
            }
            Cmd::SetTable(i, t) => {
                if (i as usize) < ENGINE_SLOTS {
                    let old = core::mem::replace(&mut self.tables[i as usize], t);
                    return old.map(Garbage::Table);
                }
                t.map(Garbage::Table)
            }
            Cmd::SetTableSet(i, t) => {
                if (i as usize) < ENGINE_SLOTS {
                    let old = core::mem::replace(&mut self.tablesets[i as usize], t);
                    return old.map(Garbage::TableSet);
                }
                t.map(Garbage::TableSet)
            }
            Cmd::SetInstrument(i, t) => {
                if (i as usize) < ENGINE_SLOTS {
                    for v in self.voices.iter_mut().filter(|v| v.active && v.slot == i && v.zone != NO_ZONE) {
                        v.active = false;
                        v.held = false;
                        v.sustained = false;
                    }
                    let old = core::mem::replace(&mut self.instruments[i as usize], t);
                    return old.map(Garbage::Instrument);
                }
                t.map(Garbage::Instrument)
            }
            Cmd::SetRoutes(r) => Some(Garbage::Routes(core::mem::replace(&mut self.routes, r))),
            Cmd::SetMaster { db } => {
                let db = db.clamp(-80.0, MAX_MASTER_DB);
                self.master_target = db_to_gain(db);
                self.stats.master_db.store(db.to_bits(), Ordering::Relaxed);
                None
            }
            Cmd::SetTuning { a4_hz, bend_range } => {
                self.a4 = a4_hz.clamp(380.0, 480.0);
                self.bend_range = bend_range.clamp(0.0, 24.0);
                None
            }
            Cmd::SetOscMode(m) => {
                self.osc_mode = m;
                None
            }
            Cmd::Midi { port, len, bytes } => {
                self.midi(port, &bytes[..(len as usize).min(3)]);
                None
            }
            Cmd::Run(on) => {
                self.run_target = if on { 1.0 } else { 0.0 };
                self.stats.running.store(on, Ordering::Relaxed);
                if !on {
                    self.all_notes_off();
                }
                None
            }
            Cmd::Panic => {
                self.all_sound_off();
                None
            }
        }
    }

    fn release_slot(&mut self, slot: u8) {
        for v in self.voices.iter_mut().filter(|v| v.active && v.slot == slot) {
            v.amp.gate_off();
            v.fenv.gate_off();
            v.held = false;
            v.sustained = false;
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.amp.gate_off();
            v.fenv.gate_off();
            v.held = false;
            v.sustained = false;
        }
        for p in self.chans.iter_mut() {
            for c in p.iter_mut() {
                c.sustain = false;
            }
        }
    }

    /// Fast release of everything (a 3 ms release instead of a cut, to avoid a click).
    pub fn all_sound_off(&mut self) {
        let sr = self.sr;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.amp.set(0.001, 0.001, 0.0, 0.003, sr);
            v.amp.gate_off();
            v.held = false;
            v.sustained = false;
        }
        for p in self.chans.iter_mut() {
            for c in p.iter_mut() {
                *c = Chan::default();
            }
        }
    }

    /// Handles one complete MIDI 1.0 channel message from input port slot `port`.
    pub fn midi(&mut self, port: u8, m: &[u8]) {
        if m.is_empty() || (port as usize) >= MAX_PORTS {
            return;
        }
        let st = m[0] & 0xF0;
        let ch = m[0] & 0x0F;
        let d1 = m.get(1).copied().unwrap_or(0) & 0x7F;
        let d2 = m.get(2).copied().unwrap_or(0) & 0x7F;
        match st {
            0x90 if d2 > 0 => self.note_on(port, ch, d1, d2 as f32 / 127.0),
            0x80 | 0x90 => self.note_off(port, ch, d1),
            0xB0 => match d1 {
                1 => self.chans[port as usize][ch as usize].modw = d2 as f32 / 127.0,
                64 => {
                    let on = d2 >= 64;
                    self.chans[port as usize][ch as usize].sustain = on;
                    if !on {
                        for v in self.voices.iter_mut() {
                            if v.active && v.sustained && v.port == port && v.ch == ch {
                                v.sustained = false;
                                v.amp.gate_off();
                                v.fenv.gate_off();
                            }
                        }
                    }
                }
                120 => self.all_sound_off(),
                123 => self.all_notes_off(),
                _ => {}
            },
            0xE0 => {
                let v = ((d2 as i32) << 7 | d1 as i32) - 8192;
                self.chans[port as usize][ch as usize].bend = (v as f32 / 8192.0).clamp(-1.0, 1.0);
            }
            _ => {}
        }
    }

    fn note_on(&mut self, port: u8, ch: u8, note: u8, vel: f32) {
        let mut hits = self.hits;
        let n = self.routes.resolve(port, ch, note, &mut hits);
        for h in &hits[..n] {
            let s = h.slot as usize;
            if s >= ENGINE_SLOTS || !self.slots[s].enabled {
                continue;
            }
            let vel = (vel * h.vel_scale).clamp(0.0, 1.0);
            if self.slots[s].wave == Waveform::Sampler {
                self.start_sampler(port, ch, note, h.note, h.slot, vel);
            } else {
                self.start_voice(port, ch, note, h.note, h.slot, vel, NO_ZONE);
            }
        }
    }

    fn start_sampler(&mut self, port: u8, ch: u8, src: u8, key: u8, slot: u8, vel: f32) {
        let Some(inst) = self.instruments[slot as usize].clone() else { return };
        let rr = self.rr[slot as usize][key as usize] as u32;
        self.rr[slot as usize][key as usize] = self.rr[slot as usize][key as usize].wrapping_add(1);
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        let rand = (self.rng >> 8) as f32 / (1u32 << 24) as f32;
        let mut zs = [0u16; MAX_ZONE_LAYERS];
        let n = inst.select(key, (vel * 127.0).round().clamp(1.0, 127.0) as u8, rr, rand, &mut zs);
        for &zi in &zs[..n] {
            let z = inst.zones[zi as usize];
            if z.group != 0 {
                // Choke: voices whose off_by names this group stop quickly (open hi-hat).
                let sr = self.sr;
                for v in self.voices.iter_mut() {
                    if v.active && v.slot == slot && v.zone != NO_ZONE {
                        if inst.zones.get(v.zone as usize).is_some_and(|o| o.off_by == z.group) {
                            v.amp.set(0.001, 0.001, 0.0, 0.02, sr);
                            v.amp.gate_off();
                            v.held = false;
                            v.sustained = false;
                        }
                    }
                }
            }
            self.start_voice(port, ch, src, key, slot, vel, zi);
            // Start position for the new voice.
            if let Some(v) = self.voices.iter_mut().find(|v| v.active && v.age == self.age) {
                v.spos = z.offset as f64;
            }
        }
        drop(inst); // the slot still holds a reference; nothing is freed here
    }

    fn start_voice(&mut self, port: u8, ch: u8, src: u8, key: u8, slot: u8, vel: f32, zone: u16) {
        self.age += 1;
        let age = self.age;
        // Retrigger the same note on the same slot, else a free voice, else steal.
        let idx = self
            .voices
            .iter()
            .position(|v| v.active && v.port == port && v.ch == ch && v.src_note == src && v.slot == slot && v.zone == zone)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .unwrap_or_else(|| {
                let mut best = 0;
                let mut best_key = (true, u64::MAX);
                for (i, v) in self.voices.iter().enumerate() {
                    let k = (v.held || v.sustained, v.age);
                    if k < best_key {
                        best_key = k;
                        best = i;
                    }
                }
                best
            });
        let p = self.slots[slot as usize];
        let sr = self.sr;
        let bend = self.chans[port as usize][ch as usize].bend;
        let v = &mut self.voices[idx];
        if !v.active || v.zone != zone {
            v.uph = [0.33, 0.67];
            v.lfo = 0.0;
            v.osc.reset();
            v.svf.reset();
            v.svf_r.reset();
            v.amp.level = 0.0;
            v.fenv.level = 0.0;
            v.bend_s = bend;
            v.wave = p.wave;
            v.prev_wave = p.wave;
            v.xf = 0;
        }
        v.active = true;
        v.held = true;
        v.sustained = false;
        v.port = port;
        v.ch = ch;
        v.src_note = src;
        v.key = key;
        v.slot = slot;
        v.vel = vel;
        v.age = age;
        v.zone = zone;
        v.spos = 0.0;
        v.ctl = 0;
        v.amp.set(p.amp_env.a, p.amp_env.d, p.amp_env.s, p.amp_env.r, sr);
        v.fenv.set(p.filter_env.a, p.filter_env.d, p.filter_env.s, p.filter_env.r, sr);
        v.amp.gate_on();
        v.fenv.gate_on();
    }

    fn note_off(&mut self, port: u8, ch: u8, note: u8) {
        let sus = self.chans[port as usize][ch as usize].sustain;
        for v in self.voices.iter_mut() {
            if v.active && v.held && v.port == port && v.ch == ch && v.src_note == note {
                v.held = false;
                if v.zone != NO_ZONE {
                    let one_shot = self.instruments[v.slot as usize]
                        .as_deref()
                        .and_then(|i| i.zones.get(v.zone as usize))
                        .is_some_and(|z| z.loop_mode == LoopMode::OneShot);
                    if one_shot {
                        continue;
                    }
                }
                if sus {
                    v.sustained = true;
                } else {
                    v.amp.gate_off();
                    v.fenv.gate_off();
                }
            }
        }
    }

    /// Renders `l.len()` frames (l and r must be the same length). Allocation-free.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.process_ext(l, r, None);
    }

    /// Like [`Engine::process`], with an external stereo source (FluidSynth, ZynAddSubFX)
    /// mixed in before the master level, limiter and soft clip.
    pub fn process_ext(&mut self, l: &mut [f32], r: &mut [f32], ext: Option<(&[f32], &[f32])>) {
        let n = l.len().min(r.len());
        let ext = ext.filter(|(a, b)| a.len() >= n && b.len() >= n);
        let mut off = 0;
        while off < n {
            let len = (n - off).min(BLOCK);
            self.render_block(len);
            if let Some((el, er)) = ext {
                for i in 0..len {
                    let (a, b) = (el[off + i], er[off + i]);
                    if a.is_finite() && b.is_finite() {
                        self.buf_l[i] += a;
                        self.buf_r[i] += b;
                    }
                }
            }
            self.post(&mut l[off..off + len], &mut r[off..off + len], len);
            off += len;
        }
        self.stats.voices.store(self.active_voices() as u32, Ordering::Relaxed);
        self.stats.blocks.fetch_add(1, Ordering::Relaxed);
    }

    fn render_block(&mut self, len: usize) {
        self.buf_l[..len].fill(0.0);
        self.buf_r[..len].fill(0.0);
        let sr = self.sr;
        let a4 = self.a4;
        let br = self.bend_range;
        let xfl = self.xfade_len.max(1);
        let bands: &Bands = &self.bands;
        let blep_mode = self.osc_mode == OscMode::Blep;
        for v in self.voices.iter_mut() {
            if !v.active {
                continue;
            }
            let p = &self.slots[v.slot as usize];
            let table = self.tables[v.slot as usize].as_deref();
            let set = if p.wave == Waveform::Table { self.tablesets[v.slot as usize].as_deref() } else { None };
            let chan = self.chans[v.port as usize][v.ch as usize];
            if v.zone != NO_ZONE {
                let inst = self.instruments[v.slot as usize].as_deref();
                let zs = inst.and_then(|i| i.zones.get(v.zone as usize).map(|z| (i, z)));
                let Some((inst, z)) = zs else {
                    v.active = false;
                    v.held = false;
                    v.sustained = false;
                    continue;
                };
                render_sample_voice(v, p, chan, inst, z, sr, a4, br, len, &mut self.buf_l, &mut self.buf_r);
                continue;
            }
            for i in 0..len {
                if v.ctl == 0 {
                    v.ctl = CTL;
                    v.bend_s += (chan.bend - v.bend_s) * 0.35;
                    let note = v.key as f32 + p.octave as f32 * 12.0 + p.detune_cents * 0.01 + v.bend_s * br;
                    let f = note_to_hz(note, a4).min(sr * 0.49);
                    v.dt = f / sr;
                    if let Some(t) = set {
                        v.level = t.level_for_dt(v.dt);
                    } else if let Some(t) = table {
                        v.level = t.level_for_dt(v.dt);
                    }
                    if set.is_some() {
                        let lfo = (v.lfo * core::f32::consts::TAU).sin();
                        v.lfo += p.lfo_hz.clamp(0.0, 40.0) * CTL as f32 / sr;
                        if v.lfo >= 1.0 {
                            v.lfo -= 1.0;
                        }
                        v.pos = (p.position + p.pos_env * v.fe + p.pos_lfo * 0.5 * lfo + p.pos_mod * chan.modw).clamp(0.0, 1.0);
                    }
                    v.blevel = bands.saw.level_for_dt(v.dt);
                    v.pw = (p.pulse_width + chan.modw * p.pwm_mod).clamp(0.02, 0.98);
                    if p.wave != v.wave {
                        v.prev_wave = v.wave;
                        v.wave = p.wave;
                        v.xf = xfl;
                    }
                    if p.filter.enabled {
                        let oct = v.fe * p.filter.env_amount
                            + v.vel * p.filter.vel_amount
                            + p.filter.key_track * (v.key as f32 - 60.0) / 12.0;
                        let fc = (p.filter.cutoff_hz * oct.clamp(-10.0, 10.0).exp2()).clamp(20.0, sr * 0.45);
                        v.svf.set(fc, p.filter.resonance, sr);
                    }
                    let vg = (1.0 - p.vel_to_amp) + p.vel_to_amp * v.vel * v.vel;
                    let g = db_to_gain(p.level_db.clamp(-60.0, 6.0)) * vg;
                    let ang = (p.pan.clamp(-1.0, 1.0) + 1.0) * core::f32::consts::FRAC_PI_4;
                    v.gain_l = g * ang.cos();
                    v.gain_r = g * ang.sin();
                }
                v.ctl -= 1;
                let (warp, wa) = (p.warp, p.warp_amount);
                let (vpos, vdt, vpw, vlevel, vblevel) = (v.pos, v.dt, v.pw, v.level, v.blevel);
                let shape = |w: Waveform, ph: f64| {
                    let ph = warp_phase(warp, wa, ph);
                    if let (Waveform::Table, Some(ts)) = (w, set) {
                        ts.sample(vpos, vlevel, ph as f32)
                    } else if blep_mode {
                        Osc::shape(w, ph, vdt, vpw, table, vlevel)
                    } else {
                        Osc::shape_table(w, ph, vpw, bands, vblevel, table, vlevel)
                    }
                };
                let mut s = shape(v.wave, v.osc.phase);
                if v.xf > 0 {
                    let a = v.xf as f32 / xfl as f32;
                    let s0 = shape(v.prev_wave, v.osc.phase);
                    s = s * (1.0 - a) + s0 * a;
                    v.xf -= 1;
                }
                let uni = p.unison.clamp(1, 3) as usize;
                if uni > 1 {
                    let r = (p.unison_detune.clamp(0.0, 100.0) / 1200.0).exp2() as f64;
                    let ratios = [r, 1.0 / r];
                    for k in 0..uni - 1 {
                        s += shape(v.wave, v.uph[k]);
                        v.uph[k] += v.dt as f64 * ratios[k];
                        if v.uph[k] >= 1.0 {
                            v.uph[k] -= 1.0;
                        }
                    }
                    s *= if uni == 2 { core::f32::consts::FRAC_1_SQRT_2 } else { 0.577 };
                }
                if warp == Warp::Fold && wa > 0.0 {
                    s = (s * (1.0 + wa * 5.0) * core::f32::consts::FRAC_PI_2).sin();
                }
                if p.drive > 0.0 {
                    let g = 1.0 + p.drive.clamp(0.0, 1.0) * 8.0;
                    s = soft_clip(s * g) / (1.0 + p.drive.clamp(0.0, 1.0) * 1.5);
                }
                v.osc.phase += v.dt as f64;
                if v.osc.phase >= 1.0 {
                    v.osc.phase -= 1.0;
                }
                v.fe = v.fenv.next();
                let env = v.amp.next();
                let y = if p.filter.enabled { v.svf.run(s).0 } else { s } * env;
                self.buf_l[i] += y * v.gain_l;
                self.buf_r[i] += y * v.gain_r;
            }
            if !v.amp.active() {
                v.active = false;
                v.held = false;
                v.sustained = false;
            }
        }
    }

    fn post(&mut self, l: &mut [f32], r: &mut [f32], len: usize) {
        let mut pl = 0f32;
        let mut pr = 0f32;
        let mut min_gain = 1f32;
        for i in 0..len {
            self.master += (self.master_target - self.master) * self.master_k;
            if self.run < self.run_target {
                self.run = (self.run + self.run_step).min(self.run_target);
            } else if self.run > self.run_target {
                self.run = (self.run - self.run_step).max(self.run_target);
            }
            let g = self.master * self.run;
            let mut x = [self.buf_l[i] * g, self.buf_r[i] * g];
            // DC blocker
            for c in 0..2 {
                let y = x[c] - self.dc_x[c] + 0.9995 * self.dc_y[c];
                self.dc_x[c] = x[c];
                self.dc_y[c] = if y.abs() < 1e-20 { 0.0 } else { y };
                x[c] = y;
            }
            // Peak limiter at -1 dBFS: instant attack, 100 ms release.
            let pk = x[0].abs().max(x[1].abs());
            let want = if pk > LIMIT { LIMIT / pk } else { 1.0 };
            if want < self.lim_gain {
                self.lim_gain = want;
            } else {
                self.lim_gain += (want - self.lim_gain) * self.lim_rel;
            }
            min_gain = min_gain.min(self.lim_gain);
            for c in x.iter_mut() {
                *c = soft_clip(*c * self.lim_gain);
            }
            if !x[0].is_finite() || !x[1].is_finite() {
                x = [0.0, 0.0];
                self.dc_x = [0.0; 2];
                self.dc_y = [0.0; 2];
                self.stats.clips.fetch_add(1, Ordering::Relaxed);
            }
            l[i] = x[0];
            r[i] = x[1];
            pl = pl.max(x[0].abs());
            pr = pr.max(x[1].abs());
        }
        Stats::hold_peak(&self.stats.peak_l, pl);
        Stats::hold_peak(&self.stats.peak_r, pr);
        Stats::hold_peak(&self.stats.sticky_l, pl);
        Stats::hold_peak(&self.stats.sticky_r, pr);
        let db = crate::tuning::gain_to_db(min_gain);
        self.stats.limiter_db.store(db.to_bits(), Ordering::Relaxed);
    }
}

/// Phase transform for the oscillator warp modes (Fold acts on the output instead).
#[inline]
fn warp_phase(w: Warp, a: f32, ph: f64) -> f64 {
    let a = a as f64;
    match w {
        Warp::None | Warp::Fold => ph,
        Warp::Bend => {
            // a > 0 compresses the start of the cycle, a < 0 the end.
            let e = (-(a.clamp(-1.0, 1.0)) * 2.0).exp2();
            ph.powf(e)
        }
        Warp::Sync => {
            let x = ph * (1.0 + a.clamp(0.0, 1.0) * 7.0);
            x - x.floor()
        }
        Warp::Mirror => {
            let m = if ph < 0.5 { 2.0 * ph } else { 2.0 - 2.0 * ph };
            let m = m.min(0.999_999);
            ph + (m - ph) * a.clamp(0.0, 1.0)
        }
        Warp::Asym => {
            let k = (0.5 + 0.45 * a.clamp(-1.0, 1.0)).clamp(0.05, 0.95);
            if ph < k { 0.5 * ph / k } else { 0.5 + 0.5 * (ph - k) / (1.0 - k) }
        }
    }
}

/// Renders one sampler voice into the mix buffers. Allocation-free.
#[allow(clippy::too_many_arguments)]
fn render_sample_voice(
    v: &mut Voice,
    p: &SlotParams,
    chan: Chan,
    inst: &Instrument,
    z: &sampler::Zone,
    sr: f32,
    a4: f32,
    br: f32,
    len: usize,
    out_l: &mut [f32],
    out_r: &mut [f32],
) {
    let smp = &inst.samples[z.sample as usize];
    let stereo = smp.channels > 1;
    let last = (z.end as f64 - 1.0).max(0.0);
    let looping = |v: &Voice| match z.loop_mode {
        LoopMode::Continuous => true,
        LoopMode::Sustain => v.held || v.sustained,
        _ => false,
    };
    let span = (z.loop_end as f64 - z.loop_start as f64 + 1.0).max(1.0);
    for i in 0..len {
        if v.ctl == 0 {
            v.ctl = CTL;
            v.bend_s += (chan.bend - v.bend_s) * 0.35;
            let semis = (v.key as f32 - z.keycenter) * z.keytrack
                + p.octave as f32 * 12.0
                + p.detune_cents * 0.01
                + v.bend_s * br;
            v.sstep = (smp.rate / sr) as f64 * (semis / 12.0).exp2() as f64 * (a4 / 440.0) as f64;
            if p.filter.enabled {
                let oct = v.fe * p.filter.env_amount
                    + v.vel * p.filter.vel_amount
                    + p.filter.key_track * (v.key as f32 - 60.0) / 12.0;
                let fc = (p.filter.cutoff_hz * oct.clamp(-10.0, 10.0).exp2()).clamp(20.0, sr * 0.45);
                v.svf.set(fc, p.filter.resonance, sr);
                v.svf_r.set(fc, p.filter.resonance, sr);
            }
            let vg = (1.0 - p.vel_to_amp) + p.vel_to_amp * v.vel * v.vel;
            let g = db_to_gain(p.level_db.clamp(-60.0, 6.0)) * vg * z.gain;
            let ang = ((p.pan + z.pan).clamp(-1.0, 1.0) + 1.0) * core::f32::consts::FRAC_PI_4;
            // Equal-power pan, normalised so centre is unity per side for stereo samples.
            v.gain_l = g * ang.cos() * core::f32::consts::SQRT_2;
            v.gain_r = g * ang.sin() * core::f32::consts::SQRT_2;
        }
        v.ctl -= 1;
        let lp = looping(v);
        if v.spos > last && !lp {
            v.amp.level = 0.0;
            v.active = false;
            break;
        }
        let sl = sampler::read(smp, z, 0, v.spos, lp);
        let sr_ = if stereo { sampler::read(smp, z, 1, v.spos, lp) } else { sl };
        v.spos += v.sstep;
        if lp && v.spos > z.loop_end as f64 {
            v.spos -= span;
        }
        v.fe = v.fenv.next();
        let env = v.amp.next();
        let (yl, yr) = if p.filter.enabled { (v.svf.run(sl).0, v.svf_r.run(sr_).0) } else { (sl, sr_) };
        out_l[i] += yl * env * v.gain_l;
        out_r[i] += yr * env * v.gain_r;
    }
    if !v.amp.active() || !v.active {
        v.active = false;
        v.held = false;
        v.sustained = false;
    }
}

/// Unity below 0.5, then a smooth knee into a bound of ±1 (uses zpod `sat`).
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.5 {
        x
    } else {
        let y = 0.5 + 0.5 * sat((a - 0.5) / 0.5);
        y.copysign(x)
    }
}
