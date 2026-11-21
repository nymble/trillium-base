// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Realtime MIDI routing table: (input port, channel, note range) → sound slot,
//! with transpose and velocity scaling. Several matching routes layer.

use crate::{MAX_LAYERS, MAX_ROUTES};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompiledRoute {
    /// Bit `p` set = input port slot `p` matches.
    pub port_mask: u32,
    /// 0 = omni, 1..=16 = that channel only.
    pub channel: u8,
    pub note_lo: u8,
    pub note_hi: u8,
    pub transpose: i8,
    pub vel_scale: f32,
    pub slot: u8,
}

impl Default for CompiledRoute {
    fn default() -> Self {
        CompiledRoute { port_mask: 0, channel: 0, note_lo: 0, note_hi: 127, transpose: 0, vel_scale: 1.0, slot: 0 }
    }
}

/// One resolved target of a note.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hit {
    pub slot: u8,
    pub note: u8,
    pub vel_scale: f32,
}

#[derive(Clone, Debug)]
pub struct RouteTable {
    routes: [CompiledRoute; MAX_ROUTES],
    n: usize,
}

impl Default for RouteTable {
    fn default() -> Self {
        RouteTable { routes: [CompiledRoute::default(); MAX_ROUTES], n: 0 }
    }
}

impl RouteTable {
    pub fn new() -> Self {
        Self::default()
    }
    /// Adds a route; returns false when the table is full.
    pub fn push(&mut self, r: CompiledRoute) -> bool {
        if self.n >= MAX_ROUTES {
            return false;
        }
        self.routes[self.n] = r;
        self.n += 1;
        true
    }
    pub fn len(&self) -> usize {
        self.n
    }
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
    pub fn routes(&self) -> &[CompiledRoute] {
        &self.routes[..self.n]
    }

    /// Resolves a note from `port` on wire channel `ch0` (0..=15). Writes up to
    /// [`MAX_LAYERS`] hits and returns how many. Allocation-free.
    pub fn resolve(&self, port: u8, ch0: u8, note: u8, out: &mut [Hit; MAX_LAYERS]) -> usize {
        let mut k = 0;
        for r in &self.routes[..self.n] {
            if k >= MAX_LAYERS {
                break;
            }
            if r.port_mask & (1u32 << (port as u32 & 31)) == 0 {
                continue;
            }
            if r.channel != 0 && r.channel != ch0 + 1 {
                continue;
            }
            if note < r.note_lo || note > r.note_hi {
                continue;
            }
            let n = note as i16 + r.transpose as i16;
            if !(0..=127).contains(&n) {
                continue;
            }
            out[k] = Hit { slot: r.slot, note: n as u8, vel_scale: r.vel_scale };
            k += 1;
        }
        k
    }
}
