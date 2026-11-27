<!-- Copyright (C) 2026 Paul Lambert. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Changelog

The workspace version is the Trillium Console / pilabd version advertised on the LAN. API remains `/api/v1`. This file is the release notes for software builds. An SD image, git tag or GitHub release is a separate step and is not made from this entry alone.

## Unreleased

### Licensing and copyright

- Copyright is now "Copyright (C) 2026 Paul Lambert" in every first-party file and in the console About text (was "M-of-N"); licence stays `GPL-3.0-or-later OR LicenseRef-Commercial`. One `LICENSE-COMMERCIAL.md` text shared with the other repositories; added `LICENSES/`, `REUSE.toml`, `CONTRIBUTING.md` (interim contribution terms) and `NOTICE.md` (names and marks, third-party exceptions). The Cabbage patch is labelled GPL-3.0-only, the sounds manifest names the TimGM6mb exception, `/Users/...` build paths are removed from the Rust SBOM and the owner's name from `docs/security.md`. No functional change.

### Six-view console

- Navigation is **Play, Listen, Record, Devices, Sounds & apps, Settings** (six tabs on the phone bar too). System, Patch panel (was Signals), Scenes, Scene MIDI map and Set up sit under Settings > Advanced or Sounds & apps, with a back link.
- **Listen**: DAC state, master level, AirPlay on/off, recordings with in-browser playback (`GET /api/v1/recordings/{id}/audio`).
- **Record**: one big start/stop with the take time, where takes go, and the recordings list.
- **Settings > Pair another device**: one-time pairing code with QR and countdown (`POST /api/v1/pair/code`).

### Music

- New **Music** page (one nav entry, between Listen and Record): albums grid with cover art, artists, songs, playlists, a Recordings tab (takes on this unit, kept apart from published music) and a Radio tab; search; album, artist and playlist pages; now-playing bar with transport, seek and volume; queue drawer; library sources with Scan now. iPad-sized touch targets. See `docs/music.md`.
- Library index in a new crate, `pilab-music`: folder and M3U sources, SQLite + FTS5 index, incremental scan (tags read only for new or changed files), MusicBrainz ids, folder and file-name fallbacks, folder or embedded cover art with cached thumbnails. Schema already holds the empty tables for later editing, merging and duplicate handling.
- Playback in MPD as a separate process (`pilab-mpd.service`, private socket, PipeWire output to the DAC), driven by pilabd with the `zpod-mpd` client. Player volume starts at 40% and is capped at 80%; the DAC level and synth master are unchanged. Formats other than 48 kHz are resampled by PipeWire (shown in the bar).
- API: `/api/v1/music/...` (status, sources, scan, albums, artists, tracks, search, takes, streams, art, playlists, play, queue, control). CLI: `trillium-base music ...`.
- Demo content fetched on install: three CC0 albums from the Internet Archive (credits in `library/ATTRIBUTION.md`, id `music-demo`) and four radio links. Tests: `cargo test -p pilab-music`, `scripts/music-smoke.py`.

### MIDI router

- Any input (USB/Bluetooth MIDI, Trillium Zero, console, Zero motion out) to any destination (built-in synth, FluidSynth, ZynAddSubFX, any MIDI output port) with channel filter and remap, key-range splits, merges, message-type filters and transpose; per-device profiles; a live monitor with CSV export. Saved in `~/.config/pilab/midi-router.json`; console (Devices), API and `trillium-base midi ...`. See `docs/midi-router.md`.

### Engines and sounds

- FluidSynth 2.4.4 with FluidR3_GM, and ZynAddSubFX 3.0.6 with its 32 banks, installed on the lab unit and run as sandboxed user services; their audio goes through the master level and limiter. New scenes **general midi** (FluidSynth) and **zynaddsubfx**. Engines card in Sounds & apps: start/stop, SoundFont, GM program per channel, Zyn bank and instrument.
- **MuldjordKit** (acoustic rock kit, CC BY 4.0) mixed to stereo with a GM map. Its credit, "Drum samples provided by DrumGizmo.org", is shown in the Sounds browser, Sounds & apps and Settings > About, and listed in the sounds manifest and ATTRIBUTION.

### Play view, navigation and setup

- New **Play** view, the default: big scene chips, Zero mapping chips, twelve GM drum pads (velocity from where the pad is touched; channel 10 over `/midi`), master level and Panic. Built for iPad and phone touch (targets of 44 px or more, no hover-only controls).
- **Set up** walks the six first-run steps from `docs/first-run.md`; progress is stored with `GET|PUT /api/v1/setup` (`~/.config/pilab/setup.json`) and the console opens it once on a fresh install.
- PNG home-screen icons (180, 192, 512 px, rendered from the existing app icon) and Apple web-app tags, so iPad Safari "Add to Home Screen" gets a proper icon.

### Trillium Zero mapping editor and drum kit

