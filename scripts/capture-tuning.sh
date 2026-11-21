#!/bin/sh
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Read-only capture of a Trillium Base's kernel, boot and real-time tuning.
# Run from a machine that can `ssh lab@pilab.local`:
#   sh scripts/capture-tuning.sh [host] > kernel/snapshots/$(date +%F)-pilab.txt
# Changes nothing on the device. Scrubs the MAC address the firmware puts on the kernel
# command line, the root PARTUUID and global IPv6 addresses. Prints no secrets (no token, no keys).
set -eu
HOST=${1:-lab@pilab.local}
echo "# Read-only capture from $HOST, $(date +%F), by scripts/capture-tuning.sh."
ssh "$HOST" 'sh -s' <<'REMOTE' | tr -d '\000' | sed -E 's/smsc95xx\.macaddr=[^ ]+/smsc95xx.macaddr=<scrubbed>/; s/root=PARTUUID=[^ ]+/root=PARTUUID=<scrubbed>/; s/2[0-9a-f]{3}:[0-9a-f:]{4,}/<ipv6>/g'
echo "## uname"; uname -a
echo "## cmdline"; cat /proc/cmdline
echo "## cmdline.txt"; cat /boot/firmware/cmdline.txt
echo "## config.txt"; cat /boot/firmware/config.txt
echo "## autoboot/tryboot"; ls -l /boot/firmware/autoboot.txt /boot/firmware/tryboot.txt 2>&1 || true
echo "## kernel pkgs"; dpkg -l 'linux-image*' raspi-firmware rpi-eeprom 2>/dev/null | grep ^ii || true
echo "## bootloader"; vcgencmd bootloader_version 2>&1 || true
echo "## isolated"; cat /sys/devices/system/cpu/isolated; echo "nohz_full: $(cat /sys/devices/system/cpu/nohz_full 2>/dev/null)"
echo "## governor"; cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort | uniq -c
echo "## irq affinity (count by cpu list)"; for d in /proc/irq/[0-9]*; do cat $d/smp_affinity_list; done | sort | uniq -c
echo "## default_smp_affinity"; cat /proc/irq/default_smp_affinity
echo "## irqbalance"; systemctl is-active irqbalance 2>&1 || true
echo "## systemd affinity"; grep -rh -E '^\s*CPUAffinity' /etc/systemd/ 2>/dev/null || true
echo "## limits"; grep -h -v '^#' /etc/security/limits.d/*.conf 2>/dev/null || true; ulimit -r; ulimit -l
echo "## pw settings"; pw-metadata -n settings 2>&1 | head -12 || true
echo "## rt threads"; ps -eLo pid,cls,rtprio,psr,comm | awk '$2!="TS"' | grep -v -E 'ksoftirq|migration|cpuhp|idle_inject|rcuc|ktimer|irq/' | head -30
echo "## irq threads prio"; ps -eLo cls,rtprio,psr,comm | grep 'irq/' | grep -i -E 'i2s|snd|usb|xhci|eth|mmc|dma' | head -20
echo "## storage"; lsblk -o NAME,SIZE,TYPE,MOUNTPOINT,FSTYPE
echo "## usb"; lsusb
echo "## os"; . /etc/os-release; echo "$PRETTY_NAME"; cat /proc/device-tree/model; echo
REMOTE
