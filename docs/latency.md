<!-- Copyright (C) 2026 M-of-N. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Gesture-to-sound latency

Measured on the lab Pi 5 (PREEMPT_RT, PipeWire 48 kHz, quantum 64, pilabd on isolated CPU 3, Katana DAC, Trillium Zero firmware 0.8.0) on 24 November 2026, America/Los_Angeles.

## Method

1. **MIDI-to-recorded-audio (in process).** Arm the console recorder, wait 400 ms, send a short note through `trillium-base note`, stop the take, and measure the time from the note command to the first sample above −60 dBFS in the 32-bit WAV (post master and limiter). This path does **not** include the DAC output buffer.
2. **Telemetry clock.** Take a 2 s, 20 Hz telemetry lease and look at `time_ms` deltas and how they compare to wall time.
3. **Budget to the speakers.** Add the output buffer (typically two periods of 64 frames) to (1). Add the device's own sensing and USB transport for a full gesture-to-sound figure. The device's instrument MIDI (tilt as CC 74, and so on) already runs on the Zero; the Base only aligns that MIDI to a quantum and renders.

A physical electrical loop-back at the DAC and a controlled mechanical tap on the Zero were not run (no extra wiring, and tap percussion stayed off).

## Numbers

| Segment | Result |
|---|---|
| PipeWire / synth quantum | 64 frames at 48 kHz = **1.33 ms**. `pw-top` showed Trillium Synth at quantum 64, 48 kHz, ~4–6 µs busy, 0 xruns in that snapshot |
| MIDI command → first recorded sample | **2.55 ms** (about two quanta, including the CLI/HTTP hop into pilabd) |
| Typical DAC output buffer (2 × 64) | **2.67 ms** (not in the recording) |
| MIDI-to-speakers (estimated) | **about 5.2 ms** (2.55 + 2.67) |
| Telemetry at 20 Hz | 40 frames in 2.07 s; `time_ms` delta mean **50.0 ms**, min 49, max 51 |
| Full gesture-to-sound (estimated) | Device sensing 2.4–7 ms + USB full-speed ≤ 1 ms + MIDI-to-speakers ≈ 5.2 ms ≈ **8–13 ms** at the speakers |

The firmware project's target is about 10 ms at the 95th percentile for gesture to sound. The Base's share of that budget is the 2.55 ms in-process figure plus the DAC buffer. The device side was not re-measured here.

## How to repeat

```
trillium-base record on
# wait 400 ms, then:
trillium-base note 72 80 0.25
# wait 800 ms, then:
trillium-base record off
```

Find the first frame in the new WAV whose absolute value is at least 10^(−60/20), subtract the 400 ms pre-roll. Keep the DAC at 0.70 and master at −18 dB. Leave tap percussion off (parameter 11 = 0).
