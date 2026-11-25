<!-- Copyright (C) 2026 M-of-N -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Trillium Zero on Trillium Base

What a Trillium Zero (T0) looks like to the Pi, measured on a Pi 5 (kernel
6.18.60-rt, PipeWire 1.4.2) with a T0 on firmware 0.8.0, USB-only build.
The firmware, its SysEx configuration protocol and the hardware live in
[nymble/trillium](https://github.com/nymble/trillium).

## USB enumeration (verified)

| Item | Value |
|---|---|
| VID:PID | `1209:0003` (pid.codes test PID, development only) |
| Product / manufacturer | `Trill XIAO MIDI+PCM (dev)` / `nymble/trill`, `bcdDevice` 2.00, full speed, 100 mA |
| Interface 0–1 | CDC ACM diagnostics (`cdc_acm`, `/dev/trillium-zero-diag`) |
| Interface 2–3 | USB Audio Control + MIDI Streaming, class compliant (`snd-usb-audio`, `snd_usbmidi_lib`), ALSA card `MIDIPCMdev` |
| Interface 4–5 | CDC ACM microphone test stream, 16 kHz 16-bit mono, off by default (`/dev/trillium-zero-pcm`) |
| Not present | USB Audio streaming (no PCM sound card), HID, mass storage, MIDI 2.0 |

ALSA shows two rawmidi ports (`hw:N,0,0` instrument, `hw:N,0,1` configuration
SysEx); the sequencer client has two ports with the *same* name. PipeWire's MIDI
bridge exposes them as two `32 bit raw UMP` ports, both named
`Trill XIAO MIDI-PCM (dev) MIDI  (capture)` ("+" written as "-"); through the
JACK API the second one is `...(capture)-<object.serial>`.

## Console routing

Before this branch the console saw both ports but routed neither
("cable number unknown, not routed"): neither the name nor the ALSA lookup
could tell the cables apart. `identity.rs` now takes the ALSA client and cable
from the bridge port's `object.path` (`alsa:seq:default:client_36:capture_1`)
via `pw-dump`, with a `+`/`-` tolerant name fallback. With the default device
class, cable 0 (instrument) is routed and cable 1 (configuration) is not.

Check it on a device (read-only):

```sh
pw-jack pilabd midi-sources
```

## Serial ports: a hazard

The diagnostics port treats every received byte as a key, and several keys
change the RAM mapping (`k`, `q`, `t`, `x`) or the LED. A tty opened with the
kernel default `ECHO` sends the device's own status text back to it as keys.
`etc/udev/rules.d/70-trillium-zero.rules` (installed by `install-console.sh`)
sets both ports raw with no echo when they appear, tells ModemManager to leave
them alone, and adds the stable names above.

## Sensors and the note path

A Zero is not always built the same way. The motion-only builds have no Trill Flex
touch strip. The Base works out what is fitted from telemetry and status, so it
needs no setting. The inventory is in `GET /api/v1/devices` (`zeros[].sensors`,
`zeros[].note_path`), in `trillium-base devices show [KEY]`, and in the console
under Devices, on the Zero card, as the **Sensors** table (updated live).

| Sensor | States shown | How it is decided |
| --- | --- | --- |
| Motion (IMU) | running, starting, disabled, not fitted, lost | telemetry `imu_state` (running once the gyro is calibrated, about 1 s still) |
| Touch strip (Trill) | running, checking, not fitted, disabled, lost | `trill_state`. The firmware retries a missing strip every 1 to 2 s and reports Retry; after 20 frames (1 s) of Retry that never reached Running the strip is **not fitted**. It is not a fault. It reads **lost** only if it ran earlier in this session and stopped. |
| Microphone | on, off, unavailable | the 16 kHz test stream (needs the PCM firmware variant) |
| Battery | present, absent, checking | GET_STATUS |

The Base logs one event when the inventory first settles or changes, for example
`sensors: Motion (IMU): running, Touch strip (Trill): not fitted`. It does not
repeat the Trill retry.

Without a touch strip the only things that make a note are the motion paths in the
firmware:

* **Tap percussion** (setting 11 = 1): an IMU tap above the tap threshold (setting
  25, default 80 centi-g, spacing setting 26, 120 ms) plays the percussion note
  (setting 13, default 39) on channel setting 12 + 1 (default channel 10). The
  gyro must have calibrated (about 1 s still) first.
* **Strike directions** (setting 31 > 0): taps along the chosen axes play the
  direction notes (settings 32 to 37) instead.
* **Tilt-strike** (mode, setting 14 = 3): a tap plays the selected note, pitch from tilt.
* Mode 1 and 2 play notes from tilt alone, with no strike.
* Mode 0 (touch) plays nothing without a strip.

With tap percussion off and no strip, a strike plays nothing. `trillium-base devices show`
and the Sensors panel say so, and `trillium-base measure strike` explains it
instead of asking for a touch.

The lab Zero is saved as motion-only strikes: mode 0, tap percussion on, strike
directions off, and the microphone on. Turning tap percussion on while the mic is
already running makes the Base stop the mic and restore settings, so enable tap
percussion first and start the mic after; on the lab unit both then run together.
If the mic ever has to stay off, keep the Yeti input as the acoustic reference.

## Configuration from the Base (protocol, not yet in the console)

SysEx v1 on cable 1: `F0 7D 54 52 01 <type> <req> <payload> F7`, at most 64
bytes. Read-only requests: HELLO (`01`), GET_CONFIG (`02`, pages 0–2),
GET_STATUS (`07`). TELEMETRY_START (`05`, 1–100 Hz, 5 s lease) streams 54-byte
IMU/touch frames. Settings are RAM only on this firmware; a replug restores
defaults. Writes (`03` SET_CONFIG, `04` ACTION): the console can send a stored profile
with SET_CONFIG only from an explicit, confirmed Devices action. ACTION 7 starts
the 16 kHz microphone test stream and ACTION 8 stops it; the Devices toggle and
`trillium-base devices mic` send only those codes. ACTION 6 (bootloader) is not
sent from this console.


## Battery and the pack switch

GET_STATUS can report the battery as `absent` while a millivolt reading is still
present. On this hardware that means the pack's on/off switch is off, or the pack
is unplugged — not that the controller lacks a battery. With the switch on, the
same device reports `present` and a charge percentage.
