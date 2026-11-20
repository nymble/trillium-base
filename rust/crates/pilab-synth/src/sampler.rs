// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Sampler sound kind: multisample instruments loaded from SFZ (a practical subset) with
//! WAV or FLAC samples, preloaded into RAM as f32. Loading allocates and runs on the host;
//! the engine only reads an `Arc<Instrument>`.
//!
//! Supported SFZ: headers `<control> <global> <master> <group> <region>` with inheritance,
//! `#define $VAR value`, opcodes `sample default_path lokey hikey key pitch_keycenter
//! pitch_keytrack lovel hivel tune transpose volume pan offset end loop_mode loop_start
//! loop_end seq_length seq_position lorand hirand group off_by trigger ampeg_attack
//! ampeg_decay ampeg_sustain ampeg_release`. Other opcodes are counted and ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One decoded sample, interleaved when stereo.
pub struct Sample {
    pub data: Vec<f32>,
    pub channels: usize,
    pub frames: usize,
    pub rate: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    NoLoop,
    OneShot,
    Continuous,
    Sustain,
}

#[derive(Clone, Copy, Debug)]
pub struct Zone {
    pub sample: u32,
    pub lokey: u8,
    pub hikey: u8,
    pub lovel: u8,
    pub hivel: u8,
    /// Key at which the sample plays at its own pitch, including `tune`/`transpose`.
    pub keycenter: f32,
    /// 0..1 (pitch_keytrack / 100).
    pub keytrack: f32,
    pub gain: f32,
    /// -1..1
    pub pan: f32,
    pub loop_mode: LoopMode,
    pub loop_start: u32,
    pub loop_end: u32,
    pub offset: u32,
    /// Last frame + 1.
    pub end: u32,
    pub seq_length: u8,
    pub seq_position: u8,
    pub lorand: f32,
    pub hirand: f32,
    pub group: u32,
    pub off_by: u32,
}

/// Envelope hints from the SFZ global/group level (seconds, sustain 0..1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EnvHint {
    pub attack: Option<f32>,
    pub decay: Option<f32>,
    pub sustain: Option<f32>,
    pub release: Option<f32>,
}

pub struct Instrument {
    pub name: String,
    pub samples: Vec<Sample>,
    pub zones: Vec<Zone>,
    pub env: EnvHint,
    /// Opcodes seen but not supported, with counts.
    pub unsupported: BTreeMap<String, usize>,
    pub bytes: usize,
}

impl Instrument {
    pub fn is_empty(&self) -> bool {
        self.zones.is_empty()
    }

    /// Zones that a note starts, in file order, honouring velocity, round robin and random
    /// ranges. Allocation-free; writes up to `out.len()` zone indexes.
    pub fn select(&self, key: u8, vel: u8, rr: u32, rand: f32, out: &mut [u16]) -> usize {
        let mut n = 0;
        for (i, z) in self.zones.iter().enumerate() {
            if n == out.len() {
                break;
            }
            if key < z.lokey || key > z.hikey || vel < z.lovel || vel > z.hivel {
                continue;
            }
            if z.seq_length > 1 && (rr % z.seq_length as u32) + 1 != z.seq_position as u32 {
                continue;
            }
            if rand < z.lorand || rand >= z.hirand {
                continue;
            }
            out[n] = i as u16;
            n += 1;
        }
        n
    }

    /// A short decimated view of the sample a note would play, for the console.
    pub fn preview(&self, key: u8, points: usize) -> Vec<f32> {
        let mut idx = [0u16; 1];
        if self.select(key, 100, 0, 0.0, &mut idx) == 0 && self.select(key, 127, 0, 0.0, &mut idx) == 0 {
            return vec![0.0; points];
        }
        let z = &self.zones[idx[0] as usize];
        let s = &self.samples[z.sample as usize];
        let n = (z.end as usize).min(s.frames).max(1);
        (0..points)
            .map(|i| {
                let a = i * n / points;
                let b = ((i + 1) * n / points).max(a + 1).min(n);
                let mut pk = 0f32;
                for f in a..b {
                    let v = s.data[f * s.channels];
                    if v.abs() > pk.abs() {
                        pk = v;
                    }
                }
                pk
            })
            .collect()
    }
}

