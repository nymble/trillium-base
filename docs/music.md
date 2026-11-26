<!-- Copyright (C) 2026 Paul Lambert. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Music: library and player (v1)

The console's **Music** page browses a music library and plays it on the unit, through the
DAC, beside the synth. The browser is the remote; sound comes out of the Trillium Base.

## Pieces

| Piece | What it is | Where |
| --- | --- | --- |
| Library index | `pilab-music` crate: sources, scanner, SQLite + FTS5 index, cover-art cache, M3U lists | `rust/crates/pilab-music` |
| API | `/api/v1/music/...` in pilabd (list: `GET /api/v1/endpoints`) | `rust/crates/pilabd/src/music.rs` |
| CLI | `trillium-base music ...` | `rust/crates/pilabd/src/music_cli.rs` |
| Player | MPD, a separate GPL process (`pilab-mpd.service`), driven over its private UNIX socket with the `zpod-mpd` client; pilabd does not link MPD | `home/.config/pilab/mpd.conf.in`, `home/.config/systemd/user/pilab-mpd.service` |
| Page | `web/music.js`, `web/music.css`; one nav entry | `rust/crates/pilabd/web` |
| Demo content | Three CC0 albums from the Internet Archive and four radio links | `scripts/fetch-music-demo.sh`, `home/.config/pilab/music/streams.m3u`, `library/ATTRIBUTION.md` (id `music-demo`) |

## Sources and collections

Sources are listed in `~/.config/pilab/music/sources.toml` (defaults below when the file is
absent; `PUT /api/v1/music/sources` writes it). Every source is read-only: the scanner never
writes to music files.

| id | kind | path | collection |
| --- | --- | --- | --- |
| demo | folder | `~/.local/share/pilab/music/demo` | music |
| local | folder | `~/Music` | music |
| drive | folder | `/srv/library/music` (the USB SSD, when mounted) | music |
| recordings | folder | the recorder's folder (`[recording] location`, default `~/.local/share/pilab/recordings`) | recordings |
| drive-recordings | folder | `/srv/library/recordings` | recordings |
| radio | urls | `~/.config/pilab/music/streams.m3u` | music |

Two collections never mix: **Music** (published releases) and **Recordings** (takes made on
this unit, with their record cards). A missing optional source is "not connected" and keeps
its last index, so unplugging the drive does not empty the library until the next scan with
the source removed.

## Index

`~/.local/state/pilab/music/library.sqlite` (WAL). Holdings (one row per file: path, size,
mtime, codec, rate, bit depth, channels, duration, tags as JSON) are kept between scans;
tags are read again only when size or mtime change. Releases, artists, tracks, takes,
streams and the search table are rebuilt from holdings after every scan, with stable ids
(track id = holding id; release, artist and stream ids are hashes), so playlists keep working
across rescans. Tables for later library management are created empty now: `override`
(edits), `merge`, `dup_candidate`, `dup_decision`, and `holding.sha256` / `fingerprint`.

Grouping and fallbacks:

- Release = MusicBrainz release id if tagged, else album + album artist, else album + folder.
- Release artist = album artist, else the one track artist, else "Various Artists".
- Untagged files: title and number from `NN - Title`, `NN. Title`; album and artist from an
  `Artist - Album` folder name.
- MusicBrainz ids (recording, track, release, release group, artist) are read and stored.
- Cover art: `cover|folder|front|album|albumart|0cover.jpg|png` in the album folder (or the
  only image there), else the first embedded picture. Thumbnails: 300 and 600 px square JPEGs
  in `~/.cache/pilab/music-art`.
- Search: FTS5, prefix match per word, accents ignored (`beyon` finds Beyoncé).

