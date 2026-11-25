// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Strike-to-sound measurement helpers (onset thresholds and the GO cue).

/// CLOCK_MONOTONIC in nanoseconds (cheap vDSO call; safe on the realtime thread).
pub fn now_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// What to tell the person at the device, from the sensors that can actually make a note.
/// `plan` is None when no Zero is connected.
pub fn cue(plan: Option<&crate::devices::StrikePlan>, rehearse: bool) -> String {
    if rehearse {
        return "REHEARSAL — no strike needed: the Base injects one note on the Zero's input and times it to the synth.".into();
    }
    match plan {
        None => "GO — no Trillium Zero is connected; make a loud sound near the input microphone once, then keep still.".into(),
        Some(p) if p.trill_fitted && p.path.strike_ready => "GO — touch the Trillium Zero strip once, then keep still.".into(),
        Some(p) if p.imu_running && p.path.strike_ready => "GO — hold the Trillium Zero still for 2 s, then give it one firm tap or strike (its motion sensor plays the note; there is no touch strip), then keep still.".into(),
        Some(p) => format!("GO — but no Zero strike will play a note: {}", p.path.hint),
    }
}

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