/// 4-point, 3rd-order Hermite interpolation of one channel at fractional frame `pos`.
/// Frames past a loop end wrap into the loop when `wrap` is set; elsewhere they clamp.
#[inline]
pub fn read(s: &Sample, z: &Zone, ch: usize, pos: f64, wrap: bool) -> f32 {
    let i = pos.floor() as i64;
    let t = (pos - i as f64) as f32;
    let last = (z.end as i64 - 1).max(0);
    let (ls, le) = (z.loop_start as i64, z.loop_end as i64);
    let get = |k: i64| -> f32 {
        let mut k = k;
        if wrap && le > ls && k > le {
            k -= le - ls + 1;
        }
        let k = k.clamp(0, last) as usize;
        s.data[k * s.channels + ch.min(s.channels - 1)]
    };
    let (xm1, x0, x1, x2) = (get(i - 1), get(i), get(i + 1), get(i + 2));
    let c1 = 0.5 * (x1 - xm1);
    let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
    let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
    ((c3 * t + c2) * t + c1) * t + x0
}

// ------------------------------------------------------------------ decoding

/// Reads a WAV (PCM 8/16/24/32-bit or float32, any channel count; first two channels kept).
pub fn read_wav(b: &[u8]) -> Result<Sample, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let mut pos = 12;
    let (mut tag, mut chans, mut rate, mut bits) = (0u16, 0usize, 0u32, 0u16);
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let len = u32::from_le_bytes([b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]]) as usize;
        let body = &b[pos + 8..(pos + 8 + len).min(b.len())];
        if id == b"fmt " && body.len() >= 16 {
            tag = u16::from_le_bytes([body[0], body[1]]);
            chans = u16::from_le_bytes([body[2], body[3]]) as usize;
            rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            bits = u16::from_le_bytes([body[14], body[15]]);
            if tag == 0xFFFE && body.len() >= 26 {
                tag = u16::from_le_bytes([body[24], body[25]]);
            }
        } else if id == b"data" {
            data = Some(body);
        }
        pos += 8 + len + (len & 1);
    }
    let d = data.ok_or("no data chunk")?;
    if chans == 0 || rate == 0 {
        return Err("bad fmt chunk".into());
    }
    let bps = (bits as usize).div_ceil(8);
    let frame = bps * chans;
    let frames = d.len() / frame.max(1);
    let keep = chans.min(2);
    let mut out = Vec::with_capacity(frames * keep);
    for f in 0..frames {
        for c in 0..keep {
            let o = f * frame + c * bps;
            let x = &d[o..o + bps];
            let v = match (tag, bits) {
                (1, 8) => (x[0] as f32 - 128.0) / 128.0,
                (1, 16) => i16::from_le_bytes([x[0], x[1]]) as f32 / 32768.0,
                (1, 24) => ((i32::from_le_bytes([0, x[0], x[1], x[2]])) >> 8) as f32 / 8_388_608.0,
                (1, 32) => i32::from_le_bytes([x[0], x[1], x[2], x[3]]) as f32 / 2_147_483_648.0,
                (3, 32) => f32::from_le_bytes([x[0], x[1], x[2], x[3]]),
                _ => return Err(format!("unsupported WAV format tag {tag} / {bits} bits")),
            };
            out.push(v);
        }
    }
    Ok(Sample { data: out, channels: keep, frames, rate: rate as f32 })
}

