# ZPOD convergence

Trillium Synth (in this repository, nymble/pilab) reuses ZPOD crates instead of forking them. This page records what is
used, what pilab adds on its side, and the API changes proposed to ZPOD so the two code
bases converge. Nothing in the ZPOD crates is modified here.

## Pin

`rust/Cargo.toml`:

```toml
zpod-voices     = { git = "https://github.com/nymble/zpod", rev = "2dedb10863d3d9dbc6d312504faaafaf637c71e0" }
zpod-midi       = { git = "https://github.com/nymble/zpod", rev = "2dedb10..." }
zpod-music-core = { git = "https://github.com/nymble/zpod", rev = "2dedb10..." }
```

Tag `licenses-dual-mofn` (commit `2dedb10`, branch `test8`). At this revision the crates
are dual licensed, `GPL-3.0-or-later OR LicenseRef-Commercial`, copyright Paul Lambert, the same
terms as this repository; each crate has a LICENSE file pointing at ZPOD's root LICENSE
(GPL-3.0) and LICENSE-COMMERCIAL.md. Earlier revisions, including the previous pin
`525bc84` (branch `pipit-seq`) and older tags, are MIT, so the pin must not move back to
them. The move from `525bc84` needed no code change here; zpod-midi gained an `input`
module that this repository does not use yet.

The Pi has no GitHub credentials, so `scripts/deploy-console.sh` sends a git bundle of the
pinned commit and `install-console.sh` resolves the URL to a local mirror through
`url.<mirror>.insteadOf` for the build only.

## Reused as is

| Crate | Item | Used for |
| --- | --- | --- |
| zpod-voices | `dsp::Adsr` | Amp and filter envelopes per voice |
| zpod-voices | `dsp::Svf` | Per-voice low-pass filter |
| zpod-voices | `dsp::sat` | Soft clip after the limiter |
| zpod-voices | `dsp::saw`, `dsp::square`, `dsp::blep` | `OscMode::Blep` oscillators |
| zpod-midi | `encode::Msg` | Encoding console notes, bend and CC (`POST /midi`, WebSocket) |
| zpod-music-core | `harmony::note_name_parts` | Note names in the MIDI monitor |

## Adapted or added on the pilab side

- **Wavetable oscillator** (`pilab-synth/src/wavetable.rs`): mipmapped Fourier tables at
  third-octave levels, used by default for saw, square, pulse, triangle and AKWF tables.
  ZPOD's two-point polyBLEP measures about −23 to −36 dB alias at high notes; the tables
  measure −77 to −115 dB. polyBLEP stays available as `OscMode::Blep`.
- **polyBLAMP triangle** next to ZPOD's saw and square.
- **Voice engine** (`engine.rs`): 32 voices across 16 slots, with routing, layering, sustain
  and stealing. ZPOD's `rack` is built around sequencer tracks with `MAX_POLY = 4`.
- **Audio output**: ZPOD writes to an `aplay` pipe. pilab has its own `AudioBackend`
  trait with a JACK-API backend (PipeWire through `pw-jack`) and a null backend.
- **MIDI input**: through JACK MIDI ports on the PipeWire graph. ZPOD's MIDI input is
  coming on branch `test7-r1`.
- **Output safety**: start level, ramps, DC blocker and limiter, ahead of `sat`.

## Proposed ZPOD API changes

1. **`AudioOutput` trait** in zpod-voices (or a small `zpod-audio` crate):
   `fn start(&mut self, sr: f32, block: usize, render: impl FnMut(&mut [f32], &mut [f32]) + Send)`,
   with the `aplay` pipe as one implementation. pilab would contribute the PipeWire/JACK
   implementation behind a feature flag, so ZPOD on a Pi uses the same real-time path.
2. **Configurable voice budget**: make `MAX_POLY` (4) and the per-rack voice count const
   generics or a builder parameter. pilab needs 32 voices; ZPOD on a Zero stays at 4.
3. **Wavetable voice kind**: add `Kind::Wavetable { table: TableId }` to `def::Kind`, with a
   mipmapped table type (the pilab `Wavetable` can move into zpod-voices unchanged; it is
   `no_std`-friendly except for table construction). Banks could then name AKWF cycles.
4. **Allocation-free block render with a block-size parameter**: `Rack::render` already
   avoids allocation; documenting that and adding a test like pilab's counting allocator
   makes it a contract.
5. **MIDI input type**: when `test7-r1` lands, share one event type between the zpod-midi
   router and the pilab engine (`{port, len, bytes[3]}` today on both sides) so routes can
   move between the two.
6. **Split zpod-music-core's dependencies**: it pulls in `zpod-mpd` and `libc` even when only
   the harmony tables are needed. A `harmony`-only feature would keep synth builds lean.
7. **Shared scene and route format**: pilab's `pilab-scene` v1 TOML (slots plus routes) and
   ZPOD's `pipit-voices` banks could share one route schema so the console manages both.
