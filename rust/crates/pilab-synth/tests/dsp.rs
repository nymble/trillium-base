// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Oscillator, tuning, wavetable and engine-safety tests.

use pilab_synth::engine::{soft_clip, MAX_MASTER_DB, START_MASTER_DB};
use pilab_synth::osc::{naive, Osc};
use pilab_synth::tuning::{db_to_gain, gain_to_db, note_to_hz};
use pilab_synth::*;
use rustfft::{num_complex::Complex, FftPlanner};

const SR: f32 = 48_000.0;

/// Power spectrum of exactly one second (1 Hz bins).
fn spectrum(x: &[f32]) -> Vec<f64> {
    let mut buf: Vec<Complex<f64>> = x.iter().map(|&v| Complex::new(v as f64, 0.0)).collect();
    FftPlanner::new().plan_fft_forward(buf.len()).process(&mut buf);
    buf[..buf.len() / 2].iter().map(|c| c.norm_sqr()).collect()
}

/// Alias power relative to harmonic power, in dB, for an integer-Hz fundamental.
fn alias_db(x: &[f32], f0: usize) -> f64 {
    let p = spectrum(x);
    let (mut harm, mut alias) = (0f64, 0f64);
    for (bin, &v) in p.iter().enumerate().skip(1) {
        let near_harm = (bin % f0 <= 1) || (f0 - bin % f0 <= 1);
        if near_harm {
            harm += v;
        } else {
            alias += v;
        }
    }
    10.0 * (alias / harm).log10()
}

#[derive(Clone, Copy, PartialEq)]
enum M {
    Naive,
    Blep,
    Table,
}

fn render_osc(w: Waveform, f0: f32, pw: f32, m: M) -> Vec<f32> {
    let mut o = Osc::default();
    let dt = f0 / SR;
    let bands = Bands::shared();
    (0..SR as usize)
        .map(|_| {
            if m == M::Table {
                o.next_table(w, dt, pw, &bands, None, 0)
            } else if m == M::Blep {
                o.next(w, dt, pw, None, 0)
            } else {
                let s = naive(w, o.phase as f32, pw);
                o.phase = (o.phase + dt as f64).fract();
                s
            }
        })
        .collect()
}

#[test]
fn note_to_hz_is_12tet() {
    assert!((note_to_hz(69.0, 440.0) - 440.0).abs() < 1e-3);
    assert!((note_to_hz(60.0, 440.0) - 261.6256).abs() < 1e-3);
    assert!((note_to_hz(81.0, 440.0) - 880.0).abs() < 1e-3);
    assert!((note_to_hz(21.0, 440.0) - 27.5).abs() < 1e-4);
    assert!((note_to_hz(108.0, 440.0) - 4186.009).abs() < 1e-2);
    assert!((note_to_hz(69.0, 432.0) - 432.0).abs() < 1e-3);
    assert!((note_to_hz(71.0, 440.0) - 493.8833).abs() < 1e-3);
    assert!((gain_to_db(db_to_gain(-18.0)) + 18.0).abs() < 1e-4);
}

#[test]
fn band_limited_shapes_alias_less_than_naive() {
    for &(w, pw) in &[(Waveform::Saw, 0.5), (Waveform::Square, 0.5), (Waveform::Pulse, 0.3), (Waveform::Triangle, 0.5)] {
        for &f0 in &[441usize, 1999, 4001] {
            let tb = alias_db(&render_osc(w, f0 as f32, pw, M::Table), f0);
            let bl = alias_db(&render_osc(w, f0 as f32, pw, M::Blep), f0);
            let nv = alias_db(&render_osc(w, f0 as f32, pw, M::Naive), f0);
            println!("{:>8} f0={f0:>5}: table {tb:7.1} dB, polyBLEP {bl:7.1} dB, naive {nv:7.1} dB", w.name());
            assert!(bl < nv - 6.0, "{} at {f0} Hz: polyBLEP {bl:.1} dB vs naive {nv:.1} dB", w.name());
            assert!(tb < -60.0, "{} at {f0} Hz table alias floor {tb:.1} dB", w.name());
        }
    }
    let s = alias_db(&render_osc(Waveform::Sine, 4001.0, 0.5, M::Table), 4001);
    println!("sine f0= 4001: {s:.1} dB");
    assert!(s < -80.0, "sine alias {s}");
}

