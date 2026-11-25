<!-- Copyright (C) 2026 M-of-N. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Sounds manifest

Every sound pack Trillium Base installs, with its licence. Packs are downloaded onto the device by
`scripts/pilab-samples.py` (pinned by sha256 or git blob id) and never committed to git. Each installed
bank also gets a `pilab.manifest/v1` record under `~/.local/share/pilab/library/manifest/` with
the per-file sha256 list. Anything whose licence is not CC0, public domain, MIT, LGPL or GPL
compatible with `GPL-3.0-or-later OR LicenseRef-Commercial` is not installed.

## Sample banks (sampler, SFZ)

| Id | Title | Category | Licence | Download (MiB) | Source |
| --- | --- | --- | --- | --- | --- |
| upright-piano-kw | Upright Piano KW (small) | keys | CC0-1.0 | 2.9 | FreePats (freepats.zenvoid.org) |
| old-piano-fb | Old Piano FB (small) | keys | CC0-1.0 | 6.2 | FreePats (github.com) |
| fm-piano-2 | FM Synthesized Piano #2 | keys | CC0-1.0 | 8.7 | FreePats (github.com) |
| drawbar-organ | Drawbar Organ Emulation | keys | CC0-1.0 | 5.8 | FreePats (freepats.zenvoid.org) |
| button-accordion-hn | Button Accordion HN | keys | CC0-1.0 | 4.8 | FreePats (github.com) |
| spanish-guitar | Spanish Classical Guitar | strings-and-plucked | CC0-1.0 | 4.5 | FreePats (freepats.zenvoid.org) |
| picked-bass-yr | Bass Guitar YR (pick) | strings-and-plucked | CC0-1.0 | 2.7 | FreePats (github.com) |
| eguitar-fsbs-clean | FSBS Electric Guitar Clean #1 (small, bridge) | strings-and-plucked | CC0-1.0 | 3.0 | FreePats (github.com) |
| ukulele | Ukulele | strings-and-plucked | CC0-1.0 | 2.3 | FreePats (github.com) |
| concert-harp | Concert Harp (small) | strings-and-plucked | CC0-1.0 | 2.5 | FreePats (freepats.zenvoid.org) |
| kalimba | Kalimba | strings-and-plucked | CC0-1.0 | 10.3 | FreePats (freepats.zenvoid.org) |
| clarinet | Clarinet | winds | CC0-1.0 | 6.6 | FreePats (freepats.zenvoid.org) |
| tenor-sax | Tenor Saxophone (small) | winds | CC0-1.0 | 4.7 | FreePats (freepats.zenvoid.org) |
| ocarina | Ocarina | winds | CC0-1.0 | 4.2 | FreePats (github.com) |
| xylophone | Xylophone (medium mallets) | mallets-and-bells | CC0-1.0 | 2.3 | FreePats (freepats.zenvoid.org) |
| tubular-bells | Tubular Bells (small) | mallets-and-bells | CC0-1.0 | 2.1 | FreePats (github.com) |
| timpani | Timpani | mallets-and-bells | CC0-1.0 | 8.9 | FreePats (github.com) |
| world-percussion | World Percussion | drums | CC0-1.0 | 6.6 | FreePats (github.com) |
| synth-percussion | FreePats Synthesizer Percussion | drums | CC0-1.0 | 1.8 | FreePats (freepats.zenvoid.org) |
| synth-bass-1 | Synth Bass #1 | synths | CC0-1.0 | 0.7 | FreePats (github.com) |
| lately-bass | Lately Bass | synths | CC0-1.0 | 1.1 | FreePats (github.com) |
| synth-brass-2 | Synth Brass #2 | synths | CC0-1.0 | 1.9 | FreePats (github.com) |
| synth-strings-1 | Synth Strings #1 | synths | CC0-1.0 | 3.5 | FreePats (github.com) |
| synth-pad-choir | Synth Pad Choir | synths | CC0-1.0 | 6.5 | FreePats (github.com) |
| sweep-pad | Sweep Pad | synths | CC0-1.0 | 2.9 | FreePats (github.com) |
| synth-lead-square | Synth Lead Square | synths | CC0-1.0 | 6.9 | FreePats (github.com) |
| synth-lead-calliope | Synth Lead Calliope | synths | CC0-1.0 | 3.5 | FreePats (github.com) |
| synth-crystal | Synth Crystal | synths | CC0-1.0 | 2.5 | FreePats (github.com) |
| vcsl-starter-kit | Starter Kit (GM) | drums | CC0-1.0 | 36.6 | VCSL c1ea7bc |
| vcsl-full-kit | VCSL Full Kit (GM), notes 27-87 | drums | CC0-1.0 | 82.8 | VCSL c1ea7bc |
| vcsl-marimba | Marimba | mallets-and-bells | CC0-1.0 | (in vcsl-tonal, 113.2 total) | VCSL c1ea7bc |
| vcsl-vibraphone | Vibraphone | mallets-and-bells | CC0-1.0 | (in vcsl-tonal, 113.2 total) | VCSL c1ea7bc |
| vcsl-glockenspiel | Glockenspiel | mallets-and-bells | CC0-1.0 | (in vcsl-tonal, 113.2 total) | VCSL c1ea7bc |
| vcsl-balafon | Balafon | mallets-and-bells | CC0-1.0 | (in vcsl-tonal, 113.2 total) | VCSL c1ea7bc |
| vcsl-mbira | Mbira, Tanzania | strings-and-plucked | CC0-1.0 | (in vcsl-tonal, 113.2 total) | VCSL c1ea7bc |
| muldjord-kit | MuldjordKit (GM), acoustic rock kit | drums | **CC-BY-4.0** | 60.3 | sfzinstruments/DrumGizmo.MuldjordKit fc16571 |

**Credit (required, CC BY 4.0):** Drum samples provided by DrumGizmo.org (MuldjordKit, CC BY 4.0; SFZ port by sfzinstruments). Shown in the Sounds browser, the Sounds & apps credits and Settings > About.

## Instruments and players (apps, from Debian)

| App | Licence | Sound data | Data licence | Status |
| --- | --- | --- | --- | --- |
| Trillium Synth (pilab-synth) | GPL-3.0-or-later OR LicenseRef-Commercial | AKWF wavetables | CC0-1.0 | installed |
| FluidSynth 2.4.4 (`fluidsynth`) | LGPL-2.1-or-later | FluidR3_GM (`fluid-soundfont-gm` 3.1, 148 MB), the default | MIT | installed; engine `pilab-fluidsynth.service`, scene "general midi" |
| | | TimGM6mb (`timgm6mb-soundfont`) | GPL-2.0-only | installed, selectable |
| ZynAddSubFX 3.0.6 (`zynaddsubfx`) | GPL-2.0-or-later | `zynaddsubfx-data` (32 banks) | GPL-2.0-or-later | installed; engine `pilab-zynaddsubfx.service`, scene "zynaddsubfx" |

App manifests: `etc/trillium/apps.d/fluidsynth.toml`, `etc/trillium/apps.d/zynaddsubfx.toml`.

## Allowed but not shipped

| Pack | Licence | Why not |
| --- | --- | --- |
| Surge XT | GPL-3.0 | no headless Pi build in Debian trixie arm64 yet; candidate app |
| sfizz | BSD-2-Clause | pilab-synth has its own SFZ player; only needed for SFZ features it lacks |

Licences were checked against each upstream's licence file on 2026-11-19 (FreePats, VCSL) and
2026-11-25 (Debian package copyright files). See `library/sources.yaml` and `library/ATTRIBUTION.md`.
