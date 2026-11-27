// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Trillium Zero mapping model: every configuration parameter with its range, default,
//! group and display kind, the built-in mapping presets, and conversion to and from the
//! `trill-preset` JSON file format (protocol version 1) so presets move between the
//! Base console and the Trillium app.
//!
//! Ranges and defaults follow the published configuration protocol (firmware 0.5.0+,
//! 41 parameters). Values are never clamped here: out-of-range input is an error, the
//! same rule the firmware applies.

use serde::Serialize;
use serde_json::{json, Map, Value};

/// How a value is shown and edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// 0 = MIDI channel 1.
    Channel,
    Note,
    Cc,
    Bool,
    Choice,
    Number,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Spec {
    pub id: u8,
    /// Key in the `trill-preset` JSON format.
    pub key: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub kind: Kind,
    pub min: u16,
    pub max: u16,
    pub default: u16,
    pub unit: &'static str,
    /// (value, label) for Choice parameters; empty otherwise.
    pub choices: &'static [(u16, &'static str)],
    pub help: &'static str,
    /// Lowest firmware that has the parameter.
    pub since: &'static str,
}

/// Groups in signal-flow order, as the editor shows them.
pub const GROUPS: &[(&str, &str, &str)] = &[
    ("source", "What plays the notes", "Mode and the touch strip voice"),
    ("notes", "Notes", "Pitch layout for touch and tilt modes"),
    ("strikes", "Taps and strikes", "Percussion: one note per strike direction, on the drum channel"),
    ("motion", "Motion controllers", "Tilt and rotation as MIDI CC"),
    ("tilt", "Tilt modes", "Tilt notes, theremin and tilt + strike"),
    ("flick", "Flick gestures", "Fast turns play notes"),
];

const MODES: &[(u16, &str)] = &[(0, "Touch strip"), (1, "Tilt notes"), (2, "Theremin"), (3, "Tilt + strike")];
const SCALES: &[(u16, &str)] = &[(0, "Chromatic"), (1, "Major pentatonic"), (2, "Major"), (3, "Natural minor")];
const PITCH: &[(u16, &str)] = &[(0, "Continuous bend"), (1, "Stepped notes")];
const SOURCES: &[(u16, &str)] = &[
    (0, "Off"),
    (1, "Rotation speed"),
    (2, "Tilt forward"),
    (3, "Tilt back"),
    (4, "Roll right"),
    (5, "Roll left"),
    (6, "Pitch, centred"),
    (7, "Roll, centred"),
];
const AXES: &[(u16, &str)] = &[(0, "Off"), (1, "X axis"), (2, "Y axis"), (3, "Z axis")];
const DIRS: &[(u16, &str)] = &[(0, "Off (every tap plays the tap note)"), (2, "Down / up"), (4, "Down / up / left / right"), (6, "All six directions")];
const ZONES: &[(u16, &str)] = &[(0, "Equal"), (1, "Finer near level")];
const NONE: &[(u16, &str)] = &[];

macro_rules! p {
    ($id:expr, $key:expr, $name:expr, $group:expr, $kind:expr, $min:expr, $max:expr, $def:expr, $unit:expr, $choices:expr, $since:expr, $help:expr) => {
        Spec { id: $id, key: $key, name: $name, group: $group, kind: $kind, min: $min, max: $max, default: $def, unit: $unit, choices: $choices, help: $help, since: $since }
    };
}

use Kind::*;

