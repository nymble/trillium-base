# Library sources and credits

Generated from `library/sources.yaml` by `scripts/library-attribution.py`. Do not edit by hand.

Sources checked: 2026-11-19. Packs installed later add their own CREDITS file
with any per-item credit lines the upstream licence requires.

## Shipped or shippable

### Adventure Kid Waveforms (AKWF-FREE)

- Source: <https://www.adventurekid.se/akrt/waveforms/adventure-kid-waveforms/>
- Fetched from: <https://github.com/KristofferKarlAxelEkstrand/AKWF-FREE>
- Licence: CC0-1.0
- Type: waveforms; tier: base
- Attribution required: no
- Credit: Single-cycle waveforms from Adventure Kid Waveforms (AKWF-FREE), CC0 1.0.
- Checked: 2026-11-19: LICENSE.md is CC0 1.0; sizes from the GitHub git tree API

### Versilian Community Sample Library (VCSL)

- Source: <https://github.com/sgossner/VCSL>
- Licence: CC0-1.0
- Type: samples; tier: drive
- Attribution required: no
- Credit: Samples from the Versilian Community Sample Library (VCSL) by Versilian Studios LLC, CC0 1.0.
- Checked: 2026-11-19: README and GitHub licence metadata say CC0 1.0; sizes from git tree API
- Installed packs: vcsl-starter-kit, vcsl-full-kit, vcsl-marimba, vcsl-vibraphone, vcsl-glockenspiel, vcsl-balafon, vcsl-mbira (see `sbom/sounds-manifest.md`)

### FreePats sound banks, CC0 banks only

- Source: <https://freepats.zenvoid.org/>
- Licence: CC0-1.0
- Type: samples; tier: base
- Attribution required: no
- Credit: Instrument banks from the FreePats project (CC0 1.0 banks only).
- Checked: 2026-11-19: licence stated on each bank page; sizes from each bank's download table

### Standard Ebooks

