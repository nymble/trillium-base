# Console test plan

Nine steps from a cold boot to a saved setup, with Trillium as the MIDI controller. Run
them after each console release. Keep headphones off or the amp down until step 3.

Automated tests (`cd rust && cargo test --release`) run first: oscillator pitch and
aliasing, level safety and limiter, zero allocation in the audio path, routing (splits,
layers, channels, transpose, globs, audition bypass), scene round trip, settings defaults.

## 1. Boot and quiet start

- Reboot the Pi. Without logging in, `pilabd.service` starts (linger is on).
- `ssh lab@pilab.local pilabctl status` shows `running: true`, `master_db: -18` or lower,
  `voices: 0`, `xruns: 0`.
- Pass: no sound and no click on the DAC output at start; the journal shows
  `synth started (fade-in)`.

## 2. Discovery

- Mac: `dns-sd -B _pilab._tcp local.` lists `pilab console on pilab` within 2 s;
  `dns-sd -L` shows port 8410 and the TXT `id`.
- Open `http://pilab.local:8410/`. The pairing screen shows the host and device id.
- Pass: found by name; the IP fallback (`http://192.168.1.244:8410/`) also loads.

## 3. Pairing and first sound

- On the Pi run `pilabctl pair`, type the token into the browser.
- Open the keyboard drawer, press C4 at velocity 90.
- Pass: a soft saw note at about −18 dBFS. Meters move amber. Panic silences instantly.
- Wrong token is rejected; `pilabctl pair --rotate` logs the browser out within 2 s.

## 4. Trillium as MIDI input

- USB: connect Trillium (dev ids `1209:0003` / `1209:0002`, product "Trill XIAO MIDI ...").
  Overview → MIDI inputs shows cable 0 on `in_N` with the **Trillium** badge and
  `USB · 1209:000x · Trill XIAO MIDI · cable 0`. Cable 1 (config SysEx) is listed as
  **not connected** with the reason; Signals shows no link from it. The two CDC ACM serial
  ports do not appear at all.
- BLE: connect Trillium (advertised as `Trillium XXXX`) to the Pi's BLE MIDI server.
  Within about 6 s the `BLE MIDI 1` input shows the Trillium badge and
  `BLE · Trillium XXXX`. With no Trillium connected, `BLE MIDI 1` shows no badge.
- MIDI Map monitor shows note on/off, velocity, bend and CC1 from Trillium.
- Pass: notes play through the default route; bend covers ±2 semitones; mod wheel moves
  PWM on the PWM Lead sound; no SysEx from cable 1 reaches the synth.

## 5. Sound editing

- Sounds → PWM Lead: change wave, cutoff, resonance, ADSR while holding a note.
- Pick an AKWF table (e.g. `AKWF_bw_saw`) for slot 5 and play it in "Selected sound" mode.
- Pass: changes are heard within about 100 ms with no clicks; the waveform preview
  matches; the scene shows the unsaved mark.

## 6. Routing: split and layer

- Load the `trillium split` sample scene, or build it: `device:trillium` notes C-1..B3 →
  Sine Pad transposed −12; C4..G9 → PWM Lead; C2..C7 → Soft Triangle at velocity × 0.5.
- Pass: low and high halves play different sounds; the layer sounds on every note; the
  keyboard lanes show the split and the layer; the web keyboard ("console") still plays
  only the routes that match it.

## 7. Scenes

- Save as `trillium split`. Load `default`, then load `trillium split` again.
- Restart the service (`systemctl --user restart pilabd`).
- Pass: the scene round-trips exactly, it is active after restart, and the level after
  restart is −18 dBFS or lower even if the scene was saved louder.

## 8. Signals and other apps

- Start another JACK/PipeWire MIDI app or synth (e.g. the Faust GUI or SuperCollider).
- In Signals, link its MIDI output to `pilab-synth:midi_in`, then unlink it.
- Pass: links appear and disappear in the console and in `pw-jack jack_lsp -c`; the synth
  keeps running with no xruns.

## 9. Recording, archive switch and resilience

- Press Rec, play 20 s, stop. Settings lists the take as "only on this device"; the WAV
  and `record.toml` are in `~/.local/share/pilab/recordings/<id>/`.
- Turn NAS archive on: it reports **not configured**, nothing mounts.
- Load the CPU (`stress-ng --cpu 3 -t 30` on CPUs 0-2) while playing: no dropouts.
  Force xruns (raise synth voices plus a small quantum) to see the watchdog fire once,
  log it and back off for 120 s.
- Pass: the take plays back cleanly; archive stays off until configured; audio survives
  load; the watchdog acts and logs.
