# Trillium Base roadmap

Trillium Base is the Raspberry Pi 5 sound module in the Trillium family (Trillium, an
Ambient Lab project). It runs Trillium Synth and is controlled from Trillium Console. This
page plans three tracks that turn the lab unit into something others can build, flash and
configure:

1. open hardware design;
2. a tuned Linux kernel;
3. release tooling.

It starts from what the lab unit actually runs (captured read-only on 2026-11-20, see
`kernel/README.md` and `kernel/snapshots/`), not from a wish list.

**v0 release = the OS image.** Software is the product. The hardware for v0 is
do-it-yourself from off-the-shelf parts (Pi 5 + Allo Katana DAC HAT), documented as a
reference build; no boards or kits are sold with v0.

## Decisions (2026-11-20)

| # | Topic | Decision |
| --- | --- | --- |
| 1 | Hardware licence | **CERN-OHL-S-2.0** (strongly reciprocal) for everything in `hardware/` |
| 2 | v0 scope | **Image only**; hardware is DIY; software is the main product. SD card reference; NVMe, DIN MIDI and a front panel are later |
| 3 | DAC | **Allo Katana** stays for now (high-end); a cheaper variant can come later |
| 4 | Kernel tuning | **Approved and applied** on the lab unit: CPU 3 isolation, repeatable via `kernel/cpu-isolation.sh`, documented as a feature |
| 5 | Image tooling | ZPOD's tooling is ahead on A/B boot; this image is ahead on real time. Separate report and recommendation; shared crates expected on both targets over time |
| 6 | Secure boot | **Not yet**; listed as a feature for a later release |
| 7 | Pairing | **QR code with a one-time code** (never the raw token). Implemented: `trillium-base pair --qr` |
| 8 | Hostnames | New units are **`tbase-xxxx`** (last 4 hex of the board serial, MAC as fallback). The lab unit keeps `pilab` |

## Features

**Released (in this repository, running on the lab unit):**

| Feature | What | Where |
| --- | --- | --- |
| PREEMPT_RT kernel | Stock Raspberry Pi RT kernel 6.18.60, 48 kHz / quantum 64 (1.33 ms) PipeWire graph | `kernel/README.md`, `etc/pipewire` |
| RT kernel tuning | Performance governor, rtprio/memlock limits, systemd keeps services on CPUs 0-2 | `etc/systemd`, `etc/security` |
| CPU isolation for audio | `isolcpus=managed_irq,domain,3 irqaffinity=0-2` plus a boot service that moves IRQs, unbound workqueues and kernel threads off CPU 3. Idempotent apply, backups, revert | `kernel/cpu-isolation.sh` |
| Per-core latency bench | cyclictest on all four cores incl. the isolated one, CPU 3 interrupt rate, isolation state | `bin/pilab-bench`, `bench/` |
| QR pairing | One-time 8-character code (5 min, single use, burned after 5 wrong tries) shown as a terminal QR; the console exchanges it for the token | `trillium-base pair --qr`, `POST /api/v1/pair/redeem` |
| Patch panel | Audio and MIDI routing with stereo pairs, meters, faders, undo | Trillium Console |
| BLE MIDI in | BLE-MIDI GATT service (PipeWire bluez5) autoconnected to Trillium Synth; time-boxed advertising tool for pairing a Mac/iPhone | `tools/ble-midi-advertise.py`, `docs/bluetooth-midi.md` |
| Boot-time output retry | pilabd waits up to 60 s for the DAC sink at boot instead of starting unpatched | `pilabd` |
| `tbase-xxxx` hostname tool | Computes and (on first boot of an image) sets the unit hostname | `tools/tbase-hostname.sh`, `etc/systemd/system/tbase-hostname.service` |

**Planned:**