pub const SPECS: &[Spec] = &[
    p!(1, "channel", "Touch voice channel", "source", Channel, 0, 15, 0, "", NONE, "0.2.0", "MIDI channel for touch and tilt notes"),
    p!(2, "base_note", "Base note", "notes", Note, 0, 127, 48, "", NONE, "0.2.0", "Lowest note (left end of the strip, or fully rolled left)"),
    p!(3, "range_semitones", "Range", "notes", Number, 1, 96, 24, "semitones", NONE, "0.2.0", "Span across the strip or the tilt range"),
    p!(4, "scale", "Scale", "notes", Choice, 0, 3, 0, "", SCALES, "0.2.0", "Major and minor need firmware 0.5.0"),
    p!(5, "pitch_mode", "Pitch", "notes", Choice, 0, 1, 0, "", PITCH, "0.2.0", "Continuous pitch bend or stepped notes"),
    p!(6, "bend_range_semitones", "Pitch-bend range", "notes", Number, 1, 24, 24, "semitones", NONE, "0.2.0", "Set the synth's bend range to match"),
    p!(7, "velocity_min", "Velocity at onset", "notes", Number, 1, 127, 30, "", NONE, "0.2.0", "Velocity of the lightest touch"),
    p!(8, "motion_enabled", "Motion controllers", "motion", Bool, 0, 1, 1, "", NONE, "0.2.0", "Send controllers A and B"),
    p!(9, "rotation_full_dps", "Rotation for CC 127", "motion", Number, 30, 2000, 360, "°/s", NONE, "0.2.0", "Turn rate that reaches full scale"),
    p!(10, "tilt_full_deg", "Tilt for CC 127", "motion", Number, 5, 90, 60, "°", NONE, "0.2.0", "Tilt angle that reaches full scale"),
    p!(11, "percussion_enabled", "Tap percussion", "strikes", Bool, 0, 1, 0, "", NONE, "0.2.0", "Taps and strikes play notes on the percussion channel"),
    p!(12, "percussion_channel", "Percussion channel", "strikes", Channel, 0, 15, 9, "", NONE, "0.2.0", "Channel 10 is General MIDI drums"),
    p!(13, "percussion_note", "Tap note (or unclear direction)", "strikes", Note, 0, 127, 39, "", NONE, "0.2.0", "Played by every tap when directions are off, otherwise by a strike with no clear direction"),
    p!(14, "mode", "Mode", "source", Choice, 0, 3, 0, "", MODES, "0.4.0", "What plays the notes; tilt + strike needs 0.5.0"),
    p!(15, "cc_a_source", "Controller A source", "motion", Choice, 0, 7, 1, "", SOURCES, "0.4.0", ""),
    p!(16, "cc_a_number", "Controller A CC", "motion", Cc, 1, 119, 74, "", NONE, "0.4.0", ""),
    p!(17, "cc_b_source", "Controller B source", "motion", Choice, 0, 7, 2, "", SOURCES, "0.4.0", ""),
    p!(18, "cc_b_number", "Controller B CC", "motion", Cc, 1, 119, 1, "", NONE, "0.4.0", ""),
    p!(19, "cc_deadzone_percent", "Controller deadzone", "motion", Number, 0, 50, 0, "%", NONE, "0.4.0", ""),
    p!(20, "cc_smoothing_ms", "Controller smoothing", "motion", Number, 0, 500, 0, "ms", NONE, "0.4.0", "0 = off"),
    p!(21, "tilt_span_deg", "Roll range", "tilt", Number, 10, 90, 45, "°", NONE, "0.4.0", "Roll from centre to each end of the note range"),
    p!(22, "gate_deg", "Gate angle", "tilt", Number, 5, 60, 15, "°", NONE, "0.4.0", "Forward tilt that starts a note (closes 3° lower)"),
    p!(23, "step_hysteresis_percent", "Step hysteresis", "tilt", Number, 0, 49, 20, "%", NONE, "0.4.0", ""),
    p!(24, "expression_cc", "Pressure CC", "tilt", Number, 0, 119, 0, "", NONE, "0.4.0", "CC that mirrors note pressure in tilt modes; 0 = off"),
    p!(25, "tap_threshold_cg", "Tap threshold", "strikes", Number, 30, 400, 80, "centi-g", NONE, "0.4.0", "Lower is more sensitive"),
    p!(26, "tap_cooldown_ms", "Time between taps", "strikes", Number, 50, 1000, 120, "ms", NONE, "0.4.0", "Shortest gap between two strikes"),
    p!(27, "gesture_axis", "Flick axis", "flick", Choice, 0, 3, 0, "", AXES, "0.4.0", ""),
    p!(28, "gesture_threshold_dps", "Flick threshold", "flick", Number, 100, 2000, 400, "°/s", NONE, "0.4.0", ""),
    p!(29, "gesture_cooldown_ms", "Time between flicks", "flick", Number, 100, 2000, 300, "ms", NONE, "0.4.0", ""),
    p!(30, "gesture_base_note", "Flick notes from", "flick", Note, 0, 125, 60, "", NONE, "0.4.0", "Negative flick plays this note, positive +1, zig-zag +2"),
    p!(31, "strike_directions", "Strike directions", "strikes", Choice, 0, 6, 0, "", DIRS, "0.5.0", "The direction the Zero moves when stopped picks the note"),
    p!(32, "strike_note_down", "Strike down", "strikes", Note, 0, 127, 38, "", NONE, "0.5.0", ""),
    p!(33, "strike_note_up", "Strike up", "strikes", Note, 0, 127, 49, "", NONE, "0.5.0", ""),
    p!(34, "strike_note_left", "Strike left", "strikes", Note, 0, 127, 36, "", NONE, "0.5.0", ""),
    p!(35, "strike_note_right", "Strike right", "strikes", Note, 0, 127, 42, "", NONE, "0.5.0", ""),
    p!(36, "strike_note_forward", "Strike forward", "strikes", Note, 0, 127, 45, "", NONE, "0.5.0", ""),
    p!(37, "strike_note_back", "Strike back", "strikes", Note, 0, 127, 51, "", NONE, "0.5.0", ""),
    p!(38, "note_length_ms", "Tilt + strike note length", "tilt", Number, 20, 2000, 300, "ms", NONE, "0.5.0", ""),
    p!(39, "register_tilt", "Tilt shifts the octave", "tilt", Bool, 0, 1, 1, "", NONE, "0.5.0", "Forward/back past the gate angle shifts an octave"),
    p!(40, "zone_curve", "Note zones", "tilt", Choice, 0, 1, 1, "", ZONES, "0.5.0", ""),
    p!(41, "strike_voicing", "Strike adds a chord tone", "tilt", Bool, 0, 1, 1, "", NONE, "0.5.0", "Strike direction adds a chord tone in tilt + strike"),
];

