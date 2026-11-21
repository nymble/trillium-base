# Trillium Base

**Trillium Base** is a Raspberry Pi 5 sound module in the Trillium family (Trillium, an Ambient Lab project): a tuned real-time Linux machine running **Trillium Synth**, controlled from **Trillium Console** (the web console now; a Mac app later). This repository holds its config, rebuild notes and software, with a later path toward packet radio, security tools (own gear only), and local AI. Roadmap: [docs/roadmap-base.md](./docs/roadmap-base.md).

Names that stay as they are (infrastructure and compatibility): the lab unit's hostname `pilab` (`pilab.local`) and user `lab`, the paths `~/.config/pilab` and `~/.local/share/pilab`, the binaries `pilabd` and `pilabctl` (`trillium-base` is an alias of `pilabctl`), the crates `pilab-core` and `pilab-synth`, the mDNS type `_pilab._tcp` (now alongside `_trillium-base._tcp`), and the AirPlay receiver name "pilab".

This repository is the versioned record of the process. It is not a disk image. Image snapshots, if taken later, stay out of git.

Repository: [github.com/nymble/trillium-base](https://github.com/nymble/trillium-base) (formerly `nymble/pilab`; the old URL redirects).

## Hardware and software (as of 2026-11-18)

- Raspberry Pi 5, booting from a 32 GB SD card
- HiFi DAC Pro HAT with an ES9038Q2M chip (headphone and balanced outputs, up to 384 kHz / 32-bit). The in-tree overlay is `allo-katana-dac-audio`. ALSA shows the card as **Allo Katana**; the card number can change, so always address it by name. In PipeWire the sink is currently node "Built-in Audio Stereo" (`alsa_output.platform-soc_107c000000_sound.stereo-fallback`).
- Raspberry Pi OS Lite 64-bit, Trixie (image `2026-10-06-raspios-trixie-arm64-lite`, Debian 13)
- Official PREEMPT_RT kernel: `linux-image-rpi-v8-rt` 6.18.60, booted with `kernel=kernel8_rt.img` under `[pi5]`
- PipeWire 1.4.2 with `pipewire-jack`, quantum 64 at 48 kHz, plus WirePlumber and rtkit
- Audio limits (`rtprio` 95, memlock unlimited), CPU governor `performance`, systemd `CPUAffinity=0-2` with CPU 3 reserved for audio
- SuperCollider 3.13, Pure Data, Csound, Faust
- labwc / Wayland desktop (`rpd-wayland-core`) with autologin as `lab`
- wayvnc as a **user** service bound to `127.0.0.1:5900`, reached from a Mac through an SSH tunnel and TigerVNC (`mac/pilab-desktop.command`)

The login user is `lab` (key-only SSH, passwordless sudo). Hostname is `pilab` (`pilab.local` via mDNS).

## Repo layout

| Path | What |
| --- | --- |
| [BUILD.md](./BUILD.md) | Step-by-step rebuild from a blank card |
| `boot/` | `config.txt` and cloud-init `user-data` (SSH key replaced by a placeholder) |
| `etc/` | Audio limits, CPU governor and affinity, PipeWire, system wayvnc bind address, getty autologin |
| `home/` | Files for user `lab`: wayvnc user config and unit, SuperCollider patches |
| `bin/` | `sc-play` and `pilab-bench`, installed on the Pi at `/usr/local/bin/` |
| `mac/pilab-desktop.command` | Mac launcher: SSH tunnel plus TigerVNC |
| `packages/` | `uname -a`, `apt-mark showmanual`, and versions of the key packages |
| `bench/` | Latest `pilab-bench` / cyclictest results pulled from the Pi |
| `scripts/pull-from-pi.sh` | Idempotent, read-only refresh of the files above |
| `scripts/capture-tuning.sh` | Read-only kernel/boot/RT tuning snapshot into `kernel/snapshots/` |
| `bin/pilab-airplay`, [docs/airplay.md](./docs/airplay.md) | Optional AirPlay 2 receiver ("pilab") and AirPlay sender to LAN speakers |
| `scripts/build-airplay.sh` | Builds nqptp and shairport-sync (AirPlay 2, PipeWire) from source on the Pi |
| `rust/`, [docs/console.md](./docs/console.md) | Trillium Console and Trillium Synth: synth (`pilab-synth` crate), scenes and routing (`pilab-core`), service and web console (`pilabd`, `pilabctl` / `trillium-base`) |
| `scripts/deploy-console.sh`, `scripts/install-console.sh` | Build and install the console on the Pi (run the first from the Mac) |
| `scripts/fetch-akwf.sh` | Fetches the CC0 AKWF waveform library onto the Pi (never into git) |
| [docs/roadmap-base.md](./docs/roadmap-base.md), [kernel/](./kernel/README.md) | Trillium Base roadmap (hardware, kernel, release tooling); captured kernel config and RT tuning |
| [docs/discovery.md](./docs/discovery.md) | mDNS records, audit, failure modes and fallbacks |
| [docs/console-test-plan.md](./docs/console-test-plan.md), [docs/mac-console-requirements.md](./docs/mac-console-requirements.md), [docs/zpod-convergence.md](./docs/zpod-convergence.md) | Test plan, native Mac console requirements, ZPOD reuse and proposed API changes |

## Trillium Console

`http://pilab.local:8410/` on the home LAN. Get the pairing token in a terminal on the Pi
with `trillium-base pair` (same as `pilabctl pair`). The synth starts quiet (−18 dBFS at most, with a fade-in and an
output limiter). Details in [docs/console.md](./docs/console.md); security notes in
[docs/security.md](./docs/security.md).

## Licence

Copyright (C) 2026 M-of-N. Dual licensed, SPDX `GPL-3.0-or-later OR LicenseRef-Commercial`:

- **GPL-3.0-or-later**: [LICENSE](./LICENSE) (the full GPL text);
- **LicenseRef-Commercial**: commercial terms from M-of-N, see
  [LICENSE-COMMERCIAL.md](./LICENSE-COMMERCIAL.md).

Third-party parts keep their own licences (ZPOD crates, fonts, sample libraries, system
packages).

"Trillium", "Trillium Base", "Trillium Console", "Trillium Synth" and the Rhodonea mark
are trademarks of M-of-N and are not covered by the GPL or the commercial licence; see
[rust/crates/pilabd/web/brand/README.md](./rust/crates/pilabd/web/brand/README.md).

## Refreshing from a running Pi

From a machine that can `ssh lab@pilab.local`:

```sh
./scripts/pull-from-pi.sh
```

The script does not change anything on the Pi. It scrubs SSH public keys out of `boot/user-data` before they can be committed. It never copies TLS keys, `~/.ssh`, or disk images.

## Ground rules

- No radio transmitting, and no flashing of cards, without explicit approval.
- Security tools only against this lab's own gear.
- GitHub only as `nymble`.
- No personal names in code, comments, config, or docs (an optional copyright line is the only exception).
- Never commit secrets, private keys, SSH host keys, passwords, or disk images.

## Status

The machine is running with the RT kernel, DAC, PipeWire, SuperCollider (chord confirmed through the DAC), and a labwc desktop reachable over the SSH-tunneled VNC session. Since 2026-11-20 the console runs as a user service on port 8410; since v0.2.0 it is Trillium Console on Trillium Base, advertised as `_trillium-base._tcp` and `_pilab._tcp`. Ansible roles and image snapshots are later work, not in this tree.
