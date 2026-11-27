<!-- Copyright (C) 2026 Paul Lambert. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# MIDI router and sound engines

The router sits in the audio thread between every MIDI input and every MIDI destination.
With the default configuration (one route, every input to the scene) the unit behaves exactly
as before: whatever is played reaches the built-in synth, which then applies the scene's own
MIDI map (Sounds & apps > Scene MIDI map).

## Sources and destinations

| Source keyword | Matches |
| --- | --- |
| `any` | every input: USB and Bluetooth MIDI, Trillium Zero, console pads and keyboard |
| `zero` | every connected Trillium Zero |
| `console` | the console's pads, drum pads and on-screen keyboard (auditions are not routed) |
| `zero-motion` | the Zero motion stream, CC 16-18 (`zero_motion` out) |
| a glob, e.g. `*Launchkey*` | inputs whose name or device name matches |

| Destination keyword | Goes to |
| --- | --- |
| `scene` | the scene's engine: the built-in synth, or FluidSynth / ZynAddSubFX for the "general midi" and "zynaddsubfx" scenes |
| `synth` | the built-in Trillium Synth (and its scene MIDI map) |
| `fluidsynth`, `zynaddsubfx` | the engine, when it is running |
| a glob, e.g. `*Volca*` | any MIDI output port (a USB MIDI synth, another program); up to six outputs at once |

Several routes from one input **split** it (by key range or channel); several routes into one
destination **merge** inputs. A route can keep only some channels, re-channel everything to
one channel, keep a key range, keep only some message types (`note`, `cc`, `program`,
`bend`, `pressure` (channel aftertouch; poly aftertouch counts as `note`)) and transpose notes.

**Device profiles** apply to one input before any route: turn the device off, force its
channel, transpose, fixed velocity or a velocity scale, and drop message types.

SysEx, clock and other system messages (0xF0 and up) are not routed. The Zero's own SysEx
settings protocol is unaffected (it does not go through the router).

## Persistence

`~/.config/pilab/midi-router.json` (`format: pilab-midi-router`, `version: 1`), written
atomically on every change. `POST /api/v1/midi/router/reset` restores the default.

## Monitor

The console's Devices view shows every event live with where it went ("filtered" when no
route took it). The daemon keeps the last 5000 events: `GET /api/v1/midi/monitor?since=SEQ`,
`GET /api/v1/midi/monitor.csv` (time, source, kind, channel, text, bytes, destinations),
`DELETE /api/v1/midi/monitor` to clear.

## CLI

```
trillium-base midi routes
trillium-base midi route add zero synth --ch 10 --name "Zero drums"
trillium-base midi route add "*Launchkey*" fluidsynth --keys C2-B3 --to-ch 2
trillium-base midi route add "*Launchkey*" "*Volca*" --keys C4-C7 --types note,cc --transpose -12
trillium-base midi route off ID | on ID | rm ID
trillium-base midi profile set "*Launchkey*" channel=1 velocity_fixed=100 block=pressure
trillium-base midi profile rm "*Launchkey*"
trillium-base midi monitor --follow | --csv > take.csv | --clear
trillium-base midi reset
```

## API

| Method | Path | |
| --- | --- | --- |
| GET, PUT | `/api/v1/midi/router` | whole configuration with live status, sources, destinations |
| POST | `/api/v1/midi/router/routes` | add a route |
| PUT, DELETE | `/api/v1/midi/router/routes/{id}` | change or remove one route |
| PUT, DELETE | `/api/v1/midi/router/profiles` (`?matches=` for DELETE) | set or remove a device profile |
| POST | `/api/v1/midi/router/reset` | back to "every input to the scene" |
| GET, DELETE | `/api/v1/midi/monitor` | recent events (`since`, `limit`), clear |
| GET | `/api/v1/midi/monitor.csv` | the same as CSV |

## Sound engines

FluidSynth (General MIDI, FluidR3_GM SoundFont by default, TimGM6mb selectable) and
ZynAddSubFX (32 banks from `zynaddsubfx-data`) run as user services
`pilab-fluidsynth.service` and `pilab-zynaddsubfx.service`, under PipeWire's JACK, with no
network access (`PrivateNetwork=yes`: FluidSynth's command shell and ZynAddSubFX's OSC port
stay off the LAN). Their audio comes back into Trillium Synth's `engine_l/engine_r` inputs
and goes through the same master level and limiter as the built-in synth.

Loading the "general midi" scene (Sounds & apps > Play the General MIDI scene, or
`trillium-base engines scene fluidsynth`) starts FluidSynth and routes the scene there; the
same for "zynaddsubfx". Other scenes stop nothing: an engine keeps running until stopped.

```
trillium-base engines list
trillium-base engines start fluidsynth | stop fluidsynth
trillium-base engines sound fluidsynth /usr/share/sounds/sf2/TimGM6mb.sf2
trillium-base engines program 1 25          # channel 1: GM program 25 (Acoustic Guitar, nylon)
trillium-base engines sound zynaddsubfx /usr/share/zynaddsubfx/banks/Pads/0033-Soft\ Pad.xiz
trillium-base engines scene zynaddsubfx
```

API: `GET /api/v1/engines`, `POST /api/v1/engines/{id}/start|stop|sound|program|scene`.
