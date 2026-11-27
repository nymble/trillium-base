<!-- Copyright (C) 2026 Paul Lambert. SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Trillium Zero firmware

## How the Zero is updated

The Zero is a Seeed XIAO nRF52840 Sense with the Adafruit UF2 bootloader and SoftDevice S140 7.3.0.

1. ACTION 6 over the config SysEx (cable 1) restarts it into the bootloader, when the firmware
   reports capability bit `0x40`. (Double-tapping reset does the same by hand.)
2. A USB drive labelled `XIAO-SENSE` appears with `INFO_UF2.TXT` and `CURRENT.UF2`.
3. Copying a `.uf2` file onto the drive writes it; the bootloader restarts into the new firmware.

Releases are published on the Trillium repository as `xiao-sense-fw-X.Y.Z.uf2`.

## What Trillium Base checks

`pilab-core/src/uf2.rs` reads every 512-byte UF2 block and refuses the file unless:

- the magic numbers, block numbering and payload sizes are right;
- the family id is nRF52840 (`0xADA52840`); a bootloader image (`0xD663823C`) is refused;
- every address falls in the application window `0x27000..0xEC000` (not the SoftDevice, not the bootloader);
- it is at most 1 MiB.

It reports blocks, bytes, address range, sha256 and the version from the file name, and compares
that to the running firmware (a downgrade needs `--allow-downgrade`).

## Console and CLI

- Devices → *Firmware* card: running version, latest published release, checked files; upload a
  `.uf2` to check it. *Install* needs the word "install" typed in a dialog.
- CLI: `trillium-base firmware status | latest | check FILE | update FILE --confirm [--allow-downgrade]`.
- API: `GET /firmware[?refresh=1]`, `POST /firmware/check?name=|path=`, `POST /firmware/update`.

The update path: ACTION 6 → wait up to 20 s for the drive (mounting it with udisksctl if needed) →
confirm `INFO_UF2.TXT` names an nRF52840 board → back up `CURRENT.UF2` to
`~/.local/share/pilab/firmware/backups/` → write the image and fsync → wait up to 45 s for the Zero
to come back. Checked files are kept in `~/.local/share/pilab/firmware/`.

## Status on the lab unit (2026-11-25)

- Running firmware 0.8.0 (capabilities include pages 1 and 2 and `0x40`), MCU only: IMU running, no touch strip.
- Latest published release: v0.7.0, which is older than what is running, so installing it would be a downgrade.
- `xiao-sense-fw-0.7.0.uf2` passes the checker (application window, nRF52840 family).
- The update itself has **not** been run on the device: flashing needs an explicit OK each time.