- All 41 Zero settings with names, ranges and app-style groups in the Devices mapping editor, the CLI (`trillium-base zero ...`) and the API (`/zero/schema`, `/zero/presets`, `/devices/{key}/settings|preset|export|import|action|auto`). Changes apply live and save to the profile. See `docs/zero-mapping.md`.
- Presets: trillium-kit, tap-drum, tilt-melody, tilt-strike, theremin, touch-strip, flick-gestures, plus saved presets; import and export in the Trillium app's `trill-preset` format.
- The saved profile is re-applied on connect, replug and reset (the Zero keeps settings in RAM only); `zero auto off` keeps the old manual behaviour.
- Scene **trillium kit**: Zero strike directions and taps on channel 10 play the VCSL kit (down snare, up crash, left kick, right closed hat, forward low tom, back open hat).

### Zero firmware

- Firmware card, `trillium-base firmware status|latest|check|update` and `/api/v1/firmware*`: running version, latest release, UF2 checks (nRF52840 family, application window, no bootloader or SoftDevice images) and a confirmed update path with a backup of `CURRENT.UF2`. The update was not run on the lab unit. See `docs/firmware.md`.

### Sounds

- VCSL Full Kit (GM notes 27-87: toms, ride, crash, splash, china, Latin percussion) and tuned VCSL packs: marimba, vibraphone, glockenspiel, balafon, mbira, all CC0, with presets in the Sounds browser.
- FluidSynth and ZynAddSubFX app manifests.
- `sbom/sounds-manifest.md` lists every pack and instrument app with its licence.

### Trillium Zero sensors and strike measurement

- Sensor inventory per Zero (Motion/IMU, Touch strip/Trill, Microphone, Battery) derived from telemetry and status, in `GET /api/v1/devices` (`sensors`, `note_path`), `trillium-base devices show [KEY]`, and a live **Sensors** table on the Devices Zero card. A Trill strip that stays in Retry is shown as "not fitted" (a motion-only Zero), not as a fault; the retry is logged once instead of repeating.
- `measure strike` follows the sensors: no "touch the play surface" cue without a strip, and it says what makes a note (IMU tap percussion, strike directions, tilt-strike). It waits for the first Zero note-on and reports MIDI to synth and acoustic to synth in one result; `--rehearse` checks the path with an injected note. `GET /api/v1/measure/strike/cue` returns the cue.
- Note-on timestamps are taken on the realtime MIDI thread, so the MIDI to synth delta is no longer quantised to the 66 ms status loop.

### Console polish

- Header carries the Trillium lockup (glyph and wordmark, from the brand kit and the live Trillium pages) with "Base" and the host line. The old header and pairing mark used the glyph as an `<img>` with `currentColor`, which rendered black on black; both now use the lockup.
- Responsive shell: the header wraps to two rows on narrower windows, and on a phone Panic, Rec and the master level stay in the header while the sections become a bottom tab bar. Cards scroll sideways instead of widening the page.
- Footer shows the console version and device id with "Copyright (C) 2026 Paul Lambert" and links to the Trillium Base page and the source.
- Settings gains an About card (version, commit, hardware, licence, trademark notice, links) and an API reference built from `GET /api/v1/endpoints`; the DAC sink field offers the sinks from `GET /api/v1/audio/devices`.
- Browser `confirm()` and `prompt()` are replaced by the console's own dialogs (load, delete and duplicate scene, monitor to DAC, send stored Zero settings).
- Accessibility: real links for navigation with `aria-current`, a skip link, visible focus rings, labelled controls, keyboard-operable rows, `h1` per page, polite live regions for toasts, reduced-motion support, and secondary text lifted to at least 4.5:1 on panels. An axe-core run (WCAG 2 A/AA and best practice) went from 709 findings across the seven views at two widths to none.
- Web app manifest (SVG icon, black theme colour) so a phone can add the console to its home screen. No raster icons are shipped.
- The overview no longer re-renders under a keyboard-focused control, and shows IPv4 addresses only in its subtitle.

### Trillium Zero microphone

- Devices toggle, API and CLI (`devices mic KEY on|off`). ACTION 7/8 only; settings are checked afterwards; the flag is stored in the profile and restored on reconnect. PipeWire source `Trillium Zero Mic` at 48 kHz (from 16 kHz `TRPC` on `/dev/trillium-zero-pcm`). DAC monitor still off by default. The console toggle uses the same styled dialog as other device writes.
- Default synth amplitude-envelope attack 10 ms → 1 ms (existing saved scenes are unchanged).
- Measurement helpers: `tools/measure-midi-responsiveness.py`, `tools/measure-strike-to-sound.py`. Latency notes and before/after numbers in `docs/latency.md` (`PIPEWIRE_LATENCY=32/48000` was tried on the lab unit and reverted: xruns and ~100 % DSP load).
- `trillium-base measure strike` (and `POST /api/v1/measure/strike`): prints GO, waits up to 60 s for a tap and the synth onset, and writes `~/.local/share/pilab/measurements/strike-YYYYMMDD-HHMMSS.json` plus a console event. The take records MIDI note-ons/offs, keeps the input strip out of the WAV (so a room mic cannot be mistaken for synth audio), and reports a signed strike-to-sound time (the Zero mic path can lag the synth). The saved Zero profile has tap percussion off, so a body strike is heard but sends no note; touch the play surface, or enable tap percussion (a device write, not done automatically).

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
