# Trillium Console

Trillium Console is the control surface for a **Trillium Base** (this Pi 5) and its sound
modules. v0 ships one module,
**Trillium Synth** (crate and service id `pilab-synth`), and is built so that other MIDI, generative and audio
apps run beside it and are patched together through PipeWire.

Names are set once in `rust/crates/pilabd/src/brand.rs`: `PRODUCT_NAME` = Trillium
Console (page title, header, API `product`), `DEVICE_NAME` = Trillium Base (mDNS instance
and TXT, Overview, API `device`), `SYNTH_NAME` = Trillium Synth. Internal ids keep the
`pilab` prefix: `pilabd`, `pilabctl` (alias `trillium-base`, a symlink installed next to
it), `pilab-core`, `pilab-synth`, `~/.config/pilab`, `_pilab._tcp`.

## Architecture

```
 USB / BLE MIDI (Trillium, keyboards) ─┐
 ALSA / JACK MIDI apps ────────────────┤   PipeWire graph (JACK API via pw-jack)
                                       ▼
 ┌──────────────────────── pilabd (user service) ─────────────────────────┐
 │  JACK client "Trillium Synth"                                          │
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
(`pw-jack`), so the synth is an ordinary PipeWire node named **Trillium Synth** that any
patchbay (or the console's Signals view) can see and wire. Its ports are
`Trillium Synth:out_l`/`out_r`, `midi_in` and `in_1`..`in_7`, plus eight hidden meter
inputs (`meter_1`..`meter_8`) that are never listed.

Rename (Nov 2026): the node was called `pilab-synth`. That stays the stable internal id
(crate, service id; `trillium-synth` is accepted as an alias), and port names that start
with `pilab-synth:` (API calls, `settings.toml` patterns) are mapped to the new name. The
outputs are linked to `audio.output_ports` (the DAC) at start, as before. `NullBackend` runs the engine
on a timer with no audio device, for development and screenshots (`--null-audio`).
If JACK is unavailable at start, pilabd logs it and falls back to the null backend so the
console stays reachable.

MIDI input: port 0 `midi_in` is left for apps. Hardware and ALSA sources are assigned to
`in_1`..`in_7` as they appear and connected automatically (Settings → MIDI; an ignore
list of globs is honoured). Port 8 is the web keyboard; port 9 is audition (the sound
open in the editor, bypassing the map).

### Known devices (device classes)

Routes can name a **device class** instead of a port name: `input = "device:trillium"`.
Classes live in `~/.config/pilab/settings.toml` under `[[midi.devices]]`, are matched in
order, and the first match wins. Within a class the USB rule is tried before the BLE rule.
For each MIDI source pilabd works out an identity, read-only, from:

- **USB/ALSA**: the ALSA sequencer client and port (`/proc/asound/seq/clients`), the
  sound card behind it and its `VID:PID` (`/proc/asound/cardN/usbid`). The ALSA port
  number is the USB MIDI cable (0-based).
- **BLE**: PipeWire names BLE MIDI ports `BLE MIDI 1`, which is not the device's name. The
  Pi's BLE MIDI node runs in server role and does not name its peer, so pilabd asks BlueZ
  for the aliases of connected devices (`bluetoothctl devices Connected` and `info`, polled
  at most every 6 s while a BLE MIDI port exists) and matches those against the class globs.

Default (development ids; they will change, for example to a "Trillium Zero" product
string, so edit settings rather than code):

```toml
[[midi.devices]]
id = "trillium"
label = "Trillium"
usb_ids = ["1209:0003", "1209:0002"]           # VID:PID, either matches
name_prefixes = ["Trill XIAO MIDI", "Trillium Zero"]   # USB/ALSA product, client or port name
ble_aliases = ["Trillium *"]                   # BlueZ alias globs ("Trillium XXXX")
route_cables = [0]                             # only cable 0 (the instrument) is connected and routed
note = "USB cable 0 is the instrument; cable 1 is configuration SysEx and is never connected; ..."
```

Rules that follow from it:

- A matched USB port whose cable is not in `route_cables` (Trillium cable 1, config SysEx)
  is **not auto-connected** and never reaches the synth. If it was connected earlier it is
  disconnected. A matched port whose cable number cannot be read is also left alone.
- Trillium's two CDC ACM serial ports are not MIDI ports; the console never opens them.
- Other USB devices (for example the Nordic `1915:1025` composite device) do not match.
- `*` (any input) still means every connected source, so a skipped port stays out of it.
- Overview → MIDI inputs shows each source's identity (`USB · 1209:0003 · Trill XIAO MIDI ·
  cable 0`, or `BLE · Trillium 7F2A`), the class it matched and by which rule, and lists
  skipped ports with the reason.

Route inputs, in short: `device:<id>`, `console`, `virtual`, `*`, or a glob on the graph
port name (e.g. `*Keystation*`).

### Engine (Waveform module v0)

- 32 voices, oldest-voice stealing, sustain pedal, 16-sample control rate, 5 ms crossfade on
  wave changes.
- Oscillators: sine, saw, square, pulse (PWM from the mod wheel), triangle, AKWF
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

### Wavetables

Common practice (Serum, Vital, Surge XT, Ableton Wavetable, Waldorf) treats a wavetable as
an ordered set of single cycles scanned by a **position** control, followed by optional
warp transforms, unison, then the usual filter / envelope / FX chain. Trillium Synth follows that:

- Oscillator types per slot: sine, saw, square, pulse, triangle, **wavetable**, **sampler**.
- Wavetable = one AKWF folder. Every cycle in it is a frame; `position` (0..1) crossfades
  between neighbouring frames. Clicking a wave in the browser selects the folder and sets
  the position to that wave, so a "single wave" is just a parked position.
- Position modulation: the mod envelope (the filter envelope), a per-voice sine LFO
  (`lfo_hz`, `pos_lfo`), and the mod wheel (CC 1, `pos_mod`).
- Warp: bend (phase skew), sync (1-8 cycles per period), mirror, asym (PWM-style moved
  midpoint), fold (wavefolder on the output). Works on every oscillator type.
- Unison 1-3 copies with a cents spread; drive (soft saturation) before the filter.
- Names: `web/data/akwf-catalog.json` maps folder names to readable names and tags (Basic,
  Analog, Digital/FM, Instrument, Voice, Chip, Noise/Odd, Mixed). Edit the file to rename;
  unknown folders fall back to the folder name. The wave number is the secondary label.
- Not yet: a second oscillator with mix, chorus/delay/reverb FX, spectral warps. Each is a
  contained engine change if wanted.

### Selecting sounds and slot names

Every selection in Sounds (oscillator type, wavetable group, wave, sample bank, preset) is
sent to pilabd, which makes the slot playable before replying: a wavetable slot without a
group gets the previous or a default group and its first audible wave (silent or flat
cycles are skipped), with `position` on that wave; a sampler slot without a bank gets one,
and the bank is loaded before the reply (the editor shows a loading state meanwhile). The
console reads the slot back from the reply, and "Preview on select" plays only after that.
Updates go out one at a time, in order.

Slots are named after their source ("Saw", "FM synth #41", "Kalimba") until a name is
typed. Typing sets `name_locked = true` in the scene; "Auto name" clears it. Older scenes
without the field read as unlocked.

### Sampler

`wave = "sampler"` plays a multisample instrument loaded from SFZ (a practical subset:
`<control>/<global>/<master>/<group>/<region>` inheritance, `#define`, `sample`,
`default_path`, key and velocity ranges, `pitch_keycenter`, `tune`, `transpose`,
`volume`, `pan`, `offset`, `end`, loop modes `no_loop`, `one_shot`, `loop_continuous`,
`loop_sustain` with loop points, `seq_length`/`seq_position` round robin,
`lorand`/`hirand`, `group`/`off_by` choke, `trigger=attack`). Unknown opcodes are counted
and ignored. Samples are WAV (8/16/24/32-bit, float) or FLAC (pure-Rust decoder),
preloaded into RAM as f32; the audio thread only reads them. Pitch uses 4-point Hermite
interpolation; the slot's ADSR and filter apply. One-shot regions ignore note-off (drums).

