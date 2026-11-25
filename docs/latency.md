<!-- Copyright (C) 2026 M-of-N. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Gesture-to-sound latency

Measured on the lab Pi 5 (PREEMPT_RT, PipeWire 1.4.2 at 48 kHz, pilabd on isolated CPU 3, built-in / Katana DAC at 0.70, master −18 dB, Trillium Zero firmware 0.8.0) on 24 November 2026, America/Los_Angeles. Tap percussion stayed off (parameter 11 = 0). Deployed build: `880d841` (6:09 PM PT).

## Method

1. **MIDI-to-recorded-audio (in process).** Arm the console recorder, wait 400 ms, send a short note through `trillium-base note 72 80 0.25`, stop the take, and measure the time from the note command to the first sample above −60 dBFS and −40 dBFS in the 32-bit float WAV (post master and limiter). This path includes the CLI/HTTP hop into pilabd and the synth attack. It does **not** include the DAC output buffer. Keep the DAC at 0.70 and master at −18 dB. With master at −18 dB the recorded peak is typically around −31 dBFS, so −20 dBFS is not used as a threshold. Input-strip recording is turned off for these takes so the Yeti is not mixed in, then restored.
2. **Telemetry clock.** Take a 2 s, 20 Hz telemetry lease and look at `time_ms` deltas and how they compare to wall time.
3. **MIDI responsiveness (mic off vs on).** `tools/measure-midi-responsiveness.py` records ALSA sequencer port 0 arrival deltas (instrument MIDI), telemetry cadence, CPU on core 3, and PipeWire xruns. The Zero's MIDI and 16 kHz PCM share one full-speed USB device and the firmware's medium executor, so this is also a check for firmware-side delay when the PCM stream is running. The script does not write to the device unless `--mic` is passed. **Not run in this pass:** the Zero was unplugged.
4. **Strike-to-sound (needs a physical tap).** `tools/measure-strike-to-sound.py` arms a take and prints `GO — strike the Trillium Zero once, then keep still.` It then reports the first onset after GO. For a two-source comparison, also capture the Yeti (or a DAC loopback into the Yeti) in the same window and cross-correlate onsets. A mixed console take alone cannot split the acoustic tap from the synth output. **Not run in this pass.**
5. **Budget to the speakers.** Add the output buffer (period count × quantum) to (1). Add the device's own sensing and USB transport for a full gesture-to-sound figure.

## Numbers

### Baseline (0.4.0-beta.1, saved default scene, 10 ms Warm Saw attack, quantum 64)

`PIPEWIRE_LATENCY=64/48000`. The lab's saved `default` scene has Warm Saw `amp_env.a = 0.01` (10 ms). That is what "every input" plays. Two takes of the same path, same build, show CLI/HTTP and quantum alignment jitter of about one period (1.33 ms).

| Segment | Result |
|---|---|
| PipeWire / synth quantum | 64 frames at 48 kHz = **1.33 ms** |
| MIDI command → first recorded sample (earlier take) | **2.55 ms** to −60 dBFS |
| Same path, later takes | **2.62–3.96 ms** to −60 dBFS, **5.19–6.52 ms** to −40 dBFS, peak ≈ −31.36 dBFS |
| Typical DAC output buffer (2 × 64) | **2.67 ms** (not in the recording) |
| MIDI-to-speakers (estimated) | **about 5.2–6.6 ms** (2.55–3.96 + 2.67) |
| Telemetry at 20 Hz (earlier) | 40 frames in 2.07 s; `time_ms` delta mean **50.0 ms**, min 49, max 51 |
| Full gesture-to-sound (estimated) | Device sensing 2.4–7 ms + USB full-speed ≤ 1 ms + MIDI-to-speakers ≈ 5.2–6.6 ms ≈ **8–15 ms** at the speakers |

