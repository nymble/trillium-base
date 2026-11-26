<!-- Copyright (C) 2026 Paul Lambert -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial -->
# Robustness review

How Trillium Base behaves when things go wrong, what is covered, and what to fix next.
Measured on the lab unit on 2026-11-24 (Pi 5 16 GB, 32 GB SD, kernel 6.18.60 RT, console
0.4.0-beta.1). IT Department owns backups and hardening baselines; this page lists what they
need to cover rather than doing it.

## What is already solid

| Area | State |
| --- | --- |
| Service restart | `pilabd` and the AirPlay sender restart on failure after 3 s; no failed units on the lab unit |
| Hardware watchdog | `/dev/watchdog` exists and systemd's runtime watchdog is on (1 minute), so a hung kernel or init resets the board |
| Boot time | 10.8 s to the graphical target; `pilabd` waits up to 60 s for the DAC before giving up |
| USB hotplug | A Trillium Zero or a USB microphone replugged is found again within seconds, its profile follows it, and a daemon restart is not mistaken for a reconnect |
| Filesystem | ext4, `noatime`, `fsck.repair=yes`; the boot partition mounts `errors=remount-ro` |
| Clock | `systemd-timesyncd` active and synchronised; the Pi 5 RTC (`rpi-rtc`) is present |
| Writes | Scenes and device profiles are written to a temp file and renamed. **New:** settings and state files are too |
| Safe audio defaults | Master starts at -18 dBFS with a fade-in and a limiter, so a restart cannot be loud |
| Persistent logs | Journald is persistent as of today (17 MB so far), so a dropout after a reboot can now be diagnosed |
| CPU isolation | CPU 3 is isolated by a repeatable script with backups of `cmdline.txt` and a `revert` command |

## Gaps, ranked

| # | Gap | Impact | Fix |
| --- | --- | --- | --- |
| R1 | No health supervision inside `pilabd`: `Restart=on-failure` only catches a crash, not a hang (a deadlock, a wedged audio callback). The unit is not `Type=notify` and has no `WatchdogSec` | Silence until someone notices | **next**: `sd_notify` READY and a watchdog ping every 2 s from a thread that checks the audio callback counter and the control loop; `WatchdogSec=10`, `Restart=always` |
| R2 | Updates are single-slot with no rollback: a bad deploy leaves the console down until a new build | Console and sound down | **done**: `install-console.sh` keeps `pilabd.prev` and `pilabctl.prev`; `scripts/rollback-console.sh` swaps them back. **next**: A/B `tryboot` for the image (below) |
| R3 | No backup or restore of `~/.config/pilab` (token, settings, scenes, device profiles) and the recordings | A dead SD card loses the work | **IT**: nightly copy of both folders to the NAS. **next**: `trillium-base backup` and `restore` (a tar of the config folder, 5 kept, restore refuses to run while the service is up) |
| R4 | Config corruption: a half-written `settings.toml` or `state.toml` after a power cut, then silently replaced by defaults | Lost settings | **done**: atomic writes; a file that fails to parse is kept as `settings.toml.bad-<time>` and the log says so. **next**: keep the last good copy as `settings.toml.prev` and use it before falling back to defaults |
| R5 | Unbounded growth on the SD card: the apt cache holds 1.1 GB, the Rust build cache 1.2 GB and `~/src` 0.9 GB on the lab unit; an unused 2 GB `/var/swap` file left on the card (swap itself is zram, in RAM) | Disk full, card wear | **IT**: `apt-get clean`, delete `/var/swap`, cap build caches. **done (repo)**: `etc/journald-persistent.conf` caps the journal at 200 MB and keeps 2 GB free. Recordings stop below `min_free_mb` (2048) |
| R6 | A boot without NTP (studio LAN with no internet): timesyncd cannot sync, there is no `fake-hwclock`, and the RTC only keeps time with a battery. The pairing code expiry, recording names and TLS certificates all use the wall clock | A code that never expires or expires at once; recordings dated wrongly | **next**: expire pairing codes on the monotonic clock; show "clock not synchronised" in the console; enable RTC battery charging in `config.txt` and fall back to the router as an NTP source |
| R7 | PipeWire restarts (a package upgrade, a crash) drop `pilabd`'s audio client and its links. Cabbage's two links must be relaunched by hand | Silence after an upgrade | **next**: test `systemctl --user restart pipewire` on the lab unit; have `pilabd` exit when the JACK client is shut down so systemd restarts it; remember patch-panel links so they are restored for any client that returns (Cabbage included). Not tested today because it interrupts audio |
| R8 | One burst of dropouts about 80 s after a boot, cause unknown (logs did not persist) | Glitch right after boot | Persistent logs are on. **owner**: approve a one-hour dropout test and a few reboots |
| R9 | The published image (v0.3.0-beta.1) has not been booted on hardware | Unknown first-boot failures | **owner**: flash a spare card and run the checks in `docs/image.md` before any non-beta tag |
| R10 | No self-check command, so support means reading logs | Slow diagnosis | **next**: `trillium-base doctor`: clock sync, free space, DAC present, links, isolation, RT kernel, xruns, failed units, pending security updates, journald size, token file mode |
| R11 | `nqptp.service` had `Restart=no`, so a crash ended AirPlay until reboot | AirPlay dead | **done (repo)**: `Restart=on-failure`; **IT** or the next image applies it to the lab unit |
| R12 | Power loss on the SD card: ext4 ordered mode survives, but a cut during a write to the boot partition (FAT) can break boot | Unit does not boot | Keep boot writes rare (only `cpu-isolation.sh`, which backs up first). Longer term, a safe-shutdown button (`gpio-shutdown` overlay) and a read-only root |

