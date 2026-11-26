// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Band-limited oscillators. Saw and square use the polyBLEP helpers from `zpod-voices`;
//! pulse adds a second BLEP at the moving edge; triangle uses polyBLAMP at its corners;
//! sine uses an f64 phase accumulator.

use serde::{Deserialize, Serialize};
use zpod_voices::dsp::{blep, saw as blep_saw, square as blep_square};

use crate::wavetable::{Bands, Wavetable};

/// How the classic shapes are rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OscMode {
    /// Mipmapped Fourier tables (third-octave levels). Cleanest; the pilab default.
    #[default]
    Table,
    /// Two-sample polyBLEP/polyBLAMP from `zpod-voices`. Cheapest; the ZPOD-class path.
    Blep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Waveform {
    Sine,
    #[default]
    Saw,
    Square,
    Pulse,
    Triangle,
    Table,
    /// Multisample instrument (SFZ); rendered by the engine's sampler path.
    Sampler,
}

impl Waveform {
    pub const ALL: [Waveform; 7] = [
        Waveform::Sine,
        Waveform::Saw,
        Waveform::Square,
        Waveform::Pulse,
        Waveform::Triangle,
        Waveform::Table,
        Waveform::Sampler,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Waveform::Sine => "sine",
            Waveform::Saw => "saw",
            Waveform::Square => "square",
            Waveform::Pulse => "pulse",
            Waveform::Triangle => "triangle",
            Waveform::Table => "table",
            Waveform::Sampler => "sampler",
        }
    }
}

/// Integrated polyBLEP residual (polyBLAMP), two samples wide.
#[inline]
pub fn blamp(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt - 1.0;
        -x * x * x / 3.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt + 1.0;
        x * x * x / 3.0
    } else {
        0.0
    }
}

#[inline]
fn wrap01(p: f32) -> f32 {
    p - p.floor()
}

/// Naive (aliasing) reference shapes, used by tests to measure what band-limiting buys.
pub fn naive(w: Waveform, ph: f32, pw: f32) -> f32 {
    match w {
        Waveform::Sine => (ph * core::f32::consts::TAU).sin(),
        Waveform::Saw => 2.0 * ph - 1.0,
        Waveform::Square => {
            if ph < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Waveform::Pulse => {
            (if ph < pw { 1.0 } else { -1.0 }) - (2.0 * pw - 1.0)
        }
        Waveform::Triangle => 4.0 * (ph - 0.5).abs() - 1.0,
        Waveform::Table | Waveform::Sampler => 0.0,
    }
}

/// One oscillator: phase plus the shape renderer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Osc {
    pub phase: f64,
}

impl Osc {
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Current sample for `w` at phase increment `dt` (cycles per sample), then advance.
    /// `pw` is the pulse width (0..1), `table`/`level` select a wavetable mip level.
    #[inline]
    pub fn next(&mut self, w: Waveform, dt: f32, pw: f32, table: Option<&Wavetable>, level: usize) -> f32 {
        let s = Self::shape(w, self.phase, dt, pw, table, level);
        self.advance(dt);
        s
    }

    /// Like [`Osc::next`] but classic shapes come from the band-limited tables.
    #[inline]
    pub fn next_table(&mut self, w: Waveform, dt: f32, pw: f32, bands: &Bands, user: Option<&Wavetable>, level: usize) -> f32 {
        let blevel = bands.saw.level_for_dt(dt);
        let s = Self::shape_table(w, self.phase, pw, bands, blevel, user, level);
        self.advance(dt);
        s
    }

    #[inline]
    fn advance(&mut self, dt: f32) {
        self.phase += dt as f64;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
    }

    /// Table-based renderer: `blevel` is the mip level of `bands` for this pitch.
    #[inline]
    pub fn shape_table(w: Waveform, phase: f64, pw: f32, bands: &Bands, blevel: usize, user: Option<&Wavetable>, level: usize) -> f32 {
        let ph = phase as f32;
        match w {
            Waveform::Sine => (phase * core::f64::consts::TAU).sin() as f32,
            Waveform::Saw => bands.saw.sample(blevel, ph),
            Waveform::Square | Waveform::Pulse => {
                let pw = if w == Waveform::Square { 0.5 } else { pw.clamp(0.02, 0.98) };
                bands.saw.sample(blevel, wrap01(ph + 1.0 - pw)) - bands.saw.sample(blevel, ph)
            }
            Waveform::Triangle => bands.tri.sample(blevel, ph),
            Waveform::Table => match user {
                Some(t) => t.sample(level, ph),
                None => 0.0,
            },
            Waveform::Sampler => 0.0,
        }
    }

    #[inline]
    pub fn shape(w: Waveform, phase: f64, dt: f32, pw: f32, table: Option<&Wavetable>, level: usize) -> f32 {
        let ph = phase as f32;
        match w {
            Waveform::Sine => (phase * core::f64::consts::TAU).sin() as f32,
            Waveform::Saw => blep_saw(ph, dt),
            Waveform::Square => blep_square(ph, dt),
            Waveform::Pulse => {
                let pw = pw.clamp(0.02, 0.98);
                let mut s = if ph < pw { 1.0 } else { -1.0 };
                s += blep(ph, dt);
                s -= blep(wrap01(ph - pw + 1.0), dt);
                s - (2.0 * pw - 1.0)
            }
            Waveform::Triangle => {
                let mut s = 4.0 * (ph - 0.5).abs() - 1.0;
                s -= 4.0 * dt * (blamp(ph, dt) - blamp(wrap01(ph + 0.5), dt));
                s
            }
            Waveform::Table => match table {
                Some(t) => t.sample(level, ph),
                None => 0.0,
            },
            Waveform::Sampler => 0.0,
        }
    }
}
