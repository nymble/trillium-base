// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Console settings: audio safety, recording and archival switches, watchdog.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub audio: Audio,
    pub input: Input,
    pub midi: Midi,
    pub recording: Recording,
    pub archive: Archive,
    pub watchdog: Watchdog,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Audio {
    /// Ports the synth output is connected to on start (PipeWire names via the JACK API).
    pub output_ports: Vec<String>,
    /// PipeWire sink the watchdog suspends and resumes.
    pub dac_sink: String,
    /// Start the engine (with a fade-in) when the service starts.
    pub start_engine: bool,
}

impl Default for Audio {
    fn default() -> Self {
        Audio {
            output_ports: vec!["Built-in Audio Stereo:playback_FL".into(), "Built-in Audio Stereo:playback_FR".into()],
            dac_sink: "alsa_output.platform-soc_107c000000_sound.stereo-fallback".into(),
            start_engine: true,
        }
    }
}

/// The console's audio input strip: an external source (a USB microphone, an audio
/// interface) connected to the synth node's `input_l`/`input_r` ports. It never reaches
/// the DAC unless `monitor` is on, so a microphone near the speakers cannot feed back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Input {
    /// Glob on graph port names for the source, e.g. "Yeti Nano*:capture_*". The first
    /// two matching audio outputs feed left and right (a mono source feeds both).
    /// Empty: nothing is connected automatically.
    pub source: String,
    /// Connect the source whenever it appears (at start, after a replug).
    pub autoconnect: bool,
    /// Input gain in dB (-60..=+24).
    pub gain_db: f32,
    /// Mix the input into the synth output (and so the DAC). Off by default.
    pub monitor: bool,
    /// Include the input in local recordings even when it is not monitored.
    pub record: bool,
}

impl Default for Input {
    fn default() -> Self {
        Input { source: String::new(), autoconnect: true, gain_db: 0.0, monitor: false, record: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Midi {
    /// Connect every MIDI source in the graph to an input slot automatically.
    pub autoconnect: bool,
    /// Sources never auto-connected (globs).
    pub ignore: Vec<String>,
    /// Known controllers, matched in order. Routes use `input = "device:<id>"`.
    pub devices: Vec<crate::devices::DeviceClass>,
}

impl Default for Midi {
    fn default() -> Self {
        Midi { autoconnect: true, ignore: vec!["*Midi Through*".into()], devices: crate::devices::default_classes() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recording {
    /// Allow recording the synth output to local storage.
    pub enabled: bool,
    /// Directory for recordings; `~` expands to the service user's home.
    pub location: String,
    /// Recording stops when free space falls below this.
    pub min_free_mb: u64,
    /// Warn about recordings not filed to the NAS after this many hours.
    pub unfiled_warn_hours: u32,
}

impl Default for Recording {
    fn default() -> Self {
        Recording {
            enabled: true,
            location: "~/.local/share/pilab/recordings".into(),
            min_free_mb: 2048,
            unfiled_warn_hours: 24,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Archive {
    /// Copy finished recordings to the NAS. Off by default; local may be enough.
    pub enabled: bool,
    /// "manual" or "after-each".
    pub mode: String,
    /// SMB share for recordings (the dedicated PilabRecordings share).
    pub target: String,
    /// NAS account that may write only to that share.
    pub account: String,
    /// Root-only credentials file on the Pi (never read by the console).
    pub credentials_file: String,
    /// Set once the NAS account exists and a test login passed.
    pub configured: bool,
}

impl Default for Archive {
    fn default() -> Self {
        Archive {
            enabled: false,
            mode: "manual".into(),
            target: String::new(), // e.g. "//<nas-host>/Recordings"; set in the console
            account: "pilab-rec".into(),
            credentials_file: "/etc/pilab/secrets/nas-rec.cred".into(),
            configured: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Watchdog {
    /// Suspend and resume the DAC sink when a dropout storm is detected.
    pub enabled: bool,
    /// Xruns per second that count as a storm.
    pub xruns_per_s: f32,
    /// Seconds the storm must last.
    pub hold_s: u32,
    /// Minimum seconds between two interventions.
    pub cooldown_s: u32,
}

impl Default for Watchdog {
    fn default() -> Self {
        Watchdog { enabled: true, xruns_per_s: 5.0, hold_s: 6, cooldown_s: 120 }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            audio: Audio::default(),
            input: Input::default(),
            midi: Midi::default(),
            recording: Recording::default(),
            archive: Archive::default(),
            watchdog: Watchdog::default(),
        }
    }
}

impl Settings {
    pub fn from_toml(s: &str) -> Result<Settings, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }
    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| e.to_string())
    }
    /// Human status of the archive switch for the console.
    pub fn archive_status(&self) -> (&'static str, String) {
        let a = &self.archive;
        match (a.enabled, a.configured) {
            (false, _) => ("off", "Off. Recordings stay on this device only.".into()),
            (true, _) if a.target.trim().is_empty() => (
                "not-configured",
                "Not configured: set a target share (for example //nas-host/Recordings). Nothing is mounted or copied.".into(),
            ),
            (true, false) => (
                "not-configured",
                format!(
                    "Not configured: the NAS account {} for {} has not been set up yet. Nothing is mounted or copied.",
                    a.account, a.target
                ),
            ),
            (true, true) => ("ready", format!("Ready: {} mode to {} as {}.", a.mode, a.target, a.account)),
        }
    }
}