| Feature | Release | Notes |
| --- | --- | --- |
| OS image (rpi-image-gen) | v0 | First boot runs `tbase-hostname.service` and prints a pairing QR |
| `nohz_full` + `rcu_nocbs` on CPU 3 | v0/v1 | Needs the Trillium kernel build (`kernel/trillium-rt.fragment`); the stock kernel lacks `NO_HZ_FULL` and `RCU_NOCB_CPU`. `cpu-isolation.sh` adds them automatically when the kernel supports them |
| QR on the console Settings page | v0 | Same one-time code, rendered in the browser |
| A/B boot with tryboot | later | Arrives with the image's partition layout; align with ZPOD's A/B tooling |
| Signed releases (offline minisign key) | later | Needs the owner's OK on key handling |
| Secure boot (signed boot.img, OTP key hash) | later release | OTP writes are irreversible; not before the image and update path are stable |
| Bluetooth off by default | v0 | BLE MIDI advertising only on request (time-boxed) |
| Open hardware files (CERN-OHL-S) | after v0 | BOM, DIN MIDI board, front panel, enclosure under `hardware/` |
| Full mixer, effects, second oscillator | later | See `docs/console.md` |

## Starting point

| Area | Today |
| --- | --- |
| Board | Raspberry Pi 5 Model B Rev 1.1, bootloader 2025-08-28 |
| Storage | 32 GB SD card, one boot partition, no NVMe |
| OS | Raspberry Pi OS Lite 64-bit, Debian 13 trixie |
| Kernel | Stock `linux-image-rpi-v8-rt` 6.18.60 (PREEMPT_RT, 4K pages, HZ 250), unsigned modules |
| CPU | Governor `performance` (unit); systemd `CPUAffinity=0-2`; audio thread on CPU 3; kernel isolation since 2026-11-20 (`isolcpus=managed_irq,domain,3 irqaffinity=0-2`, 1 of 53 IRQs on CPU 3, the per-core PMU) |
| Audio | Allo Katana DAC HAT (ES9038Q2M), PipeWire 1.4.2 at 48 kHz, quantum 64 (1.33 ms) |
| Latency | cyclictest, isolated CPU 3: max 9 µs idle, 38 µs under load (all cores: 22 / 77 µs) |
| Network | Ethernet only, home LAN; mDNS `_trillium-base._tcp` + `_pilab._tcp`; console on 8410, plain HTTP with a pairing token |
| MIDI | BLE MIDI (bluez) into Trillium Synth; USB MIDI by class driver; no DIN yet |
| Software | `pilabd` (Trillium Console + Trillium Synth), Cabbage/Csound, SuperCollider, Faust, optional AirPlay |
| Updates | `scripts/deploy-console.sh` from a Mac over SSH; no image, no signed releases, no A/B boot |

## Track 1: open hardware design

**Reference build (BOM v0)**, the parts the lab unit uses or needs next:

| Part | Lab unit | Notes |
| --- | --- | --- |
| Compute | Raspberry Pi 5, 4 or 8 GB | 8 GB if local AI stays on the roadmap; check which the lab unit has |
| DAC | Allo Katana (ES9038Q2M) HAT, headphone + balanced out | Alternative: a cheaper I²S HAT for a "Base Lite"; the Katana uses most GPIO and gets warm |
| Storage | 32 GB SD | NVMe via the Pi 5 PCIe FFC (M.2 HAT+) for reliability and A/B; HAT stacking with the DAC is a mechanical question |
| Power | Official 27 W USB-C PD | A clean supply matters for the DAC; measure noise before choosing |
| Cooling | (check) | Active cooler or a heatsink case; must not touch the HAT; fan noise matters for a music device |
| MIDI | BLE, USB | Add 5-pin DIN in/out (opto-isolated input, 3.3 V UART at 31250 baud on a free UART) on a small board |
| Controls | none | Optional: encoder, one status LED (Signal orange, used sparingly), power button through the Pi 5 power header |
| Enclosure | none | Desktop / Eurorack-adjacent; aluminium or printed; vents placed away from the DAC |
| Radio (optional) | none | SDR or packet-radio dongles via USB, **receive-only** until transmit is approved |

**License (decided).** Hardware files under **CERN-OHL-S-2.0** (strongly reciprocal, like
the GPL); text in `hardware/LICENSE-HARDWARE`, notice in `hardware/README.md`. It matches the
GPL-3.0 + commercial model of the software. A company that wants to ship a closed derivative
takes a commercial licence, as with the code. The enclosure CAD sits under the same licence. Logos and the
Trillium name stay trademarks and are outside the licence. Copyright line: "Copyright (C)
2026 M-of-N".

**Repository layout** (new top-level `hardware/`, SVG and text formats only, no rendered
images in git):

