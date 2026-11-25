// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! External sound engines (FluidSynth, ZynAddSubFX) run as user services that pilabd starts
//! and stops. Their MIDI comes from the MIDI router and their audio goes into
//! `Trillium Synth:engine_l/r`, before the master level and limiter.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Def {
    pub id: &'static str,
    pub name: &'static str,
    pub unit: &'static str,
    pub bin: &'static str,
    pub license: &'static str,
    /// JACK client name prefix of the running engine.
    pub client: &'static str,
    pub midi_in: &'static str,
    pub audio: [&'static str; 2],
    pub env: &'static str,
}

pub const DEFS: [Def; 2] = [
    Def { id: "fluidsynth", name: "FluidSynth (General MIDI)", unit: "pilab-fluidsynth.service", bin: "/usr/bin/fluidsynth", license: "LGPL-2.1-or-later", client: "fluidsynth", midi_in: "midi_00", audio: ["left", "right"], env: "fluidsynth.env" },
    Def { id: "zynaddsubfx", name: "ZynAddSubFX", unit: "pilab-zynaddsubfx.service", bin: "/usr/bin/zynaddsubfx", license: "GPL-2.0-or-later", client: "zynaddsubfx", midi_in: "midi_input", audio: ["out_1", "out_2"], env: "zynaddsubfx.env" },
];

pub const SF2_DIRS: [&str; 1] = ["/usr/share/sounds/sf2"];
pub const ZYN_BANKS: &str = "/usr/share/zynaddsubfx/banks";

pub fn def(id: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.id == id)
}

/// Is this JACK port one of an engine's? (They are never MIDI sources or DAC clients.)
pub fn is_engine_port(name: &str) -> bool {
    DEFS.iter().any(|d| name.starts_with(d.client))
}

fn systemctl(args: &[&str]) -> Result<String, String> {
    let o = Command::new("systemctl").arg("--user").args(args).output().map_err(|e| format!("systemctl: {e}"))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        Err(format!("systemctl {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()))
    }
}

pub fn installed(d: &Def) -> bool {
    Path::new(d.bin).exists()
}

pub fn active(d: &Def) -> bool {
    Command::new("systemctl").args(["--user", "is-active", "--quiet", d.unit]).status().map(|s| s.success()).unwrap_or(false)
}

pub fn start(id: &str) -> Result<(), String> {
    let d = def(id).ok_or("unknown engine")?;
    if !installed(d) {
        return Err(format!("{} is not installed (apt install {})", d.name, if d.id == "fluidsynth" { "fluidsynth fluid-soundfont-gm" } else { "zynaddsubfx zynaddsubfx-data" }));
    }
    systemctl(&["start", d.unit]).map(|_| ())
}

pub fn stop(id: &str) -> Result<(), String> {
    let d = def(id).ok_or("unknown engine")?;
    systemctl(&["stop", d.unit]).map(|_| ())
}

fn env_path(cfg: &Path, d: &Def) -> PathBuf {
    cfg.join(d.env)
}

fn read_env(cfg: &Path, d: &Def, key: &str) -> String {
    std::fs::read_to_string(env_path(cfg, d))
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix(&format!("{key}=")).map(|v| v.trim().trim_matches('"').to_string())))
        .unwrap_or_default()
}

fn safe_path(p: &str, roots: &[&str], ext: &str) -> Result<PathBuf, String> {
    if p.contains(['"', '\n', '$', '`', '\\']) || p.contains("..") {
        return Err("path has characters that are not allowed".into());
    }
    let pb = PathBuf::from(p);
    if !pb.extension().map(|e| e.eq_ignore_ascii_case(ext)).unwrap_or(false) {
        return Err(format!("expected a .{ext} file"));
    }
    if !roots.iter().any(|r| pb.starts_with(r)) {
        return Err(format!("file must be under {}", roots.join(" or ")));
    }
    if !pb.is_file() {
        return Err("no such file".into());
    }
    Ok(pb)
}

/// Picks the instrument file (SoundFont for FluidSynth, .xiz for ZynAddSubFX) and restarts
/// the engine if it is running.
pub fn set_sound(cfg: &Path, id: &str, file: &str) -> Result<(), String> {
    let d = def(id).ok_or("unknown engine")?;
    let (key, pb) = match id {
        "fluidsynth" => ("SF2", safe_path(file, &SF2_DIRS, "sf2")?),
        _ => ("ZYN_XIZ", safe_path(file, &[ZYN_BANKS], "xiz")?),
    };
    std::fs::create_dir_all(cfg).map_err(|e| e.to_string())?;
    let tmp = env_path(cfg, d).with_extension("tmp");
    std::fs::write(&tmp, format!("# written by pilabd\n{key}=\"{}\"\n", pb.display())).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, env_path(cfg, d)).map_err(|e| e.to_string())?;
    if active(d) {
        systemctl(&["restart", d.unit])?;
    }
    Ok(())
}

