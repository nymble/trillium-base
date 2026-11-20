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
4. Masters are never evicted. A copy pilab holds only as a cache can be deleted; a copy
   that is the only one (a recording made on the unit) cannot, until the archive confirms it.
5. Nothing on the network is mounted automatically. Discovery suggests; the console adds.

## Four tiers

| tier | where | size | what | writable |
| --- | --- | --- | --- | --- |
| a. base | `/usr/share/pilab/library` in the image | budget 256 MiB | starter waveforms, samples, presets, manuals | no |
| b. drive | optional SSD/USB disk at `/srv/library` | 0.5-2 TB | full libraries, music and book shelves, recordings, cache | yes |
| c. network | hosts on the same LAN: NAS, Macs, other pilabs, ZPOD | any | read-only mounts or on-demand rsync | no (pilab side) |
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

### b. Local drive

Optional disk labelled `PILAB-LIB`, ext4, mounted at `/srv/library` with
`nofail,noexec,nodev,nosuid,noatime`. A 1 TB USB 3 SSD covers every source in
`sources.yaml` except big ZIM archives several times over. Layout mirrors the NAS Media
shelf so content can move between the two without renaming:

```text
/srv/library/
  manifest/                  # one manifest per installed pack or mirrored collection
  music/Artist/Album/track   # same as NAS Media/Music: FLAC/ALAC masters, tags are the catalog
  books/Author/Title/        # EPUB/PDF; same shape as NAS Media/Books (to confirm with Librarian)
  sounds/{source-id}/{pack}/ # samples and soundfonts, one folder per pack
  waveforms/{source-id}/
  datasets/{source-id}/
  recordings/YYYY/YYYY-MM-DD_{scene}/   # unique: made on this unit; masters until archived
  presets/                   # user presets and scenes (unique)
  cache/                     # evictable copies of network items
```

Without a drive, `cache/` falls back to `/var/cache/pilab/library` with a 2 GiB cap.

### c. Network sources on the same LAN

pilab and its sources can be anywhere on the same network. Supported root kinds:

- `smb`: NAS shares, read-only, SMB 3.1.1 with signing and encryption. Mounted on demand
  by systemd automount under `/mnt/net/{root-id}` and unmounted after idle.
- `nfs`: NFSv4 only (no rpcbind needed), read-only, for NAS exports when available.
- `ssh`: rsync over SSH from a Mac or Linux host. Nothing is mounted; files are fetched.
  Host key pinned. Preferred for laptops.
- `pilab`: another pilab or a ZPOD serving its manifests and files read-only over HTTPS
  with a pinned certificate after pairing.

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
license: CC0-1.0
credits: ""                                 # filled for attribution licences
types: [samples]
root: base                                  # root id where these bytes live on this unit
path: samples/freepats/upright-piano-kw-small
role: copy                                  # master | mirror | copy | cache
pinned: true                                # never evicted while pinned
bytes: 3460000
file_count: 92
files: files.tsv                            # path<TAB>bytes<TAB>sha256<TAB>media-type
sha256_of_files_tsv: "<hex>"
kb_record: ""                               # kb library/{id}, set when Librarian catalogues it
holdings:                                   # where else verified copies are known to be
  - nas://NAS-4x12/Media/Sounds/freepats/upright-piano-kw-small   # example only
dc:
  title: Upright Piano KW (small)
  type: Collection                          # DCMI Type
  format: audio/flac
  rights: CC0-1.0
  source: https://freepats.zenvoid.org/
  identifier: pilab://pilab.local/base/samples/freepats/upright-piano-kw-small
  date: "2019"
  subject: [sound, redownloadable, media]   # kb concept ids
```

Crosswalk to kb record fields (kb `docs/ontology.md`):

| pilab manifest | kb record | Dublin Core |
| --- | --- | --- |
| `id` | `id` | |
| `title` | `title` | `dc:title` |
| `dc.identifier` (`pilab://host/root/path`) | `path` / `sources` | `dc:identifier` |
| `holdings` (NAS) | `nas_path` | `dc:identifier` |
| `bytes`, `file_count` | `bytes`, `file_count` | `dc:extent` |
| `dc.type` | `dc.type` | `dc:type` |
| `dc.format` | `dc.format` | `dc:format` |
| `dc.subject` | `concepts` | `dc:subject` |
| `license`, `credits` | (new: `rights`) | `dc:rights` |
| `role: master` | concept `unique` | |
| `role: copy/mirror/cache` | concept `redownloadable` | |

The `pilab://` identifier is a URI so kb can point at pilab holdings the same way it points
at `file://` and `nas://` paths. Dedup is by sha256, as in kb.

### pilab as a partial store and archive customer

- **Partial store.** pilab holds copies of some collections (base packs, drive packs, a
  mirrored slice of the music shelf). Its manifests list what it holds and the sha256 of
  every file. kb can record pilab as one more holding of a collection.
- **Archive customer.** Recordings, presets and scenes made on pilab are unique. They are
  `role: master` until a verified copy exists on NAS `Archive`; then they become `copy` and
  may be evicted. Proposed NAS home: `Archive/topics/{topic-id}/` per the kb folder
  contract, with the topic to be chosen by Librarian. Bytes move only after a kb record is
  `accepted`, same as every other archive move.

## Moving content on and off a unit

All moves use rsync (installed on the Pi) with checksums verified against the manifest.

| command | does |
| --- | --- |
| `pilab lib ls [--root R] [--type T]` | list packs and collections across roots, with licence and role |
| `pilab lib fetch <id> [--to drive]` | rsync from the best available root (drive, then LAN, then internet pack), verify sha256, write manifest |
| `pilab lib pin <id>` / `unpin` | keep or release a copy against eviction |
| `pilab lib evict <id>` | delete a local copy if it is `copy` or `cache`, unpinned, and a verified holding exists elsewhere |
| `pilab lib push <id> --to <root>` | rsync a master (recordings) to an archive root; mark `copy` after the remote checksum matches |
| `pilab lib verify [<id>]` | re-hash files against manifests; report drift |
| `pilab lib gc` | evict least-recently-used cache items until under `cache_budget_gib` |

Fetch picks a source in this order: already on drive, LAN roots allowed by the current
profile, internet pack. Eviction never touches `master`, `pinned`, or anything without a
verified holding elsewhere.

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
  role (master, copy, cache), pinned, size, where else it is held.
- Actions with size and licence shown before confirming: fetch, pin, evict, push, verify.
- Budget: base image used against 256 MiB; drive used; cache used against budget.
- Filters: licence (e.g. "CC0 only" for a public install), type, root, role.
- Warnings: unverified licence, NC item in a venue scene, master not yet archived,
  network root on a non-home profile.

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

## Open questions

- Drive size and model to buy (1 TB suggested).
- Whether pilab may mount the NAS Media share read-only, with a separate read-only NAS account.
- Whether Media gets a `Sounds/` shelf for sample libraries, and the `Books/` layout.
- Which kb topic pilab records link to, and the NAS Archive home for pilab recordings.
- Repo licence for presets and our own content (none is set yet).
