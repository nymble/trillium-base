// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Scenes: named, versioned TOML files holding sound slots and a MIDI routing map.

use pilab_synth::{Env, FilterParams, SlotParams, Waveform, MAX_ROUTES, MAX_SLOTS};
use serde::{Deserialize, Serialize};

pub const SCENE_FORMAT: &str = "pilab-scene";
pub const SCENE_VERSION: u32 = 1;

/// One sound slot: an instrument preset bound to a slot id (1..=16).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Slot {
    pub id: u8,
    pub name: String,
    /// Wavetable path relative to the AKWF library root (wave = "table" only).
    pub table: String,
    /// Wavetable: an AKWF group (folder) scanned by `sound.position` (wave = "table").
    /// When empty, `table` names a single cycle file instead.
    pub wavetable: String,
    /// Sample bank id from the library index (wave = "sampler" only), e.g. "freepats-kalimba".
    pub sample: String,
    /// True once the name was typed by hand. While false, picking a new source (oscillator
    /// type, wavetable group or wave, sample bank, preset) renames the slot automatically.
    /// Absent in older scenes, which reads as false.
    #[serde(skip_serializing_if = "is_false")]
    pub name_locked: bool,
    pub sound: SlotParams,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Default for Slot {
    fn default() -> Self {
        Slot { id: 1, name: "Init".into(), table: String::new(), sample: String::new(), wavetable: String::new(), name_locked: false, sound: SlotParams::default() }
    }
}

/// One routing rule. Every enabled rule that matches a note plays its slot, so
/// overlapping rules layer and disjoint note ranges split.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Route {
    pub enabled: bool,
    pub label: String,
    /// "*" = any input, "virtual" = the synth's own MIDI port, "console" = on-screen
    /// keyboard, otherwise a case-insensitive glob on the source name, e.g. "*Trillium*".
    pub input: String,
    /// 0 = omni, 1..=16
    pub channel: u8,
    pub note_lo: u8,
    pub note_hi: u8,
    pub transpose: i8,
    pub velocity_scale: f32,
    /// Target slot id (1..=16).
    pub slot: u8,
}

impl Default for Route {
    fn default() -> Self {
        Route {
            enabled: true,
            label: String::new(),
            input: "*".into(),
            channel: 0,
            note_lo: 0,
            note_hi: 127,
            transpose: 0,
            velocity_scale: 1.0,
            slot: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub description: String,
    /// Requested master level. Capped at -6 dBFS, and at -18 dBFS when the service starts.
    pub master_db: f32,
    pub a4_hz: f32,
    pub bend_range: f32,
    #[serde(rename = "slot")]
    pub slots: Vec<Slot>,
    #[serde(rename = "route")]
    pub routes: Vec<Route>,
    /// External engine that plays this scene instead of the built-in synth: "fluidsynth" or
    /// "zynaddsubfx". Empty = the built-in synth. The MIDI router's "scene" destination
    /// follows it, and loading the scene starts the engine.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub engine: String,
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            format: SCENE_FORMAT.into(),
            version: SCENE_VERSION,
            name: "untitled".into(),
            description: String::new(),
            master_db: -18.0,
            a4_hz: 440.0,
            bend_range: 2.0,
            slots: vec![],
            routes: vec![],
            engine: String::new(),
        }
    }
}

pub fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    match c.next() {
        Some(f) if f.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    n.len() <= 64 && c.all(|ch| ch.is_ascii_alphanumeric() || " _-.".contains(ch)) && !n.contains("..")
}