pub fn read_flac(path: &Path) -> Result<Sample, String> {
    let mut r = claxon::FlacReader::open(path).map_err(|e| e.to_string())?;
    let info = r.streaminfo();
    let chans = info.channels as usize;
    let keep = chans.min(2);
    let scale = 1.0 / (1u64 << (info.bits_per_sample - 1)) as f32;
    let mut out = Vec::with_capacity(info.samples.unwrap_or(0) as usize * keep);
    let mut c = 0;
    for s in r.samples() {
        let s = s.map_err(|e| e.to_string())?;
        if c < keep {
            out.push(s as f32 * scale);
        }
        c += 1;
        if c == chans {
            c = 0;
        }
    }
    let frames = out.len() / keep.max(1);
    Ok(Sample { data: out, channels: keep, frames, rate: info.sample_rate as f32 })
}

pub fn read_audio(path: &Path) -> Result<Sample, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "flac" => read_flac(path),
        "wav" => read_wav(&std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?),
        other => Err(format!("unsupported sample format .{other}")),
    }
}

// ------------------------------------------------------------------ SFZ

/// SFZ note: a number, or a name like c4 / C#4 / eb3 (c4 = 60).
pub fn sfz_note(v: &str) -> Option<u8> {
    let v = v.trim();
    if let Ok(n) = v.parse::<i32>() {
        return (0..=127).contains(&n).then_some(n as u8);
    }
    let b = v.as_bytes();
    let base = match b.first()?.to_ascii_lowercase() {
        b'c' => 0,
        b'd' => 2,
        b'e' => 4,
        b'f' => 5,
        b'g' => 7,
        b'a' => 9,
        b'b' => 11,
        _ => return None,
    };
    let rest = &v[1..];
    let (acc, rest) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b').filter(|r| r.parse::<i32>().is_ok()) {
        (-1, r)
    } else {
        (0, rest)
    };
    let oct: i32 = rest.parse().ok()?;
    let n = (oct + 1) * 12 + base + acc;
    (0..=127).contains(&n).then_some(n as u8)
}

/// Parsed region: merged opcodes from every enclosing header.
pub type Opcodes = BTreeMap<String, String>;

/// Splits SFZ text into regions with inherited opcodes. Also returns `<control>` opcodes
/// and the global/group-level envelope hints (from the first region's inheritance).
pub fn parse_sfz(text: &str) -> (Vec<Opcodes>, Opcodes) {
    // Strip comments, apply #define.
    let mut defines: Vec<(String, String)> = vec![];
    let mut clean = String::with_capacity(text.len());
    for line in text.lines() {
        let line = match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        };
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("#define") {
            let mut it = rest.split_whitespace();
            if let (Some(k), Some(v)) = (it.next(), it.next()) {
                defines.push((k.to_string(), v.to_string()));
            }
            continue;
        }
        let mut l = line.to_string();
        for (k, v) in &defines {
            l = l.replace(k.as_str(), v);
        }
        clean.push_str(&l);
        clean.push('\n');
    }
    let mut control = Opcodes::new();
    let mut global = Opcodes::new();
    let mut master = Opcodes::new();
    let mut group = Opcodes::new();
    let mut regions: Vec<Opcodes> = vec![];
    let mut cur: Option<Opcodes> = None;
    let mut level = "";
    // Tokenise into headers and key=value pairs; values may contain spaces (sample paths),
    // so a value runs until the next " key=" or header.
    let mut i = 0;
    let bytes = clean.as_bytes();
    let flush = |cur: &mut Option<Opcodes>, regions: &mut Vec<Opcodes>| {
        if let Some(r) = cur.take() {
            regions.push(r);
        }
    };
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'<' {
            let end = clean[i..].find('>').map(|e| i + e).unwrap_or(bytes.len() - 1);
            let h = clean[i + 1..end].trim().to_ascii_lowercase();
            flush(&mut cur, &mut regions);
            match h.as_str() {
                "control" => level = "control",
                "global" => {
                    level = "global";
                    global.clear();
                    master.clear();
                    group.clear();
                }
                "master" => {
                    level = "master";
                    master.clear();
                    group.clear();
                }
                "group" => {
                    level = "group";
                    group.clear();
                }
                "region" => {
                    level = "region";
                    let mut r = global.clone();
                    r.extend(master.clone());
                    r.extend(group.clone());
                    cur = Some(r);
                }
                _ => level = "other",
            }
            i = end + 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // key=value
        let Some(eq) = clean[i..].find('=').map(|e| i + e) else { break };
        let key = clean[i..eq].trim().to_string();
        let mut j = eq + 1;
        // Value ends at newline, '<', or whitespace followed by "word=".
        let mut end = j;
        while j < bytes.len() {
            let ch = bytes[j];
            if ch == b'\n' || ch == b'<' {
                break;
            }
            if ch.is_ascii_whitespace() {
                let rest = &clean[j..];
                let t = rest.trim_start();
                let word: String = t.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                if !word.is_empty() && t[word.len()..].starts_with('=') {
                    break;
                }
            }
            j += 1;
            end = j;
        }
        let val = clean[eq + 1..end].trim().to_string();
        let target = match level {
            "control" => &mut control,
            "global" => &mut global,
            "master" => &mut master,
            "group" => &mut group,
            "region" => cur.as_mut().unwrap(),
            _ => {
                i = end;
                continue;
            }
        };
        target.insert(key, val);
        i = end;
    }
    flush(&mut cur, &mut regions);
    // Envelope hints from global+master+group-level defaults: use the first region's values.
    (regions, control)
}

