<!-- Copyright (C) 2026 M-of-N. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Changelog

The workspace version is the Trillium Console / pilabd version advertised on the LAN. API remains `/api/v1`. This file is the release notes for software builds. An SD image, git tag or GitHub release is a separate step and is not made from this entry alone.

## 0.4.0-beta.1 — 2026-11-24

Toward the next Base image. Deployed to the lab Pi; no image, tag or GitHub release in this step.

### Trillium Zero

- Devices module stores a read-only settings profile per USB socket and re-applies it only on an explicit, confirmed console or CLI action (`trillium-base devices reapply KEY`).
- A Zero moved to another USB port reuses the only stored profile with the same USB id (it is not treated as a brand-new device).
- A pilabd restart no longer flags a still-connected Zero as a reconnect. A genuine replug or a boot-id change is logged distinctly.
- Unplugged Zeros stay in the Devices list as disconnected, so their last settings and profile remain visible.
- Live sensor plot in the Devices view (20 Hz telemetry ring).
- Pitch and roll are also a patch-panel MIDI source, `Trillium Synth:zero_motion` (CC 16 / 17 / 18), not auto-routed to the synth.
- The Zero's own 16 kHz test microphone stays off. Enabling it would write to the device, so it is not a patch-panel source in this build.
- Battery status "absent" means the pack switch is off or the pack is unplugged, not that the hardware is missing.
- USB MIDI cable 0 routes automatically to Trillium Synth; cable 1 (configuration) never does (from 0.2.0 / 883cc9c, still in this build).

### Audio input

- Yeti Nano (and any other JACK capture that matches `[input] source`) feeds `Trillium Synth:input_l/r`. Monitoring to the DAC stays off unless confirmed, to avoid microphone feedback.
- The Nordic `1915:1025` USB capture can be selected as the input strip source from the Devices view.
- Record cards name the source as the synth output plus the input strip when input recording is on.

### Other

- `GET/POST /api/v1/input`, `GET /api/v1/devices`, snapshot / reapply / telemetry endpoints, and `trillium-base devices|input` commands.
- Optional persistent journald (`etc/journald-persistent.conf`) so boot-time dropouts survive a reboot.
- Gesture-to-sound latency method and numbers: [docs/latency.md](docs/latency.md).

## 0.2.0 — 2026-11-20

Trillium Console on Trillium Base, advertised as `_trillium-base._tcp` and `_pilab._tcp`.
