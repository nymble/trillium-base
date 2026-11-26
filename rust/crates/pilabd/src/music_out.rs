// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Where the player plays, and what happens when audio hardware comes and goes.
//!
//! The player's PipeWire stream is started with `node.autoconnect = false` (pilab-mpd.service),
//! so WirePlumber never links it and a newly plugged interface can never take it over. This
//! module links it by node *name* (never by ALSA card number) to the outputs chosen on the
//! Music page: the DAC by default, any other sink (a USB interface such as a Scarlett 2i2,
//! AirPlay speakers from the AirPlay sender), or several at once. A chosen output that is not
//! present is skipped; if none is present the DAC is used.
//!
//! The same loop watches for audio hardware being added or removed: it reports it as a
//! console event, never routes the synth or the input strip to it, and re-links the synth
//! to the DAC when the DAC's PipeWire node is re-created (e.g. after WirePlumber restarts).
//! It also flags a sound card the kernel has but PipeWire has not picked up.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::App;

/// node.name prefix of the player's stream(s) (MPD names them "mpd.<output name>").
pub const PLAYER_PREFIX: &str = "mpd.";
const LOOP: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Sink {
    pub id: u32,
    pub name: String,
    pub label: String,
    /// dac | usb | airplay | bluetooth | hdmi | other
    pub kind: String,
    pub channels: Vec<String>,
    #[serde(skip)]
    pub ports: Vec<(u32, String)>,
    pub state: String,
    pub card: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Link {
    pub id: u32,
    pub out_port: u32,
    pub in_port: u32,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Device {
    pub id: u32,
    pub name: String,
    pub label: String,
    pub api: String,
    pub bus: String,
    pub card: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub sinks: Vec<Sink>,
    /// Player stream nodes: (node id, node name, output ports (id, channel))
    pub players: Vec<(u32, String, Vec<(u32, String)>)>,
    pub links: Vec<Link>,
    pub devices: Vec<Device>,
    /// Sources (inputs), for the hotplug report: (name, label)
    pub sources: Vec<(String, String)>,
}

fn s(v: &Value, k: &str) -> String {
    match &v[k] {
        Value::String(x) => x.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn chan_rank(c: &str) -> usize {
    const ORDER: [&str; 8] = ["FL", "FR", "FC", "LFE", "RL", "RR", "SL", "SR"];
    if let Some(i) = ORDER.iter().position(|x| *x == c) {
        return i;
    }
    // AUX0, AUX1 ... / 1, 2 ...
    100 + c.trim_start_matches(|ch: char| !ch.is_ascii_digit()).parse::<usize>().unwrap_or(50)
}

pub fn kind_of(name: &str, api: &str, dac: &str) -> &'static str {
    if name == dac {
        "dac"
    } else if name.starts_with("raop_sink") || api == "raop" {
        "airplay"
    } else if api == "bluez5" || name.starts_with("bluez_") {
        "bluetooth"
    } else if name.contains("hdmi") {
        "hdmi"
    } else if name.contains("usb") {
        "usb"
    } else {
        "other"
    }
}

/// Parse `pw-dump` output.
pub fn parse(v: &Value, dac: &str) -> Graph {
    let mut g = Graph::default();
    let Some(arr) = v.as_array() else { return g };
    let mut ports: BTreeMap<u32, Vec<(u32, String, String)>> = BTreeMap::new(); // node -> (port, dir, channel)
    for o in arr {
        let ty = o["type"].as_str().unwrap_or("");
        let id = o["id"].as_u64().unwrap_or(0) as u32;
        let info = &o["info"];
        let p = &info["props"];
        match ty {
            "PipeWire:Interface:Port" => {
                let node = p["node.id"].as_u64().unwrap_or(0) as u32;
                if p["port.monitor"].as_bool() == Some(true) || s(p, "port.monitor") == "true" {
                    continue;
                }
                let ch = {
                    let c = s(p, "audio.channel");
                    if c.is_empty() {
                        s(p, "port.name")
                    } else {
                        c
                    }
                };
                ports.entry(node).or_default().push((id, info["direction"].as_str().unwrap_or("").to_string(), ch));
            }
            "PipeWire:Interface:Link" => g.links.push(Link {
                id,
                out_port: info["output-port-id"].as_u64().unwrap_or(0) as u32,
                in_port: info["input-port-id"].as_u64().unwrap_or(0) as u32,
            }),
            "PipeWire:Interface:Device" if s(p, "media.class") == "Audio/Device" => g.devices.push(Device {
                id,
                name: s(p, "device.name"),
                label: {
                    let d = s(p, "device.description");
                    if d.is_empty() {
                        s(p, "device.name")
                    } else {
                        d
                    }
                },
                api: s(p, "device.api"),
                bus: s(p, "device.bus"),
                card: s(p, "api.alsa.card").parse().ok(),
            }),
            _ => {}
        }
    }
    for o in arr {
        if o["type"].as_str() != Some("PipeWire:Interface:Node") {
            continue;
        }
        let id = o["id"].as_u64().unwrap_or(0) as u32;
        let p = &o["info"]["props"];
        let name = s(p, "node.name");
        let class = s(p, "media.class");
        let mut ps = ports.get(&id).cloned().unwrap_or_default();
        ps.sort_by_key(|x| chan_rank(&x.2));
        if class == "Audio/Sink" {
            let api = s(p, "device.api");
            let card_name = s(p, "alsa.card_name");
            let desc = s(p, "node.description");
            let label = if !card_name.is_empty() && api == "alsa" { card_name } else if desc.is_empty() { name.clone() } else { desc };
            let ins: Vec<(u32, String)> = ps.iter().filter(|x| x.1 == "input").map(|x| (x.0, x.2.clone())).collect();
            g.sinks.push(Sink {
                id,
                kind: kind_of(&name, &api, dac).into(),
                channels: ins.iter().map(|x| x.1.clone()).collect(),
                ports: ins,
                state: o["info"]["state"].as_str().unwrap_or("").to_string(),
                card: s(p, "api.alsa.card").parse().ok(),
                label,
                name,
            });
        } else if class == "Audio/Source" && !name.ends_with(".monitor") {
            let desc = s(p, "node.description");
            g.sources.push((name.clone(), if desc.is_empty() { name } else { desc }));
        } else if class == "Stream/Output/Audio" && name.starts_with(PLAYER_PREFIX) {
            let outs: Vec<(u32, String)> = ps.iter().filter(|x| x.1 == "output").map(|x| (x.0, x.2.clone())).collect();
            g.players.push((id, name, outs));
        }
    }
    g.sinks.sort_by(|a, b| (a.kind != "dac", a.label.clone()).cmp(&(b.kind != "dac", b.label.clone())));
    g
}

/// The outputs actually used: the chosen ones that are present, else the DAC.
pub fn effective(g: &Graph, want: &[String], dac: &str) -> Vec<String> {
    let v: Vec<String> = want.iter().filter(|w| g.sinks.iter().any(|s| &s.name == *w)).cloned().collect();
    if !v.is_empty() {
        return v;
    }
    if g.sinks.iter().any(|s| s.name == dac) {
        vec![dac.to_string()]
    } else {
        vec![]
    }
}

/// Links to add (out port, in port) and link ids to remove so that every player stream
/// feeds exactly the `targets` sinks: first output channel to the sink's first input,
/// second to its second; a one-input sink gets both.
pub fn plan(g: &Graph, targets: &[String]) -> (Vec<(u32, u32)>, Vec<u32>) {
    let mut want: BTreeSet<(u32, u32)> = BTreeSet::new();
    for (_, _, outs) in &g.players {
        for t in targets {
            let Some(sk) = g.sinks.iter().find(|s| &s.name == t) else { continue };
            if sk.ports.is_empty() {
                continue;
            }
            for (i, (op, _)) in outs.iter().enumerate() {
                let ip = if sk.ports.len() == 1 { sk.ports[0].0 } else { sk.ports.get(i).map(|x| x.0).unwrap_or(sk.ports[sk.ports.len() - 1].0) };
                want.insert((*op, ip));
            }
        }
    }
    let player_ports: BTreeSet<u32> = g.players.iter().flat_map(|p| p.2.iter().map(|x| x.0)).collect();
    // Only links into sinks are ours to manage (the console's meters also tap the player).
    let sink_ports: BTreeSet<u32> = g.sinks.iter().flat_map(|s| s.ports.iter().map(|p| p.0)).collect();
    let mine = |l: &&Link| player_ports.contains(&l.out_port) && sink_ports.contains(&l.in_port);
    let have: BTreeSet<(u32, u32)> = g.links.iter().filter(mine).map(|l| (l.out_port, l.in_port)).collect();
    let add = want.difference(&have).cloned().collect();
    let rm = g.links.iter().filter(mine).filter(|l| !want.contains(&(l.out_port, l.in_port))).map(|l| l.id).collect();
    (add, rm)
}

/// Sinks the player currently feeds (by links).
pub fn active(g: &Graph) -> Vec<String> {
    let player_ports: BTreeSet<u32> = g.players.iter().flat_map(|p| p.2.iter().map(|x| x.0)).collect();
    let ins: BTreeSet<u32> = g.links.iter().filter(|l| player_ports.contains(&l.out_port)).map(|l| l.in_port).collect();
    g.sinks.iter().filter(|s| s.ports.iter().any(|p| ins.contains(&p.0))).map(|s| s.name.clone()).collect()
}

/// Kernel sound cards (`/proc/asound/cards`): (number, id, long name).
pub fn alsa_cards(text: &str) -> Vec<(u32, String, String)> {
    let mut v = vec![];
    let lines: Vec<&str> = text.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        let Some((num, rest)) = t.split_once(' ') else { continue };
        let Ok(n) = num.parse::<u32>() else { continue };
        let id = rest.trim_start().trim_start_matches('[').split(']').next().unwrap_or("").trim().to_string();
        let long = lines.get(i + 1).map(|x| x.trim().to_string()).unwrap_or_default();
        let short = rest.split_once("]: ").map(|x| x.1.trim().to_string()).unwrap_or_default();
        v.push((n, id, if long.is_empty() { short } else { long }));
    }
    v
}

/// USB audio stream capabilities (`/proc/asound/cardN/stream0`): "2 out, 2 in; 44.1-192 kHz; 24-bit".
pub fn usb_caps(text: &str) -> Value {
    let mut out = json!({});
    let mut sec = "";
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with("Playback:") {
            sec = "playback";
        } else if t.starts_with("Capture:") {
            sec = "capture";
        } else if sec.is_empty() {
            continue;
        } else if let Some(c) = t.strip_prefix("Channels:") {
            out[sec]["channels"] = json!(c.trim().parse::<u32>().unwrap_or(0));
        } else if let Some(r) = t.strip_prefix("Rates:") {
            let rates: Vec<u32> = r.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            out[sec]["rates"] = json!(rates);
        } else if let Some(b) = t.strip_prefix("Bits:") {
            out[sec]["bits"] = json!(b.trim().parse::<u32>().unwrap_or(0));
        } else if let Some(f) = t.strip_prefix("Format:") {
            out[sec]["format"] = json!(f.trim());
        }
    }
    out
}

// ---- runtime -----------------------------------------------------------------------------

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    outputs: Vec<String>,
}

