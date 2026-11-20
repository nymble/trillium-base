// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Sound-slot parameters. All `Copy`, so they cross into the audio thread without allocation.

use serde::{Deserialize, Serialize};

use crate::osc::Waveform;

/// ADSR in seconds (sustain is a level 0..1).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Env {
    pub a: f32,
    pub d: f32,
    pub s: f32,
    pub r: f32,
}

impl Default for Env {
    fn default() -> Self {
        Env { a: 0.01, d: 0.3, s: 0.7, r: 0.4 }
    }
}

impl Env {
    pub fn clamped(self) -> Env {
        Env {
            a: self.a.clamp(0.0005, 20.0),
            d: self.d.clamp(0.001, 20.0),
            s: self.s.clamp(0.0, 1.0),
            r: self.r.clamp(0.003, 30.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterParams {
    pub enabled: bool,
    /// Low-pass cutoff in Hz before modulation.
    pub cutoff_hz: f32,
    /// 0..0.97
    pub resonance: f32,
    /// Filter envelope depth in octaves (can be negative).
    pub env_amount: f32,
    /// 0 = fixed, 1 = cutoff follows the key one octave per octave.
    pub key_track: f32,
    /// Velocity to cutoff in octaves at full velocity.
    pub vel_amount: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        FilterParams { enabled: true, cutoff_hz: 4000.0, resonance: 0.15, env_amount: 1.5, key_track: 0.5, vel_amount: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlotParams {
    pub enabled: bool,
    pub wave: Waveform,
    /// Pulse width 0.02..0.98 (pulse wave only).
    pub pulse_width: f32,
    /// Mod wheel to pulse width depth (pulse wave only).
    pub pwm_mod: f32,
    pub octave: i8,
    pub detune_cents: f32,
    /// Slot level in dB (before master).
    pub level_db: f32,
    /// -1 left .. +1 right
    pub pan: f32,
    /// 0 = velocity ignored for level, 1 = full velocity response.
    pub vel_to_amp: f32,
    pub amp_env: Env,
    pub filter: FilterParams,
    pub filter_env: Env,
}

impl Default for SlotParams {
    fn default() -> Self {
        SlotParams {
            enabled: true,
            wave: Waveform::Saw,
            pulse_width: 0.5,
            pwm_mod: 0.3,
            octave: 0,
            detune_cents: 0.0,
            level_db: -6.0,
            pan: 0.0,
            vel_to_amp: 0.8,
            amp_env: Env::default(),
            filter: FilterParams::default(),
            filter_env: Env { a: 0.005, d: 0.4, s: 0.2, r: 0.4 },
        }
    }
}
