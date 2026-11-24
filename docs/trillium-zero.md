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

## Configuration from the Base (protocol, not yet in the console)

SysEx v1 on cable 1: `F0 7D 54 52 01 <type> <req> <payload> F7`, at most 64
bytes. Read-only requests: HELLO (`01`), GET_CONFIG (`02`, pages 0–2),
GET_STATUS (`07`). TELEMETRY_START (`05`, 1–100 Hz, 5 s lease) streams 54-byte
IMU/touch frames. Settings are RAM only on this firmware; a replug restores
defaults. Writes (`03` SET_CONFIG, `04` ACTION): the console can send a stored profile
with SET_CONFIG only from an explicit, confirmed Devices action. ACTION is unused
here (it would be needed to enable the 16 kHz test mic, which stays off).


## Battery and the pack switch

GET_STATUS can report the battery as `absent` while a millivolt reading is still
present. On this hardware that means the pack's on/off switch is off, or the pack
is unplugged — not that the controller lacks a battery. With the switch on, the
same device reports `present` and a charge percentage.