Slots name a bank by id (`sample = "freepats-kalimba"`). Banks come from
`samples/index.json` under the library root (see "Sample library" in docs/library.md).

### Audition

Every sound has a **Play** button (Sounds editor, samples and presets, wavetable waves,
MIDI Map route rows). It plays about 1.5 s: C3, C4, G4 for most sounds; a held C4 for pads
(attack ≥ 0.25 s or release ≥ 1.2 s); kick, hat, snare, hat on channel 10 for drum kits.

- **DAC** (default): played through the synth's audition port on the Pi's DAC (headphone /
  line out) at the current master level, through the same limiter and soft clip. It never
  raises the level. The toast names the output and the level.
- **This browser**: pilabd renders the same phrase offline at the current master level
  (never above) into a small mono WAV (`/audition/render`), and the page plays it.
- **Preview on select** (off by default, remembered per browser) plays the phrase whenever
  you pick a slot, preset, bank or wave.

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
input = "device:trillium"    # a [[midi.devices]] class, "*" any, "virtual", "console", or a name glob
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

Two scenes are created on first start: `default` (every input plays Warm Saw) and
`trillium split` (Trillium: pad below C4, lead above, triangle layer; web keys on Warm Saw).

Settings live in `~/.config/pilab/settings.toml` (audio ports, DAC sink name, MIDI
autoconnect, ignore list and device classes, recording, archive, watchdog). The active scene name is kept
in `~/.config/pilab/state.toml`.

