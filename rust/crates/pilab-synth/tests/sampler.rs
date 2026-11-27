// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Sampler: SFZ parsing, zone selection, pitch, loops, one-shots and choke groups.

use std::sync::Arc;

use pilab_synth::sampler::{self, LoopMode, Sample, Zone};
use pilab_synth::*;

const SR: f32 = 48_000.0;

fn sine(hz: f32, rate: f32, secs: f32) -> Sample {
    let n = (rate * secs) as usize;
    let data = (0..n).map(|i| (i as f32 / rate * hz * std::f32::consts::TAU).sin() * 0.5).collect();
    Sample { data, channels: 1, frames: n, rate }
}

fn engine(inst: sampler::Instrument) -> Engine {
    let mut e = Engine::new(SR);
    let p = SlotParams {
        wave: Waveform::Sampler,
        filter: FilterParams { enabled: false, ..Default::default() },
        amp_env: Env { a: 0.001, d: 0.1, s: 1.0, r: 0.05 },
        vel_to_amp: 0.0,
        ..Default::default()
    };
    e.apply(Cmd::SetSlot(0, p));
    e.apply(Cmd::SetInstrument(0, Some(Arc::new(inst))));
    let mut rt = RouteTable::new();
    rt.push(CompiledRoute { port_mask: u32::MAX, slot: 0, ..Default::default() });
    e.apply(Cmd::SetRoutes(Box::new(rt)));
    e.apply(Cmd::Run(true));
    e
}

fn run(e: &mut Engine, secs: f32) -> Vec<f32> {
    let n = (secs * SR) as usize;
    let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
    for (cl, cr) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
        e.process(cl, cr);
    }
    l
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn hz(x: &[f32]) -> f64 {
    let (mut first, mut last, mut count) = (None, 0.0, 0);
    for i in 1..x.len() {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            let t = (i - 1) as f64 + (-x[i - 1] / (x[i] - x[i - 1])) as f64;
            if first.is_none() {
                first = Some(t);
            } else {
                count += 1;
            }
            last = t;
        }
    }
    count as f64 * SR as f64 / (last - first.unwrap())
}

#[test]
fn note_names() {
    assert_eq!(sampler::sfz_note("c4"), Some(60));
    assert_eq!(sampler::sfz_note("C#4"), Some(61));
    assert_eq!(sampler::sfz_note("eb3"), Some(51));
    assert_eq!(sampler::sfz_note("b3"), Some(59));
    assert_eq!(sampler::sfz_note("a-1"), Some(9));
    assert_eq!(sampler::sfz_note("36"), Some(36));
    assert_eq!(sampler::sfz_note("x"), None);
}

#[test]
fn sfz_inheritance_and_paths() {
    let text = "// comment\n<control> default_path=samples/\n#define $V 90\n<global> ampeg_release=0.7\n<group> lovel=1 hivel=$V loop_mode=one_shot\n<region> sample=Piano C4.flac lokey=c4 hikey=d4 pitch_keycenter=c4\n<region>sample=b.wav key=40 tune=-50 seq_length=2 seq_position=2\n<group> lovel=91\n<region> sample=c.wav key=40 // late\n";
    let (regions, control) = sampler::parse_sfz(text);
    assert_eq!(control.get("default_path").unwrap(), "samples/");
    assert_eq!(regions.len(), 3);
    assert_eq!(regions[0].get("sample").unwrap(), "Piano C4.flac");
    assert_eq!(regions[0].get("hivel").unwrap(), "90");
    assert_eq!(regions[0].get("ampeg_release").unwrap(), "0.7");
    assert_eq!(regions[1].get("tune").unwrap(), "-50");
    assert_eq!(regions[1].get("loop_mode").unwrap(), "one_shot");
    assert_eq!(regions[2].get("lovel").unwrap(), "91");
    assert!(regions[2].get("hivel").is_none());
}

