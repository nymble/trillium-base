// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Service side of the MIDI router: the saved configuration, compiling it against the ports
//! present now, wiring the router's JACK outputs and engine audio, and the MIDI monitor
//! history (with CSV export).

use crate::audio::{AudioBackend, ENGINE_PORTS, ROUTER_OUT_PREFIX};
use crate::engines;
use pilab_core::midi_router::{compile, Compiled, Ports, RouterConfig, Table, MAX_OUTS};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

pub const HISTORY: usize = 5000;

#[derive(Clone, Debug, Serialize)]
pub struct MonEv {
    pub seq: u64,
    /// Seconds since the service started.
    pub t: f64,
    pub source: String,
    pub kind: &'static str,
    pub ch: u8,
    pub text: String,
    pub bytes: Vec<u8>,
    /// Destinations reached ("synth", an engine or a port name); empty = filtered out.
    pub to: Vec<String>,
}

pub struct Router {
    pub path: PathBuf,
    pub cfg: Mutex<RouterConfig>,
    pub compiled: Mutex<Compiled>,
    tx: Mutex<rtrb::Producer<Box<Table>>>,
    gc: Mutex<rtrb::Consumer<Box<Table>>>,
    direct: Mutex<rtrb::Producer<[u8; 5]>>,
    sig: Mutex<String>,
    pub history: Mutex<VecDeque<MonEv>>,
    seq: Mutex<u64>,
}