pub fn spec(id: u8) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.id == id)
}

/// A parameter by numeric id, `trill-preset` key, or display name (case-insensitive).
pub fn lookup(name: &str) -> Option<&'static Spec> {
    let n = name.trim();
    if let Ok(id) = n.parse::<u8>() {
        return spec(id);
    }
    let l = n.to_ascii_lowercase().replace([' ', '-'], "_");
    SPECS.iter().find(|s| s.key == l || s.name.to_ascii_lowercase().replace([' ', '-'], "_") == l)
}

/// Checks one value with the firmware's rule (rejected, never clamped).
pub fn validate(id: u8, v: u16) -> Result<(), String> {
    let s = spec(id).ok_or_else(|| format!("unknown parameter {id}"))?;
    if v < s.min || v > s.max {
        return Err(format!("{} ({id}) must be {}..={}, got {v}", s.name, s.min, s.max));
    }
    if id == 31 && v % 2 == 1 {
        return Err("Strike directions (31) takes 0, 2, 4 or 6".into());
    }
    Ok(())
}

/// Parses a user value: a number, a note name (C4 = 60, also "C#3", "Eb2"), a 1-based
/// channel for Channel parameters ("ch10" or "10" both mean channel 10 = value 9),
/// on/off for Bool, or a choice label.
pub fn parse_value(s: &Spec, text: &str) -> Result<u16, String> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    let v = match s.kind {
        Kind::Bool => match lower.as_str() {
            "1" | "on" | "true" | "yes" => 1,
            "0" | "off" | "false" | "no" => 0,
            _ => return Err(format!("{}: on or off", s.name)),
        },
        Kind::Channel => {
            let n: u16 = lower.trim_start_matches("ch").trim().parse().map_err(|_| format!("{}: channel 1..16", s.name))?;
            if !(1..=16).contains(&n) {
                return Err(format!("{}: channel 1..16", s.name));
            }
            n - 1
        }
        Kind::Note => match t.parse::<u16>() {
            Ok(n) => n,
            Err(_) => note_number(t).ok_or_else(|| format!("{}: a MIDI note number or a name like C4 or F#2", s.name))? as u16,
        },
        Kind::Choice => match t.parse::<u16>() {
            Ok(n) => n,
            Err(_) => s.choices.iter().find(|(_, l)| l.to_ascii_lowercase().starts_with(&lower)).map(|c| c.0).ok_or_else(|| format!("{}: one of {}", s.name, s.choices.iter().map(|c| c.1).collect::<Vec<_>>().join(", ")))?,
        },
        _ => t.parse::<u16>().map_err(|_| format!("{}: a number", s.name))?,
    };
    validate(s.id, v)?;
    Ok(v)
}