```
hardware/
  README.md            BOM, assembly, licence notice
  LICENSE-HARDWARE     CERN-OHL-S-2.0 text
  bom/base-v0.csv      part, supplier ref, qty, alternates
  midi-din/            KiCad 9 project (.kicad_pro/.kicad_sch/.kicad_pcb), fab notes
  front-panel/         KiCad project for encoder/LED/power button (later)
  enclosure/           FreeCAD or OpenSCAD source + STEP export, SVG drawings
  docs/                pinout.svg (GPIO used by DAC, MIDI UART, panel)
```

**Unknowns / owner's choices:** the Pi RAM size; DAC choice for a release (Katana vs a
cheaper HAT vs a custom DAC board later); SD vs NVMe as the reference; form factor
(desktop box vs rack/Eurorack); whether DIN MIDI is in v1; CERN-OHL-S vs -W.

**Next 3 steps**

1. Write `hardware/README.md` and `bom/base-v0.csv` from the lab unit, with gaps marked.
   (Read the RAM size and cooler from the Pi read-only.)
2. Draw the GPIO/pinout map. Confirm which pins the Katana takes and which UART is free
   for DIN MIDI, from the overlay and `pinctrl` read-only.
3. Start the KiCad DIN MIDI board (schematic only), based on the MIDI 1.0 electrical
   spec, with a 6N138 / H11L1 opto on the input.

**Needs the owner's OK:** the licence choice; ordering any parts or PCBs (purchases); anything
that adds a radio transmitter.

## Track 2: tuned Linux kernel

**Captured and checked in** (this commit):

- `kernel/configs/6.18.60+rpt-rpi-v8-rt.config`, the running config;
- `kernel/snapshots/2026-11-20-pilab.txt`, with uname, `/proc/cmdline`, `config.txt`, IRQ
  affinity, governor, limits, PipeWire clock and RT threads;
- `scripts/capture-tuning.sh` to repeat the snapshot read-only;
- `kernel/trillium-rt.fragment`: `NO_HZ_FULL`, `RCU_NOCB_CPU`, performance governor
  default, module signing, `IKCONFIG`, `-trillium-rt` local version;
- `kernel/cmdline-proposed.txt`: `isolcpus=managed_irq,domain,3 nohz_full=3 rcu_nocbs=3 irqaffinity=0-2`;
- `kernel/build-deb.sh`: a recipe from captured config plus fragment to `.deb`, not run yet.

**Biggest finding:** CPU 3 was only "isolated" by systemd affinity; the kernel still put
ticks, RCU work and most IRQs on it.

**Applied 2026-11-20 (stock kernel):** `kernel/cpu-isolation.sh apply` added
`isolcpus=managed_irq,domain,3 irqaffinity=0-2` to `cmdline.txt` (backup kept) and installed
`trillium-cpu-isolation.service`, which at boot moves every movable IRQ, the unbound
workqueues and unbound kernel threads to CPUs 0-2. `nohz_full`/`rcu_nocbs` are skipped
because the stock kernel lacks `NO_HZ_FULL`/`RCU_NOCB_CPU`; the script adds them once the
Trillium kernel is installed. Results, same bench (`bench/20261120-*-isolation.txt`,
cyclictest P95 45 s per phase, stress-ng cpu 4 / vm 2 / io 1):

| | Before | After |
| --- | --- | --- |
| IRQs allowed on CPU 3 | 50 / 53 | 1 / 53 (per-core PMU only) |
| CPU 3 interrupts/s, idle | 1228 | 999 (timer tick + IPIs; needs `nohz_full` to go lower) |
| CPU 3 max latency, idle | 10 µs | 9 µs |
| CPU 3 max latency, load | 58 µs (avg 7) | **38 µs** (avg 4) |
| All cores max, load | 96 µs | 77 µs |
| Temperature / throttling | 57 °C / 0x0 | 57 °C / 0x0 |

Audio after the change (two reboots): pilabd up, PipeWire 48 kHz / quantum 64, Katana
present, DAC volume 0.70, master −18 dB, audio thread pinned to CPU 3, synth → DAC and
BLE MIDI → synth links restored at boot. **0 xruns** in 60 s of the same stress-ng load with
notes playing. One boot showed a dropout burst about 80 s after power-up that the pilabd
watchdog recovered (DAC sink suspend/resume); cause not identified yet (no persistent
journal). To watch in the xrun soak.