#[test]
fn shapes_are_bounded_and_dc_free() {
    for w in [Waveform::Sine, Waveform::Saw, Waveform::Square, Waveform::Pulse, Waveform::Triangle] {
        let x = render_osc(w, 441.0, 0.25, M::Table);
        let peak = x.iter().fold(0f32, |a, v| a.max(v.abs()));
        let mean = x.iter().map(|&v| v as f64).sum::<f64>() / x.len() as f64;
        let bound = if w == Waveform::Pulse { 1.8 } else { 1.2 };
        assert!(peak < bound, "{} peak {peak}", w.name());
        assert!(mean.abs() < 0.01, "{} dc {mean}", w.name());
    }
}

/// Frequency from interpolated rising zero crossings.
fn measure_hz(x: &[f32], sr: f32) -> f64 {
    let mut first = None;
    let mut last = 0f64;
    let mut count = 0usize;
    for i in 1..x.len() {
        if x[i - 1] < 0.0 && x[i] >= 0.0 {
            let t = (i - 1) as f64 + (-x[i - 1] as f64) / (x[i] as f64 - x[i - 1] as f64);
            if first.is_none() {
                first = Some(t);
            } else {
                count += 1;
            }
            last = t;
        }
    }
    count as f64 * sr as f64 / (last - first.unwrap())
}

fn sine_engine() -> Engine {
    let mut e = Engine::new(SR);
    let p = SlotParams { wave: Waveform::Sine, filter: FilterParams { enabled: false, ..Default::default() }, amp_env: Env { a: 0.001, d: 0.1, s: 1.0, r: 0.1 }, ..Default::default() };
    e.apply(Cmd::SetSlot(0, p));
    let mut rt = RouteTable::new();
    rt.push(CompiledRoute { port_mask: u32::MAX, slot: 0, ..Default::default() });
    e.apply(Cmd::SetRoutes(Box::new(rt)));
    e.apply(Cmd::Run(true));
    e
}

fn run(e: &mut Engine, secs: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (secs * SR) as usize;
    let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
    for (cl, cr) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
        e.process(cl, cr);
    }
    (l, r)
}

#[test]
fn engine_pitch_accuracy_and_bend() {
    let mut e = sine_engine();
    e.midi(1, &[0x90, 69, 100]);
    let (l, _) = run(&mut e, 2.0);
    let hz = measure_hz(&l[24_000..], SR);
    assert!((hz - 440.0).abs() < 0.01, "A4 measured {hz}");

    e.midi(1, &[0xE0, 0x7F, 0x7F]); // full bend up = +2 semitones
    let (l, _) = run(&mut e, 1.0);
    let hz = measure_hz(&l[12_000..], SR);
    assert!((hz - 493.883).abs() < 0.05, "bent {hz}");

    e.apply(Cmd::SetTuning { a4_hz: 432.0, bend_range: 2.0 });
    e.midi(1, &[0xE0, 0x00, 0x40]);
    let (l, _) = run(&mut e, 1.0);
    let hz = measure_hz(&l[12_000..], SR);
    assert!((hz - 432.0).abs() < 0.02, "A4=432 measured {hz}");
}

#[test]
fn starts_quiet_ramps_and_caps_level() {
    let mut e = sine_engine();
    assert_eq!(e.stats.master_db.load(core::sync::atomic::Ordering::Relaxed), START_MASTER_DB.to_bits());
    assert!((e.master_gain_target() - db_to_gain(START_MASTER_DB)).abs() < 1e-6);
    e.midi(1, &[0x90, 69, 127]);
    let (l, _) = run(&mut e, 0.2);
    // Fade-in: the first millisecond stays far below the settled level.
    let early = l[..48].iter().fold(0f32, |a, v| a.max(v.abs()));
    let late = l[4800..].iter().fold(0f32, |a, v| a.max(v.abs()));
    assert!(early < late * 0.1, "early {early} late {late}");
    assert!(late <= db_to_gain(START_MASTER_DB) * 1.05, "settled peak {late} above -18 dBFS start level");

    e.apply(Cmd::SetMaster { db: 12.0 });
    assert!((e.master_gain_target() - db_to_gain(MAX_MASTER_DB)).abs() < 1e-6, "master capped at -6 dBFS");
}