pub struct Out {
    path: PathBuf,
    want: Mutex<Vec<String>>,
    last: Mutex<Graph>,
    pub unclaimed: Mutex<Vec<Value>>,
    app: Arc<App>,
}

static OUT: OnceLock<Arc<Out>> = OnceLock::new();

pub fn get() -> Option<Arc<Out>> {
    OUT.get().cloned()
}

fn dump() -> Option<Value> {
    let o = Command::new("pw-dump").output().ok()?;
    if !o.status.success() {
        return None;
    }
    serde_json::from_slice(&o.stdout).ok()
}

impl Out {
    fn dac(&self) -> String {
        self.app.settings().audio.dac_sink.clone()
    }

    pub fn wanted(&self) -> Vec<String> {
        self.want.lock().unwrap().clone()
    }

    /// Choose the outputs (sink node names). Empty means the DAC.
    pub fn set(&self, names: Vec<String>) -> Result<Value, String> {
        let mut v: Vec<String> = vec![];
        for n in names {
            if n.is_empty() || n.len() > 200 || n.contains('\n') {
                return Err("bad output name".into());
            }
            if !v.contains(&n) {
                v.push(n);
            }
        }
        if v.len() > 4 {
            return Err("at most 4 outputs at once".into());
        }
        *self.want.lock().unwrap() = v.clone();
        let txt = serde_json::to_string_pretty(&Saved { outputs: v }).unwrap_or_default();
        if let Some(d) = self.path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        std::fs::write(&self.path, txt).map_err(|e| format!("save outputs: {e}"))?;
        self.apply();
        Ok(self.json())
    }

