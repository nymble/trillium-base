// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Named sound presets. Sample presets point at a bank id in the library index
//! (samples/index.json); applying one to a slot sets the sampler, bank and envelope.

use pilab_synth::{Env, FilterParams, SlotParams, Waveform};
use serde::Serialize;

use crate::scene::Slot;

#[derive(Clone, Debug, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    /// Library bank id ("" for oscillator presets).
    pub bank: &'static str,
    /// GM drum layout: route on MIDI channel 10 by default.
    pub drum: bool,
    pub sound: SlotParams,
}

impl Preset {
    pub fn slot(&self, id: u8) -> Slot {
        Slot { id, name: self.name.into(), table: String::new(), sample: self.bank.into(), wavetable: String::new(), name_locked: false, sound: self.sound }
    }
}

fn sampler(level_db: f32, amp_env: Env) -> SlotParams {
    SlotParams {
        wave: Waveform::Sampler,
        level_db,
        vel_to_amp: 0.8,
        amp_env,
        filter: FilterParams { enabled: false, ..FilterParams::default() },
        ..SlotParams::default()
    }
}

pub fn all() -> Vec<Preset> {
    vec![
        Preset { id: "upright-piano", name: "Upright Piano", category: "keys", bank: "freepats-upright-piano-kw", drum: false, sound: sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.6 }) },
        Preset { id: "starter-kit", name: "Starter Kit (GM)", category: "drums", bank: "vcsl-starter-kit", drum: true, sound: sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.3 }) },
        Preset { id: "full-kit", name: "Full Kit (GM)", category: "drums", bank: "vcsl-full-kit", drum: true, sound: sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.4 }) },
        Preset { id: "marimba", name: "Marimba", category: "mallets-and-bells", bank: "vcsl-marimba", drum: false, sound: sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.8 }) },
        Preset { id: "vibraphone", name: "Vibraphone", category: "mallets-and-bells", bank: "vcsl-vibraphone", drum: false, sound: sampler(-6.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 1.2 }) },
        Preset { id: "glockenspiel", name: "Glockenspiel", category: "mallets-and-bells", bank: "vcsl-glockenspiel", drum: false, sound: sampler(-8.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 1.0 }) },
        Preset { id: "balafon", name: "Balafon", category: "mallets-and-bells", bank: "vcsl-balafon", drum: false, sound: sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.5 }) },
        Preset { id: "mbira", name: "Mbira (Tanzania)", category: "strings-and-plucked", bank: "vcsl-mbira", drum: false, sound: sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.9 }) },
        Preset { id: "picked-bass", name: "Picked Bass", category: "strings-and-plucked", bank: "freepats-picked-bass-yr", drum: false, sound: sampler(-3.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.25 }) },
        Preset { id: "kalimba", name: "Kalimba", category: "strings-and-plucked", bank: "freepats-kalimba", drum: false, sound: sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.8 }) },
        Preset { id: "choir-pad", name: "Choir Pad", category: "synths", bank: "freepats-synth-pad-choir", drum: false, sound: sampler(-8.0, Env { a: 0.4, d: 0.5, s: 0.9, r: 1.6 }) },
        Preset { id: "sweep-pad", name: "Sweep Pad", category: "synths", bank: "freepats-sweep-pad", drum: false, sound: sampler(-8.0, Env { a: 0.3, d: 0.5, s: 0.9, r: 1.5 }) },
        Preset { id: "nylon-guitar", name: "Nylon Guitar", category: "strings-and-plucked", bank: "freepats-spanish-guitar", drum: false, sound: sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.5 }) },
        Preset { id: "drawbar-organ", name: "Drawbar Organ", category: "keys", bank: "freepats-drawbar-organ", drum: false, sound: sampler(-8.0, Env { a: 0.005, d: 0.1, s: 1.0, r: 0.1 }) },
        Preset { id: "synth-percussion", name: "Synth Percussion", category: "drums", bank: "freepats-synth-percussion", drum: true, sound: sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.3 }) },
    ]
}

pub fn find(id: &str) -> Option<Preset> {
    all().into_iter().find(|p| p.id == id)
}

/// Default sampler sound for a bank applied without a preset (category-based envelope).
pub fn for_bank(category: &str) -> SlotParams {
    match category {
        "drums" => sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.3 }),
        "synths" => sampler(-7.0, Env { a: 0.01, d: 0.3, s: 0.9, r: 0.8 }),
        _ => sampler(-4.0, Env { a: 0.001, d: 0.1, s: 1.0, r: 0.5 }),
    }
}
