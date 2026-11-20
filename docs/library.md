# pilab library: sources, tiers, roots and manifests

pilab is a fixed-size distribution and also a small local archive. This document says
what ships in the image, where bigger collections live, how content moves on and off a
unit, and how pilab fits the kb catalog (kb `docs/ontology.md` and `docs/file-archive.md`).

Files:

| file | what it is |
| --- | --- |
| `library/sources.yaml` | every third-party source we ship, fetch or index, with licence, size and status |
| `library/ATTRIBUTION.md` | credits, generated from `sources.yaml` |
| `library/base-pack.yaml` | exact base-image contents and the size budget |
| `library/roots.example.yaml` | example library roots registry |
| `scripts/library-attribution.py` | regenerates `ATTRIBUTION.md` |

## Rules

1. Records and typed links, not copies. pilab never copies third-party bytes into this repo.
   The repo holds manifests; bytes come from the source at build or install time.
2. Every byte traces to a `sources.yaml` entry. Unknown licence means it does not install.
3. The base image is CC0-only (plus our own repo content), so its credits are short and
   carry no personal names. Attribution-licensed packs install their own CREDITS file.
4. A pilab copy is a working mirror, never the master, and never counts as a backup.
   The NAS is canonical for anything mirrored from Media. The one exception: recordings
   made on pilab are masters until they are filed on NAS Archive, and only until then.
5. Masters are never evicted. Mirrors and cache copies can be deleted at any time because
   the canonical copy is elsewhere.
6. Nothing on the network is mounted automatically. Discovery suggests; the console adds.
7. pilab never formats, repartitions or relabels a disk. It works inside its own
   `pilab-library/` folder on whatever drive is attached.
8. Licences are recorded, never guessed. Unknown means `LicenseRef-unknown`.

## Four tiers

| tier | where | size | what | writable |
| --- | --- | --- | --- | --- |
| a. base | `/usr/share/pilab/library` in the image | budget 256 MiB | starter waveforms, samples, presets, manuals | no |
| b. drive | `pilab-library/` folder on an attached existing disk, exposed at `/srv/library` | whatever is free | sound packs, books, recordings, cache | yes, inside `pilab-library/` only |
| c. network | hosts on the same LAN: NAS (canonical), Macs, other pilabs, ZPOD | any | read-only mounts or on-demand rsync; one push target for recordings | read-only, except the recordings push |
| d. external | internet, through the pack installer | any | packs fetched with checksum and licence check | no |

### a. Base image (fixed size)

Budget: **256 MiB** unpacked, enforced by the image build. Current plan is about 185 MiB,
leaving about 70 MiB headroom. Exact list in `library/base-pack.yaml`:

| group | source | size |
| --- | --- | --- |
| AKWF original single-cycle set, 4,358 waveforms | akwf (CC0) | 6.1 MiB |
| 28 small FreePats banks: 2 pianos, FM piano, drawbar organ, accordion, nylon and electric guitar, bass, ukulele, harp, kalimba, clarinet, tenor sax, ocarina, xylophone, tubular bells, timpani, world and synth percussion, 9 synth banks | freepats-cc0 (CC0) | 122 MiB |
| VCSL starter kit: kick, snare, toms, hi-hat, cymbals, claps and 8 hand percussion sets, converted to FLAC | vcsl (CC0) | ~45 MiB (76.5 MiB as WAV) |
| Named presets per synthesis family, scenes, demo patterns | pilab | ~4 MiB |
| pilab manuals plus up to five public domain EPUBs | pilab, standard-ebooks | ~8 MiB |

Why 256 MiB: the whole image should fit and flash quickly on a 16 GB card, survive many
rebuilds, and still play a full General-MIDI-style range out of the box (keys, plucked,
winds, mallets, drums, pads, leads) plus the wavetable oscillator. Anything bigger is a pack.

### b. Local drive (existing, non-empty disk)

The drive is one of the owner's existing disks with other data on it. It is not empty and
is never reformatted. Rules:

- Mounted by filesystem UUID at `/mnt/pilab-drive` with `nofail,noexec,nodev,nosuid,noatime`.
  The UUID is pinned the first time the owner adds the root in the console; another disk
  in the same port is ignored until added.
- pilab creates and uses one folder, `/mnt/pilab-drive/pilab-library/`, bind-mounted to
  `/srv/library`. It never reads, lists, moves or deletes anything outside that folder.
  The console shows the rest of the disk only as used space.
- No format, repartition, relabel, `fsck -y`, or ownership change of the disk root. If the
  filesystem cannot be mounted safely read-write on Linux (e.g. APFS, or NTFS/exFAT needing
  a journal replay), the console offers read-only use and says why.
- On exFAT or NTFS there are no Unix permissions or symlinks; the layout below uses neither.
- The cache budget is set against free space on the disk, not its size, and pilab stops
  writing when free space falls below a floor (default 10 % of the disk or 20 GiB,
  whichever is smaller) so the owner's other data always has room.

