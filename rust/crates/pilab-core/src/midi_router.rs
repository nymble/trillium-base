// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! MIDI router: any-to-any routes between MIDI sources (USB MIDI devices, the Trillium Zero,
//! the console, Zero motion CCs) and destinations (the active scene, the built-in synth,
//! external engines such as FluidSynth and ZynAddSubFX, USB MIDI device outputs), with
//! channel filters and remapping, key-range splits, message-type filters, transposition and
//! per-device input profiles. Several routes to one destination merge; several routes from
//! one source split.
//!
//! [`RouterConfig`] is the saved form (JSON). [`compile`] resolves its names against the ports
//! present now into a fixed-size [`Table`] the realtime thread runs without allocating.

use serde::{Deserialize, Serialize};

pub const MAX_ROUTES: usize = 32;
pub const MAX_SRC: usize = 16;
/// External MIDI outputs the router can drive at once (JACK ports out_1..out_N).
pub const MAX_OUTS: usize = 6;
/// Source index for the Zero motion CCs generated inside the synth host.
pub const SRC_ZERO_MOTION: u8 = 10;
/// Source index for the console (on-screen pads and keys).
pub const SRC_CONSOLE: u8 = 8;
/// Destination 0 is the built-in synth; 1..=MAX_OUTS are external outputs.
pub const DST_SYNTH: u8 = 0;

pub const T_NOTE: u8 = 1;
pub const T_CC: u8 = 2;
pub const T_BEND: u8 = 4;
pub const T_PRESSURE: u8 = 8;
pub const T_PROGRAM: u8 = 16;
pub const TYPE_NAMES: [(&str, u8); 5] = [("note", T_NOTE), ("cc", T_CC), ("bend", T_BEND), ("pressure", T_PRESSURE), ("program", T_PROGRAM)];

pub const ENGINES: [&str; 2] = ["fluidsynth", "zynaddsubfx"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RouteCfg {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// "any", "console", "zero", "zero-motion", or a glob on the source port name.
    pub from: String,
    /// "scene", "synth", "fluidsynth", "zynaddsubfx", or a glob on a MIDI input port name.
    pub to: String,
    /// Channels 1..=16 that pass; empty = all.
    pub channels: Vec<u8>,
    /// Send on this channel (1..=16); 0 keeps the channel.
    pub to_channel: u8,
    pub note_lo: u8,
    pub note_hi: u8,
    /// "note", "cc", "bend", "pressure", "program"; empty = all.
    pub types: Vec<String>,
    pub transpose: i8,
}

impl Default for RouteCfg {
    fn default() -> Self {
        RouteCfg { id: String::new(), name: String::new(), enabled: true, from: "any".into(), to: "scene".into(), channels: vec![], to_channel: 0, note_lo: 0, note_hi: 127, types: vec![], transpose: 0 }
    }
}

/// Per-device input profile, applied before any route. `matches` is a glob on the source name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub matches: String,
    pub name: String,
    /// False ignores the device entirely.
    pub enabled: bool,
    /// Force every message onto this channel (1..=16); 0 keeps it.
    pub channel: u8,
    pub transpose: i8,
    pub velocity_scale: f32,
    /// Every note-on at this velocity (1..=127); 0 keeps it.
    pub velocity_fixed: u8,
    /// Message types dropped from this device.
    pub block: Vec<String>,
}