/// "C4" → 60 (middle C = C4). Accepts sharps (#) and flats (b).
pub fn note_number(s: &str) -> Option<u8> {
    let b = s.trim().as_bytes();
    let base: i32 = match b.first()?.to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    };
    let mut i = 1;
    let mut acc = 0;
    while i < b.len() && (b[i] == b'#' || b[i] == b'b') {
        acc += if b[i] == b'#' { 1 } else { -1 };
        i += 1;
    }
    let oct: i32 = std::str::from_utf8(&b[i..]).ok()?.parse().ok()?;
    let n = (oct + 1) * 12 + base + acc;
    (0..=127).contains(&n).then_some(n as u8)
}

pub fn note_name(n: u16) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", N[(n % 12) as usize], n as i32 / 12 - 1)
}

/// General MIDI percussion names (notes 27–87), for the drum note pickers.
pub fn gm_drum(n: u16) -> &'static str {
    const T: [&str; 61] = [
        "High Q", "Slap", "Scratch Push", "Scratch Pull", "Sticks", "Square Click", "Metronome Click", "Metronome Bell",
        "Acoustic Bass Drum", "Bass Drum 1", "Side Stick", "Acoustic Snare", "Hand Clap", "Electric Snare", "Low Floor Tom",
        "Closed Hi-Hat", "High Floor Tom", "Pedal Hi-Hat", "Low Tom", "Open Hi-Hat", "Low-Mid Tom", "Hi-Mid Tom",
        "Crash Cymbal 1", "High Tom", "Ride Cymbal 1", "Chinese Cymbal", "Ride Bell", "Tambourine", "Splash Cymbal",
        "Cowbell", "Crash Cymbal 2", "Vibraslap", "Ride Cymbal 2", "Hi Bongo", "Low Bongo", "Mute Hi Conga",
        "Open Hi Conga", "Low Conga", "High Timbale", "Low Timbale", "High Agogo", "Low Agogo", "Cabasa", "Maracas",
        "Short Whistle", "Long Whistle", "Short Guiro", "Long Guiro", "Claves", "Hi Wood Block", "Low Wood Block",
        "Mute Cuica", "Open Cuica", "Mute Triangle", "Open Triangle", "Shaker", "Jingle Bell", "Bell Tree", "Castanets",
        "Mute Surdo", "Open Surdo",
    ];
    if (27..=87).contains(&n) {
        T[(n - 27) as usize]
    } else {
        ""
    }
}

/// Every parameter at its firmware default.
pub fn defaults() -> Vec<(u8, u16)> {
    SPECS.iter().map(|s| (s.id, s.default)).collect()
}

/// A mapping preset: a full or partial parameter set.
#[derive(Clone, Debug, Serialize)]
pub struct MapPreset {
    pub id: String,
    pub name: String,
    pub summary: String,
    /// Suggested Base scene for this mapping ("" = keep the current scene).
    pub scene: String,
    pub builtin: bool,
    pub values: Vec<(u8, u16)>,
}

fn with(base: &[(u8, u16)], over: &[(u8, u16)]) -> Vec<(u8, u16)> {
    let mut v = base.to_vec();
    for &(id, val) in over {
        match v.iter_mut().find(|x| x.0 == id) {
            Some(x) => x.1 = val,
            None => v.push((id, val)),
        }
    }
    v.sort_by_key(|x| x.0);
    v
}

/// The drum-kit mapping: six strike directions on GM channel 10, laid out like a kit
/// in front of the player. Every note exists in the VCSL kits on the Base.
pub const KIT: &[(u8, u16)] = &[
    (8, 0),    // no controllers: the kit wants notes only
    (11, 1),   // tap percussion on
    (12, 9),   // channel 10
    (13, 38),  // unclear direction: snare
    (14, 0),   // touch-strip mode (strikes come from the motion sensor)
    (25, 70),  // a little more sensitive than default
    (26, 90),  // allow faster rolls
    (31, 6),   // all six directions
    (32, 38),  // down: snare
    (33, 49),  // up: crash
    (34, 36),  // left: kick
    (35, 42),  // right: closed hi-hat
    (36, 45),  // forward: low tom
    (37, 46),  // back: open hi-hat
];