    /// Read the graph and make the player's links match the choice. Returns the graph.
    pub fn apply(&self) -> Graph {
        let dac = self.dac();
        let Some(v) = dump() else { return self.last.lock().unwrap().clone() };
        let g = parse(&v, &dac);
        let targets = effective(&g, &self.wanted(), &dac);
        let (add, rm) = plan(&g, &targets);
        for id in &rm {
            let _ = Command::new("pw-link").args(["-d", &id.to_string()]).output();
        }
        for (o, i) in &add {
            let _ = Command::new("pw-link").args([o.to_string(), i.to_string()]).output();
        }
        let g = if add.is_empty() && rm.is_empty() { g } else { dump().map(|v| parse(&v, &dac)).unwrap_or(g) };
        *self.last.lock().unwrap() = g.clone();
        g
    }

    pub fn json(&self) -> Value {
        let g = self.last.lock().unwrap().clone();
        let dac = self.dac();
        let want = self.wanted();
        let eff = effective(&g, &want, &dac);
        let act = active(&g);
        let caps = |card: Option<u32>| card.and_then(|c| std::fs::read_to_string(format!("/proc/asound/card{c}/stream0")).ok()).map(|t| usb_caps(&t)).unwrap_or(Value::Null);
        let outs: Vec<Value> = g
            .sinks
            .iter()
            .map(|s| {
                json!({
                    "name": s.name, "label": s.label, "kind": s.kind, "channels": s.channels, "state": s.state,
                    "chosen": want.contains(&s.name) || (want.is_empty() && s.name == dac),
                    "used": eff.contains(&s.name), "active": act.contains(&s.name),
                    "caps": if s.kind == "usb" { caps(s.card) } else { Value::Null },
                    "latency": if s.kind == "airplay" { "about 2 s" } else if s.kind == "bluetooth" { "about 0.2 s" } else { "" },
                })
            })
            .collect();
        let missing: Vec<&String> = want.iter().filter(|w| !g.sinks.iter().any(|s| &s.name == *w)).collect();
        json!({
            "outputs": outs, "chosen": want, "used": eff, "active": act, "missing": missing,
            "dac": dac, "player_linked": !g.players.is_empty(),
            "unclaimed_cards": self.unclaimed.lock().unwrap().clone(),
        })
    }
}