Note: the earlier "14 µs idle / 94 µs load" figures came from a bench that inherited
systemd's CPUAffinity=0-2 and never measured CPU 3. `pilab-bench` now starts cyclictest
through `taskset -c 0-3` so every core, including the isolated one, is measured.

**Latency measurement.** Run `pilab-bench` (cyclictest idle and under load) before and
after each change, and save the results in `bench/`. Watch xruns in `/api/v1/status` and
`pw-top`. Acceptance: no xruns in a 1-hour soak at quantum 64 with 32 voices, then repeat
at 32. Record the temperature and `get_throttled` alongside.

**A/B and signed updates.**

- The Pi 5 bootloader supports `tryboot`. `autoboot.txt` with `[all] tryboot_a_b=1` picks
  boot partition 2 or 3. `reboot '0 tryboot'` boots the other slot once, and the update
  client marks it good after health checks: pilabd up, DAC present, no xruns for N minutes.
  This needs a new partition layout (two boot and two root partitions, or boot A/B plus a
  read-only root and a data partition), so it arrives with the first image, not on the
  running card.
- Signing has two parts:
  - Release artifacts (image, packs, `pilabd` bundles) are signed with an offline key
    (minisign/ed25519) and checked by the update client before anything is written.
  - The Pi 5 secure boot (signed `boot.img` against a key hash in OTP) is possible later.
    Writing OTP is irreversible, so it is explicitly out of scope unless the owner decides
    otherwise.

**ZPOD coordination.** ZPOD (Pi Zero 2 W) and Trillium Base can share:

- the image tooling: one `rpi-image-gen` config layer for "Trillium common" (users,
  SSH-key-only, mDNS, update client) and a board layer each;
- one Rust update client crate, which checks signatures, writes the inactive slot, then
  runs tryboot and marks it good;
- one release-signing key policy;
- the device-identity scheme (`id` = machine-id hash).

The update client should live in its own repo or crate that both pin by commit, the way
pilab pins `zpod` crates today. ZPOD's own crates stay unmodified.

**Next 3 steps**

1. Pin the exact `raspberrypi/linux` commit for `1:6.18.60-1+rpt1`, by reading the package
   changelog read-only. Run `build-deb.sh` on the box to prove the recipe; the build output
   stays out of git.
2. Add an xrun soak mode to `pilab-bench` (1 hour, quantum 64, 32 voices; then quantum 32).
3. ~~Try `cmdline-proposed.txt` on the stock kernel and re-bench~~ (done 2026-11-20, see
   above). Next: the Trillium kernel with `NO_HZ_FULL`/`RCU_NOCB_CPU`, installed through
   tryboot, then re-bench.

**Needs the owner's OK:** any change to `/boot/firmware` (`cmdline.txt`, `config.txt`,
`autoboot.txt`) and the reboot it needs; installing a custom kernel; any EEPROM/bootloader
update; secure boot / OTP (strongly suggest: not now).

## Track 3: release tooling

**Image builder.** Use **rpi-image-gen**, the Raspberry Pi tool for Pi 5 and trixie. It is
declarative YAML layers and suits a product image better than pi-gen's stage scripts. The
fallback is pi-gen if a needed feature is missing. The layers are:

- base trixie lite;
- the Trillium common layer (shared with ZPOD);
- the Trillium Base layer: RT kernel, Katana overlay, PipeWire config, limits,
  CPU affinity, pilabd and the `trillium-base` CLI, avahi service;
- an optional "lab" layer: desktop, VNC, Cabbage, SuperCollider, AirPlay.

The build runs on CI and produces an `.img.xz`, its SHA-256 and a signature. Building an
image does not flash anything. **Flashing a card is always a manual step by the owner.**

**First boot.** The image boots to a "setup" state:

- hostname `tbase-xxxx`, set once by `tbase-hostname.service` (`tools/tbase-hostname.sh`):
  the last 4 hex digits of the board serial number, or of the first MAC address if there is
  no serial. A host that was named by hand is never renamed;
- no SSH password; SSH off until a key is added;
- the console is reachable at `http://tbase-xxxx.local:8410/`;
- a pairing QR (one-time code) is printed on the HDMI console and by `trillium-base pair --qr`.