pub fn soundfonts() -> Vec<Value> {
    let mut v = vec![];
    for dir in SF2_DIRS {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("sf2")).unwrap_or(false) && !p.is_symlink()).collect();
        files.sort();
        for p in files {
            let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let license = match name.as_str() {
                "FluidR3_GM" => "MIT",
                "TimGM6mb" => "GPL-2.0-only",
                _ => "see its package",
            };
            v.push(json!({ "path": p, "name": name, "license": license, "bytes": p.metadata().map(|m| m.len()).unwrap_or(0) }));
        }
    }
    v
}

pub fn zyn_instruments() -> Vec<Value> {
    let mut v = vec![];
    let Ok(rd) = std::fs::read_dir(ZYN_BANKS) else { return v };
    let mut banks: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    banks.sort();
    for b in banks {
        let bank = b.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let Ok(rd) = std::fs::read_dir(&b) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "xiz").unwrap_or(false)).collect();
        files.sort();
        for f in files {
            let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            // "0001-DX Rhodes 1" -> "DX Rhodes 1"
            let name = stem.split_once('-').filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit())).map(|(_, r)| r.to_string()).unwrap_or(stem);
            v.push(json!({ "bank": bank, "name": name, "path": f }));
        }
    }
    v
}

pub fn status(cfg: &Path, ports: &[(String, String)]) -> Value {
    Value::Array(
        DEFS.iter()
            .map(|d| {
                let sound = match d.id {
                    "fluidsynth" => {
                        let s = read_env(cfg, d, "SF2");
                        if s.is_empty() { "/usr/share/sounds/sf2/FluidR3_GM.sf2".to_string() } else { s }
                    }
                    _ => read_env(cfg, d, "ZYN_XIZ"),
                };
                json!({
                    "id": d.id, "name": d.name, "license": d.license, "unit": d.unit,
                    "installed": installed(d), "running": active(d),
                    "midi_port": ports.iter().find(|p| p.0 == d.id).map(|p| p.1.clone()),
                    "sound": sound,
                })
            })
            .collect(),
    )
}

pub const GM_PROGRAMS: [&str; 128] = [
    "Acoustic Grand Piano", "Bright Acoustic Piano", "Electric Grand Piano", "Honky-tonk Piano", "Electric Piano 1", "Electric Piano 2", "Harpsichord", "Clavinet",
    "Celesta", "Glockenspiel", "Music Box", "Vibraphone", "Marimba", "Xylophone", "Tubular Bells", "Dulcimer",
    "Drawbar Organ", "Percussive Organ", "Rock Organ", "Church Organ", "Reed Organ", "Accordion", "Harmonica", "Tango Accordion",
    "Acoustic Guitar (nylon)", "Acoustic Guitar (steel)", "Electric Guitar (jazz)", "Electric Guitar (clean)", "Electric Guitar (muted)", "Overdriven Guitar", "Distortion Guitar", "Guitar Harmonics",
    "Acoustic Bass", "Electric Bass (finger)", "Electric Bass (pick)", "Fretless Bass", "Slap Bass 1", "Slap Bass 2", "Synth Bass 1", "Synth Bass 2",
    "Violin", "Viola", "Cello", "Contrabass", "Tremolo Strings", "Pizzicato Strings", "Orchestral Harp", "Timpani",
    "String Ensemble 1", "String Ensemble 2", "Synth Strings 1", "Synth Strings 2", "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit",
    "Trumpet", "Trombone", "Tuba", "Muted Trumpet", "French Horn", "Brass Section", "Synth Brass 1", "Synth Brass 2",
    "Soprano Sax", "Alto Sax", "Tenor Sax", "Baritone Sax", "Oboe", "English Horn", "Bassoon", "Clarinet",
    "Piccolo", "Flute", "Recorder", "Pan Flute", "Blown Bottle", "Shakuhachi", "Whistle", "Ocarina",
    "Lead 1 (square)", "Lead 2 (sawtooth)", "Lead 3 (calliope)", "Lead 4 (chiff)", "Lead 5 (charang)", "Lead 6 (voice)", "Lead 7 (fifths)", "Lead 8 (bass + lead)",
    "Pad 1 (new age)", "Pad 2 (warm)", "Pad 3 (polysynth)", "Pad 4 (choir)", "Pad 5 (bowed)", "Pad 6 (metallic)", "Pad 7 (halo)", "Pad 8 (sweep)",
    "FX 1 (rain)", "FX 2 (soundtrack)", "FX 3 (crystal)", "FX 4 (atmosphere)", "FX 5 (brightness)", "FX 6 (goblins)", "FX 7 (echoes)", "FX 8 (sci-fi)",
    "Sitar", "Banjo", "Shamisen", "Koto", "Kalimba", "Bagpipe", "Fiddle", "Shanai",
    "Tinkle Bell", "Agogo", "Steel Drums", "Woodblock", "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal",
    "Guitar Fret Noise", "Breath Noise", "Seashore", "Bird Tweet", "Telephone Ring", "Helicopter", "Applause", "Gunshot",
];