impl Router {
    pub fn new(path: PathBuf, tx: rtrb::Producer<Box<Table>>, gc: rtrb::Consumer<Box<Table>>, direct: rtrb::Producer<[u8; 5]>) -> Router {
        let cfg = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<RouterConfig>(&t).ok())
            .filter(|c| c.validate().is_ok())
            .unwrap_or_default();
        Router { path, cfg: Mutex::new(cfg), compiled: Mutex::new(Compiled::default()), tx: Mutex::new(tx), gc: Mutex::new(gc), direct: Mutex::new(direct), sig: Mutex::new(String::new()), history: Mutex::new(VecDeque::new()), seq: Mutex::new(0) }
    }

    pub fn config(&self) -> RouterConfig {
        self.cfg.lock().unwrap().clone()
    }

    /// Validates, saves (atomically) and marks for recompilation.
    pub fn set_config(&self, c: RouterConfig) -> Result<(), String> {
        c.validate()?;
        let text = serde_json::to_string_pretty(&c).map_err(|e| e.to_string())?;
        crate::paths::write_atomic(&self.path, text.as_bytes()).map_err(|e| e.to_string())?;
        *self.cfg.lock().unwrap() = c;
        self.sig.lock().unwrap().clear();
        Ok(())
    }

    /// Engine id -> its MIDI input port, for engines whose ports are in the graph.
    pub fn engine_ports(sinks: &[String]) -> Vec<(String, String)> {
        engines::DEFS
            .iter()
            .filter_map(|d| sinks.iter().find(|s| s.starts_with(d.client) && s.ends_with(&format!(":{}", d.midi_in))).map(|p| (d.id.to_string(), p.clone())))
            .collect()
    }

    /// Recompiles when ports, engines, scene engine or the config changed; then wires outputs.
    pub fn refresh(&self, backend: &dyn AudioBackend, sources: &[Option<String>], zero_sources: &[u8], scene_engine: &str, force: bool) -> bool {
        while let Ok(t) = self.gc.lock().unwrap().pop() {
            drop(t);
        }
        let sinks: Vec<String> = backend.midi_sinks();
        let eng = Self::engine_ports(&sinks);
        let ext_sinks: Vec<String> = sinks.iter().filter(|s| !engines::is_engine_port(s) && !s.contains("Midi Through")).cloned().collect();
        let cfg = self.config();
        let sig = format!("{sources:?}|{zero_sources:?}|{ext_sinks:?}|{eng:?}|{scene_engine}|{}", serde_json::to_string(&cfg).unwrap_or_default());
        {
            let mut s = self.sig.lock().unwrap();
            if !force && *s == sig {
                self.wire_engine_audio(backend);
                return false;
            }
            *s = sig;
        }
        let c = compile(&cfg, &Ports { sources, zero_sources, sinks: &ext_sinks, engines: &eng, scene_engine });
        let _ = self.tx.lock().unwrap().push(Box::new(c.table));
        // Wire router outputs: out k -> c.outs[k]; drop anything else.
        let graph = backend.graph();
        let ours = |k: usize| format!("{}:{ROUTER_OUT_PREFIX}{}", crate::audio::CLIENT_NAME, k + 1);
        for k in 0..MAX_OUTS {
            let me = ours(k);
            let want = c.outs.get(k);
            let have: Vec<String> = graph.iter().find(|p| p.name == me).map(|p| p.connections.clone()).unwrap_or_default();
            for h in &have {
                if Some(h) != want {
                    let _ = backend.disconnect(&me, h);
                }
            }
            if let Some(w) = want {
                if !have.contains(w) {
                    let _ = backend.connect(&me, w);
                }
            }
        }
        *self.compiled.lock().unwrap() = c;
        self.wire_engine_audio(backend);
        true
    }

    /// Engine audio goes only into Trillium Synth:engine_l/r (master and limiter apply).
    fn wire_engine_audio(&self, backend: &dyn AudioBackend) {
        let graph = backend.graph();
        let dst = ENGINE_PORTS.map(|n| format!("{}:{n}", crate::audio::CLIENT_NAME));
        for d in engines::DEFS.iter() {
            for (i, short) in d.audio.iter().enumerate() {
                let Some(p) = graph.iter().find(|p| p.dir == "out" && p.kind == "audio" && p.client.starts_with(d.client) && p.short == *short) else { continue };
                for c in &p.connections {
                    if *c != dst[i] {
                        let _ = backend.disconnect(&p.name, c);
                    }
                }
                if !p.connections.contains(&dst[i]) {
                    let _ = backend.connect(&p.name, &dst[i]);
                }
            }
        }
    }

    pub fn dest_names(&self, mask: u32) -> Vec<String> {
        if mask == 0 {
            return vec![];
        }
        let c = self.compiled.lock().unwrap();
        let mut v = vec![];
        for d in 0..=MAX_OUTS as u32 {
            if mask & (1 << d) != 0 {
                v.push(if d == 0 {
                    "synth".to_string()
                } else {
                    let p = c.outs.get(d as usize - 1).cloned().unwrap_or_default();
                    Self::engine_ports(std::slice::from_ref(&p)).first().map(|e| e.0.clone()).unwrap_or_else(|| crate::state::short_source(&p).replace(" (playback)", ""))
                });
            }
        }
        v
    }

    /// Sends one channel message straight to a running engine (program changes).
    pub fn send_to_engine(&self, engine: &str, bytes: &[u8]) -> Result<(), String> {
        let c = self.compiled.lock().unwrap();
        let k = c
            .outs
            .iter()
            .position(|o| Self::engine_ports(std::slice::from_ref(o)).first().map(|e| e.0 == engine).unwrap_or(false))
            .ok_or_else(|| format!("{engine} is not running"))?;
        if bytes.is_empty() || bytes.len() > 3 || bytes[0] < 0x80 || bytes[0] >= 0xF0 {
            return Err("one channel message expected".into());
        }
        let mut m = [(k + 1) as u8, bytes.len() as u8, 0, 0, 0];
        m[2..2 + bytes.len()].copy_from_slice(bytes);
        self.direct.lock().unwrap().push(m).map_err(|_| "router queue full".to_string())
    }

    pub fn record(&self, mut e: MonEv) {
        let mut s = self.seq.lock().unwrap();
        *s += 1;
        e.seq = *s;
        drop(s);
        let mut h = self.history.lock().unwrap();
        if h.len() >= HISTORY {
            h.pop_front();
        }
        h.push_back(e);
    }

    pub fn since(&self, seq: u64, limit: usize) -> Vec<MonEv> {
        let h = self.history.lock().unwrap();
        let v: Vec<MonEv> = h.iter().filter(|e| e.seq > seq).cloned().collect();
        let n = v.len();
        v.into_iter().skip(n.saturating_sub(limit)).collect()
    }

    pub fn csv(&self) -> String {
        let h = self.history.lock().unwrap();
        let mut s = String::from("seq,time_s,source,kind,channel,data,bytes_hex,destinations\n");
        let q = |x: &str| format!("\"{}\"", x.replace('"', "\"\""));
        for e in h.iter() {
            let hex: Vec<String> = e.bytes.iter().map(|b| format!("{b:02X}")).collect();
            s.push_str(&format!("{},{:.4},{},{},{},{},{},{}\n", e.seq, e.t, q(&e.source), e.kind, e.ch, q(&e.text), hex.join(" "), q(&e.to.join("; "))));
        }
        s
    }

    pub fn clear(&self) {
        self.history.lock().unwrap().clear();
    }
}