#[test]
fn limiter_and_soft_clip_bound_output() {
    let mut e = Engine::new(SR);
    let p = SlotParams { wave: Waveform::Square, level_db: 6.0, filter: FilterParams { enabled: false, ..Default::default() }, ..Default::default() };
    for s in 0..4 {
        e.apply(Cmd::SetSlot(s, p));
    }
    let mut rt = RouteTable::new();
    for s in 0..4 {
        rt.push(CompiledRoute { port_mask: u32::MAX, slot: s, ..Default::default() });
    }
    e.apply(Cmd::SetRoutes(Box::new(rt)));
    e.apply(Cmd::SetMaster { db: 0.0 });
    e.apply(Cmd::Run(true));
    for n in 40..48 {
        e.midi(1, &[0x90, n, 127]);
    }
    let (l, r) = run(&mut e, 0.5);
    let peak = l.iter().chain(r.iter()).fold(0f32, |a, v| a.max(v.abs()));
    assert!(peak <= 1.0, "peak {peak}");
    assert!(e.active_voices() == 32, "8 notes x 4 layers");
    assert!(soft_clip(0.3) == 0.3 && soft_clip(10.0) <= 1.0 && soft_clip(-10.0) >= -1.0);
}

#[test]
fn stop_fades_to_silence_and_releases() {
    let mut e = sine_engine();
    e.midi(1, &[0x90, 60, 100]);
    run(&mut e, 0.1);
    e.apply(Cmd::Run(false));
    let (l, _) = run(&mut e, 0.1);
    assert!(l[l.len() - 64..].iter().all(|v| v.abs() < 1e-4));
    assert!(e.is_silent());
}

#[test]
fn sustain_and_voice_stealing() {
    let mut e = sine_engine();
    e.midi(1, &[0xB0, 64, 127]);
    e.midi(1, &[0x90, 60, 100]);
    e.midi(1, &[0x80, 60, 0]);
    run(&mut e, 0.5);
    assert_eq!(e.active_voices(), 1, "held by sustain");
    e.midi(1, &[0xB0, 64, 0]);
    run(&mut e, 1.0);
    assert_eq!(e.active_voices(), 0, "released after pedal up");
    for n in 0..40u8 {
        e.midi(1, &[0x90, 30 + n, 100]);
    }
    assert_eq!(e.active_voices(), MAX_VOICES);
}

#[test]
fn wavetable_mipmaps_and_pitch() {
    // A 600-sample saw-like cycle, like an AKWF file.
    let cycle: Vec<f32> = (0..600).map(|i| 1.0 - 2.0 * i as f32 / 600.0).collect();
    let t = wavetable::Wavetable::from_cycle("test", &cycle).unwrap();
    assert_eq!(t.level_harmonics(0), 299);
    assert!(t.levels() >= 9);
    // Lower levels have fewer harmonics, and the chosen level stays under Nyquist.
    for f in [55.0f32, 440.0, 3520.0, 12000.0] {
        let dt = f / SR;
        let lv = t.level_for_dt(dt);
        assert!(t.level_harmonics(lv) as f32 * f <= SR / 2.0 + 1.0, "{f} Hz level {lv}");
    }
    // Table voice through the engine plays at pitch with low aliasing.
    let mut e = Engine::new(SR);
    e.apply(Cmd::SetSlot(0, SlotParams { wave: Waveform::Table, filter: FilterParams { enabled: false, ..Default::default() }, amp_env: Env { a: 0.001, d: 0.1, s: 1.0, r: 0.1 }, ..Default::default() }));
    e.apply(Cmd::SetTable(0, Some(std::sync::Arc::new(t))));
    let mut rt = RouteTable::new();
    rt.push(CompiledRoute { port_mask: u32::MAX, slot: 0, ..Default::default() });
    e.apply(Cmd::SetRoutes(Box::new(rt)));
    e.apply(Cmd::Run(true));
    e.midi(1, &[0x90, 69, 100]);
    let (l, _) = run(&mut e, 1.5);
    let p = spectrum(&l[24_000..72_000]);
    let peak_bin = p.iter().enumerate().skip(20).max_by(|a, b| a.1.partial_cmp(b.1).unwrap()).unwrap().0;
    assert!((peak_bin as f32 - 440.0).abs() <= 1.0, "table fundamental at {peak_bin} Hz");
}