The scan runs on a background thread at nice 15 on CPUs 0-2 (pilabd's affinity; CPU 3 is
the synth's), holds while a recording is running, writes in short transactions, starts 15 s
after pilabd starts, and re-runs when the recordings folder changes.

## Playback and levels

- MPD plays `file://` paths from the index (local socket clients only) and stream URLs. It
  keeps no database of its own and listens only on `~/.local/state/pilab/mpd/socket`
  (`MPD_HOST` in `pilabd.service`). Debian's system and user `mpd` units are masked so no
  other MPD opens the DAC.
- Output: MPD's PipeWire stream is started with `node.autoconnect = false`, so WirePlumber
  never links it. pilabd (`music_out.rs`) links it **by node name** to the outputs picked
  on the Music page: the DAC by default, or a USB interface, AirPlay speakers (from the
  AirPlay sender), or several at once. A picked output that is unplugged is skipped; if none
  is left, music falls back to the DAC. Plugging in an interface never takes the music (or
  the synth) over. The DAC sink level (0.70) and the synth master (−18 dB) are not touched.
  See "Outputs" below.
- Volume is MPD's software volume: a fresh player starts at **40%**, and pilabd holds it at
  or below **80%** (`DEFAULT_VOLUME`, `MAX_VOLUME` in `music.rs`); a higher request is capped
  and reported. ReplayGain and normalisation are off.
- Bit-perfect, honestly: the PipeWire graph stays at 48 kHz and quantum 64 for the synth.
  MPD's output is fixed at 48 kHz float and kept open (`always_on`): a 48 kHz source at 100%
  player volume reaches PipeWire unchanged (float holds 16/24-bit samples exactly); other
  rates (44.1, 96 kHz) are resampled inside MPD by soxr ("very high"), outside the real-time
  graph, and any volume below 100% scales samples. The status line shows the path
  (`44100:16:2 → resampled to 48 kHz by the player`). A bit-perfect mode (rate follows the
  source while the synth is idle, volume fixed at 100%, level set on the DAC) is a later step.
- Keeping the synth clean: MPD runs with `CPUAffinity=0-1` (off the synth's CPU 3 and the
  PipeWire driver's usual CPU 2), `Nice=10` and idle I/O. Measured on the Pi 5 (quantum 64):
  with the first config (output reopened per rate, CPUs 0-2) about one synth xrun per
  play/stop/format change; with the fixed 48 kHz output kept open, about 1 in 16 and none
  in steady playback. `node.async` was tried (2026-11-25) and removed: after a night idle the
  player's stream errored on every graph cycle, MPD reported "play" at 0:00 and nothing was
  heard (2026-11-26).
- Watchdog: if MPD reports "play" but the position has not moved for 8 s, pilabd reopens
  the player output; after 20 s it restarts `pilab-mpd` and resumes at the same place (at
  most once every 2 minutes). The Music page shows a note when this happens.

## Outputs

`GET /api/v1/music/outputs` lists every PipeWire sink with its kind (`dac`, `usb`,
`airplay`, `bluetooth`, `hdmi`, `other`), channels, whether it is chosen, used (chosen and
present) and active (linked now), the USB stream capabilities from
`/proc/asound/cardN/stream0`, and any USB sound card the kernel has that PipeWire has not
set up. `PUT {outputs:[node.name, ...]}` chooses (saved in
`~/.config/pilab/music/outputs.json`; empty = the DAC; up to 4). The player bar shows the
output chip ("Allo Katana · 2 ch"); tapping it opens the picker. CLI:
`trillium-base music outputs`, `trillium-base music output dac|NAME..` (name, or the start
of the label, e.g. `output scarlett kitchen`).

Several outputs play the same stream. They are not sample-locked: a USB interface joins the
DAC's graph as a resampled follower, and AirPlay adds about 2 s. Use one output for
critical listening.

## Deleting recordings

Recordings → ⋯ → Move to Trash (confirm, then Undo in the toast). The take (its folder:
WAV and `record.toml`) moves to `.trash/` inside the recordings folder it came from, a
rename on the same disk. Recordings → Trash lists it with Restore; Empty Trash deletes for
good and needs the word `delete` typed. Only sources in the Recordings collection qualify;
anything in the Music collection is refused. CLI: `trillium-base music take rm ID`,
`music trash`, `music trash restore ID`, `music trash empty --yes`. API:
`DELETE /music/takes/{id}`, `GET /music/trash`, `POST /music/trash/{id}/restore`,
`DELETE /music/trash/{id}` and `POST /music/trash/empty` (both `{confirm:"delete"}`).

## Console

Music (between Listen and Record): Albums (grid, sort by artist, title, year, recently
added), Artists, Songs, Playlists, Recordings, Radio; search across all; album, artist and
playlist pages; Play, Shuffle, Play next, Add to queue, Add to playlist; a now-playing bar
(transport, seek, volume, shuffle, repeat, output format) and a queue drawer. Library shows
the sources, their state and file counts, and Scan now. Touch targets are at least 44 px on
touch screens (iPad).

## CLI

```text
trillium-base music status | scan [--wait] | sources | albums | artists | songs | takes | streams
trillium-base music search TEXT..        play album|artist|track|take|stream|playlist ID
trillium-base music play TEXT..          play url URL | add KIND ID | next-up KIND ID
trillium-base music queue [clear] | toggle | pause | resume | stop | next | prev | seek SECS | vol N
trillium-base music repeat on|off | shuffle on|off | stream add NAME URL | stream rm ID | playlists
```

## Tests

- `cargo test -p pilab-music`: schema, scan (tags, fallbacks, MusicBrainz ids, art),
  incremental rescan, offline and removed sources, search, playlists, URIs, M3U, sources,
  pause during recording. `PILAB_MUSIC_DEMO=DIR` also scans the real demo albums.
- `python3 scripts/music-smoke.py [--port N] [--no-play]`: API smoke test against a running
  pilabd (reads the token from `~/.config/pilab/console-token`, never prints it); plays a few
  seconds at the default volume, checks the cap, then stops and clears the queue.

## Later (see the library plan)

Tag editing (overrides with undo), merge, duplicate detection and decisions, Apple Music
import/export, smart playlists, playing on the iPad itself, multi-room, publishing.
