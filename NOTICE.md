# Notices and attribution

## This project

Copyright (C) 2026 Paul Lambert (d/b/a M-of-N). Licensed under
`GPL-3.0-or-later OR LicenseRef-Commercial`; see [LICENSE](LICENSE),
[LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md) and [LICENSES/](LICENSES/). Contributions:
[CONTRIBUTING.md](CONTRIBUTING.md). Earlier commits of this repository carried the same
expression with the copyright line "M-of-N"; the copyright holder is the person named above.

## Names and marks

"Trillium", "Trillium Base", "Trillium Console", "Trillium Synth", "Ambient Lab" and the
Rhodonea mark are names and marks of the copyright holder. They are not licensed by the GPL
or by a commercial licence, which cover the code and content, not the names. You may say a
build is "based on Trillium Base"; a modified or redistributed build must not use these names
or marks as its own product name or logo without permission. No trademark registration is
claimed here.

## Known exceptions and third-party material

| Item | Licence | Note |
| --- | --- | --- |
| `patches/cabbage-linux-aarch64.patch` | GPL-3.0-only | Patch to Cabbage (GPL-3.0-only). A derivative of that code, distributed under the same licence. Not offered under the commercial licence. |
| TimGM6mb sound bank (`timgm6mb-soundfont`) | GPL-2.0-only | Installed and selectable as separate sample data (not linked into any program). Must not be included in any proprietary or commercially licensed artifact. |
| ZPOD crates (`zpod-voices`, `zpod-midi`, `zpod-music-core`) | `GPL-3.0-or-later OR LicenseRef-Commercial` from the ZPOD repository, pinned in `rust/Cargo.toml` | Same copyright holder; commercial terms come from the ZPOD repository's `LICENSE-COMMERCIAL.md`. Earlier ZPOD revisions were published under MIT and stay MIT as published. |
| Self-hosted fonts in `rust/crates/pilabd/web/fonts/` (Outfit, Inter, IBM Plex Mono) | SIL OFL 1.1 | Licence texts beside the fonts. |
| Sound libraries listed in `library/sources.yaml` | CC0, CC BY 4.0 (MuldjordKit, credit required), others per `library/ATTRIBUTION.md` | Some sources are per-track Creative Commons including NonCommercial variants; see `library/sources.yaml`. |
| FluidSynth, ZynAddSubFX, SuperCollider, Pd, Csound, Faust, PipeWire, Linux and other system packages | Their own licences (LGPL, GPL and others) | Separate programs installed in the image; see `sbom/`. Licences for shairport-sync and nqptp are not yet verified. |
| AcoustID web service | Free service is for non-commercial use | Not part of the code licence. |
| `packages/`, `kernel/configs/`, `kernel/snapshots/`, `boot/issue.txt`, `boot/config.txt`, `etc/systemd/system/nqptp.service.installed` | `LicenseRef-System-Snapshot` | Captures from a Raspberry Pi image or an upstream install (the stock Pi `config.txt`, nqptp's installed unit, package lists, kernel config and tuning snapshots). The project asserts no licence for them beyond what upstream provides. Everything else under `etc/`, `home/`, `boot/` and `kernel/` is first-party and is `GPL-3.0-or-later OR LicenseRef-Commercial`. |
| `hardware/` | CERN-OHL-S-2.0 | Placeholder; a commercial option is stated in `hardware/README.md`. |
| SBOM files in `sbom/` | Generated data | Describe third-party components and their licences. |
