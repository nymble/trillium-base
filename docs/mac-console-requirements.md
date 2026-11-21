# Requirements for a native Mac console (Trillium Console for Mac)

These are requirements for a native macOS app, **Trillium Console**, that controls a
**Trillium Base** (the `pilabd` service on the device).
They are vendor-neutral: any team or toolkit can meet them. The web console in
`rust/crates/pilabd/web` is the reference behaviour; the native app talks to the same
API and must not need anything the web console does not.

## 1. Discovery and connection

- R1.1 Browse `_trillium-base._tcp` and `_pilab._tcp` (older units) with Bonjour
  (`NWBrowser` or `NetServiceBrowser`). Show each instance's name (e.g.
  `Trillium Base (pilab)`), host, `id` and `version` TXT values.
- R1.2 De-duplicate by the `id` TXT record (one device can appear on IPv4, IPv6 and several
  interfaces, and its name can change after a conflict).
- R1.3 Fallbacks when browsing finds nothing: a manual URL or address field, the last
  working address per device id, and a QR/URL hand-off from `trillium-base pair` (`pilabctl pair`) output.
- R1.4 Check `GET /api/v1/info` before pairing; refuse an incompatible `api` value with a
  clear message.
- R1.5 Prefer IPv4 link addresses when both resolve; retry on the other family.

## 2. Pairing and secrets

- R2.1 Ask once per device for the pairing token (printed only by `trillium-base pair` on the
  device). Store it in the macOS Keychain keyed by device id, never in preferences or logs.
- R2.2 Send it as `Authorization: Bearer` on HTTP and `?token=` on the WebSocket.
- R2.3 On 401, drop the stored token and return to pairing (the token may have been
  rotated with `trillium-base pair --rotate`).
- R2.4 Show that the connection is plain HTTP on the LAN.

## 3. Live state

- R3.1 One WebSocket per device: `status` frames at 15 Hz, `sys` frames every 2 s.
  Reconnect with backoff; show a stale state after 3 s without frames.
- R3.2 Header: synth on/off, voices, DSP load, xruns, DAC state, temperature, scene name,
  unsaved-changes mark, stereo peak meters, master level, Rec and Panic.
- R3.3 Panic must work with one click or keystroke from every screen.

## 4. Screens (parity with the web console)

- R4.1 Overview: machine, audio backend and thread placement, services with risk notes,
  MIDI inputs with activity and matched device identity (and skipped ports with reasons), recording state, listening ports, USB devices, events.
- R4.2 Sounds: 16 slots; editor for wave, octave, detune, level, pan, velocity, pulse width,
  mod depth, amp ADSR, filter and filter ADSR; live preview from
  `/scene/slots/{id}/wave`; AKWF browser with previews.
- R4.3 MIDI Map: matrix of inputs × sounds, route table (input: device class, console,
  virtual, any, or name glob; channel, note range,
  transpose, velocity scale, sound), keyboard lanes showing splits and layers, monitor.
- R4.4 Scenes: list, load (with unsaved-changes warning), save, save as, duplicate, delete,
  export TOML.
- R4.5 Signals: audio and MIDI ports with links; link and unlink.
- R4.6 Settings: local recording, NAS archive (shows not configured until the device says
  otherwise), audio outputs, DAC sink, watchdog, MIDI autoconnect and ignore list, network.

## 5. Playing

- R5.1 On-screen keyboard plus computer keys (A W S E D F T G Y H U J K O L P, Z/X octave),
  velocity, bend (springs back) and mod wheel.
- R5.2 Target switch: through the MIDI map, or the selected sound (`POST /audition`).
- R5.3 Optional: forward a CoreMIDI device over the API, rate-limited to 1000 events/s.

## 6. Safety

- R6.1 Never send a master level above −6 dBFS (the device caps it anyway) and never raise
  the level as a side effect of loading a scene.
- R6.2 When a service is switched on, show its risk note.
- R6.3 Do not offer controls for services the device marks read-only.

## 7. Look and feel

- R7.1 Dark, dense pro-audio layout using the Trillium Stage palette and the same token
  names (`--tr-*` equivalents): black ground, graphite panels, warm white text, Signal
  orange only for the one active element and at most one primary button per screen (black
  text on orange), amber only for meters, Connected green only for status dots.
- R7.2 Outfit for headings and big numbers, Inter for UI, IBM Plex Mono for MIDI values and
  versions. Card radius 20, control radius 10, pill buttons.
- R7.3 The Rhodonea mark unaltered; footer or About reads "Trillium, an Ambient Lab project".
- R7.4 Product name from one constant, matching the device's `product` TXT value.

## 8. Platform

- R8.1 macOS 14 or later, Apple silicon and Intel; sandboxed with the network client
  entitlement and a Local Network usage description.
- R8.2 Works with several devices; one window per device.
- R8.3 No telemetry; no internet access needed.
- R8.4 Accessibility: every control reachable by keyboard and labelled for VoiceOver.
