# pilab console

The console is the control surface for the Pi's sound modules. v0 ships one module, the
**Waveform synth** (`pilab-synth`), and is built so that other MIDI, generative and audio
apps run beside it and are patched together through PipeWire.

Product name: `pilab console` for now. It is set once in
`rust/crates/pilabd/src/brand.rs` (`PRODUCT_NAME`) and injected into the web page.

## Architecture

```
 USB / BLE MIDI (Trillium, keyboards) ─┐
 ALSA / JACK MIDI apps ────────────────┤   PipeWire graph (JACK API via pw-jack)
                                       ▼
 ┌──────────────────────── pilabd (user service) ─────────────────────────┐
 │  JACK client "pilab-synth"                                             │
 │    midi_in  (port 0, for apps)   in_1..in_7 (auto-connected sources)   │
 │    audio thread on CPU 3 ── Engine (pilab-synth) ── out_l / out_r ─────┼──► DAC sink
 │         ▲ lock-free command ring          │ stats atomics, garbage ring│
 │  control side (tokio, CPUs 0-2)                                        │
 │    scenes, routing compiler, AKWF loader, recorder, watchdog, sys probes│
 │    HTTP + WebSocket on :8410  ── embedded web console (no Node build)  │
 └────────────────────────────────────────────────────────────────────────┘
        ▲ browser on the Mac, iPad or phone (token pairing)       ▲ pilabctl (terminal)
```

One process hosts both the synth and the console. That keeps v0 to one service and one
port, and the engine stays isolated from the web side: the audio callback only reads a
wait-free ring of fixed-size commands, never takes a lock and never allocates (a test
asserts this with a counting allocator). Anything that must be freed (old route tables,
replaced wavetables) goes back on a garbage ring and is dropped on the control side.

Crates (`rust/`):

| Crate | What |
| --- | --- |
| `pilab-synth` | Engine, oscillators, mipmapped wavetables, routing table, tuning. No I/O. |
| `pilab-core` | Scene format and store, routing compiler, settings, globbing. No audio. |
| `pilabd` | The service (`pilabd`) and terminal client (`pilabctl`), audio backends, API, web assets. |

ZPOD crates are used unmodified from `nymble/zpod` at `525bc84` (see
[zpod-convergence.md](./zpod-convergence.md)).

### Audio backend

