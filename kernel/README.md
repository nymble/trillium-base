# Trillium Base kernel

What the lab unit runs today, captured read-only, plus the recipe and proposed deltas for
a reproducible Trillium Base kernel. Nothing here has been built, installed or booted yet.

| File | What |
| --- | --- |
| `configs/6.18.60+rpt-rpi-v8-rt.config` | Full config of the running kernel (`/boot/config-6.18.60+rpt-rpi-v8-rt`), unmodified |
| `snapshots/2026-11-20-pilab.txt` | Boot and RT tuning snapshot (`scripts/capture-tuning.sh`, MAC scrubbed) |
| `trillium-rt.fragment` | Proposed config deltas, each with its reason |
| `cmdline-proposed.txt` | Proposed kernel command-line additions (CPU 3 isolation) |
| `build-deb.sh` | Build recipe: stock config + fragment → `.deb` packages (not run yet) |

## Running today (2026-11-20)

- Raspberry Pi 5 Model B Rev 1.1, bootloader 2025-08-28, 32 GB SD card (no NVMe), Debian 13 trixie.
- Kernel `6.18.60+rpt-rpi-v8-rt`, Debian package `linux-image-rpi-v8-rt 1:6.18.60-1+rpt1`:
  the stock Raspberry Pi PREEMPT_RT build, selected with `kernel=kernel8_rt.img` under
  `[pi5]` in `config.txt`. 4K pages (the `v8` flavour; the non-RT `2712` flavour uses 16K),
  `HZ=250`, `NO_HZ_IDLE`, no `NO_HZ_FULL`, modules unsigned, default governor ondemand.
- CPU tuning is userspace only:
  - systemd `CPUAffinity=0-2` (`etc/systemd/system.conf.d/10-cpu-affinity.conf`) keeps
    services off CPU 3; pilabd's audio thread is placed on CPU 3.
  - `cpu-performance.service` sets all four cores to `performance` (2.4 GHz).
  - **No kernel isolation**: `isolcpus`, `nohz_full` and `rcu_nocbs` are unset and 49 of
    53 IRQs may run on any core (irqbalance is off). CPU 3 still takes kernel threads,
    timer ticks and IPIs.
- Limits: `@audio` and `@pipewire` rtprio 95, memlock unlimited / 4 GiB, nice −19.
- PipeWire 1.4.2: 48 kHz, quantum 64 (1.33 ms), min 32, max 1024
  (`etc/pipewire/pipewire.conf.d`). The DAC is an Allo Katana HAT (`allo-katana-dac-audio`),
  onboard audio off.
- No A/B boot: no `autoboot.txt` or `tryboot.txt`; one boot partition.
- Latency (`bench/20261118-1835-…`, cyclictest, 4 threads, P95): idle max 9–14 µs, under
  load max 81–94 µs. That is well inside the 1.33 ms period.

## Reproducing the running kernel

The running kernel is a Debian package, so "reproduce" today means: pin
`linux-image-rpi-v8-rt=1:6.18.60-1+rpt1` and keep `configs/` as the record. Building it
from source means fetching the matching `raspberrypi/linux` commit. The package source
(`apt source` from archive.raspberrypi.com) names it; that commit is **not yet pinned**
here.

## Building the Trillium kernel

`sh kernel/build-deb.sh WORKDIR` starts from the captured config and merges
`trillium-rt.fragment`. It fails if an option is lost, and builds `bindeb-pkg` with fixed
timestamps. The custom image will not replace `kernel8_rt.img` by itself. Installing it
means pointing `kernel=` at it alongside the stock RT kernel, ideally through `tryboot`
so a bad kernel falls back. That step changes the boot partition and needs approval.

## Measuring

- `pilab-bench` (installed at `/usr/local/bin`) runs cyclictest idle and under load and
  writes `bench/<date>-<kernel>.txt`. Compare max latency per CPU, especially CPU 3.
- Xruns: `/api/v1/status` `xruns` and `xrun_rate`, and `pw-top` ERR column. Target: zero
  xruns over a 1-hour soak at quantum 64 with the synth at 32 voices, then try 32.
- Before/after every kernel or cmdline change: same bench, same load, same temperature
  check (`vcgencmd measure_temp`, `get_throttled` = 0x0).