#[test]
fn wav_parser_reads_16bit_mono() {
    let samples: Vec<i16> = (0..600).map(|i| ((i as f32 / 600.0 * 6.283).sin() * 30000.0) as i16).collect();
    let mut b = Vec::new();
    let data_len = (samples.len() * 2) as u32;
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&44100u32.to_le_bytes());
    b.extend_from_slice(&88200u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in &samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    let x = wavetable::parse_wav(&b).unwrap();
    assert_eq!(x.len(), 600);
    assert!((x[150] - 0.9155).abs() < 0.01);
}

fn set_engine(p: SlotParams) -> Engine {
    let sine: Vec<f32> = (0..600).map(|i| (i as f32 / 600.0 * std::f32::consts::TAU).sin()).collect();
    let saw: Vec<f32> = (0..600).map(|i| i as f32 / 300.0 - 1.0).collect();
    let set = std::sync::Arc::new(TableSet {
        name: "s".into(),
        frames: vec![Wavetable::from_cycle("sine", &sine).unwrap(), Wavetable::from_cycle("saw", &saw).unwrap()],
    });
    let mut e = Engine::new(SR);
    e.apply(Cmd::SetSlot(0, SlotParams { wave: Waveform::Table, filter: FilterParams { enabled: false, ..Default::default() }, amp_env: Env { a: 0.001, d: 0.1, s: 1.0, r: 0.1 }, ..p }));
    e.apply(Cmd::SetTableSet(0, Some(set)));
    let mut rt = RouteTable::new();
    rt.push(CompiledRoute { port_mask: u32::MAX, slot: 0, ..Default::default() });
    e.apply(Cmd::SetRoutes(Box::new(rt)));
    e.apply(Cmd::Run(true));
    e
}

/// Energy above the fundamental (harmonics 2..20) relative to the fundamental, in dB.
fn harmonics_db(x: &[f32], f0: usize) -> f64 {
    let sp = spectrum(x);
    let fund = sp[f0];
    let rest: f64 = (2..20).map(|k| sp[f0 * k]).sum();
    10.0 * (rest / fund).log10()
}

#[test]
fn wavetable_position_morphs_between_frames() {
    let n = 1 << 14;
    let f0 = 64usize; // bin index
    let note_hz = f0 as f32 * SR / n as f32;
    let note = 69.0 + 12.0 * (note_hz / 440.0).log2();
    let mut out = vec![];
    for pos in [0.0f32, 1.0] {
        let mut e = set_engine(SlotParams { position: pos, detune_cents: (note - note.round()) * 100.0, ..Default::default() });
        e.midi(0, &[0x90, note.round() as u8, 100]);
        let (l, _) = run(&mut e, 0.6);
        out.push(harmonics_db(&l[l.len() - n..], f0));
    }
    assert!(out[0] < -30.0, "position 0 is the sine frame: {out:?}");
    assert!(out[1] > -10.0, "position 1 is the saw frame: {out:?}");
}

#[test]
fn warp_unison_and_drive_stay_bounded() {
    for w in Warp::ALL {
        let mut e = set_engine(SlotParams { warp: w, warp_amount: 0.8, unison: 3, drive: 1.0, position: 0.5, pos_lfo: 1.0, lfo_hz: 5.0, ..Default::default() });
        for n in [36u8, 60, 84] {
            e.midi(0, &[0x90, n, 127]);
        }
        let (l, r) = run(&mut e, 0.5);
        assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() <= 1.0), "{w:?}");
        assert!(l.iter().map(|v| v.abs()).fold(0.0, f32::max) > 0.01, "{w:?} silent");
    }
}