impl Default for Profile {
    fn default() -> Self {
        Profile { matches: String::new(), name: String::new(), enabled: true, channel: 0, transpose: 0, velocity_scale: 1.0, velocity_fixed: 0, block: vec![] }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RouterConfig {
    pub format: String,
    pub version: u32,
    pub routes: Vec<RouteCfg>,
    pub profiles: Vec<Profile>,
}

impl Default for RouterConfig {
    fn default() -> Self {
        RouterConfig {
            format: "pilab-midi-router".into(),
            version: 1,
            routes: vec![RouteCfg { id: "default".into(), name: "Every input to the scene".into(), ..RouteCfg::default() }],
            profiles: vec![],
        }
    }
}

pub fn types_mask(v: &[String]) -> Result<u8, String> {
    if v.is_empty() {
        return Ok(0xFF);
    }
    let mut m = 0;
    for t in v {
        m |= TYPE_NAMES.iter().find(|x| x.0 == t.trim().to_ascii_lowercase()).map(|x| x.1).ok_or_else(|| format!("unknown message type {t:?} (note, cc, bend, pressure, program)"))?;
    }
    Ok(m)
}

impl RouterConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.routes.len() > MAX_ROUTES {
            return Err(format!("at most {MAX_ROUTES} routes"));
        }
        let mut ids = std::collections::HashSet::new();
        for r in &self.routes {
            let at = if r.name.is_empty() { r.id.clone() } else { r.name.clone() };
            if r.id.is_empty() || r.id.len() > 40 || !r.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err(format!("route {at:?}: id must be 1-40 letters, digits, - or _"));
            }
            if !ids.insert(r.id.clone()) {
                return Err(format!("route id {:?} used twice", r.id));
            }
            if r.from.trim().is_empty() || r.to.trim().is_empty() || r.from.len() > 120 || r.to.len() > 120 {
                return Err(format!("route {at:?}: from and to are required"));
            }
            if r.from == "zero-motion" && (r.to == "scene" || r.to == "synth") {
                // allowed: motion CCs into the synth are fine (filters, CC maps)
            }
            if r.channels.iter().any(|c| !(1..=16).contains(c)) || r.to_channel > 16 {
                return Err(format!("route {at:?}: channels are 1-16"));
            }
            if r.note_lo > 127 || r.note_hi > 127 || r.note_lo > r.note_hi {
                return Err(format!("route {at:?}: note range must be low <= high within 0-127"));
            }
            types_mask(&r.types).map_err(|e| format!("route {at:?}: {e}"))?;
        }
        for p in &self.profiles {
            if p.matches.trim().is_empty() {
                return Err("profile needs a device match".into());
            }
            if p.channel > 16 || p.velocity_fixed > 127 || !(0.0..=4.0).contains(&p.velocity_scale) {
                return Err(format!("profile {:?}: channel 0-16, fixed velocity 0-127, scale 0-4", p.matches));
            }
            types_mask(&p.block).map_err(|e| format!("profile {:?}: {e}", p.matches))?;
        }
        Ok(())
    }

    pub fn new_id(&self, base: &str) -> String {
        let base: String = base.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(24).collect::<String>().to_ascii_lowercase();
        let base = if base.is_empty() { "route".to_string() } else { base };
        if !self.routes.iter().any(|r| r.id == base) {
            return base;
        }
        (2..).map(|n| format!("{base}-{n}")).find(|c| !self.routes.iter().any(|r| r.id == *c)).unwrap()
    }
}

// ---------------------------------------------------------------- compiled form

#[derive(Clone, Copy, Debug, Default)]
pub struct CRoute {
    pub src_mask: u32,
    pub dst: u8,
    pub ch_mask: u16,
    pub remap: u8,
    pub lo: u8,
    pub hi: u8,
    pub types: u8,
    pub transpose: i8,
}

#[derive(Clone, Copy, Debug)]
pub struct CProfile {
    pub active: bool,
    pub enabled: bool,
    pub remap: u8,
    pub transpose: i8,
    pub vel_scale: f32,
    pub vel_fixed: u8,
    pub block: u8,
}

impl Default for CProfile {
    fn default() -> Self {
        CProfile { active: false, enabled: true, remap: 0, transpose: 0, vel_scale: 1.0, vel_fixed: 0, block: 0 }
    }
}

/// Realtime table: plain arrays, `Copy`, no heap.
#[derive(Clone, Copy, Debug)]
pub struct Table {
    pub routes: [CRoute; MAX_ROUTES],
    pub n: usize,
    pub profiles: [CProfile; MAX_SRC],
}

impl Default for Table {
    fn default() -> Self {
        // Everything except Zero motion to the synth: the behaviour before the router existed.
        let mut t = Table { routes: [CRoute::default(); MAX_ROUTES], n: 1, profiles: [CProfile::default(); MAX_SRC] };
        t.routes[0] = CRoute { src_mask: all_inputs_mask(), dst: DST_SYNTH, ch_mask: 0xFFFF, remap: 0, lo: 0, hi: 127, types: 0xFF, transpose: 0 };
        t
    }
}

fn all_inputs_mask() -> u32 {
    // device ports 0..7 and the console (8); not audition (9), not Zero motion (10)
    0x1FF
}