## SD card wear and the read-only option

What writes to the card today: recordings (WAV, the largest), the journal, and scene, profile and
state saves (tiny). Swap is zram, so it does not touch the card. Recordings are the user's data and should go to a
bigger, better medium: NVMe on the Pi 5's PCIe connector or a USB SSD, which `docs/roadmap-base.md`
already prefers for A/B.

| Option | Verdict |
| --- | --- |
| `log2ram` | Not needed: the journal is small and now capped; it adds a way to lose the last minutes before a crash, which is when logs matter |
| Overlay read-only root (`raspi-config nonint enable_overlayfs`) | Works for a locked-down unit but breaks every write the console makes unless config and recordings live on a separate writable partition. Do it with the A/B layout, not before |
| Separate `/data` partition (config, scenes, recordings) with a read-only root | **Recommended** for v1: the root can be replaced by an update or reset without touching user data, and the root can be mounted read-only |
| Swap | Already zram (RAM): keep it; delete the unused `/var/swap` file |

## Update and rollback

| Stage | Today | Next |
| --- | --- | --- |
| Console binaries | Deploy from a Mac replaces `pilabd`; previous kept as `.prev`; rollback script | Signed release tarball, verified by an update client |
| Image | New card flashed from `.img.xz` | A/B with `tryboot` (`autoboot.txt`, `tryboot_a_b=1`): the update client writes the idle slot, reboots once into it, and marks it good only after `pilabd` has reported healthy (R1 gives it a health signal) |
| Kernel and CPU isolation | `kernel/cpu-isolation.sh apply|revert` with backups | Part of the image slot so a rollback restores the matching command line |
| Data | Not versioned | `/data` partition survives slot changes; config files carry a schema version (scenes already do) |

## Verification to run

1. Pull the power during a settings save, ten times (script: loop `PUT /settings` while a relay or
   a hard reset cuts power); check the file parses every time.
2. `systemctl --user restart pipewire` while a note is held; check recovery and links.
3. Fill the recordings folder to `min_free_mb`; check the stop and the message.
4. Boot with the network cable out; check the clock message, pairing code expiry and recording names.
5. Three reboots with persistent logs, then the one-hour dropout test.

## For IT Department

Backups of `~/.config/pilab` and `~/.local/share/pilab`; `apt-get clean` and the unused `/var/swap` file; the
hardening items are in `docs/security.md`.