## Signals: patch panel, levels and meters

Sources (outputs) are on the left, destinations (inputs) on the right. Each side is a
grid: device (one band per node, alternately shaded), port name, a stereo bracket, and a
column of jacks right next to the port names (mirrored on the right). Cables run from
jack centre to jack centre.

- **Jacks**: filled = has at least one cable; ring = free; dimmed = cannot take the cable
  being dragged (wrong type or direction). Audio jacks are warm white, MIDI jacks ash.
  Orange marks only the cable or jack under the pointer, the selected cable, or the
  cable being drawn. (Before this, filled meant audio and ring meant MIDI, which is why
  some looked solid and others hollow regardless of connections.)
- **Patching**: drag from a jack (or port name) to a jack to connect; drag a stereo
  bracket (or Shift-drag a jack of a pair) to patch L→L and R→R in one go. Click a cable
  to select it; Delete/Backspace or its × removes it. Click a jack, then another, as an
  alternative to dragging. No confirmations; Undo (⌘Z / Ctrl+Z) reverts the last 50
  changes.
- **Filters**: All / Audio / MIDI, hide unconnected, show sink monitor outputs, search by
  device or port.
- **Levels**: every audio node shows a meter (Amber). Sinks, capture sources and app
  streams have a volume fader and mute that set the PipeWire node volume through
  `wpctl` in WirePlumber's cubic scale (the % pavucontrol shows), ramped in 6 steps over
  about 100 ms and capped at 100%: the console never boosts a node, so the synth's own
  −6 dBFS cap and limiter still bound what reaches the DAC. The synth's fader is its
  master (−60 to −6 dBFS) and its M fades the engine out. JACK apps (Cabbage/JUCE,
  SuperCollider, Faust under pw-jack) have no PipeWire volume; their meter shows and the
  level is set in the app.
- **Meters**: the synth meters itself. Other nodes are metered by linking pilabd's
  hidden meter inputs to running non-AirPlay sinks' monitor ports and to app and capture
  outputs (up to four stereo pairs, peak per block, no allocation). The links are made
  only while a Signals page polls and removed 15 s after it stops, and never to a
  suspended or AirPlay sink, so metering cannot wake a device or a speaker. A sink with
  no meter of its own shows the peak of the sources patched into it.

### Next: a full mixer (plan, not built)

1. **Channel strips** per source: input gain (pre), mute/solo, pan, 3-band EQ and a
   high-pass, a compressor, post-fader meter. For JACK apps the strip is a small
   pilabd-hosted insert (a JACK in/out pair the app is patched through), so every source
   gets gain even without PipeWire volume.
2. **Buses**: a main bus to the DAC with the existing limiter and soft clip, plus up to
   four aux buses (e.g. reverb, headphones cue, AirPlay feed, recording). Buses are
   pilabd JACK ports, mixed in the realtime callback with the same no-allocation rule.
3. **Sends**: pre/post-fader send levels from each strip to each aux bus.
4. **Scene recall of levels**: scenes store strip, bus and send levels and the patch
   (links) by device class rather than raw port names; loading a scene ramps levels over
   about 200 ms and never raises the DAC path above the current master (same rule as
   today).
5. **Safety**: one output safety stage (limiter, −6 dBFS master cap, start quiet) on
   everything that reaches the DAC, not only the synth.
6. **UI**: the mixer as a fourth panel under Signals (strips in a row, meters Amber,
   one orange element for the active strip), with the patch panel showing buses as
   nodes.

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
risk note, and live state from systemd (and the process table for Faust and scsynth),
refreshed every 6 s and right after a switch.