pub fn builtin_presets() -> Vec<MapPreset> {
    let d = defaults();
    let mk = |id: &str, name: &str, summary: &str, scene: &str, over: &[(u8, u16)]| MapPreset {
        id: id.into(),
        name: name.into(),
        summary: summary.into(),
        scene: scene.into(),
        builtin: true,
        values: with(&d, over),
    };
    vec![
        mk("trillium-kit", "Drum kit", "Strike in six directions: down snare, up crash, left kick, right closed hat, forward tom, back open hat (GM channel 10)", "trillium kit", KIT),
        mk("tap-drum", "Tap drum", "Every tap plays one drum note (snare) on channel 10", "trillium kit", &[(11, 1), (12, 9), (13, 38), (31, 0)]),
        mk("tilt-melody", "Tilt melody", "Roll picks the note in C major pentatonic, tilt forward to play", "", &[(14, 1), (2, 60), (3, 24), (4, 1), (5, 1), (11, 0)]),
        mk("tilt-strike", "Tilt + strike", "Tilt chooses the note, a strike plays it", "", &[(14, 3), (2, 48), (3, 24), (4, 2), (11, 1), (31, 2)]),
        mk("theremin", "Theremin", "Continuous pitch from roll, volume from forward tilt", "", &[(14, 2), (2, 48), (3, 24), (5, 0), (6, 24), (11, 0)]),
        mk("touch-strip", "Touch strip (defaults)", "Firmware defaults: the touch strip plays, motion sends CC74 and CC1", "", &[]),
        mk("flick-gestures", "Flick gestures", "Flicks around Z play toms on channel 10; taps play the snare", "trillium kit", &[(11, 1), (12, 9), (13, 38), (27, 3), (28, 500), (30, 47)]),
    ]
}

/// Values as a `trill-preset` file (the Trillium app's format).
pub fn to_trill_preset(id: &str, name: &str, values: &[(u8, u16)]) -> Value {
    let mut m = Map::new();
    for &(pid, v) in values {
        if let Some(s) = spec(pid) {
            m.insert(s.key.into(), json!(v));
        }
    }
    json!({ "format": "trill-preset", "version": 1, "protocol_version": 1, "id": id, "name": name, "values": Value::Object(m) })
}

/// Reads a `trill-preset` file. Unknown keys and out-of-range values are errors, missing
/// keys take the firmware default (the app's import rule).
pub fn from_trill_preset(v: &Value) -> Result<(String, Vec<(u8, u16)>), String> {
    if v.get("format").and_then(|x| x.as_str()) != Some("trill-preset") {
        return Err("not a trill-preset file (format)".into());
    }
    if v.get("version").and_then(|x| x.as_u64()) != Some(1) {
        return Err("unsupported trill-preset version".into());
    }
    let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("Imported").to_string();
    let vals = v.get("values").and_then(|x| x.as_object()).ok_or("missing values")?;
    let mut out = defaults();
    for (k, x) in vals {
        let s = SPECS.iter().find(|s| s.key == k).ok_or_else(|| format!("unknown key {k}"))?;
        let n = x.as_u64().ok_or_else(|| format!("{k}: not a number"))?;
        let n = u16::try_from(n).map_err(|_| format!("{k}: out of range"))?;
        validate(s.id, n)?;
        out.iter_mut().find(|p| p.0 == s.id).unwrap().1 = n;
    }
    Ok((name, out))
}

/// Parameters the firmware can take, from its HELLO capability bits.
pub fn supported(id: u8, caps: u8) -> bool {
    match crate::zero::page_of(id) {
        0 => true,
        1 => caps & crate::zero::CAP_PAGED != 0,
        _ => caps & crate::zero::CAP_PAGE2 != 0,
    }
}