Layout inside `pilab-library/` mirrors the NAS Media shelves so content moves between them
without renaming:

```text
/srv/library/  (= /mnt/pilab-drive/pilab-library/)
  manifest/                  # one manifest per installed pack or mirrored collection
  sounds/{library}/          # mirrors NAS Media/Sounds/{library}/ (VCSL, FreePats, ...)
  waveforms/{source-id}/
  datasets/{source-id}/
  books/                     # flat per-pack folders for now; NAS Media/Books layout is unconfirmed
  music/                     # empty for now; see "Music" below
  recordings/{record-id}/    # unique masters until filed on NAS Archive
  presets/                   # user presets and scenes
  cache/                     # evictable copies of network items
```

Without a drive, `cache/` falls back to `/var/cache/pilab/library` with a 2 GiB cap, and
recordings go to `/var/lib/pilab/recordings/` on the SD card with a size warning.

### c. Network sources on the same LAN

pilab and its sources can be anywhere on the same network. Supported root kinds:

- `smb`: NAS shares, read-only, SMB 3.1.1 with signing and encryption. Mounted on demand
  by systemd automount under `/mnt/net/{root-id}` and unmounted after idle. Approved:
  NAS `Media` read-only through a separate read-only NAS account (never the owner's own
  login), on the `home` profile only. This is the main remote-files use case.
- `nfs`: NFSv4 only (no rpcbind needed), read-only, for NAS exports when available.
- `ssh`: rsync over SSH from a Mac or Linux host. Nothing is mounted; files are fetched.
  Host key pinned. Preferred for laptops.
- `pilab`: another pilab or a ZPOD serving its manifests and files read-only over HTTPS
  with a pinned certificate after pairing.

### NAS shelves pilab uses

| NAS path | pilab use | notes |
| --- | --- | --- |
| `Media/Sounds/{library}/` | read-only source; mirror into `sounds/{library}/` | approved shelf for third-party sample libraries (VCSL, FreePats). One kb collection card per library with `dc.rights` and source URL. Lowest backup priority: re-downloadable. |
| `Media/Books/` | read-only browsing only | layout unconfirmed (intended Calibre `Author/Title/`). Nothing in pilab depends on its shape; treat it as a tree of files and read metadata from the files. |
| `Media/Music/` | not mirrored yet | currently unsorted. Mirror slices only once the clean MusicBrainz-tagged `Artist/Album/` FLAC tree exists. Until then the console can browse it read-only but offers no mirror action. |
| `Archive/topics/pilab/recordings/{record-id}/` | push target for recordings | the only NAS path pilab writes to; separate account, disabled until approved |

### d. External sources

Fetched only by the pack installer (`pilab lib install <pack>`):

1. Read the pack index (signed by us) and its manifest.
2. Check the pack's `source` exists in `sources.yaml` with status `shippable` or
   `allowed-not-shipped`. Check every item's licence is allowed (e.g. Freesound: CC0 and
   CC-BY only, never NC or Sampling+).
3. Download to `cache/`, verify sha256 per file, unpack, write the pack's CREDITS,
   move into place, write `manifest/{pack}.yaml`.

## Library roots registry

The console manages `/etc/pilab/library/roots.yaml` (example: `library/roots.example.yaml`).
Each root:

| field | meaning |
| --- | --- |
| `id` | kebab-case, unique on the unit |
| `kind` | `base`, `drive`, `smb`, `nfs`, `ssh`, `http`, `pilab` |
| `path` / `url` / `host` / `share` | where it is |
| `types` | which library types it holds: waveforms, samples, soundfont, dataset, books, music, recordings, presets |
| `read_only` | pilab never writes to it |
| `mount` | `at-boot`, `on-demand`, `never` (fetch only) |
| `trust` | `image`, `owned`, `home`, `venue`, `public` |
| `network_profiles` | which network profiles may activate it |
| `credentials_ref` / `host_key_ref` / `pinned_cert_ref` | names of root-only secret files, never the secret |
| `cache_budget_gib` | for writable roots: how much the cache may use |
| `risk` | one-line note the console shows when the root is turned on |

Availability is computed, not stored: `online`, `offline`, `blocked-by-profile`,
`auth-failed`, `key-mismatch`.

### Network profiles and venue risk

