// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! 12-TET tuning and level helpers.

/// Frequency of a (fractional) MIDI note in 12-TET with a configurable A4.
#[inline]
pub fn note_to_hz(note: f32, a4_hz: f32) -> f32 {
    a4_hz * ((note - 69.0) / 12.0).exp2()
}

#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    if db <= -120.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    if g <= 1e-6 {
        -120.0
    } else {
        20.0 * g.log10()
    }
}