- Source: <https://standardebooks.org/>
- Licence: CC0-1.0 (the project's own work); texts are U.S. public domain
- Type: books; tier: base
- Attribution required: no
- Credit: Ebooks from Standard Ebooks; their work is dedicated to the public domain (CC0 1.0).
- Checked: 2026-11-19: About page states the texts are believed U.S. public domain and the project's work is dedicated to the public domain

## Allowed, fetched on request, not in the base image

### FreePats sound banks under CC-BY

- Source: <https://freepats.zenvoid.org/>
- Licence: CC-BY-3.0 OR CC-BY-4.0 (per bank)
- Type: samples; tier: drive
- Attribution required: yes
- Credit: Copy the credit line from each bank page into the pack's CREDITS at install time.
  - Salamander Grand Piano, CC-BY-3.0 (707 MiB SFZ+FLAC (48 kHz 24-bit); smaller variants 296-394 MiB)
  - YDP Grand Piano, CC-BY-3.0 (36 MiB SF2)
  - MuldjordKit (acoustic drums), CC-BY-4.0 (157 MiB SFZ+FLAC)
- Checked: 2026-11-19: licence stated on the bank pages

### FreePats banks under GPL with the sound-sample exception

- Source: <https://freepats.zenvoid.org/>
- Licence: GPL-3.0-or-later WITH LicenseRef-FreePats-sound-exception
- Type: soundfont; tier: drive
- Attribution required: yes
- Credit: FreePats General MIDI set, GPL-3.0-or-later with the FreePats sound-sample exception; licence text shipped with the pack.
  - FreePats General MIDI set (127 MiB SFZ+FLAC, 227 MiB SF2)
  - FreePats General MIDI percussion set (15 MiB)
  - FSS Steel-String Acoustic Guitar (12-13 MiB)
- Checked: 2026-11-19: bank pages state GPL-3.0-or-later with exception; the GM page notes some samples inside may carry other GPL-compatible licences

### Freesound, CC0 sounds only

- Source: <https://freesound.org/>
- Licence: CC0-1.0 (filter); per-sound licence recorded in the pack manifest
- Type: samples; tier: external
- Attribution required: no
- Credit: Sounds from Freesound (CC0 1.0); each sound's page URL is listed in the pack manifest.
- Checked: 2026-11-19: FAQ lists CC0, CC-BY 4.0, CC-BY-NC 4.0 and legacy Sampling+. API key terms unverified.

### Mutable Instruments Eurorack firmware (algorithm source)

- Source: <https://github.com/pichenettes/eurorack>
- Licence: MIT (STM32 projects, e.g. Plaits, Braids, Rings, Clouds); GPL-3.0 (AVR projects); hardware CC-BY-SA-3.0
- Type: code-reference; tier: external
- Attribution required: yes
- Credit: Ports keep the upstream MIT copyright notice in the ported files.
- Checked: 2026-11-19: README licence section

### Project Gutenberg

- Source: <https://www.gutenberg.org/>
- Licence: LicenseRef-Project-Gutenberg (U.S. public domain texts plus a trademark licence)
- Type: books; tier: external
- Attribution required: no
- Checked: unverified: licence page returned 503 on 2026-11-19

### Kiwix ZIM archives (offline Wikipedia and others)

- Source: <https://library.kiwix.org/>
- Licence: CC-BY-SA-4.0 for Wikipedia content (per archive)
- Type: dataset; tier: drive
- Attribution required: yes
- Credit: Per archive; ZIM files carry their own source and licence metadata.
- Checked: unverified this run

### Musopen recordings

- Source: <https://musopen.org/>
- Licence: per recording (public domain, CC0, CC-BY and others)
- Type: music; tier: external
- Attribution required: yes
- Credit: Per recording; recorded in the pack manifest.
- Checked: unverified this run

### Free Music Archive

- Source: <https://freemusicarchive.org/>
- Licence: per track (Creative Commons, including NonCommercial variants)
- Type: music; tier: external
- Attribution required: yes
- Credit: Per track; recorded in the pack manifest.
- Checked: 2026-11-19 partial: FAQ confirms per-track Creative Commons licences including NonCommercial

## Reference only: ideas or descriptors, no copied bytes or code

### NSynth dataset

- Source: <https://magenta.withgoogle.com/datasets/nsynth>
- Licence: CC-BY-4.0
- Type: dataset; tier: external
- Attribution required: yes
- Credit: NSynth dataset (Magenta), CC BY 4.0; cite the paper listed on the dataset page.
- Checked: 2026-11-19: licence CC BY 4.0 from the dataset page; size unverified

### TidalCycles

- Source: <https://github.com/tidalcycles/Tidal>
- Licence: GPL-3.0
- Type: code-reference; tier: external
- Attribution required: no
- Why: GPL-3.0, and the project treats ports based on its code as derivative works. Build our own pattern language from published papers.
- Checked: 2026-11-19: GitHub licence metadata

### Strudel

- Source: <https://github.com/tidalcycles/strudel>
- Licence: AGPL-3.0
- Type: code-reference; tier: external
- Attribution required: no
- Why: AGPL-3.0. Fine as a separate, unmodified service on the network; do not embed or copy code.
- Checked: 2026-11-19: GitHub licence metadata

## Not allowed in packs or on shared storage

### Philharmonia sound samples

- Source: <https://philharmonia.co.uk/resources/sound-samples/>
- Licence: LicenseRef-Philharmonia-samples
- Type: samples; tier: drive
- Attribution required: no
- Why: Free to use in music, including commercial works, but "must not be sold or made available 'as is' (i.e. as samples or as a sampler instrument)". So no packs, no base image, and never on a share or pilab endpoint other devices can read. A private copy on an owner's own drive for composing is fine.
- Checked: 2026-11-19: licence text on the sound-samples page


### FluidSynth and General MIDI SoundFonts (Debian packages, optional app)

- FluidSynth: LGPL-2.1-or-later, <https://www.fluidsynth.org>
- TimGM6mb: GPL-2.0-only (Debian `timgm6mb-soundfont`); FluidR3_GM: MIT (Debian `fluid-soundfont-gm`)
- Used as separate programs and data files; not linked into Trillium Base.

### ZynAddSubFX (Debian packages, optional app)

- ZynAddSubFX and zynaddsubfx-data banks: GPL-2.0-or-later, <https://zynaddsubfx.sourceforge.io>
- Runs as a separate program; not linked into Trillium Base.