pub fn msg_type(status: u8) -> u8 {
    match status & 0xF0 {
        0x80 | 0x90 | 0xA0 => T_NOTE,
        0xB0 => T_CC,
        0xE0 => T_BEND,
        0xD0 => T_PRESSURE,
        0xC0 => T_PROGRAM,
        _ => 0,
    }
}

impl Table {
    /// Routes one channel message from source `src`. `emit(dst, bytes, len)` is called once
    /// per matching route. Returns the destination bit mask (bit d = destination d), 0 if dropped.
    #[inline]
    pub fn route(&self, src: u8, bytes: &[u8], mut emit: impl FnMut(u8, [u8; 3], usize)) -> u32 {
        if bytes.is_empty() || bytes.len() > 3 || bytes[0] < 0x80 || bytes[0] >= 0xF0 {
            return 0;
        }
        let len = bytes.len();
        let mut b = [0u8; 3];
        b[..len].copy_from_slice(bytes);
        let ty = msg_type(b[0]);
        if let Some(p) = self.profiles.get(src as usize).filter(|p| p.active) {
            if !p.enabled || p.block & ty != 0 {
                return 0;
            }
            if p.remap > 0 {
                b[0] = (b[0] & 0xF0) | (p.remap - 1);
            }
            if ty == T_NOTE {
                if p.transpose != 0 {
                    let k = b[1] as i16 + p.transpose as i16;
                    if !(0..=127).contains(&k) {
                        return 0;
                    }
                    b[1] = k as u8;
                }
                if b[0] & 0xF0 == 0x90 && b[2] > 0 {
                    b[2] = if p.vel_fixed > 0 { p.vel_fixed } else { ((b[2] as f32 * p.vel_scale).round() as i32).clamp(1, 127) as u8 };
                }
            }
        }
        let ch = b[0] & 0x0F;
        let mut hit = 0u32;
        for r in &self.routes[..self.n] {
            if r.src_mask & (1 << src) == 0 || r.ch_mask & (1 << ch) == 0 || r.types & ty == 0 {
                continue;
            }
            let mut o = b;
            if ty == T_NOTE {
                if o[1] < r.lo || o[1] > r.hi {
                    continue;
                }
                if r.transpose != 0 {
                    let k = o[1] as i16 + r.transpose as i16;
                    if !(0..=127).contains(&k) {
                        continue;
                    }
                    o[1] = k as u8;
                }
            }
            if r.remap > 0 {
                o[0] = (o[0] & 0xF0) | (r.remap - 1);
            }
            emit(r.dst, o, len);
            hit |= 1 << r.dst;
        }
        hit
    }
}

/// What the ports look like now, for [`compile`].
pub struct Ports<'a> {
    /// Source name per source index (device ports 0..7; console 8; Zero motion 10).
    pub sources: &'a [Option<String>],
    /// Source indexes that are a Trillium Zero.
    pub zero_sources: &'a [u8],
    /// MIDI input ports of other programs and devices (full JACK names), for globs.
    pub sinks: &'a [String],
    /// Resolved port of each engine that is running, e.g. ("fluidsynth", "fluidsynth-midi:midi_00").
    pub engines: &'a [(String, String)],
    /// The active scene's engine ("" = the built-in synth).
    pub scene_engine: &'a str,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Compiled {
    /// Out port index (0-based) -> destination port name.
    pub outs: Vec<String>,
    /// Per route id: resolved sources and destination, or why it does nothing.
    pub status: Vec<RouteStatus>,
    #[serde(skip)]
    pub table: Table,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RouteStatus {
    pub id: String,
    pub sources: Vec<String>,
    pub dest: String,
    pub active: bool,
    pub note: String,
}

fn src_matches(pat: &str, idx: u8, name: &str, p: &Ports) -> bool {
    match pat {
        "any" | "*" => (idx as u32) < 9,
        "console" => idx == SRC_CONSOLE,
        "zero-motion" => idx == SRC_ZERO_MOTION,
        "zero" => p.zero_sources.contains(&idx),
        g => idx < 8 && crate::glob::matches(g, name),
    }
}

