// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Compiles scene routes against the current MIDI input assignment.

use pilab_synth::{CompiledRoute, RouteTable, MAX_PORTS, PORT_AUDITION, PORT_CONSOLE, PORT_VIRTUAL};

use crate::glob;
use crate::scene::Route;

/// Source name per input port slot (`None` = unassigned).
pub type PortNames = [Option<String>; MAX_PORTS];

pub fn port_mask(input: &str, ports: &PortNames) -> u32 {
    let input = input.trim();
    match input {
        // The audition port only ever plays the slot picked in the editor.
        "" | "*" | "any" => ((1u32 << MAX_PORTS) - 1) & !(1 << PORT_AUDITION),
        "virtual" => 1 << PORT_VIRTUAL,
        "console" => 1 << PORT_CONSOLE,
        pat => {
            let mut m = 0u32;
            for (i, p) in ports.iter().enumerate() {
                if i == PORT_AUDITION as usize {
                    continue;
                }
                if let Some(name) = p {
                    if glob::matches(pat, name) {
                        m |= 1 << i;
                    }
                }
            }
            m
        }
    }
}

/// Slot ids are 1-based in scenes and 0-based in the engine.
pub fn compile_routes(routes: &[Route], ports: &PortNames) -> RouteTable {
    let mut t = RouteTable::new();
    for r in routes.iter().filter(|r| r.enabled && r.slot >= 1) {
        let mask = port_mask(&r.input, ports);
        if mask == 0 {
            continue;
        }
        t.push(CompiledRoute {
            port_mask: mask,
            channel: r.channel.min(16),
            note_lo: r.note_lo.min(127),
            note_hi: r.note_hi.min(127),
            transpose: r.transpose,
            vel_scale: r.velocity_scale.clamp(0.0, 2.0),
            slot: r.slot - 1,
        });
    }
    t
}