impl Scene {
    /// The factory scene: four slots and one route from every input to slot 1.
    pub fn factory() -> Scene {
        let saw = Slot { id: 1, name: "Warm Saw".into(), table: String::new(), sample: String::new(), wavetable: String::new(), name_locked: false, sound: SlotParams::default() };
        let pwm = Slot {
            id: 2,
            name: "PWM Lead".into(),
            table: String::new(), sample: String::new(), wavetable: String::new(), name_locked: false,
            sound: SlotParams {
                wave: Waveform::Pulse,
                pulse_width: 0.35,
                pwm_mod: 0.4,
                amp_env: Env { a: 0.005, d: 0.2, s: 0.8, r: 0.25 },
                filter: FilterParams { cutoff_hz: 2500.0, resonance: 0.3, ..FilterParams::default() },
                ..SlotParams::default()
            },
        };
        let pad = Slot {
            id: 3,
            name: "Sine Pad".into(),
            table: String::new(), sample: String::new(), wavetable: String::new(), name_locked: false,
            sound: SlotParams {
                wave: Waveform::Sine,
                amp_env: Env { a: 0.6, d: 1.0, s: 0.8, r: 1.8 },
                filter: FilterParams { enabled: false, ..FilterParams::default() },
                level_db: -8.0,
                ..SlotParams::default()
            },
        };
        let tri = Slot {
            id: 4,
            name: "Soft Triangle".into(),
            table: String::new(), sample: String::new(), wavetable: String::new(), name_locked: false,
            sound: SlotParams {
                wave: Waveform::Triangle,
                amp_env: Env { a: 0.01, d: 0.5, s: 0.6, r: 0.6 },
                filter: FilterParams { cutoff_hz: 6000.0, env_amount: 0.5, ..FilterParams::default() },
                ..SlotParams::default()
            },
        };
        let wt = Slot {
            id: 5,
            name: "AKWF Table".into(),
            table: "AKWF_bw_saw/AKWF_saw_0001.wav".into(), sample: String::new(), wavetable: String::new(), name_locked: false,
            sound: SlotParams {
                wave: Waveform::Table,
                amp_env: Env { a: 0.02, d: 0.6, s: 0.7, r: 0.8 },
                ..SlotParams::default()
            },
        };
        Scene {
            name: "default".into(),
            description: "Factory scene: every input plays Warm Saw".into(),
            slots: vec![saw, pwm, pad, tri, wt],
            routes: vec![Route { label: "All inputs".into(), ..Route::default() }],
            ..Scene::default()
        }
    }

    /// Sample scene for Trillium (matched by `[[midi.devices]]` id "trillium"):
    /// pad below C4 an octave down, lead from C4 up, a soft triangle layered across the
    /// middle, and the web keyboard on Warm Saw.
    pub fn trillium_sample() -> Scene {
        let dev = |label: &str, lo: u8, hi: u8, transpose: i8, vel: f32, slot: u8| Route {
            label: label.into(),
            input: "device:trillium".into(),
            note_lo: lo,
            note_hi: hi,
            transpose,
            velocity_scale: vel,
            slot,
            ..Route::default()
        };
        Scene {
            name: "trillium split".into(),
            description: "Trillium: pad below C4, lead above, triangle layer; web keys on Warm Saw".into(),
            routes: vec![
                dev("Trillium low", 0, 59, -12, 1.0, 3),
                dev("Trillium high", 60, 127, 0, 1.0, 2),
                dev("Triangle layer", 36, 96, 0, 0.5, 4),
                Route { label: "Web keyboard".into(), input: "console".into(), slot: 1, ..Route::default() },
            ],
            ..Scene::factory()
        }
    }

    /// Sample-instrument demo: piano, GM kit on channel 10, bass, kalimba and pad, one
    /// MIDI channel each; the web keyboard plays the piano.
    pub fn samples_demo() -> Scene {
        let slots: Vec<Slot> = ["upright-piano", "starter-kit", "picked-bass", "kalimba", "choir-pad"]
            .iter()
            .enumerate()
            .map(|(i, id)| crate::presets::find(id).unwrap().slot((i + 1) as u8))
            .collect();
        let ch = |label: &str, channel: u8, slot: u8| Route { label: label.into(), input: "*".into(), channel, slot, ..Route::default() };
        Scene {
            name: "samples".into(),
            description: "Sample instruments: piano ch 1, GM kit ch 10, bass ch 2, kalimba ch 3, pad ch 4; web keys play the piano".into(),
            slots,
            routes: vec![
                ch("Piano", 1, 1),
                ch("Drums (GM)", 10, 2),
                ch("Bass", 2, 3),
                ch("Kalimba", 3, 4),
                ch("Pad", 4, 5),
                Route { label: "Web keyboard".into(), input: "console".into(), slot: 1, ..Route::default() },
            ],
            ..Scene::factory()
        }
    }

    /// Drum kit for the Trillium Zero: strikes arrive on GM channel 10 (the Zero's
    /// "Drum kit" mapping) and play the kit in slot 1; touch or tilt notes on channel 1
    /// play kalimba in slot 2; the web keyboard and the Play pads play the kit.
    pub fn trillium_kit(kit_preset: &str) -> Scene {
        let kit = crate::presets::find(kit_preset).or_else(|| crate::presets::find("starter-kit")).unwrap();
        let slots = vec![kit.slot(1), crate::presets::find("kalimba").unwrap().slot(2)];
        let r = |label: &str, input: &str, channel: u8, slot: u8| Route { label: label.into(), input: input.into(), channel, slot, ..Route::default() };
        Scene {
            name: "trillium kit".into(),
            description: "Trillium Zero drum kit: strike down snare, up crash, left kick, right closed hat, forward tom, back open hat (ch 10); touch or tilt notes on ch 1 play kalimba".into(),
            slots,
            routes: vec![r("Drum kit (ch 10)", "*", 10, 1), r("Melody (ch 1)", "*", 1, 2), r("Web keyboard and pads", "console", 0, 1)],
            ..Scene::factory()
        }
    }