fn f(o: &Opcodes, k: &str) -> Option<f32> {
    o.get(k).and_then(|v| v.trim().parse::<f32>().ok())
}

const SUPPORTED: &[&str] = &[
    "sample", "default_path", "lokey", "hikey", "key", "pitch_keycenter", "pitch_keytrack", "lovel", "hivel", "tune",
    "transpose", "volume", "pan", "offset", "end", "loop_mode", "loopmode", "loop_start", "loopstart", "loop_end",
    "loopend", "seq_length", "seq_position", "lorand", "hirand", "group", "off_by", "trigger", "ampeg_attack",
    "ampeg_decay", "ampeg_sustain", "ampeg_release",
];

/// Loads an SFZ file and every sample it names (deduplicated).
pub fn load_sfz(path: &Path) -> Result<Instrument, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let (regions, control) = parse_sfz(&text);
    let default_path = control.get("default_path").cloned().unwrap_or_default();
    let mut samples: Vec<Sample> = vec![];
    let mut by_path: BTreeMap<PathBuf, u32> = BTreeMap::new();
    let mut zones = vec![];
    let mut unsupported = BTreeMap::new();
    let mut env = EnvHint::default();
    for (ri, o) in regions.iter().enumerate() {
        for k in o.keys() {
            if !SUPPORTED.contains(&k.as_str()) && !k.starts_with("//") {
                *unsupported.entry(k.clone()).or_insert(0) += 1;
            }
        }
        if o.get("trigger").is_some_and(|t| t != "attack") {
            continue; // release triggers and the like are not played
        }
        let Some(name) = o.get("sample") else { continue };
        if name.starts_with('*') {
            continue; // generated waveforms (*sine, *noise) are not supported
        }
        let rel = format!("{default_path}{}", name.replace('\\', "/"));
        let p = dir.join(&rel);
        let si = match by_path.get(&p) {
            Some(&i) => i,
            None => {
                let s = read_audio(&p)?;
                samples.push(s);
                let i = (samples.len() - 1) as u32;
                by_path.insert(p.clone(), i);
                i
            }
        };
        let s = &samples[si as usize];
        let key = o.get("key").and_then(|v| sfz_note(v));
        let lokey = o.get("lokey").and_then(|v| sfz_note(v)).or(key).unwrap_or(0);
        let hikey = o.get("hikey").and_then(|v| sfz_note(v)).or(key).unwrap_or(127);
        let center = o.get("pitch_keycenter").and_then(|v| sfz_note(v)).or(key).unwrap_or(60) as f32;
        let tune = f(o, "tune").unwrap_or(0.0) / 100.0 + f(o, "transpose").unwrap_or(0.0);
        let lm = o.get("loop_mode").or(o.get("loopmode")).map(|s| s.as_str()).unwrap_or("");
        let ls = f(o, "loop_start").or(f(o, "loopstart")).unwrap_or(0.0) as u32;
        let le = f(o, "loop_end").or(f(o, "loopend")).unwrap_or(0.0) as u32;
        let loop_mode = match lm {
            "one_shot" => LoopMode::OneShot,
            "loop_continuous" if le > ls => LoopMode::Continuous,
            "loop_sustain" if le > ls => LoopMode::Sustain,
            _ => LoopMode::NoLoop,
        };
        let frames = s.frames as u32;
        let end = f(o, "end").map(|e| (e as u32 + 1).min(frames)).unwrap_or(frames);
        zones.push(Zone {
            sample: si,
            lokey,
            hikey,
            lovel: f(o, "lovel").unwrap_or(0.0).clamp(0.0, 127.0) as u8,
            hivel: f(o, "hivel").unwrap_or(127.0).clamp(0.0, 127.0) as u8,
            keycenter: center - tune,
            keytrack: f(o, "pitch_keytrack").unwrap_or(100.0) / 100.0,
            gain: 10f32.powf(f(o, "volume").unwrap_or(0.0).clamp(-144.0, 12.0) / 20.0),
            pan: (f(o, "pan").unwrap_or(0.0) / 100.0).clamp(-1.0, 1.0),
            loop_mode,
            loop_start: ls.min(end.saturating_sub(1)),
            loop_end: le.min(end.saturating_sub(1)),
            offset: (f(o, "offset").unwrap_or(0.0) as u32).min(end.saturating_sub(1)),
            end,
            seq_length: f(o, "seq_length").unwrap_or(1.0).clamp(1.0, 255.0) as u8,
            seq_position: f(o, "seq_position").unwrap_or(1.0).clamp(1.0, 255.0) as u8,
            lorand: f(o, "lorand").unwrap_or(0.0),
            hirand: f(o, "hirand").unwrap_or(1.0).max(1e-6),
            group: f(o, "group").unwrap_or(0.0).max(0.0) as u32,
            off_by: f(o, "off_by").unwrap_or(0.0).max(0.0) as u32,
        });
        if ri == 0 {
            env = EnvHint {
                attack: f(o, "ampeg_attack"),
                decay: f(o, "ampeg_decay"),
                sustain: f(o, "ampeg_sustain").map(|s| s / 100.0),
                release: f(o, "ampeg_release"),
            };
        }
    }
    if zones.is_empty() {
        return Err("no playable regions".into());
    }
    if zones.len() > u16::MAX as usize {
        return Err("too many regions".into());
    }
    let bytes = samples.iter().map(|s| s.data.len() * 4).sum();
    let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    Ok(Instrument { name, samples, zones, env, unsupported, bytes })
}

/// An instrument from in-memory samples and zones (tests, generated sounds).
pub fn instrument(name: &str, samples: Vec<Sample>, zones: Vec<Zone>) -> Instrument {
    let bytes = samples.iter().map(|s| s.data.len() * 4).sum();
    Instrument { name: name.into(), samples, zones, env: EnvHint::default(), unsupported: BTreeMap::new(), bytes }
}

impl Zone {
    /// A full-range zone for `sample` at `keycenter`, no loop.
    pub fn full(sample: u32, frames: u32, keycenter: f32) -> Zone {
        Zone {
            sample,
            lokey: 0,
            hikey: 127,
            lovel: 0,
            hivel: 127,
            keycenter,
            keytrack: 1.0,
            gain: 1.0,
            pan: 0.0,
            loop_mode: LoopMode::NoLoop,
            loop_start: 0,
            loop_end: 0,
            offset: 0,
            end: frames,
            seq_length: 1,
            seq_position: 1,
            lorand: 0.0,
            hirand: 1.0,
            group: 0,
            off_by: 0,
        }
    }
}
