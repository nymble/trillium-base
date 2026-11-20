// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Volume and mute for PipeWire nodes (sinks, capture sources, Pulse/PipeWire app streams),
//! read with `pw-dump` and set with `wpctl`, in WirePlumber's cubic scale (the number
//! pavucontrol and `wpctl status` show). Changes are ramped and capped at unity (1.0):
//! nothing here can boost a node, so the synth's own -6 dBFS cap and limiter still bound
//! what reaches the DAC. JACK apps (pw-jack) have no PipeWire volume; their level is set in
//! the app. The synth's level is its master (see /master).

use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::brand::{LEGACY_SYNTH_CLIENT, SYNTH_NAME};

/// Highest volume the console sets on any node (unity, no boost).
pub const VOLUME_CAP: f32 = 1.0;
const RAMP_STEPS: usize = 6;
const RAMP_STEP_MS: u64 = 20;

#[derive(Clone, Debug, Serialize)]
pub struct MixNode {
    pub id: u32,
    /// PipeWire node.name (stable id used to check the target of a change)
    pub name: String,
    /// What JACK and the patch panel call it (node.description, else nick, else name)
    pub label: String,
    pub class: String,
    /// "sink" | "source" | "stream" | "jack"
    pub role: &'static str,
    /// Cubic volume 0..1, None when the node has no PipeWire volume
    pub volume: Option<f32>,
    pub mute: Option<bool>,
    pub state: String,
    pub airplay: bool,
}

pub struct Mixer {
    cache: Mutex<Option<(Instant, Vec<MixNode>)>>,
    /// Development: nodes from a recorded `pw-dump` (PILAB_DEMO_PWDUMP), changed in memory.
    demo: Option<Mutex<Vec<MixNode>>>,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Mixer {
    pub fn new() -> Mixer {
        let demo = std::env::var_os("PILAB_DEMO_PWDUMP")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map(|v| Mutex::new(parse_dump(&v)));
        Mixer { cache: Mutex::new(None), demo }
    }

    /// Audio nodes, cached for 2 s (pw-dump is not free).
    pub fn nodes(&self) -> Vec<MixNode> {
        if let Some(d) = &self.demo {
            return d.lock().unwrap().clone();
        }
        if let Some((t, v)) = &*self.cache.lock().unwrap() {
            if t.elapsed() < Duration::from_secs(2) {
                return v.clone();
            }
        }
        let v = Command::new("pw-dump")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice::<Value>(&o.stdout).ok())
            .map(|v| parse_dump(&v))
            .unwrap_or_default();
        *self.cache.lock().unwrap() = Some((Instant::now(), v.clone()));
        v
    }

    /// Sets volume (cubic, ramped, capped) and/or mute on node `id`, which must still be
    /// the node called `name` (ids are reused by PipeWire).
    pub fn set(&self, id: u32, name: &str, volume: Option<f32>, mute: Option<bool>) -> Result<MixNode, String> {
        if let Some(d) = &self.demo {
            let mut g = d.lock().unwrap();
            let n = g.iter_mut().find(|n| n.id == id && n.name == name).ok_or("no such node (it may have gone)")?;
            if let Some(v) = volume {
                n.volume.ok_or("this node has no PipeWire volume")?;
                n.volume = Some(v.clamp(0.0, VOLUME_CAP));
            }
            if let Some(m) = mute {
                n.mute = Some(m);
            }
            return Ok(n.clone());
        }
        *self.cache.lock().unwrap() = None;
        let cur = self.nodes().into_iter().find(|n| n.id == id && n.name == name).ok_or("no such node (it may have gone)")?;
        if let Some(target) = volume {
            let from = cur.volume.ok_or("this node has no PipeWire volume (a JACK app: set its level in the app)")?;
            let target = target.clamp(0.0, VOLUME_CAP);
            for k in 1..=RAMP_STEPS {
                let v = from + (target - from) * k as f32 / RAMP_STEPS as f32;
                wpctl(&["set-volume", &id.to_string(), &format!("{v:.4}")])?;
                if k < RAMP_STEPS {
                    std::thread::sleep(Duration::from_millis(RAMP_STEP_MS));
                }
            }
        }
        if let Some(m) = mute {
            wpctl(&["set-mute", &id.to_string(), if m { "1" } else { "0" }])?;
        }
        *self.cache.lock().unwrap() = None;
        self.nodes().into_iter().find(|n| n.id == id).ok_or_else(|| "node gone after change".into())
    }
}