    /// Every input through FluidSynth with a General MIDI SoundFont (drums on channel 10).
    pub fn general_midi() -> Scene {
        Scene {
            name: "general midi".into(),
            description: "General MIDI on FluidSynth: 128 instruments on channels 1-16, drums on channel 10 (FluidR3_GM, MIT)".into(),
            engine: "fluidsynth".into(),
            ..Scene::default()
        }
    }

    /// Every input through ZynAddSubFX (the instrument picked in Sounds & apps).
    pub fn zynaddsubfx() -> Scene {
        Scene {
            name: "zynaddsubfx".into(),
            description: "ZynAddSubFX synthesizer: pick an instrument from its banks in Sounds & apps".into(),
            engine: "zynaddsubfx".into(),
            ..Scene::default()
        }
    }

    pub fn slot(&self, id: u8) -> Option<&Slot> {
        self.slots.iter().find(|s| s.id == id)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != SCENE_FORMAT {
            return Err(format!("format must be \"{SCENE_FORMAT}\""));
        }
        if self.version == 0 || self.version > SCENE_VERSION {
            return Err(format!("unsupported scene version {} (this build reads up to {SCENE_VERSION})", self.version));
        }
        if !valid_name(&self.name) {
            return Err("name: letters, digits, space, _ - . only (max 64)".into());
        }
        if !self.engine.is_empty() && !crate::midi_router::ENGINES.contains(&self.engine.as_str()) {
            return Err(format!("engine must be empty or one of {:?}", crate::midi_router::ENGINES));
        }
        if self.slots.len() > MAX_SLOTS {
            return Err(format!("at most {MAX_SLOTS} slots"));
        }
        if self.routes.len() > MAX_ROUTES {
            return Err(format!("at most {MAX_ROUTES} routes"));
        }
        let mut seen = [false; MAX_SLOTS + 1];
        for s in &self.slots {
            if s.id == 0 || s.id as usize > MAX_SLOTS {
                return Err(format!("slot id {} out of range 1..={MAX_SLOTS}", s.id));
            }
            if seen[s.id as usize] {
                return Err(format!("duplicate slot id {}", s.id));
            }
            seen[s.id as usize] = true;
            if s.wavetable.contains('/') || s.wavetable.contains("..") {
                return Err(format!("slot {}: wavetable must be a library group name", s.id));
            }
            if !s.sample.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Err(format!("slot {}: sample must be a library bank id", s.id));
            }
            if s.table.contains("..") || s.table.starts_with('/') {
                return Err(format!("slot {}: table path must be relative to the library", s.id));
            }
        }
        for (i, r) in self.routes.iter().enumerate() {
            if r.channel > 16 {
                return Err(format!("route {}: channel 0..=16", i + 1));
            }
            if r.note_lo > 127 || r.note_hi > 127 || r.note_lo > r.note_hi {
                return Err(format!("route {}: note range", i + 1));
            }
            if r.slot == 0 || r.slot as usize > MAX_SLOTS || !seen[r.slot as usize] {
                return Err(format!("route {}: slot {} does not exist", i + 1, r.slot));
            }
        }
        if !(380.0..=480.0).contains(&self.a4_hz) {
            return Err("a4_hz must be 380..480".into());
        }
        Ok(())
    }

    pub fn to_toml(&self) -> Result<String, String> {
        let mut v = toml::Value::try_from(self).map_err(|e| e.to_string())?;
        tidy_floats(&mut v);
        let body = toml::to_string_pretty(&v).map_err(|e| e.to_string())?;
        Ok(format!("# Trillium Console scene. Edit in the console or by hand; validated on load.\n{body}"))
    }

    pub fn from_toml(s: &str) -> Result<Scene, String> {
        let sc: Scene = toml::from_str(s).map_err(|e| e.to_string())?;
        sc.validate()?;
        Ok(sc)
    }
}

/// Parameters are f32; write them in their shortest f32 form (0.3, not 0.30000001192092896).
/// Reading back gives the identical f32.
fn tidy_floats(v: &mut toml::Value) {
    match v {
        toml::Value::Float(f) => {
            if let Ok(t) = format!("{}", *f as f32).parse::<f64>() {
                *f = t;
            }
        }
        toml::Value::Array(a) => a.iter_mut().for_each(tidy_floats),
        toml::Value::Table(t) => t.iter_mut().for_each(|(_, x)| tidy_floats(x)),
        _ => {}
    }
}
