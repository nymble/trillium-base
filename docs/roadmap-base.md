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

## Starting point

| Area | Today |
| --- | --- |
| Board | Raspberry Pi 5 Model B Rev 1.1, bootloader 2025-08-28 |
| Storage | 32 GB SD card, one boot partition, no NVMe |
| OS | Raspberry Pi OS Lite 64-bit, Debian 13 trixie |
| Kernel | Stock `linux-image-rpi-v8-rt` 6.18.60 (PREEMPT_RT, 4K pages, HZ 250), unsigned modules |
| CPU | Governor `performance` (unit); systemd `CPUAffinity=0-2`; audio thread on CPU 3; **no kernel isolation** (no `isolcpus`/`nohz_full`, IRQs on all cores) |
| Audio | Allo Katana DAC HAT (ES9038Q2M), PipeWire 1.4.2 at 48 kHz, quantum 64 (1.33 ms) |
| Latency | cyclictest max 14 µs idle, 94 µs under load |
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

**License.** Hardware files under **CERN-OHL-S-2.0** (strongly reciprocal, like the GPL).
It matches the GPL-3.0 + commercial model of the software. A company that wants to ship a
closed derivative takes a commercial licence, as with the code. **CERN-OHL-W** (weakly
reciprocal) is the alternative if third parties should be able to build closed add-on
boards around an open core. The enclosure CAD can sit under the same licence. Logos and the
Trillium name stay trademarks and are outside the licence. Copyright line: "Copyright (C)
2026 Nymble".

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

**Biggest finding:** CPU 3 is only "isolated" by systemd affinity. The kernel still
puts ticks, RCU work and most IRQs on it. Latency is already good (94 µs worst case under
load against a 1.33 ms period), so this is about headroom for quantum 32 and many voices,
not a current problem. Try the command line first on the stock kernel: it is reversible by
editing one line and gives most of the gain. A custom kernel only adds `nohz_full` and
`rcu_nocbs` support and signing.

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
2. Run `pilab-bench` again now (baseline with today's load) and add an xrun soak mode.
3. With approval, try `cmdline-proposed.txt` on the stock kernel and re-bench. Revert if
   anything regresses.

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

- hostname `trillium-<4 hex of id>`;
- no SSH password; SSH off until a key is added;
- the console is reachable at `http://trillium-xxxx.local:8410/`;
- the pairing token is printed on the HDMI console and through `trillium-base pair`.

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

**QR pairing.** `trillium-base pair --qr` shows a QR in the terminal, and the console's
Settings page shows one too. Both encode the URL and device id (`http://…:8410/#id=…`).
Putting the token in the QR is a choice: it is convenient for a phone, but anyone who sees
the screen gets the token. Suggestion: the QR holds a one-time code that expires in 2
minutes and is exchanged for the token, never the token itself.

**Signed release pipeline (GitHub, as nymble).** Tags `base-vMAJOR.MINOR.PATCH`. The
console/service version (0.2.0 now) follows SemVer for the API (`/api/v1`). GitHub
Actions builds:

- the `pilabd` binaries (arm64);
- the packs;
- the image.

A separate, manual signing step uses the offline key, so the key never lives in CI
secrets. Releases are private while the repo is private. Update metadata (version,
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
2. Grow `pilabctl` / `trillium-base` with `backup`, `doctor` and `pair --qr`, plus a QR on
   the Settings page. These are software only and testable on the box.
3. Write the release workflow (`.github/workflows/release.yml`): build arm64 `pilabd`
   bundles on a tag and leave signing as a documented manual step.

**Needs the owner's OK:** flashing any card or device with a built image; creating the signing
key and where it lives; making a release public; the update channel policy.

## Decisions for the owner

1. Hardware licence: CERN-OHL-S (suggested) or CERN-OHL-W.
2. Reference storage: SD (simple) or NVMe (A/B, durability). Also whether DIN MIDI and a
   front panel are in v1.
3. DAC for a release: Katana or a cheaper HAT for a lighter variant.
4. Kernel: OK to try the CPU 3 isolation command line on the lab unit (one-line,
   reversible, needs a reboot)?
5. Image tool: rpi-image-gen (suggested) or pi-gen. Also whether the image tooling is
   shared with ZPOD from the start.
6. Signing: offline minisign key now. Secure boot / OTP later or never.
7. QR content: one-time code (suggested) or the raw token.
8. Repo name: keep `nymble/pilab` for now, or rename to `nymble/trillium-base` (see below).
9. Hostname for new images: `trillium-<4 hex>` (suggested) or keep `pilab`. The lab unit
   keeps `pilab` either way.

## Repository name

Recommendation: **keep `nymble/pilab` until the first image or public release, then
rename to `nymble/trillium-base`** in one planned step. GitHub redirects the old URL for
web, clone, fetch and push after a rename, as long as no new repo called `pilab` is
created later. What would need updating:

- `rust/Cargo.toml` `repository` field; README links; local remotes (`git remote set-url`)
  on the Mac and anywhere else it is cloned;
- any GitHub Actions, deploy keys or tokens scoped by repo name;
- update-client URLs once releases exist. This is the strongest reason to rename
  *before* the first signed release, because devices in the field would keep the URL.

What would **not** break: ZPOD. The pinned `nymble/zpod` crates (525bc84) contain no
reference to `pilab` or Trillium Base; the dependency only runs from this repo to `zpod`.
The internal ids (`pilabd`, `pilab-core`, `pilab-synth`, `_pilab._tcp`, `~/.config/pilab`)
are independent of the repo name and can stay.