fn wpctl(args: &[&str]) -> Result<(), String> {
    let o = Command::new("wpctl").args(args).output().map_err(|e| format!("wpctl: {e}"))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(format!("wpctl {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()))
    }
}

/// Audio nodes from a `pw-dump` document.
pub fn parse_dump(v: &Value) -> Vec<MixNode> {
    let mut out = vec![];
    for o in v.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
        if o["type"] != "PipeWire:Interface:Node" {
            continue;
        }
        let p = &o["info"]["props"];
        let s = |k: &str| p[k].as_str().unwrap_or("").to_string();
        let class = s("media.class");
        let jack = p["client.api"] == "jack";
        let role = match class.as_str() {
            "Audio/Sink" => "sink",
            "Audio/Source" | "Audio/Source/Virtual" => "source",
            c if c.starts_with("Stream/") && c.ends_with("/Audio") => "stream",
            _ if jack => "jack",
            _ => continue,
        };
        let mut name = s("node.name");
        let mut label = [s("node.description"), s("node.nick"), name.clone()].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
        if jack && name == LEGACY_SYNTH_CLIENT {
            name = SYNTH_NAME.into();
            label = SYNTH_NAME.into();
        }
        let props = o["info"]["params"]["Props"].as_array().and_then(|a| a.iter().find(|x| x.get("channelVolumes").is_some() || x.get("mute").is_some()).cloned());
        let (volume, mute) = match &props {
            Some(pr) if !jack => {
                let cv: Vec<f32> = pr["channelVolumes"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default();
                let lin = if cv.is_empty() { pr["volume"].as_f64().unwrap_or(1.0) as f32 } else { cv.iter().cloned().fold(0.0, f32::max) };
                (Some(lin.max(0.0).cbrt()), pr["mute"].as_bool())
            }
            _ => (None, None),
        };
        out.push(MixNode {
            id: o["id"].as_u64().unwrap_or(0) as u32,
            airplay: name.starts_with("raop_sink.") || name.starts_with("raop_source."),
            name,
            label,
            class,
            role,
            volume,
            mute,
            state: o["info"]["state"].as_str().unwrap_or("").to_string(),
        });
    }
    out
}

/// dB of a cubic volume (WirePlumber scale): 60·log10(v).
pub fn cubic_db(v: f32) -> f32 {
    if v <= 0.0 {
        -120.0
    } else {
        60.0 * v.log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sinks_and_jack_nodes() {
        let v: Value = serde_json::json!([
            {"id": 72, "type": "PipeWire:Interface:Node", "info": {"state": "running", "props": {"media.class": "Audio/Sink", "node.name": "alsa_output.x", "node.description": "Built-in Audio Stereo"},
              "params": {"Props": [{"volume": 1.0, "mute": false, "channelVolumes": [0.343, 0.343]}]}}},
            {"id": 92, "type": "PipeWire:Interface:Node", "info": {"state": "suspended", "props": {"media.class": "Audio/Sink", "node.name": "raop_sink.Yurt.local", "node.description": "Yurt"},
              "params": {"Props": [{"volume": 1.0, "mute": false, "channelVolumes": []}]}}},
            {"id": 138, "type": "PipeWire:Interface:Node", "info": {"state": "running", "props": {"client.api": "jack", "node.name": "pilab-synth", "node.description": "pilab-synth"}, "params": {}}},
            {"id": 30, "type": "PipeWire:Interface:Node", "info": {"props": {"node.name": "Dummy-Driver"}}}
        ]);
        let n = parse_dump(&v);
        assert_eq!(n.len(), 3);
        assert_eq!(n[0].label, "Built-in Audio Stereo");
        assert!((n[0].volume.unwrap() - 0.7).abs() < 0.01, "cubic volume");
        assert!(n[1].airplay && n[1].volume == Some(1.0));
        assert_eq!(n[2].label, SYNTH_NAME, "old synth node name maps to the new one");
        assert_eq!(n[2].volume, None, "JACK nodes have no PipeWire volume");
        assert!((cubic_db(0.7) + 9.29).abs() < 0.05);
    }
}