/// One-line plain description of what a mapping does, for the console and CLI.
pub fn describe(values: &[(u8, u16)]) -> String {
    let g = |id: u8| values.iter().find(|x| x.0 == id).map(|x| x.1).unwrap_or_else(|| spec(id).map(|s| s.default).unwrap_or(0));
    let mut parts = vec![];
    let mode = g(14);
    // Touch-strip mode is listed last when taps play: a motion-only Zero has no strip.
    let strip_last = mode == 0 && g(11) == 1;
    let lead = match mode {
        1 => format!("tilt notes from {} over {} semitones", note_name(g(2)), g(3)),
        2 => format!("theremin from {}", note_name(g(2))),
        3 => "tilt chooses, strike plays".to_string(),
        _ => format!("touch strip (if fitted) from {} on ch {}", note_name(g(2)), g(1) + 1),
    };
    if !strip_last {
        parts.push(lead.clone());
    }
    if g(11) == 1 {
        let ch = g(12) + 1;
        let dirs = g(31);
        if dirs == 0 {
            parts.push(format!("taps → {} ({}) on ch {ch}", note_name(g(13)), gm_drum(g(13))));
        } else {
            let names = ["down", "up", "left", "right", "forward", "back"];
            let s: Vec<String> = (0..dirs as usize).map(|i| format!("{} {}", names[i], short_drum(g(32 + i as u8)))).collect();
            parts.push(format!("strikes on ch {ch}: {}", s.join(", ")));
        }
    }
    if g(8) == 1 {
        parts.push(format!("CC{} / CC{}", g(16), g(18)));
    }
    if g(27) != 0 {
        parts.push(format!("flicks → {}..{}", g(30), g(30) + 2));
    }
    if strip_last {
        parts.push(lead);
    }
    parts.join(" · ")
}

fn short_drum(n: u16) -> String {
    let d = gm_drum(n);
    if d.is_empty() {
        note_name(n)
    } else {
        d.to_string()
    }
}

/// Schema for the console: groups, parameters and presets.
pub fn schema_json() -> Value {
    json!({
        "groups": GROUPS.iter().map(|(id, label, help)| json!({ "id": id, "label": label, "help": help })).collect::<Vec<_>>(),
        "params": SPECS,
        "gm_drums": (27u16..=87).map(|n| json!([n, gm_drum(n)])).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_cover_all_41_in_order() {
        assert_eq!(SPECS.len(), 41);
        for (i, s) in SPECS.iter().enumerate() {
            assert_eq!(s.id as usize, i + 1);
            assert!(s.default >= s.min && s.default <= s.max, "{}", s.key);
            assert!(validate(s.id, s.default).is_ok());
            assert_eq!(crate::zero::param_name(s.id).is_empty(), false);
        }
    }

    #[test]
    fn parses_notes_channels_and_choices() {
        assert_eq!(note_number("C4"), Some(60));
        assert_eq!(note_number("F#2"), Some(42));
        assert_eq!(note_number("Eb3"), Some(51));
        let ch = lookup("percussion channel").unwrap();
        assert_eq!(parse_value(ch, "10").unwrap(), 9);
        assert_eq!(parse_value(lookup("31").unwrap(), "6").unwrap(), 6);
        assert!(parse_value(lookup("31").unwrap(), "3").is_err());
        assert_eq!(parse_value(lookup("mode").unwrap(), "tilt +").unwrap(), 3);
        assert_eq!(parse_value(lookup("strike_note_left").unwrap(), "C2").unwrap(), 36);
        assert_eq!(parse_value(lookup("tap percussion").unwrap(), "on").unwrap(), 1);
        assert!(parse_value(lookup("tap_threshold_cg").unwrap(), "10").is_err());
    }

    #[test]
    fn trill_preset_round_trip_and_strict_import() {
        let kit = builtin_presets().into_iter().find(|p| p.id == "trillium-kit").unwrap();
        let j = to_trill_preset("x", "Kit", &kit.values);
        let (name, back) = from_trill_preset(&j).unwrap();
        assert_eq!(name, "Kit");
        assert_eq!(back, kit.values);
        let bad = json!({ "format": "trill-preset", "version": 1, "values": { "nope": 1 } });
        assert!(from_trill_preset(&bad).is_err());
        let oor = json!({ "format": "trill-preset", "version": 1, "values": { "channel": 16 } });
        assert!(from_trill_preset(&oor).is_err());
    }

    #[test]
    fn kit_is_valid_and_reads_like_a_kit() {
        for p in builtin_presets() {
            assert_eq!(p.values.len(), 41, "{}", p.id);
            for &(id, v) in &p.values {
                validate(id, v).unwrap();
            }
        }
        let kit = builtin_presets().into_iter().find(|p| p.id == "trillium-kit").unwrap();
        let d = describe(&kit.values);
        assert!(d.contains("left Bass Drum 1"), "{d}");
        assert!(d.contains("ch 10"), "{d}");
    }
}
