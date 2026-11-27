<!-- Copyright (C) 2026 Paul Lambert -->
<!-- SPDX-License-Identifier: CERN-OHL-S-2.0 OR LicenseRef-Commercial -->
# Trillium Base hardware

**Status: placeholder.** The v0 release of Trillium Base is the OS image only. The hardware
is do-it-yourself from off-the-shelf parts (see the reference build in
`docs/roadmap-base.md`, Track 1). Design files (BOM, MIDI DIN board, front panel,
enclosure) will be added here as they are made.

## Licence

Everything in `hardware/` is licensed under the **CERN Open Hardware Licence Version 2 -
Strongly Reciprocal** (SPDX `CERN-OHL-S-2.0`); the full text is in `LICENSE-HARDWARE`.

Copyright (C) 2026 Paul Lambert

- Anyone may study, modify, make and sell products from these designs, provided modified
  designs are shared under the same licence ("Source" made available, as in the licence).
- A commercial licence (`LicenseRef-Commercial`, see `LICENSE-COMMERCIAL.md`) is available for closed derivatives: support@m-of-n.com.
- The software in this repository is separate: `GPL-3.0-or-later OR LicenseRef-Commercial`
  (see `LICENSE` and `LICENSE-COMMERCIAL.md` at the repository root).
- The Trillium names and marks are trademarks of the copyright holder (see NOTICE.md) and are not licensed by
  CERN-OHL-S or the GPL.

## Planned layout

```
hardware/
  README.md            this file
  LICENSE-HARDWARE     CERN-OHL-S-2.0 text
  bom/base-v0.csv      part, supplier ref, qty, alternates
  midi-din/            KiCad project for 5-pin DIN MIDI in/out
  front-panel/         KiCad project for encoder/LED/power button (later)
  enclosure/           FreeCAD or OpenSCAD source + STEP export, SVG drawings
  docs/                pinout.svg (GPIO used by DAC, MIDI UART, panel)
```

Source formats only (KiCad, FreeCAD/OpenSCAD, SVG, CSV); no rendered images in git.
