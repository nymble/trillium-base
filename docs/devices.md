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