`AudioBackend` is a trait. `JackBackend` talks to PipeWire's JACK implementation
(`pw-jack`), so the synth is an ordinary PipeWire node named **pilab-synth** that any
patchbay (or the console's Signals view) can see and wire. `NullBackend` runs the engine
on a timer with no audio device, for development and screenshots (`--null-audio`).
If JACK is unavailable at start, pilabd logs it and falls back to the null backend so the
console stays reachable.

MIDI input: port 0 `midi_in` is left for apps. Hardware and ALSA sources are assigned to
`in_1`..`in_7` as they appear and connected automatically (Settings → MIDI; an ignore
list of globs is honoured), so routes can match a **source by name**, for example
`*Trillium*`. Port 8 is the web keyboard; port 9 is audition (the sound open in the
editor, bypassing the map).

### Engine (Waveform module v0)

- 32 voices, oldest-voice stealing, sustain pedal, 16-sample control rate, 5 ms crossfade on
  wave changes.
- Oscillators: sine, saw, square, pulse (PWM from the mod wheel), triangle, and AKWF
  single-cycle tables. Default mode reads mipmapped Fourier tables (2048 points, a level
  every third of an octave), measured alias floor below −60 dB at 5 kHz fundamental. A
  polyBLEP mode (ZPOD `dsp::saw`/`square`, plus polyBLAMP triangle) is kept for
  comparison.
- AKWF tables: the 600-sample cycle is resampled by DFT and built into the same mipmaps
  when loaded (off the audio thread).
- Per voice: ADSR amp envelope, state-variable low-pass filter (ZPOD `Svf`) with its own
  ADSR, key tracking and velocity amount.
- MIDI: note on/off, velocity, pitch bend (±2 semitones by default, per scene), CC1 mod
  wheel, CC64 sustain, CC120/123 all sound/notes off. A4 is per scene (default 440 Hz).

### Output safety

The DAC has a headphone output, so the output stage is fixed in code:

1. Master starts at **−18 dBFS** or lower on every service start; the console cannot set
   more than −6 dBFS.
2. 50 ms ramps whenever the synth starts or stops, and on SIGTERM.
3. DC blocker, then a peak limiter at −1 dBFS, then a soft clip (unity below 0.5).
4. Loading a scene never raises the master level above what is playing.

## Scenes

Scenes are versioned TOML files in `~/.config/pilab/scenes/<name>.toml`. Saving writes
atomically and keeps the previous file as `<name>.toml.bak`.

```toml
format = "pilab-scene"
version = 1
name = "live set"
description = "Trillium split: pad below C4, lead above"
master_db = -18.0
a4_hz = 440.0
bend_range = 2.0

[[slot]]
id = 1                       # 1..16
name = "PWM Lead"
table = ""                   # AKWF path when wave = "table", e.g. "AKWF_bw_saw/AKWF_saw_0001.wav"
[slot.sound]
enabled = true
wave = "pulse"               # sine | saw | square | pulse | triangle | table
pulse_width = 0.35
pwm_mod = 0.4                # mod wheel depth on pulse width
octave = 0
detune_cents = 0.0
level_db = -6.0
pan = 0.0
vel_to_amp = 0.8
[slot.sound.amp_env]
a = 0.005
d = 0.2
s = 0.8
r = 0.25
[slot.sound.filter]
enabled = true
cutoff_hz = 2500.0
resonance = 0.3
env_amount = 1.5             # octaves
key_track = 0.5
vel_amount = 1.0
[slot.sound.filter_env]
a = 0.005
d = 0.4
s = 0.2
r = 0.4

[[route]]
enabled = true
label = "Trillium high"
input = "*Trillium*"         # "*" any, "virtual", "console", or a glob on the source name
channel = 0                  # 0 = omni, 1..16
note_lo = 60
note_hi = 127
transpose = 0                # semitones, -48..48
velocity_scale = 1.0
slot = 1
```

Routing rules: every enabled route whose input, channel and note range match a note
plays its slot, so overlapping routes **layer** and adjacent ranges **split**. Transposed
notes outside 0..127 are dropped. Up to 64 routes, 8 layers per note.

Settings live in `~/.config/pilab/settings.toml` (audio ports, DAC sink name, MIDI
autoconnect and ignore list, recording, archive, watchdog). The active scene name is kept
in `~/.config/pilab/state.toml`.

## Recording and archival

- **Local recording** (on by default, nothing records until Rec is pressed): float32 WAV of
  the synth output after the limiter, to `~/.local/share/pilab/recordings` or a chosen
  location, with a `record.toml` card per take (role master, `filed = false`,
  rights `LicenseRef-proprietary`). It stops at 3.5 GB per take or when free space falls
  below the configured floor. The console warns when takes stay unfiled past a limit.
- **Archive to NAS** (off by default): target `//192.168.1.2/PilabRecordings` as account
  `pilab-rec`, credentials in a root-only file the service never reads into the UI. v0
  reports **not configured** and mounts nothing. Local may be enough for many uses.

## Services view

Overview lists the services that matter for sound and exposure, with their ports and a
risk note. Only the Waveform synth is switchable from the console in v0; AirPlay, Faust,
SuperCollider, VNC, SSH and mDNS are shown read-only with the command that switches them.
The listening-ports table comes from `ss` and flags anything undeclared.

## Dropout watchdog

If xruns exceed 5 per second for 6 seconds, the watchdog suspends and resumes the DAC
sink (`pactl suspend-sink`), logs it to the Events list and the journal, and then waits
120 seconds before it can fire again. All three numbers are in Settings.

## HTTP API

Base path `/api/v1` on port 8410. Every endpoint except `/info` and `/endpoints` needs
`Authorization: Bearer <token>` (the WebSocket takes `?token=`). JSON in and out; scene
downloads are TOML.

| Method | Path | What |
| --- | --- | --- |
| GET | `/info` | Product, version, device id, pairing requirement (no token needed) |
| POST | `/pair/check` | 200 if the bearer token is valid |
| GET | `/ws?token=` | WebSocket: status frames at 15 Hz, sys frames every 2 s; send {t:'midi',bytes:[..]} |
| GET | `/status` | Voices, peaks, limiter, master, xruns, recording |
| GET | `/system` | Machine state, DAC, backend, services, ports, USB, events, recording and archive status |
| POST | `/services/{id}` | {on:bool} for services the console controls (pilab-synth) |
| GET | `/scene` | Active scene (JSON) and whether it has unsaved changes |
| PUT | `/scene` | Replace the active scene (applied live, not saved) |
| PATCH | `/scene` | {master_db?, a4_hz?, bend_range?, description?} |
| PUT | `/scene/slots/{id}` | Create or replace sound slot id (1..16) |
| DELETE | `/scene/slots/{id}` | Remove a slot and routes that target it |
| GET | `/scene/slots/{id}/wave` | One-cycle preview of the slot's oscillator |
| PUT | `/scene/routes` | Replace the MIDI routing map |
| GET | `/scenes` | Saved scenes |
| GET | `/scenes/{name}` | Saved scene as TOML |
| POST | `/scenes/{name}/load` | Load a saved scene (never raises the level) |
| POST | `/scenes/{name}/save` | Save the active scene under a name |
| POST | `/scenes/{name}/duplicate` | {to} copy a saved scene |
| DELETE | `/scenes/{name}` | Delete a saved scene |
| POST | `/midi` | {bytes:[..]} or {note,vel,on,ch} / {bend} / {cc,value}; target 'route' (default) or 'sound' |
| POST | `/audition` | {slot} sound played by target 'sound' (0 = none) |
| POST | `/engine` | {run:bool} fade the synth in or out |
| POST | `/panic` | All sound off |
| POST | `/master` | {db} master level, capped at -6 dBFS |
| GET | `/midi/inputs` | MIDI input slots and their sources |
| GET | `/library/akwf` | AKWF groups |
| GET | `/library/akwf/{group}` | Files in a group |
| GET | `/library/preview?path=&points=` | Raw single cycle for drawing |
| GET | `/graph` | Audio/MIDI ports and links (PipeWire via the JACK API) |
| POST | `/graph/connect` | {from,to} link two ports |
| POST | `/graph/disconnect` | {from,to} unlink two ports |
| GET | `/settings` | Settings |
| PUT | `/settings` | Replace settings (archive 'configured' is not settable here) |
| POST | `/record` | {on:bool} start/stop local recording |
| GET | `/recordings` | Recording cards, newest first |
| GET | `/endpoints` | This list |

WebSocket frames: `{"t":"status", voices, peak_db[2], limiter_db, master_db, running,
xruns, xrun_rate, dsp_load, rec, midi[], dirty, scene}` at 15 Hz and `{"t":"sys", ...}`
(the `/system` body) every 2 s. Send `{"t":"midi", "note":60, "vel":90}` (or `bytes`,
`bend`, `cc`, plus `target`) to play.

## Files on the Pi

| Path | What |
| --- | --- |
| `~/.local/bin/pilabd`, `~/.local/bin/pilabctl` | Binaries |
| `~/.config/systemd/user/pilabd.service` | User unit, enabled (linger is on) |
| `~/.config/pilab/console-token` | Pairing token, mode 0600 |
| `~/.config/pilab/{settings,state}.toml`, `scenes/` | Settings and scenes |
| `~/.local/share/pilab/library/akwf/` | AKWF library (fetched, never in git) |
| `~/.local/share/pilab/recordings/` | Local recordings and cards |
| `/etc/avahi/services/pilab-console.service` | mDNS advertisement |

## Commands

```sh
pilabctl pair            # print the pairing token and console URLs (terminal only)
pilabctl pair --rotate   # new token; browsers must pair again
pilabctl status          # voices, level, xruns
pilabctl gain -24        # master dBFS (capped at -6)
pilabctl note 60 90 0.5  # play middle C for half a second through the map
pilabctl panic
pilabctl scenes | load NAME | save NAME
pilabctl record on|off
pilabd bench             # offline cost per voice on this CPU
journalctl --user -u pilabd -f
```