The unit picks a network profile from facts it can check: Ethernet only (Wi-Fi off),
gateway MAC address, and a pinned identity from a known host (NAS SMB server or a home
Mac's SSH host key). Profiles:

- `home`: all facts match. Home roots may mount.
- `venue`: anything else. Only `base`, `drive` and paired `pilab` peers. No credentials leave
  the unit, no SMB mounts, no automatic discovery replies beyond what the scene enables.
- `offline`: no network. Base and drive only.

Rules: never auto-mount home shares on an unknown network; never store the owner's main
NAS password (use a read-only NAS account); pinned host keys and certificates, refuse on
change; mounts are `ro,nosuid,nodev,noexec`. Each root shows its risk line in the console
when switched on, same style as other service toggles.

## Content manifest

One manifest per pack or mirrored collection, at `{root}/manifest/{id}.yaml`. One manifest
is one bundle, matching kb's "one card per bundle, never one card per file". Per-file data
goes in a sidecar TSV so the YAML stays readable.

```yaml
schema: pilab.manifest/v1
id: freepats-upright-piano-kw-small        # kebab-case, matches the kb record id if catalogued
title: Upright Piano KW (small)
source: freepats-cc0                        # id in library/sources.yaml
upstream: https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html
version: "20190703"
license: CC0-1.0                            # SPDX id; LicenseRef-proprietary or LicenseRef-unknown otherwise
credits: ""                                 # filled for attribution licences
types: [samples]
root: base                                  # root id where these bytes live on this unit
path: samples/freepats/upright-piano-kw-small
role: mirror                                # master (recordings only, until filed) | mirror | cache
pinned: true                                # never evicted while pinned
bytes: 3460000
file_count: 92
files: files.tsv                            # path<TAB>bytes<TAB>sha256<TAB>media-type
sha256_of_files_tsv: "<hex>"
kb_record: ""                               # kb library/{id}, set when Librarian catalogues it
canonical: nas://NAS-4x12/Media/Sounds/freepats/upright-piano-kw-small   # the master; empty only for unfiled recordings
holdings:                                   # other verified copies (pilab copies are never backups)
  - pilab://pilab.local/base/samples/freepats/upright-piano-kw-small
dc:
  title: Upright Piano KW (small)
  type: Collection                          # DCMI Type
  format: audio/flac
  rights: CC0-1.0                           # optional; SPDX id, LicenseRef-proprietary, LicenseRef-unknown; never guessed
  source: https://freepats.zenvoid.org/
  identifier: pilab://pilab.local/base/samples/freepats/upright-piano-kw-small
  date: "2019"
  subject: [sound, redownloadable, media]   # kb concept ids
topics: [pilab]                             # kb topic
```

Crosswalk to kb record fields (kb `docs/ontology.md`):

| pilab manifest | kb record | Dublin Core |
| --- | --- | --- |
| `id` | `id` | |
| `title` | `title` | `dc:title` |
| `dc.identifier` (`pilab://host/root/path`) | `path` / `sources` | `dc:identifier` |
| `canonical` (NAS) | `nas_path` | `dc:identifier` |
| `holdings` (`pilab://`) | `sources` | `dc:identifier` |
| `bytes`, `file_count` | `bytes`, `file_count` | `dc:extent` |
| `dc.type` | `dc.type` | `dc:type` |
| `dc.format` | `dc.format` | `dc:format` |
| `dc.subject` | `concepts` | `dc:subject` |
| `license` / `dc.rights` | `dc.rights` (optional, SPDX id) | `dc:rights` |
| `topics` | `topics` (`pilab`) | |
| `role: master` (unfiled recording) | concept `unique` | |
| `role: mirror/cache` | concept `redownloadable` | |

Rights values: an SPDX id when the licence is known (`CC0-1.0`, `CC-BY-4.0`,
`GPL-3.0-or-later`); `LicenseRef-proprietary` for commercial material; `LicenseRef-unknown`
when nobody has checked. Never guess. The pack installer refuses `LicenseRef-unknown`
items, and the console filters on this field.

`pilab://` is an accepted kb holding scheme next to `file://` and `nas://`, in the form
`pilab://{host}/{root-id}/{path}`. A `pilab://` holding is a working mirror: it is never the
master and never counts toward the 3-2-1 copies. Dedup is by sha256, as in kb.

### pilab as a partial store and archive customer

- **kb topic.** pilab records link to the kb topic `pilab` (related to
  `generative-music-languages`).
- **Partial store.** pilab holds working mirrors of some collections (base packs, sound
  libraries mirrored from `Media/Sounds`). Its manifests list what it holds with a sha256
  per file. kb may list pilab as a holding; the NAS stays canonical.
- **Archive customer.** Recordings made on pilab are the one kind of master pilab holds,
  and only until filed. Each recording gets a record id and is pushed promptly to
  `nas://NAS-4x12/Archive/topics/pilab/recordings/{record-id}/`. After the remote checksum
  matches, its role becomes `mirror` and it may be evicted. Until then pilab holds the only
  copy, and the console says so.
- **Third-party sample libraries** on `Media/Sounds/{library}/` get one kb collection card
  each, with `dc.rights` and the source URL. They are the lowest backup priority because
  they can be downloaded again.

### Unfiled masters

The console keeps an "Unfiled masters" panel and a count badge on the home screen: every
recording with `role: master`, its size, age, and where it is (drive or SD card). The badge
turns to a warning after 24 hours, or immediately if the unit switches to a `venue` profile
or the drive is close to its free-space floor. Removing the drive or flashing the card with
unfiled masters present asks for confirmation.

## Moving content on and off a unit

All moves use rsync (installed on the Pi) with checksums verified against the manifest.

| command | does |
| --- | --- |
| `pilab lib ls [--root R] [--type T]` | list packs and collections across roots, with licence and role |
| `pilab lib fetch <id> [--to drive]` | rsync from the best available root (drive, then LAN, then internet pack), verify sha256, write manifest |
| `pilab lib pin <id>` / `unpin` | keep or release a copy against eviction |
| `pilab lib evict <id>` | delete a local copy if it is `mirror` or `cache`, unpinned, and the canonical copy is verified |
| `pilab lib push <id> --to nas-archive` | rsync a recording to `Archive/topics/pilab/recordings/{record-id}/`; mark `mirror` after the remote checksum matches |
| `pilab lib unfiled` | list recordings still held only on this unit |
| `pilab lib verify [<id>]` | re-hash files against manifests; report drift |
| `pilab lib gc` | evict least-recently-used cache items until under `cache_budget_gib` |

Fetch picks a source in this order: already on drive, LAN roots allowed by the current
profile, internet pack. Eviction never touches `master`, `pinned`, or anything whose
canonical copy is not verified.

## Discovery on the LAN

Avahi is installed and running on the Pi. The console browses mDNS for `_smb._tcp`,
`_nfs._tcp`, `_sftp-ssh._tcp` and our own `_pilab-lib._tcp` (a pilab or ZPOD advertising a
read-only manifest endpoint). Results show as "found, not added". Adding one asks for the
root fields and shows its risk line. Advertising `_pilab-lib._tcp` is off by default and a
per-scene toggle, because it tells a venue network what the unit holds.

## What the console shows

- Roots: id, kind, online/offline/blocked, trust badge, read-only badge, used and free
  space, cache budget bar, risk line, mount and unmount button.
- Library: packs and collections by type and tier, licence badge (CC0, CC-BY, GPL, NC),
  role (master, mirror, cache), pinned, size, where else it is held.
- Actions with size and licence shown before confirming: fetch, pin, evict, push, verify.
- Budget: base image used against 256 MiB; drive used; cache used against budget.
- Filters: licence (e.g. "CC0 only" for a public install), type, root, role.
- Warnings: unverified or `LicenseRef-unknown` licence, NC item in a venue scene,
  unfiled masters (see above), drive near its free-space floor, network root on a
  non-home profile.
- Drive: only the `pilab-library/` folder is shown; the rest of the disk is shown as
  "other data, not touched".

## ZPOD

ZPOD uses the same `pilab.manifest/v1` format and sources list. Differences:

- Small SD storage, so its base pack is a profile of ours: AKWF set, a few tiny FreePats
  banks at 44.1 kHz, a handful of books, and playback copies of music (AAC or Ogg), never
  masters.
- It adds and removes books and music by fetching packs from a pilab or the NAS through
  the same commands, with a smaller cache budget.
- Its own unique content (recordings, presets) is `role: master` and syncs to pilab or the
  NAS Archive the same way.
- Firmware and app packs can share the pack index and signature format later.

## Decisions log

- 2026-11-19: `pilab://` accepted as a kb holding scheme; pilab copies are working mirrors,
  never masters, never backups. Recordings are masters only until filed.
- 2026-11-19: `dc.rights` optional, SPDX id; `LicenseRef-proprietary` or
  `LicenseRef-unknown` otherwise; never guessed.
- 2026-11-19: kb topic `pilab`, related to `generative-music-languages`.
- 2026-11-19: recordings filed to `Archive/topics/pilab/recordings/{record-id}/`.
- 2026-11-19: `Media/Sounds/{library}/` approved for third-party sample libraries.
- 2026-11-19: `Media/Books` layout unconfirmed; do not depend on it.
- 2026-11-19: do not mirror `Media/Music` until the clean tagged FLAC tree exists.
- 2026-11-19: pilab may mount NAS `Media` read-only with a separate read-only account.
- 2026-11-19: the drive is an existing non-empty disk; use `pilab-library/`, never format.

## Open questions

- Which existing disk goes on pilab, and its filesystem (ext4, exFAT, NTFS, APFS).
- A NAS account for the recordings push, limited to `Archive/topics/pilab/recordings/`.
- Repo licence for presets and our own content: decided 2026-11-20, GPL-3.0-or-later with a commercial option (LICENSE, LICENSE-COMMERCIAL.md). Recordings stay `LicenseRef-proprietary` unless filed otherwise.
