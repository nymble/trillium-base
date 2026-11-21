<!-- Copyright (C) 2026 M-of-N -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Trillium Base SD card image

Version **0.3.0-beta.1** (beta). One image for the reference build: Raspberry Pi 5 with
the Allo Katana DAC HAT (ES9038Q2M), booting from an SD card. Software is the product; the
hardware is do-it-yourself (see `hardware/`).

The image is built from this repository with Raspberry Pi's
[rpi-image-gen](https://github.com/raspberrypi/rpi-image-gen) (recipe in `image/`). It
contains no SSH keys, passwords, pairing tokens, Wi-Fi credentials or network details:
every unit creates its own on first boot.

## What is in it

| Area | Contents |
| --- | --- |
| Base | Debian 13 trixie arm64 with the Raspberry Pi archive (rpi-image-gen `trixie-minbase`), systemd-networkd (DHCP on Ethernet), OpenSSH, avahi (mDNS) |
| Kernel | Raspberry Pi PREEMPT_RT kernel `linux-image-rpi-v8-rt`, selected with `kernel=kernel8_rt.img` under `[pi5]`; the stock 2712 kernel stays installed as a fallback |
| Boot config | `dtparam=audio=off`, `dtoverlay=allo-katana-dac-audio` |
| CPU isolation | `isolcpus=managed_irq,domain,3 irqaffinity=0-2` on the kernel command line, `trillium-cpu-isolation.service` (moves IRQs, workqueues and kernel threads off CPU 3 at boot), `cpu-performance.service`, systemd `CPUAffinity=0-2`. Same as `kernel/cpu-isolation.sh apply`; `sudo trillium-cpu-isolation status` / `revert` work as on the lab unit |
| Audio | PipeWire + WirePlumber + pipewire-jack/-pulse, 48 kHz, quantum 64 (min 32, max 1024); `audio` group rtprio 95 / memlock unlimited |
| Trillium | `pilabd` (Trillium Console on TCP 8410 + Trillium Synth) as a user service of `lab` started at boot (linger); `pilabctl` and its alias `trillium-base`; AKWF wavetables (CC0) in `/usr/share/pilab/library/akwf`; `pilab-bench` (cyclictest per core) |
| First boot | `tbase-hostname.service` names the unit `tbase-xxxx` (last 4 hex digits of the board serial); `trillium-firstboot.service` installs SSH keys from the boot partition and writes the mDNS record; `trillium-pair-qr.service` shows a pairing QR code on HDMI; cloud-init grows the root filesystem to the card |
| Bluetooth | BlueZ installed, adapter **off by default** (`AutoEnable=false`). `sudo trillium-ble-midi --minutes 15` powers it on and opens a time-boxed BLE MIDI pairing window (`docs/bluetooth-midi.md`) |
| Version | `/etc/trillium-release`; `GET /api/v1/info` reports `version` and `commit` |

Not in the image (lab-unit extras in `BUILD.md`): the desktop and VNC, SuperCollider,
Pure Data, Csound, Faust, Cabbage, AirPlay (shairport-sync, nqptp), NAS mounts.

## Flash

1. Download `trillium-base-0.3.0-beta.1.img.xz` and its `.sha256`, then check it:

       shasum -a 256 -c trillium-base-0.3.0-beta.1.img.xz.sha256

2. Write it with **Raspberry Pi Imager**: *Choose OS → Use custom* → the `.img.xz` file
   → your SD card (16 GB or larger). Imager may offer OS customisation: if you use it, set
   the username to `lab`, turn on SSH with **public-key authentication only** and paste your
   public key. Leave the password empty if Imager allows it. The hostname you type there is
   ignored; the unit names itself `tbase-xxxx`. Wi-Fi from Imager is not applied in this
   beta; use Ethernet.

   Any other image writer works too (for example `xz -dc ... | sudo dd of=/dev/<card> bs=4M`
   on Linux, after checking the device name twice).

## Give it your SSH key (if you did not use Imager customisation)

After writing, the card's boot partition (`BOOT`, FAT) mounts on any computer. Put your
**public** key in a file named `authorized_keys` at the top of it:

```sh
cp ~/.ssh/id_ed25519.pub /Volumes/BOOT/authorized_keys     # macOS
```

On boot, `trillium-firstboot` adds every key line in that file to
`/home/lab/.ssh/authorized_keys` and deletes the file from the boot partition. It runs on
every boot, so dropping a new `authorized_keys` there later adds another key.

Alternatively write a cloud-init `user-data` file to the boot partition (NoCloud), as in
`boot/user-data` (replace `<YOUR_SSH_PUBKEY>`; leave out the `hostname:` line).

SSH accepts public keys only (no passwords); `lab` has no password and uses passwordless
sudo. Without a key there is no remote login; the local console (HDMI + USB keyboard) shows
only the login prompt, because `lab` has no password.

## First boot

Connect Ethernet (and HDMI if you want the pairing QR), insert the card, power on. The
first boot takes a minute or two longer than later ones (host keys, filesystem resize).

- Hostname: `tbase-xxxx`, shown on the HDMI login banner. Find it from a Mac with
  `dns-sd -B _trillium-base._tcp`.
- SSH: `ssh lab@tbase-xxxx.local`. Host keys are generated on this unit at first boot, so
  the fingerprint is new for every unit.
- Console pairing token: created by pilabd on its first start, stored only on the unit
  (`~lab/.config/pilab/console-token`, mode 0600).

## Pair a browser with Trillium Console

On every boot a one-time pairing QR code appears on the HDMI console. It encodes
`http://tbase-xxxx.local:8410/#pair=XXXX-XXXX`, is valid for 5 minutes and works once; the
token itself is never shown or put in the QR. Scan it with a phone, or open the URL and type
the 8-character code. For a new code at any time:

```sh
ssh lab@tbase-xxxx.local trillium-base pair --qr
```

## Check it

```sh
cat /etc/trillium-release
uname -a                                  # ... PREEMPT_RT ... -rpi-v8-rt
cat /sys/devices/system/cpu/isolated      # 3
sudo trillium-cpu-isolation status
aplay -l                                  # Katana present
systemctl --user status pilabd            # as lab
trillium-base status
curl -s http://localhost:8410/api/v1/info
sudo pilab-bench                          # latency, writes /var/lib/pilab/bench/
```

## Build it yourself

On a native arm64 Debian 13 host (a Pi 5, or an arm64 Linux VM or container with
`CAP_SYS_ADMIN`), as a normal user:

```sh
git clone https://github.com/raspberrypi/rpi-image-gen.git && sudo rpi-image-gen/install_deps.sh
sudo apt-get install -y build-essential pkg-config libjack-jackd2-dev git curl xz-utils
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
ZPOD_SRC=/path/to/zpod sh image/build.sh /path/to/outdir
```

`image/build.sh` builds pilabd/pilabctl at the current commit, fetches AKWF, assembles the
overlay in `image/stage/`, runs rpi-image-gen (pinned in the script) with
`image/config/trillium-base.yaml` and the `trillium-base` layer in `image/layer/`, and writes
`trillium-base-<version>.img.xz` plus `.sha256`. It never writes to a block device.

## Known gaps in this beta

- Not yet boot-tested on hardware: this build was checked by mounting the image and running
  pilabd without audio, not on a Pi 5 with the Katana.
- Single boot slot; no A/B updates, no signed images, no secure boot (roadmap).
- Wi-Fi provisioning is not supported yet (Ethernet only).
- The pairing QR is shown on HDMI and in the terminal only; the console Settings page QR is
  still planned.
- The kernel is the stock Raspberry Pi RT build (no `nohz_full`/`rcu_nocbs` on CPU 3).