/// Start the output/hotplug loop.
pub fn init(app: Arc<App>) {
    let path = app.paths.config.join("music/outputs.json");
    let want = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Saved>(&t).ok()).map(|s| s.outputs).unwrap_or_default();
    let o = Arc::new(Out { path, want: Mutex::new(want), last: Mutex::new(Graph::default()), unclaimed: Mutex::new(vec![]), app: app.clone() });
    let _ = OUT.set(o.clone());
    std::thread::Builder::new()
        .name("music-out".into())
        .spawn(move || {
            unsafe {
                let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
                libc::setpriority(libc::PRIO_PROCESS, tid, 10);
            }
            std::thread::sleep(Duration::from_secs(3));
            let mut first = true;
            let mut dac_id: Option<u32> = None;
            let mut devices: BTreeMap<String, String> = BTreeMap::new();
            let mut sinks: BTreeMap<String, String> = BTreeMap::new();
            let mut cards_seen: BTreeSet<u32> = BTreeSet::new();
            let mut pending: Vec<String> = vec![];
            let mut ever: BTreeSet<String> = BTreeSet::new();
            let mut tick = 0u64;
            loop {
                let g = o.apply();
                let dac = o.dac();
                // The DAC node was re-created: put the synth back on it (links are by name).
                let now_dac = g.sinks.iter().find(|s| s.name == dac).map(|s| s.id);
                if !first && now_dac.is_some() && now_dac != dac_id {
                    if o.app.try_connect_outputs(false) {
                        o.app.event("info", "DAC reappeared in PipeWire; synth outputs re-linked by name");
                    }
                    o.app.connect_input();
                }
                if now_dac.is_none() && dac_id.is_some() {
                    o.app.event("warn", format!("DAC sink {dac} is gone from PipeWire"));
                }
                dac_id = now_dac;
                // Hardware added or removed (reported, never used automatically). A new device
                // is reported one loop later, once its outputs and inputs exist.
                let devs: BTreeMap<String, String> = g.devices.iter().map(|d| (d.name.clone(), d.label.clone())).collect();
                let snk: BTreeMap<String, String> = g.sinks.iter().map(|s| (s.name.clone(), s.label.clone())).collect();
                if !first {
                    for n in std::mem::take(&mut pending) {
                        let Some(l) = devs.get(&n) else { continue };
                        let key = n.trim_start_matches("alsa_card.").to_string();
                        let outs: Vec<String> = g.sinks.iter().filter(|s| s.name.contains(&key)).map(|s| format!("{} ({} ch)", s.label, s.channels.len())).collect();
                        let ins = g.sources.iter().filter(|s| s.0.contains(&key)).count();
                        let again = if ever.contains(&n) { "back" } else { "added" };
                        ever.insert(n.clone());
                        o.app.event("info", format!("audio device {again}: {l}: {} output(s){}, {ins} input(s). Not used until you pick it (Music output, Settings input)", outs.len(), if outs.is_empty() { String::new() } else { format!(" [{}]", outs.join(", ")) }));
                    }
                    for n in devs.keys() {
                        if !devices.contains_key(n) {
                            pending.push(n.clone());
                        }
                    }
                    for (n, l) in &devices {
                        if !devs.contains_key(n) {
                            o.app.event("info", format!("audio device removed: {l}"));
                        }
                    }
                    for (n, l) in &snk {
                        if !sinks.contains_key(n) && n.starts_with("raop_sink") {
                            o.app.event("info", format!("AirPlay speaker found: {l}"));
                        }
                    }
                } else {
                    ever.extend(devs.keys().cloned());
                }
                devices = devs;
                sinks = snk;
                // Every ~10 s: kernel sound cards PipeWire has not taken up.
                if tick % 5 == 0 {
                    let cards = alsa_cards(&std::fs::read_to_string("/proc/asound/cards").unwrap_or_default());
                    let claimed: BTreeSet<u32> = g.devices.iter().filter_map(|d| d.card).collect();
                    let mut un = vec![];
                    for (n, id, long) in cards {
                        // HDMI cards without a profile are normal; only report USB ones.
                        let usb = std::fs::read_to_string(format!("/proc/asound/card{n}/usbid")).is_ok();
                        if usb && !claimed.contains(&n) {
                            if !cards_seen.contains(&n) {
                                o.app.event("warn", format!("sound card {n} ({long}) is attached but PipeWire has not set it up; restart audio devices (Settings) or replug it"));
                                cards_seen.insert(n);
                            }
                            un.push(json!({ "card": n, "id": id, "name": long }));
                        } else {
                            cards_seen.remove(&n);
                        }
                    }
                    *o.unclaimed.lock().unwrap() = un;
                }
                first = false;
                tick += 1;
                std::thread::sleep(LOOP);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAC: &str = "alsa_output.platform-soc_sound.stereo-fallback";

    fn dump_fixture() -> Value {
        json!([
            {"id": 96, "type": "PipeWire:Interface:Node", "info": {"state": "running", "props": {"media.class": "Audio/Sink", "node.name": DAC, "node.description": "Built-in Audio Stereo", "device.api": "alsa", "alsa.card_name": "Allo Katana", "api.alsa.card": "3"}}},
            {"id": 200, "type": "PipeWire:Interface:Node", "info": {"state": "suspended", "props": {"media.class": "Audio/Sink", "node.name": "alsa_output.usb-Focusrite_Scarlett_2i2_USB-00.analog-stereo", "node.description": "Scarlett 2i2 USB Analog Stereo", "device.api": "alsa", "alsa.card_name": "Scarlett 2i2 USB", "api.alsa.card": 5}}},
            {"id": 300, "type": "PipeWire:Interface:Node", "info": {"state": "suspended", "props": {"media.class": "Audio/Sink", "node.name": "raop_sink.Kitchen.local", "node.description": "Kitchen"}}},
            {"id": 165, "type": "PipeWire:Interface:Node", "info": {"state": "running", "props": {"media.class": "Stream/Output/Audio", "node.name": "mpd.Trillium Base player"}}},
            {"id": 50, "type": "PipeWire:Interface:Port", "info": {"direction": "output", "props": {"node.id": 165, "audio.channel": "FL", "port.name": "output_FL"}}},
            {"id": 53, "type": "PipeWire:Interface:Port", "info": {"direction": "output", "props": {"node.id": 165, "audio.channel": "FR", "port.name": "output_FR"}}},
            {"id": 10, "type": "PipeWire:Interface:Port", "info": {"direction": "input", "props": {"node.id": 96, "audio.channel": "FR", "port.name": "playback_FR"}}},
            {"id": 11, "type": "PipeWire:Interface:Port", "info": {"direction": "input", "props": {"node.id": 96, "audio.channel": "FL", "port.name": "playback_FL"}}},
            {"id": 12, "type": "PipeWire:Interface:Port", "info": {"direction": "output", "props": {"node.id": 96, "audio.channel": "FL", "port.name": "monitor_FL", "port.monitor": true}}},
            {"id": 20, "type": "PipeWire:Interface:Port", "info": {"direction": "input", "props": {"node.id": 200, "audio.channel": "FL", "port.name": "playback_FL"}}},
            {"id": 21, "type": "PipeWire:Interface:Port", "info": {"direction": "input", "props": {"node.id": 200, "audio.channel": "FR", "port.name": "playback_FR"}}},
            {"id": 30, "type": "PipeWire:Interface:Port", "info": {"direction": "input", "props": {"node.id": 300, "audio.channel": "FL"}}},
            {"id": 31, "type": "PipeWire:Interface:Port", "info": {"direction": "input", "props": {"node.id": 300, "audio.channel": "FR"}}},
            {"id": 900, "type": "PipeWire:Interface:Link", "info": {"output-port-id": 50, "input-port-id": 11}},
            {"id": 901, "type": "PipeWire:Interface:Link", "info": {"output-port-id": 53, "input-port-id": 10}},
            {"id": 902, "type": "PipeWire:Interface:Link", "info": {"output-port-id": 7, "input-port-id": 11}},
            {"id": 903, "type": "PipeWire:Interface:Link", "info": {"output-port-id": 50, "input-port-id": 777}},
            {"id": 70, "type": "PipeWire:Interface:Device", "info": {"props": {"media.class": "Audio/Device", "device.name": "alsa_card.usb-Focusrite_Scarlett_2i2_USB-00", "device.description": "Scarlett 2i2 USB", "device.api": "alsa", "device.bus": "usb", "api.alsa.card": "5"}}}
        ])
    }

    #[test]
    fn parses_sinks_players_and_devices() {
        let g = parse(&dump_fixture(), DAC);
        assert_eq!(g.sinks.len(), 3);
        assert_eq!(g.sinks[0].kind, "dac");
        assert_eq!(g.sinks[0].label, "Allo Katana");
        assert_eq!(g.sinks[0].channels, vec!["FL", "FR"]); // sorted, monitor port dropped
        assert_eq!(g.sinks[0].card, Some(3));
        assert!(g.sinks.iter().any(|s| s.kind == "usb" && s.label == "Scarlett 2i2 USB"));
        assert!(g.sinks.iter().any(|s| s.kind == "airplay" && s.label == "Kitchen"));
        assert_eq!(g.players.len(), 1);
        assert_eq!(g.devices[0].card, Some(5));
        assert_eq!(active(&g), vec![DAC.to_string()]);
    }

    #[test]
    fn plan_routes_by_name_and_falls_back_to_the_dac() {
        let g = parse(&dump_fixture(), DAC);
        // Already on the DAC: nothing to do (the synth's link 902 is never touched).
        assert_eq!(plan(&g, &effective(&g, &[], DAC)), (vec![], vec![]));
        // Move to the Scarlett: two new links, the two DAC links removed.
        let t = effective(&g, &["alsa_output.usb-Focusrite_Scarlett_2i2_USB-00.analog-stereo".into()], DAC);
        let (add, rm) = plan(&g, &t);
        assert_eq!(add, vec![(50, 20), (53, 21)]);
        assert_eq!(rm, vec![900, 901]); // the meter tap (903) stays
        // DAC + AirPlay at once.
        let t = effective(&g, &[DAC.into(), "raop_sink.Kitchen.local".into()], DAC);
        let (add, rm) = plan(&g, &t);
        assert_eq!(add, vec![(50, 30), (53, 31)]);
        assert!(rm.is_empty());
        // A chosen output that is unplugged: back to the DAC.
        assert_eq!(effective(&g, &["alsa_output.usb-gone".into()], DAC), vec![DAC.to_string()]);
    }

    #[test]
    fn alsa_cards_and_usb_caps() {
        let cards = " 3 [Katana         ]: Allo_Katana - Allo Katana\n                      Allo Katana\n 5 [USB            ]: USB-Audio - Scarlett 2i2 USB\n                      Focusrite Scarlett 2i2 USB at usb-xhci-hcd.1-1, high speed\n";
        let c = alsa_cards(cards);
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].0, 5);
        assert_eq!(c[1].1, "USB");
        assert!(c[1].2.starts_with("Focusrite Scarlett 2i2 USB"));
        let st = "Focusrite Scarlett 2i2 USB at usb-xhci-hcd.1-1, high speed : USB Audio\n\nPlayback:\n  Status: Stop\n  Interface 1\n    Altset 1\n    Format: S32_LE\n    Channels: 2\n    Rates: 44100, 48000, 88200, 96000, 176400, 192000\n    Bits: 24\n\nCapture:\n  Status: Stop\n    Format: S32_LE\n    Channels: 2\n    Rates: 44100, 48000\n    Bits: 24\n";
        let v = usb_caps(st);
        assert_eq!(v["playback"]["channels"], 2);
        assert_eq!(v["playback"]["rates"][5], 192000);
        assert_eq!(v["capture"]["bits"], 24);
    }
}
