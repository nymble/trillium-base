// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Strike-to-sound measurement helpers (onset thresholds and the GO cue).

/// Printed and logged when a strike-to-sound take is armed.
pub const GO: &str = "GO — touch the Trillium Zero play surface once (a body strike does not send a note while tap percussion is off), then keep still.";

/// Onset threshold: `rise_db` above the baseline, but never quieter than `floor_db`.
pub fn threshold(baseline: f32, rise_db: f32, floor_db: f32) -> f32 {
    let rise = if baseline > 0.0 { baseline * 10f32.powf(rise_db / 20.0) } else { 0.0 };
    let floor = 10f32.powf(floor_db / 20.0);
    rise.max(floor)
}

#[cfg(test)]
mod tests {
    use super::threshold;

    #[test]
    fn silent_baseline_uses_the_floor() {
        let t = threshold(0.0, 12.0, -40.0);
        assert!((t - 10f32.powf(-40.0 / 20.0)).abs() < 1e-8);
    }

    #[test]
    fn loud_baseline_needs_a_12db_rise() {
        let base = 10f32.powf(-40.0 / 20.0);
        let t = threshold(base, 12.0, -60.0);
        assert!((t - 10f32.powf(-28.0 / 20.0)).abs() < 1e-6);
    }
}