pub fn compile(cfg: &RouterConfig, p: &Ports) -> Compiled {
    let mut out = Compiled { table: Table { routes: [CRoute::default(); MAX_ROUTES], n: 0, profiles: [CProfile::default(); MAX_SRC] }, ..Default::default() };
    // Running engines always get an output, so the console can send them program changes.
    for e in p.engines.iter().take(MAX_OUTS) {
        if !out.outs.contains(&e.1) {
            out.outs.push(e.1.clone());
        }
    }
    for (i, s) in p.sources.iter().enumerate().take(MAX_SRC) {
        let Some(name) = s else { continue };
        if i >= 8 {
            continue;
        }
        if let Some(pr) = cfg.profiles.iter().find(|pr| crate::glob::matches(&pr.matches, name)) {
            out.table.profiles[i] = CProfile {
                active: true,
                enabled: pr.enabled,
                remap: pr.channel,
                transpose: pr.transpose,
                vel_scale: pr.velocity_scale,
                vel_fixed: pr.velocity_fixed,
                block: if pr.block.is_empty() { 0 } else { types_mask(&pr.block).unwrap_or(0) },
            };
        }
    }
    for r in &cfg.routes {
        let mut st = RouteStatus { id: r.id.clone(), ..Default::default() };
        let mut mask = 0u32;
        for (i, s) in p.sources.iter().enumerate().take(MAX_SRC) {
            let name = s.clone().unwrap_or_default();
            if (s.is_some() || i as u8 == SRC_CONSOLE || i as u8 == SRC_ZERO_MOTION) && src_matches(r.from.trim(), i as u8, &name, p) {
                mask |= 1 << i;
                st.sources.push(match i as u8 {
                    SRC_CONSOLE => "console".into(),
                    SRC_ZERO_MOTION => "zero-motion".into(),
                    _ => name,
                });
            }
        }
        let to = r.to.trim();
        let to = if to == "scene" { if p.scene_engine.is_empty() { "synth" } else { p.scene_engine } } else { to };
        let dst = if to == "synth" {
            st.dest = "synth".into();
            Some(DST_SYNTH)
        } else {
            let port = if ENGINES.contains(&to) {
                p.engines.iter().find(|e| e.0 == to).map(|e| e.1.clone())
            } else {
                p.sinks.iter().find(|s| crate::glob::matches(to, s)).cloned()
            };
            match port {
                None => {
                    st.note = if ENGINES.contains(&to) { format!("{to} is not running") } else { format!("no MIDI input matches {to:?}") };
                    None
                }
                Some(port) => {
                    st.dest = port.clone();
                    let k = match out.outs.iter().position(|o| *o == port) {
                        Some(k) => Some(k),
                        None if out.outs.len() < MAX_OUTS => {
                            out.outs.push(port);
                            Some(out.outs.len() - 1)
                        }
                        None => {
                            st.note = format!("more than {MAX_OUTS} external destinations");
                            None
                        }
                    };
                    k.map(|k| (k + 1) as u8)
                }
            }
        };
        if !r.enabled {
            st.note = "off".into();
        } else if mask == 0 && st.note.is_empty() {
            st.note = "no source connected".into();
        }
        if let (true, Some(dst), true) = (r.enabled, dst, mask != 0) {
            let mut ch_mask = 0u16;
            for c in &r.channels {
                ch_mask |= 1 << (c - 1);
            }
            out.table.routes[out.table.n] = CRoute {
                src_mask: mask,
                dst,
                ch_mask: if r.channels.is_empty() { 0xFFFF } else { ch_mask },
                remap: r.to_channel,
                lo: r.note_lo,
                hi: r.note_hi,
                types: types_mask(&r.types).unwrap_or(0xFF),
                transpose: r.transpose,
            };
            out.table.n += 1;
            st.active = true;
        }
        out.status.push(st);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ports<'a>(src: &'a [Option<String>], sinks: &'a [String], eng: &'a [(String, String)], scene: &'a str) -> Ports<'a> {
        Ports { sources: src, zero_sources: &[1], sinks, engines: eng, scene_engine: scene }
    }

    fn srcs() -> Vec<Option<String>> {
        let mut v = vec![None; 11];
        v[0] = Some("virtual".into());
        v[1] = Some("Trill XIAO MIDI".into());
        v[2] = Some("Launchkey Mini MIDI 1".into());
        v
    }

    #[test]
    fn default_routes_everything_to_the_synth() {
        let s = srcs();
        let c = compile(&RouterConfig::default(), &ports(&s, &[], &[], ""));
        let mut got = vec![];
        let hit = c.table.route(2, &[0x90, 60, 100], |d, b, n| got.push((d, b[..n].to_vec())));
        assert_eq!(hit, 1);
        assert_eq!(got, vec![(0, vec![0x90, 60, 100])]);
        assert_eq!(c.table.route(SRC_ZERO_MOTION, &[0xB0, 16, 5], |_, _, _| {}), 0, "motion stays out of the synth by default");
    }

    #[test]
    fn scene_engine_split_remap_and_filters() {
        let s = srcs();
        let sinks = vec!["Midi-Bridge:Volca Keys MIDI 1 (playback)".to_string()];
        let eng = vec![("fluidsynth".to_string(), "fluidsynth-midi:midi_00".to_string())];
        let cfg = RouterConfig {
            routes: vec![
                RouteCfg { id: "a".into(), from: "zero".into(), to: "scene".into(), ..Default::default() },
                RouteCfg { id: "lo".into(), from: "*Launchkey*".into(), to: "*Volca*".into(), note_hi: 59, to_channel: 3, types: vec!["note".into()], ..Default::default() },
                RouteCfg { id: "hi".into(), from: "*Launchkey*".into(), to: "synth".into(), note_lo: 60, transpose: 12, ..Default::default() },
                RouteCfg { id: "m".into(), from: "zero-motion".into(), to: "*Volca*".into(), channels: vec![1], ..Default::default() },
            ],
            ..Default::default()
        };
        cfg.validate().unwrap();
        let c = compile(&cfg, &ports(&s, &sinks, &eng, "fluidsynth"));
        assert_eq!(c.outs, vec!["fluidsynth-midi:midi_00".to_string(), "Midi-Bridge:Volca Keys MIDI 1 (playback)".to_string()]);
        let mut got = vec![];
        c.table.route(1, &[0x99, 38, 90], |d, b, n| got.push((d, b[..n].to_vec())));
        assert_eq!(got, vec![(1, vec![0x99, 38, 90])], "zero to the scene's engine (FluidSynth)");
        got.clear();
        c.table.route(2, &[0x90, 48, 80], |d, b, n| got.push((d, b[..n].to_vec())));
        assert_eq!(got, vec![(2, vec![0x92, 48, 80])], "low split to the Volca on channel 3");
        got.clear();
        c.table.route(2, &[0x90, 64, 80], |d, b, n| got.push((d, b[..n].to_vec())));
        assert_eq!(got, vec![(0, vec![0x90, 76, 80])], "high split to the synth an octave up");
        got.clear();
        c.table.route(2, &[0xB0, 1, 64], |d, b, n| got.push((d, b[..n].to_vec())));
        assert_eq!(got, vec![(0, vec![0xB0, 1, 64])], "CC skips the note-only route");
        assert_eq!(c.table.route(SRC_ZERO_MOTION, &[0xB0, 16, 5], |_, _, _| {}), 1 << 2);
    }

    #[test]
    fn profiles_and_missing_engines() {
        let s = srcs();
        let cfg = RouterConfig {
            routes: vec![RouteCfg { id: "z".into(), from: "any".into(), to: "zynaddsubfx".into(), ..Default::default() }, RouteCfg { id: "s".into(), ..Default::default() }],
            profiles: vec![Profile { matches: "*Launchkey*".into(), channel: 5, velocity_fixed: 100, block: vec!["bend".into()], ..Default::default() }],
            ..Default::default()
        };
        let c = compile(&cfg, &ports(&s, &[], &[], ""));
        assert!(!c.status[0].active && c.status[0].note.contains("not running"));
        let mut got = vec![];
        c.table.route(2, &[0x90, 60, 20], |d, b, n| got.push((d, b[..n].to_vec())));
        assert_eq!(got, vec![(0, vec![0x94, 60, 100])]);
        assert_eq!(c.table.route(2, &[0xE0, 0, 64], |_, _, _| {}), 0);
        let bad = RouterConfig { routes: vec![RouteCfg { id: "x".into(), channels: vec![17], ..Default::default() }], ..Default::default() };
        assert!(bad.validate().is_err());
    }
}