Treat **2.6–4.0 ms** as the in-process range to −60 dBFS at a 10 ms attack. The −60 to −40 gap is about **2.6 ms** (the attack is still climbing).

The firmware project's target is about 10 ms at the 95th percentile for gesture to sound.

### After host-side changes (`880d841`, 1 ms attack on a temporary scene, quantum 64)

The factory default amplitude-envelope attack in code is now 1 ms (saved scenes are unchanged). The lab's stored `default` scene was **not** rewritten. For the numbers below, Warm Saw's attack was set to 1 ms in a temporary scene `lat-attack-1ms`, three takes were recorded, then `default` was loaded again and the temporary scene was deleted. `default.toml` was unchanged.

| Segment | Result |
|---|---|
| MIDI command → first recorded sample, quantum 64, 1 ms attack (3 takes) | **1.40–2.73 ms** to −60 dBFS, **2.73–4.06 ms** to −40 dBFS, peak −31.31 dBFS |
| −60 to −40 gap | **1.33 ms** (was ~2.6 ms at 10 ms attack) |
| Typical DAC output buffer (2 × 64) | **2.67 ms** (unchanged) |
| MIDI-to-speakers (estimated, 1 ms attack) | **about 4.1–5.4 ms** (1.40–2.73 + 2.67) |
| Improvement vs 10 ms attack, same quantum | About **1.1–1.3 ms** earlier to −60 dBFS, about **2.5 ms** earlier to −40 dBFS |

The first sample above −60 dBFS was already near the start of the 10 ms attack, so most of the audible improvement is at −40 dBFS (closer to the recorded peak).

### Quantum 64 → 32 (tried, not kept)

A user-unit drop-in set `PIPEWIRE_LATENCY=32/48000` and pilabd was restarted (DAC still 0.70, master −18 dB). This was **not** a safe improvement on this machine:

* xruns went from 2 at restart to **16** over a short measurement, with `xrun_rate` 1.5–2.0 / s
* DSP load reported **~100 %**
* `pw-top` showed the Yeti capture at quantum 32 with errors, and the DAC sink with a large error count
* MIDI-to-sample takes were unreliable (one take never crossed −60 dBFS after the pre-roll; −40 dBFS was not observed)

The drop-in was removed and the service restarted at **64 / 48000**. After that, xruns were 2 with rate 0, DSP load about 5–6 %, DAC 0.70, master −18 dB, scene `default`. System `default.clock.min-quantum` remains 32, but pilabd stays at 64 until a tighter graph is stable (for example without pulling the Yeti into a 32-frame quantum).

### Microphone stream interference and strike-to-sound

Not measured in this pass: the Trillium Zero was unplugged (no USB `1209:0003`, no `/dev/trillium-zero-pcm`). After it is plugged in: snapshot the 41 settings, start the mic only (tap percussion stays off), confirm the other settings are unchanged, then run `tools/measure-midi-responsiveness.py` with the stream off and on, and optionally `tools/measure-strike-to-sound.py`.

## Host-side levers (applied or documented)

| Lever | Status | Notes |
|---|---|---|
| Kernel / CPU 3 isolation | Already in the image (earlier approval) | pilabd audio thread on isolated core 3 |
| Default amp attack 10 ms → 1 ms | Applied in code (`880d841`) | Factory default only; the lab's saved `default` scene still has Warm Saw at 10 ms until that slot is edited or a new factory scene is saved |
| `PIPEWIRE_LATENCY` 64→32 | Tried; **reverted** | xruns and 100 % DSP load at 32; stays 64 |
| DAC period count | Not changed | Typically two periods; lowering it is a PipeWire/ALSA sink change and needs a stability check |
| Input monitoring | Remains off by default | Confirm required to send any source, including Trillium Zero Mic, to the DAC |

## Firmware notes for Trillium (not flashed from this tree)

Do not reflash the Zero from the Base. These are proposals for the firmware repository:

