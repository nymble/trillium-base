// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Waveform module v0: a polyphonic, MIDI-driven synth engine.
//!
//! The engine is backend-free: a host feeds it MIDI bytes and asks it to fill two `f32`
//! buffers. Nothing in [`Engine::process`], [`Engine::midi`] or [`Engine::apply`] allocates
//! or frees memory; tables and route maps arrive as owned pointers and leave as
//! [`Garbage`] for the host to drop on a non-realtime thread.
//!
//! DSP primitives (polyBLEP, TPT state-variable filter, ADSR, soft saturation) come from
//! the ZPOD crate `zpod-voices`.

pub mod engine;
pub mod osc;
pub mod params;
pub mod routing;
pub mod sampler;
pub mod tuning;
pub mod wavetable;

pub use engine::{Cmd, Engine, Garbage, Stats};
pub use osc::{OscMode, Waveform};
pub use params::{Env, FilterParams, SlotParams, Warp};
pub use routing::{CompiledRoute, Hit, RouteTable};
pub use sampler::Instrument;
pub use wavetable::{Bands, TableSet, Wavetable};

/// Maximum simultaneous voices.
pub const MAX_VOICES: usize = 32;
/// Maximum sound slots (instrument presets) in a scene.
pub const MAX_SLOTS: usize = 16;
/// Engine slot arrays hold one extra hidden slot for library previews.
pub const ENGINE_SLOTS: usize = MAX_SLOTS + 1;
/// 0-based index of the hidden preview slot.
pub const PREVIEW_SLOT: u8 = MAX_SLOTS as u8;
/// Sample zones one note may start (velocity crossfades, stereo pairs).
pub const MAX_ZONE_LAYERS: usize = 4;
/// MIDI input port slots: 0 = virtual, 1..=7 = auto-assigned sources, 8 = console, 9 = audition.
pub const MAX_PORTS: usize = 10;
pub const PORT_VIRTUAL: u8 = 0;
pub const PORT_CONSOLE: u8 = 8;
/// Console audition: plays the sound selected in the editor, bypassing the routing map.
pub const PORT_AUDITION: u8 = 9;
/// Maximum compiled routes.
pub const MAX_ROUTES: usize = 64;
/// Maximum slots one note can trigger (layers).
pub const MAX_LAYERS: usize = 8;
/// Internal render block; longer host buffers are processed in chunks.
pub const BLOCK: usize = 256;
