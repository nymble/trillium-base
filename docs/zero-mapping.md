<!-- Copyright (C) 2026 M-of-N. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Trillium Zero mapping

The console and CLI edit all 41 Zero settings with names, ranges and groups like the Trillium app.
The table of settings lives in `rust/crates/pilab-core/src/zero_map.rs` (written for Trillium Base;
the key names match the Trillium app's `trill-preset` files so presets move both ways).

## Where

- Console: **Devices** → Zero card → *Mapping*. Groups (Source, Notes, Strikes, Motion, Tilt,
  Flick) open and close; preset chips at the top; a strike-direction grid shows which drum each
  direction plays. Changes are sent as you edit (debounced), saved to the profile, and the
  one-line summary updates. The raw settings table is under *All settings*.
- Console: **Play** → *Zero mapping* chips switch presets in one tap.
- CLI:

```
trillium-base zero                      # settings with names and the summary
trillium-base zero schema               # all 41 settings, ranges, groups (JSON)
trillium-base zero presets              # built-in and saved presets
trillium-base zero preset trillium-kit  # apply a preset (and load its suggested scene)
trillium-base zero set strike_note_left=C2 tap_percussion=on [--no-save]
trillium-base zero export > kit.json    # trill-preset JSON (the app's format)
trillium-base zero import kit.json
trillium-base zero save-preset "My kit"
trillium-base zero auto on|off          # restore the saved profile on every connect
trillium-base zero action recalibrate   # panic, recalibrate, led-test, defaults, zero-orientation
```

Values accept note names (C4 = 60), channels 1-16, on/off and choice labels.

## API

`GET /zero/schema`, `GET|POST /zero/presets`, `DELETE /zero/presets/{id}`,
`POST /devices/{key}/settings {values, save}`, `POST /devices/{key}/preset {id, scene}`,
`GET /devices/{key}/export`, `POST /devices/{key}/import`, `POST /devices/{key}/action`,
`POST /devices/{key}/auto {manual}`. All under `/api/v1`, bearer token.

## Settings live in the Zero's RAM

The Zero keeps settings in RAM; a replug restores firmware defaults. The saved profile is
re-applied automatically on connect, replug and reset unless the profile is set to manual
(`zero auto off`), in which case the console shows a "differs from the device" warning instead.

## Trillium kit

Preset `trillium-kit` with scene **trillium kit**:

| Setting | Value |
| --- | --- |
| Channel (12) | 10 (GM drums) |
| Strike directions (31) | all six |
| Down / up / left / right / forward / back (32-37) | snare 38 / crash 49 / kick 36 / closed hat 42 / low tom 45 / open hat 46 |
| Unclear strike (13) | snare 38 |
| Tap percussion (11), threshold (25), time between taps (26) | on, 70, 90 ms |
| Motion controllers (8) | off |

The scene routes channel 10 from any input to slot 1 (VCSL Full Kit when installed, else the
Starter Kit) and channel 1 to a kalimba in slot 2. "Back" plays the open hat because the
Starter Kit has no ride; with the Full Kit, 51 (ride) is available to map.

## Trillium app vs console (gaps)

Done in the console: all 41 settings with grouping and ranges, strike-direction map, presets
(built-in, saved, app-format import/export), actions 1-5, mic on/off, telemetry and sensors,
firmware version and update check, auto-restore.

Still only in the app (or nowhere):

| App feature | Console status |
| --- | --- |
| Bluetooth MIDI configuration (actions 10-13) | not offered: no radio transmit without an explicit OK |
| MPE output and per-touch expression view | not yet |
| Multi-touch play surface display (touch strip) | not yet; this unit has no strip fitted |
| MIDI monitor with CSV export | not yet (planned with the MIDI router) |
| Sensor charts and 3-D orientation view | numbers only |
| Instrument view (on-screen keyboard tied to settings) | Play view has drum pads only |
| Settings stored on the Zero itself | firmware keeps RAM only; the console re-applies on connect |
| Preset "restore original" and preset diff | not yet |