| Service | Switch | What it runs |
| --- | --- | --- |
| Trillium Synth | yes | fades the engine in or out |
| AirPlay receiver / sender | yes | `pilab-airplay receiver|sender on|off` (the receiver also needs sudo for `nqptp`; if sudo asks for a password the switch reports it) |
| Faust web controls | yes | `faust-gui --no-qt ~/faust/synth.dsp` in the user unit `pilab-faust-gui`; off runs `faust-gui --stop` |
| SuperCollider server | yes | `scsynth -u 57110 -B 127.0.0.1` on CPU 3 in the user unit `pilab-scsynth` (localhost only) |
| SSH | yes, guarded | `sudo -n systemctl enable|disable --now ssh` |
| Console | no | an access path; stop it on the Pi only |
| VNC, mDNS | read-only | |

Switching anything on shows its risk note first. **Access paths** (SSH and the console):
the console is never switched off from the console, so one remote way in always remains.
Turning SSH off opens a dialog that explains that with SSH and the console both off the
only way back in is a local monitor and keyboard or editing the SD card, and it needs the
typed confirmation `turn off ssh` (also enforced by the API).

Overview also has a **Version** panel (console version and git commit, model, OS, kernel,
PipeWire, firmware, bootloader, device id; no serial numbers) and a **Storage** panel
(every disk and partition from `lsblk`: size, used, free, percent, filesystem, mount point,
and the drive temperature where the kernel exposes it through hwmon; no `smartctl`, which
needs root). The listening-ports table comes from `ss` and flags anything undeclared.

Signals lists **audio inputs (sources)** and **audio outputs (sinks)** from PipeWire.
Monitors are not shown as inputs. AirPlay speakers (HomePods, Macs) are outputs only and
labelled so; they appear only while the AirPlay sender is on. USB mics and interfaces show
as capture devices when plugged in.

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
| GET | `/info` | Product, version, commit, build id, device id, pairing requirement (no token needed) |
| POST | `/pair/check` | 200 if the bearer token is valid |
| GET | `/ws?token=` | WebSocket: status frames at 15 Hz, sys frames every 2 s; send {t:'midi',bytes:[..]} |
| GET | `/status` | Voices, peaks, limiter, master, xruns, recording |
| GET | `/system` | Machine state, DAC, backend, services, ports, USB, events, recording and archive status |
| POST | `/services/{id}` | {on:bool, confirm?} switch a service (SSH off needs confirm "turn off ssh"; the console is never switched) |
| GET | `/versions` | Console version and commit, OS, kernel, PipeWire, firmware, bootloader, model, device id |
| GET | `/storage` | Disks and partitions (lsblk) with use and temperature |
| GET | `/audio/devices` | Audio outputs (sinks) and inputs (sources); AirPlay speakers marked output-only |
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
| POST | `/audition/play` | {slot|bank|table|preset} play the ~1.5 s phrase on the DAC at the current master |
| GET | `/audition/render?slot=|bank=|table=|preset=` | The same phrase rendered offline as a WAV |
| GET | `/library/samples` | Installed sample banks |
| GET | `/library/presets` | Named presets (and whether their bank is installed) |
| POST | `/scene/slots/{id}/preset` | {preset} or {bank}: make the slot a sampler; drum kits get a ch 10 route |
| POST | `/engine` | {run:bool} fade the synth in or out |
| POST | `/panic` | All sound off |
| POST | `/master` | {db} master level, capped at -6 dBFS |
| GET | `/midi/inputs` | MIDI input slots and their sources |
| GET | `/library/akwf` | AKWF groups |
| GET | `/library/akwf/{group}` | Files in a group |
| GET | `/library/akwf/{group}/frames?points=` | Every cycle in a group, decimated, for thumbnails |
| GET | `/library/preview?path=&points=` | Raw single cycle for drawing |
| GET | `/graph` | Audio/MIDI ports and links (PipeWire via the JACK API; meter inputs hidden) |
| GET | `/mixer` | Audio nodes with PipeWire volume (cubic 0..1) and mute; links the meters |
| POST | `/mixer/{id}` | {name, volume?, mute?} set a node's volume (ramped, capped at 1.0) or mute; `name` must match the node |
| GET | `/meters` | Peak dBFS per metered device since the last read |
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

### Caching and updates

index.html is served `no-cache` and writes the build id (deployed commit plus a hash of the
embedded files) into every asset URL (`/app.js?v=<build>`). Those versioned URLs are
`public, max-age=31536000, immutable`; anything unversioned (fonts referenced from CSS,
icons set by script, old `?v=` values) is `no-cache` with the build id as ETag, so a
revalidation is a 304. An open tab polls `/info` every minute, on focus and after the
WebSocket reconnects; if the server's build differs from the page's, a "New version
available · Reload" banner appears.

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
# `trillium-base` is the same binary as `pilabctl` (symlink); either name works.
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
