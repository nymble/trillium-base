<!-- Copyright (C) 2026 M-of-N. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Gesture-to-sound latency

Measured on the lab Pi 5 (PREEMPT_RT, PipeWire 1.4.2 at 48 kHz, pilabd on isolated CPU 3, Katana DAC at 0.70, master −18 dB, Trillium Zero firmware 0.8.0) on 24 November 2026, America/Los_Angeles. Tap percussion stayed off (parameter 11 = 0).

## Method

1. **MIDI-to-recorded-audio (in process).** Arm the console recorder, wait 400 ms, send a short note through `trillium-base note`, stop the take, and measure the time from the note command to the first sample above −60 dBFS and −40 dBFS in the 32-bit WAV (post master and limiter). This path includes the CLI/HTTP hop into pilabd and the synth attack. It does **not** include the DAC output buffer. Keep the DAC at 0.70 and master at −18 dB. With master at −18 dB the recorded peak is typically around −31 dBFS, so −20 dBFS is not used as a threshold.
2. **Telemetry clock.** Take a 2 s, 20 Hz telemetry lease and look at `time_ms` deltas and how they compare to wall time.
3. **MIDI responsiveness (mic off vs on).** `tools/measure-midi-responsiveness.py` records ALSA sequencer port 0 arrival deltas (instrument MIDI), telemetry cadence, CPU on core 3, and PipeWire xruns. The Zero's MIDI and 16 kHz PCM share one full-speed USB device and the firmware's medium executor, so this is also a check for firmware-side delay (note timing and telemetry cadence) when the PCM stream is running. The script does not write to the device unless `--mic` is passed.
4. **Strike-to-sound (needs a physical tap).** `tools/measure-strike-to-sound.py` arms a take and prints `GO — strike the Trillium Zero once, then keep still.` It then reports the first onset after GO. For a two-source comparison, also capture the Yeti (or a DAC loopback into the Yeti) in the same window and cross-correlate onsets. A mixed console take alone cannot split the acoustic tap from the synth output.
5. **Budget to the speakers.** Add the output buffer (period count × quantum) to (1). Add the device's own sensing and USB transport for a full gesture-to-sound figure.

## Numbers

### Baseline (0.4.0-beta.1, 10 ms default attack, quantum 64)

`PIPEWIRE_LATENCY=64/48000`. Default amplitude-envelope attack was 10 ms.

| Segment | Result |
|---|---|
| PipeWire / synth quantum | 64 frames at 48 kHz = **1.33 ms** |
| MIDI command → first recorded sample (earlier take) | **2.55 ms** to −60 dBFS |
| Same path, later take (same build) | **3.96 ms** to −60 dBFS, **6.52 ms** to −40 dBFS, peak −31.36 dBFS |
| Typical DAC output buffer (2 × 64) | **2.67 ms** (not in the recording) |
| MIDI-to-speakers (estimated) | **about 5.2–6.6 ms** (2.55–3.96 + 2.67) |
| Telemetry at 20 Hz | 40 frames in 2.07 s; `time_ms` delta mean **50.0 ms**, min 49, max 51 |
| Full gesture-to-sound (estimated) | Device sensing 2.4–7 ms + USB full-speed ≤ 1 ms + MIDI-to-speakers ≈ 5.2–6.6 ms ≈ **8–15 ms** at the speakers |

The two MIDI-to-sample figures differ because the CLI/HTTP hop and the note's alignment to the quantum are not identical from run to run. Treat **2.6–4.0 ms** as the in-process range at a 10 ms attack.

The firmware project's target is about 10 ms at the 95th percentile for gesture to sound.

### After host-side changes (1 ms default attack; quantum still 64 unless noted)

Default factory amplitude-envelope attack is 1 ms (saved scenes are unchanged). Numbers below are filled after this build is deployed to the lab Pi.

| Segment | Result |
|---|---|
| MIDI command → first recorded sample, quantum 64, 1 ms attack | *to be measured after deploy* |
| Same path, quantum 32 (`PIPEWIRE_LATENCY=32/48000`) | *to be measured if xruns stay low* |
| MIDI / telemetry / CPU 3 / xruns with Zero mic off vs on | *needs the Zero plugged in* |
| Strike-to-sound (acoustic onset vs synth onset) | *needs a physical tap* |

Expected host-side effect of the 1 ms attack: the waveform reaches a comparable envelope level about 9 ms sooner than the 10 ms attack, but the first sample above −60 dBFS may only move by a millisecond or so if it was already near the start of the attack. Quantum 64→32 shortens the synth period by **0.67 ms** and a two-period DAC buffer by **1.33 ms**, at the cost of less scheduling slack (watch xruns).

## Host-side levers (applied or documented)

| Lever | Status | Notes |
|---|---|---|
| Kernel / CPU 3 isolation | Already in the image (earlier approval) | pilabd audio thread on isolated core 3 |
| Default amp attack 10 ms → 1 ms | Applied in this build | Factory default only; existing saved scenes keep their stored envelope |
| `PIPEWIRE_LATENCY` 64→32 | Candidate; measure xruns before keeping | Set in `home/.config/systemd/user/pilabd.service`. System `default.clock.min-quantum` is already 32 |
| DAC period count | Not changed here | Typically two periods; lowering it is a PipeWire/ALSA sink change and needs a stability check |
| Input monitoring | Remains off by default | Confirm required to send any source, including Trillium Zero Mic, to the DAC |

## Firmware notes for Trillium (not flashed from this tree)

Do not reflash the Zero from the Base. These are proposals for the firmware repository:

* IMU already samples at 833 Hz; the MIDI writer wakes every 2 ms. A strike can wait up to one MIDI-writer period plus USB full-speed framing before the host sees a note.
* The medium executor also writes USB and, when the test mic is on, the PCM stream. If interference measurements show note jitter or telemetry cadence stretch with the mic on, consider giving the MIDI writer priority over PCM packetization, or sending a strike note before the current PCM block is flushed.
* USB is full-speed; polling cannot go below a 1 ms frame. A tighter MIDI path helps more than trying to change the bus.

## How to repeat the MIDI-to-sample measurement

```
trillium-base record on
# wait 400 ms, then:
trillium-base note 72 80 0.25
# wait 800 ms, then:
trillium-base record off
```

Find the first frame in the new WAV whose absolute value is at least 10^(−60/20), subtract the 400 ms pre-roll.

MIDI responsiveness (Zero plugged in, mic already in the desired state):

```
python3 tools/measure-midi-responsiveness.py --seconds 20 --label mic-off
```

Strike-to-sound (one-line cue): when the script prints GO, strike the Trillium Zero once, then keep still.

```
python3 tools/measure-strike-to-sound.py --pre 1 --listen 2
```