fn write_wav(path: &std::path::Path, s: &[f32], rate: u32, chans: u16) {
    let mut b = vec![];
    let data: Vec<u8> = s.iter().flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    b.extend(b"RIFF");
    b.extend(((36 + data.len()) as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(chans.to_le_bytes());
    b.extend(rate.to_le_bytes());
    b.extend((rate * 2 * chans as u32).to_le_bytes());
    b.extend((2 * chans).to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((data.len() as u32).to_le_bytes());
    b.extend(data);
    std::fs::write(path, b).unwrap();
}

#[test]
fn loads_sfz_with_wav_samples_and_selects_zones() {
    let dir = std::env::temp_dir().join(format!("pilab-sfz-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("s")).unwrap();
    let a = sine(220.0, 44_100.0, 0.2);
    write_wav(&dir.join("s/lo.wav"), &a.data, 44_100, 1);
    let st: Vec<f32> = a.data.iter().flat_map(|v| [*v, -*v]).collect();
    write_wav(&dir.join("s/hi.wav"), &st, 44_100, 2);
    std::fs::write(
        dir.join("t.sfz"),
        "<group> ampeg_attack=0.3\n<region> sample=s/lo.wav lokey=0 hikey=59 pitch_keycenter=57 hivel=64\n<region> sample=s/lo.wav lokey=0 hikey=59 pitch_keycenter=57 lovel=65 volume=-6\n<region> sample=s/hi.wav lokey=60 hikey=127 pitch_keycenter=57 loop_mode=loop_continuous loop_start=100 loop_end=4000 opcode_x=1\n",
    )
    .unwrap();
    let inst = sampler::load_sfz(&dir.join("t.sfz")).unwrap();
    assert_eq!(inst.samples.len(), 2, "samples are shared between regions");
    assert_eq!(inst.samples[1].channels, 2);
    assert_eq!(inst.zones.len(), 3);
    assert_eq!(inst.env.attack, Some(0.3));
    assert_eq!(inst.unsupported.get("opcode_x"), Some(&1));
    assert_eq!(inst.zones[2].loop_mode, LoopMode::Continuous);
    let mut out = [0u16; 4];
    assert_eq!(inst.select(40, 30, 0, 0.5, &mut out), 1);
    assert_eq!(out[0], 0);
    assert_eq!(inst.select(40, 100, 0, 0.5, &mut out), 1);
    assert_eq!(out[0], 1);
    assert!((inst.zones[1].gain - 0.501).abs() < 0.01);
    assert_eq!(inst.select(72, 100, 0, 0.5, &mut out), 1);
    assert_eq!(out[0], 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pitch_follows_key_and_sample_rate() {
    let s = sine(440.0, 44_100.0, 2.0);
    let frames = s.frames as u32;
    let mut e = engine(sampler::instrument("s", vec![s], vec![Zone::full(0, frames, 69.0)]));
    e.midi(0, &[0x90, 81, 100]);
    let x = run(&mut e, 0.5);
    let f = hz(&x[4800..]);
    assert!((f - 880.0).abs() < 1.0, "{f}");
    e.midi(0, &[0x80, 81, 0]);
    e.midi(0, &[0x90, 62, 100]);
    let x = run(&mut e, 0.4);
    let f = hz(&x[9600..]);
    assert!((f - 293.66).abs() < 0.6, "{f}");
}

#[test]
fn unlooped_sample_ends_and_loop_sustains() {
    let s = sine(440.0, SR, 0.1);
    let frames = s.frames as u32;
    let mut z = Zone::full(0, frames, 69.0);
    let mut e = engine(sampler::instrument("s", vec![sine(440.0, SR, 0.1)], vec![z]));
    e.midi(0, &[0x90, 69, 100]);
    let x = run(&mut e, 0.3);
    assert!(rms(&x[1000..4000]) > 0.01);
    assert!(rms(&x[6000..]) < 1e-4, "sample end silences the voice");
    assert_eq!(e.active_voices(), 0);
    z.loop_mode = LoopMode::Sustain;
    z.loop_start = 1200;
    z.loop_end = 1200 + 109 * 20 - 1; // whole cycles of 440 Hz at 48 kHz (approx.)
    let mut e = engine(sampler::instrument("s", vec![s], vec![z]));
    e.midi(0, &[0x90, 69, 100]);
    let x = run(&mut e, 1.0);
    assert!(rms(&x[40_000..]) > 0.01, "looping voice keeps sounding");
    e.midi(0, &[0x80, 69, 0]);
    let x = run(&mut e, 0.5);
    assert!(rms(&x[12_000..]) < 1e-4, "release ends it");
}

#[test]
fn one_shot_ignores_note_off_and_choke_groups_cut() {
    let s = sine(300.0, SR, 0.5);
    let frames = s.frames as u32;
    let mut open = Zone::full(0, frames, 60.0);
    open.lokey = 46;
    open.hikey = 46;
    open.loop_mode = LoopMode::OneShot;
    open.group = 2;
    open.off_by = 1;
    let mut closed = open;
    closed.lokey = 42;
    closed.hikey = 42;
    closed.group = 1;
    closed.off_by = 0;
    let mut e = engine(sampler::instrument("kit", vec![s], vec![open, closed]));
    e.midi(0, &[0x99, 46, 100]);
    e.midi(0, &[0x89, 46, 0]);
    let x = run(&mut e, 0.2);
    assert!(rms(&x[4800..]) > 0.01, "one-shot plays through note off");
    assert_eq!(e.active_voices(), 1);
    e.midi(0, &[0x99, 42, 100]);
    run(&mut e, 0.1);
    assert_eq!(e.active_voices(), 1, "closed hat chokes the open hat");
}

#[test]
fn round_robin_alternates() {
    let a = sine(200.0, SR, 0.3);
    let b = sine(400.0, SR, 0.3);
    let n = a.frames as u32;
    let mut z1 = Zone::full(0, n, 60.0);
    z1.seq_length = 2;
    z1.seq_position = 1;
    let mut z2 = Zone::full(1, n, 60.0);
    z2.seq_length = 2;
    z2.seq_position = 2;
    let inst = sampler::instrument("rr", vec![a, b], vec![z1, z2]);
    let mut out = [0u16; 4];
    let picks: Vec<u16> = (0..4).map(|rr| {
        inst.select(60, 100, rr, 0.0, &mut out);
        out[0]
    }).collect();
    assert_eq!(picks, vec![0, 1, 0, 1]);
    let mut e = engine(inst);
    let mut f = vec![];
    for _ in 0..2 {
        e.midi(0, &[0x90, 60, 100]);
        let x = run(&mut e, 0.15);
        f.push(hz(&x[2000..]));
        e.midi(0, &[0x80, 60, 0]);
        run(&mut e, 0.2);
    }
    assert!((f[0] - 200.0).abs() < 1.0 && (f[1] - 400.0).abs() < 1.0, "{f:?}");
}

#[test]
fn hermite_interpolation_is_exact_on_frames_and_smooth_between() {
    let s = Sample { data: vec![0.0, 1.0, 0.0, -1.0, 0.0, 1.0], channels: 1, frames: 6, rate: SR };
    let z = Zone::full(0, 6, 60.0);
    for k in 0..6 {
        assert!((sampler::read(&s, &z, 0, k as f64, false) - s.data[k]).abs() < 1e-6);
    }
    let m = sampler::read(&s, &z, 0, 1.5, false);
    assert!(m > 0.4 && m < 0.8, "{m}");
}

#[test]
fn replacing_instrument_stops_its_voices() {
    let s = sine(440.0, SR, 1.0);
    let n = s.frames as u32;
    let mut e = engine(sampler::instrument("s", vec![s], vec![Zone::full(0, n, 69.0)]));
    e.midi(0, &[0x90, 69, 100]);
    run(&mut e, 0.05);
    assert_eq!(e.active_voices(), 1);
    let g = e.apply(Cmd::SetInstrument(0, None));
    assert!(matches!(g, Some(Garbage::Instrument(_))));
    assert_eq!(e.active_voices(), 0);
}

/// Loads real banks when `PILAB_SFZ_CHECK` lists SFZ paths (colon-separated). Skipped otherwise.
#[test]
fn loads_external_banks_when_listed() {
    let Ok(list) = std::env::var("PILAB_SFZ_CHECK") else { return };
    for p in list.split(':').filter(|p| !p.is_empty()) {
        let t = std::time::Instant::now();
        let inst = sampler::load_sfz(std::path::Path::new(p)).unwrap_or_else(|e| panic!("{p}: {e}"));
        eprintln!(
            "{p}: {} zones, {} samples, {:.1} MiB in RAM, {:?}, unsupported {:?}",
            inst.zones.len(),
            inst.samples.len(),
            inst.bytes as f64 / 1048576.0,
            t.elapsed(),
            inst.unsupported
        );
        let mut out = [0u16; 4];
        let key = if inst.select(60, 100, 0, 0.0, &mut out) > 0 {
            60
        } else {
            let z = inst.zones[inst.zones.len() / 2];
            ((z.lokey as u16 + z.hikey as u16) / 2) as u8
        };
        let mut e = engine(inst);
        e.midi(0, &[0x90, key, 100]);
        let x = run(&mut e, 0.5);
        assert!(rms(&x) > 1e-3, "{p} is silent at key {key}");
    }
}
