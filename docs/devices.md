<!-- Copyright (C) 2026 M-of-N. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Devices and the audio input strip

The console's **Devices** view (`#devices`) shows what is plugged in, the audio
input strip, and a stored profile for each Trillium Zero.

## Audio input strip

pilabd registers two audio inputs on its JACK client, `Trillium Synth:input_l`
and `input_r`. Every 2 s the system loop links the ports that match
`[input] source` in `settings.toml` (a glob over JACK port names, for example
`Yeti Nano*:capture_*`). A mono source feeds both sides. Only links the strip
made itself are ever removed by it.

| Setting | Default | Meaning |
|---|---|---|
| `source` | empty | Port-name glob; empty means no input |
| `autoconnect` | true | Link the source whenever it appears |
| `gain_db` | 0 | −60…+24 dB, applied on the realtime thread |
| `record` | true | Mix the input into console recordings |
| `monitor` | **false** | Also play it through the DAC. Off by default to avoid microphone feedback; turning it on through the API needs `confirm: true` |

The input never reaches the DAC unless `monitor` is on. With monitoring off it
goes only to the recorder. There is no synth effect input yet; that arrives with
the effects work.

API: `GET /api/v1/input`, `POST /api/v1/input {source?, gain_db?, record?, monitor?, autoconnect?, confirm?}`.
CLI: `trillium-base input`, `trillium-base input source 'Yeti Nano*:capture_*'`,
`trillium-base input gain -6`, `trillium-base input record on|off`, `trillium-base input monitor off`.
Live input peaks are in the status WebSocket frame under `input.peak_db`.

## Trillium Zero profiles

The devices thread looks for Zeros every 5 s (USB `1209:0003` in
`/proc/asound`, ALSA sequencer port 1 for configuration SysEx). Each one is keyed
by its USB socket (`zero-<usb path>`), so the same socket keeps its profile.

* **First sight:** HELLO, GET_STATUS and every configuration page are read. If no
  profile exists, the settings are stored in
  `~/.config/pilab/devices/<key>.toml`. Nothing is written to the device.
* **Reconnect** (new sequencer client, or a new boot id seen by a 30 s HELLO): the
  device is read again and compared with its profile. Differences are reported as
  an event and shown in the Devices view. **Nothing is sent automatically.**
* **Store current settings as profile** re-reads the device and overwrites the
  stored profile (read-only towards the device).
* **Re-apply** sends SET_CONFIG for only the differing settings, then reads the
  device back. It needs an explicit click and a confirm in the console, or
  `POST /api/v1/devices/{key}/reapply {"confirm": true}`. Zero settings are kept in
  its RAM, so they last until it restarts.
* **Sample sensors** takes a 1 s, 20 Hz telemetry lease and shows the last frame.

API: `GET /api/v1/devices`, `POST /api/v1/devices/{key}/snapshot`,
`POST /api/v1/devices/{key}/reapply`, `GET /api/v1/devices/{key}/telemetry?hz=&ms=`.
CLI: `trillium-base devices`.

The SysEx codec is `pilab_core::zero` (fresh code; protocol notes in
[trillium-zero.md](trillium-zero.md)). The diagnostics serial port is never
opened by pilabd.


## Profiles and USB ports

Profiles are files named `zero-<usb path>.toml`. If the same Zero is plugged into a
new socket and that socket has no profile yet, the Base reuses the only stored
profile with the same USB id whose device is not currently connected. Two Zeros
at once are not guessed between.

A daemon restart no longer counts as a reconnect. A replug (new ALSA client) or a
device reset (new boot id) is logged as such. Unplugged devices stay in the Devices
list with their last settings.

## Sensors panel

Each Zero in `GET /api/v1/devices` carries `sensors` (id, label, state, detail) and
`note_path` (`summary`, `strike_ready`, `hint`), derived from live telemetry and
status. States: `running`, `on`, `present`; `not_fitted`, `off`, `absent`,
`unavailable`, `checking`; `lost`, `disabled`. A Trill strip stuck in Retry is
reported as `not_fitted` (a motion-only Zero), not as a fault, and is logged once.

In the console, Devices shows the same data on the Zero card as a **Sensors**
table, with state written in words (not colour alone) in a polite live region, plus
a "Notes come from" line saying whether a strike makes a note. From the Pi,
`trillium-base devices show [KEY]` prints it.

`trillium-base measure strike [--rehearse]` (`POST /api/v1/measure/strike`,
`{timeout_s, rehearse}`) waits up to 60 s for one strike after its cue and reports
the first Zero note-on, the synth onset, and the acoustic onset, with
`midi_to_synth_ms` and `acoustic_to_synth_ms`. The cue
(`GET /api/v1/measure/strike/cue`) follows the sensors: it only mentions a touch
strip when one is running. `--rehearse` injects one note on the Zero's input to
check the whole measurement without a person.

## Live sensors and the patch panel

While a Zero is connected, the Base keeps a 20 Hz telemetry lease (read-only,
5 s, refreshed). The Devices view plots pitch and roll live. The same values
are offered on the Signals patch panel as the MIDI output
`Trillium Synth:zero_motion` (CC 16 = pitch, 17 = roll, 18 = touch), not
connected to the synth automatically.

The Zero's own microphone is a testing-only 16 kHz 16-bit mono PDM capture,
off at firmware boot. Action 7 starts it and action 8 (or unplug) stops it and
drives the microphone power pin low. The stream is framed `TRPC` on
`/dev/trillium-zero-pcm` (not USB Audio Class). The console toggle on the Zero
card, `POST /api/v1/devices/{key}/mic {on, confirm}`, or
`trillium-base devices mic KEY on|off` sends only that action, then checks that
the 41 settings (including tap percussion = off) are unchanged. The on/off flag
is stored in the device's profile (`mic = true`) and is sent again on reconnect.

While the stream is on, pilabd publishes a PipeWire source named
**Trillium Zero Mic** (`capture_FL` / `capture_FR`), resampling 16 kHz to 48 kHz
(exact 3× linear interpolation). It is selectable as the input strip source
(`Trillium Zero Mic*:capture_*`) and appears on the Signals patch panel. It is
not connected to the DAC unless you turn input monitoring on (confirm required).
The capture thread stays off CPU 3.

A strike-to-sound take is `trillium-base measure strike` (or `POST /api/v1/measure/strike`). It prints GO, waits up to 60 s, and writes `~/.local/share/pilab/measurements/strike-*.json` plus a console event.

The Yeti Nano and the Nordic `1915:1025` capture remain available as input
strip sources.

## Battery

GET_STATUS reports the pack as `absent` when its on/off switch is off, or when
the pack is unplugged, even if a voltage is still visible. `present` means the
switch is on and the pack is attached.