* IMU already samples at 833 Hz; the MIDI writer wakes every 2 ms. A strike can wait up to one MIDI-writer period plus USB full-speed framing before the host sees a note.
* The medium executor also writes USB and, when the test mic is on, the PCM stream. If interference measurements show note jitter or telemetry cadence stretch with the mic on, consider giving the MIDI writer priority over PCM packetization, or sending a strike note before the current PCM block is flushed.
* USB is full-speed; polling cannot go below a 1 ms frame. A tighter MIDI path helps more than trying to change the bus.

## How to repeat the MIDI-to-sample measurement

```
trillium-base input record off    # restore afterwards
trillium-base record on
# wait 400 ms, then:
trillium-base note 72 80 0.25
# wait 800 ms, then:
trillium-base record off
```

Find the first frame in the new WAV whose absolute value is at least 10^(−60/20), subtract the 400 ms pre-roll. The console writes IEEE float WAVs (format 3); a reader that only understands PCM integer WAVs will reject them.

To measure a 1 ms attack without rewriting the saved `default` scene, copy it to a temporary scene, set Warm Saw `[slot.sound.amp_env] a = 0.001`, `trillium-base load` that name, measure, then `trillium-base load default` and delete the copy.

MIDI responsiveness (Zero plugged in, mic already in the desired state):

```
python3 tools/measure-midi-responsiveness.py --seconds 20 --label mic-off
```

Strike-to-sound, from the Pi (preferred):

```
trillium-base measure strike
```

One strike, one moment. The command prints its cue from the Zero's sensors (for a motion-only Zero: hold the Zero still for 2 seconds so the gyro calibrates, then give it one firm tap). It then waits up to 60 seconds and reports the first Zero note-on, the first synth sound and the acoustic onset in one result:

* `midi_to_synth_ms`: Zero note-on reaching the Base (stamped on the realtime thread) to the synth's first sound.
* `acoustic_to_synth_ms`: the strike heard by the Zero mic (or the input strip, `acoustic_source`) to the synth's first sound. Both references are kept: `acoustic_zero_mic_to_synth_ms` and `acoustic_input_to_synth_ms`. The Zero mic path is itself buffered, so this can read short or negative; treat the Yeti input as the reference when the Zero mic is off.
* `strike_to_midi_ms`, `strike_to_sound_ms`: acoustic onset to MIDI and to sound.
* `diagnosis`: `ok`, or why no note arrived (for example tap percussion off with no strip).

The result is printed, logged as a console event, and written to `~/.local/share/pilab/measurements/strike-YYYYMMDD-HHMMSS.json` (America/Los_Angeles on the lab unit). The synth onset is taken from a sticky peak that the status loop does not drain. The take also records MIDI note-ons and writes a synth-only WAV (the input strip is left out for the duration, then restored). `trillium-base measure strike --rehearse` runs the same measurement with an injected note instead of a person (a rehearsal on the lab unit measured MIDI to synth at 2.1 ms).

The first take on 25 November 2026 at 7:57 AM PT (before this Zero was set up for motion-only strikes) heard the strike on the Zero mic (`acoustic_ms` 7006, peak −31.7 dBFS, baseline −49.9) but recorded `synth_ms` null and `baseline_synth_db` −120. Cable 0 was still linked to `Trillium Synth:in_1`; Warm Saw (1 ms attack), master −18 dB and DAC 0.70 were unchanged; the 41 Zero settings still matched the saved profile (tap percussion = 0, strike directions = 0). The mixed recording (Yeti input strip + synth) showed room noise around −30 dBFS and a 0 dBFS clip at 6.85 s from the Yeti, not from the engine. A host-generated `trillium-base note 72 80 0.25` immediately produced a voice and a peak of about −38 dBFS. The synth was fine; the Zero sent no note-on for a body strike with tap percussion off.

The older helper still works:

```
python3 tools/measure-strike-to-sound.py --pre 1 --listen 2
```