The console then shows a setup wizard:

1. name the device;
2. pick the audio output (DAC detected or not);
3. confirm a quiet test tone at −18 dBFS;
4. MIDI sources;
5. optionally add an SSH key;
6. optionally set up a NAS for recordings;
7. update channel.

The wizard writes `~/.config/pilab/settings.toml` (path kept) through the existing API.

**`trillium-base` CLI** (today an alias of `pilabctl`; it grows into):

- `pair [--rotate] [--qr]`;
- `status`;
- `update check|apply|rollback` (talks to the update client);
- `backup` / `restore` (scenes, settings, patch map, tarred, **excluding** the token);
- `bench`;
- `doctor` (DAC, PipeWire, xruns, mDNS, disk).

It runs on the device; from a Mac, Trillium Console (web now, Mac app later) does the
same over the API.

**QR pairing (implemented).** `trillium-base pair --qr` writes a one-time code to
`~/.config/pilab/pair-code` (0600) and prints a terminal QR (via `qrencode`) for
`http://<host>.local:8410/#pair=XXXX-XXXX`. The code is 8 characters from a 31-symbol
alphabet without look-alikes, valid 5 minutes, works once, and is destroyed after 5 wrong
attempts (each failure also waits 0.7 s). Opening the URL makes the console call
`POST /api/v1/pair/redeem {code}` (no token needed), store the returned token and drop the
code from the address bar. The code can also be typed into the pairing box. The QR never
contains the token. Still to do: show the same QR on the console's Settings page for
pairing a second device from a paired one, and on the HDMI console at first boot.

**Signed release pipeline (GitHub, as nymble).** Tags `base-vMAJOR.MINOR.PATCH`. The
console/service version (0.2.0 now) follows SemVer for the API (`/api/v1`). GitHub
Actions builds:

- the `pilabd` binaries (arm64);
- the packs;
- the image.

A separate, manual signing step uses the offline key, so the key never lives in CI
secrets. Update metadata (version,
SHA-256, signature, minimum version) is a small JSON file signed the same way.

**Fixed image vs loadable packs.**

| In the image (signed, A/B) | Loadable packs (signed, on the data partition) |
| --- | --- |
| Kernel, firmware, overlays, `config.txt` | Sample banks (CC0 / licensed) |
| OS, PipeWire, limits, systemd units | AKWF and other wavetable sets |
| `pilabd`, `trillium-base`, update client | Scenes, patch presets, MIDI maps |
| Trillium Console web assets | Optional engines (Cabbage/Csound, SuperCollider patches, Faust) |
| Default quiet settings | Lab extras (desktop/VNC, AirPlay) |

Packs carry their own licence file and a manifest. Third-party GPL tools (nqptp,
shairport-sync) stay separate processes with source offered, as today.

**Next 3 steps**

1. Add `image/` with an rpi-image-gen config that reproduces the lab unit minus the
   desktop, built on the box and **not flashed**. Compare its package list with
   `packages/apt-mark-showmanual.txt`.
2. Grow `pilabctl` / `trillium-base` with `backup` and `doctor` (`pair --qr` is done), plus
   a QR on the Settings page. These are software only and testable on the box.
3. Write the release workflow (`.github/workflows/release.yml`): build arm64 `pilabd`
   bundles on a tag and leave signing as a documented manual step.

**Needs the owner's OK:** flashing any card or device with a built image; creating the signing
key and where it lives; making a release public; the update channel policy.

## Decisions for the owner

All eight were answered on 2026-11-20; see "Decisions (2026-11-20)" at the top. Still open:
the Pi RAM size for the BOM, the release signing key handling, and the image tooling
recommendation (shared with ZPOD or not).

## Repository name

Renamed on 2026-11-20 from `nymble/pilab` to `nymble/trillium-base` and made public after a
history and HEAD audit. GitHub redirects the old URL for web, clone, fetch and push as long
as no new repository named `pilab` is created. ZPOD is unaffected: the pinned `nymble/zpod`
crates contain no reference to this repository. The internal ids (`pilabd`, `pilab-core`,
`pilab-synth`, `_pilab._tcp`, `~/.config/pilab`) are independent of the repo name and stay.
