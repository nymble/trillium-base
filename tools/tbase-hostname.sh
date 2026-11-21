#!/bin/sh
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
#
# Trillium Base unit hostname: tbase-XXXX, where XXXX is the last 4 hex digits of the
# board serial number (Raspberry Pi: device tree serial-number / cpuinfo Serial), or of the
# first wired/wireless MAC address when no serial is available. Lower case, stable across
# reflashes of the same board, short enough to type: tbase-1a2b.local
#
#   tbase-hostname.sh            print the name this board should have
#   tbase-hostname.sh --apply    set it (hostnamectl, /etc/hosts), restart avahi, and
#                                record /var/lib/trillium/hostname-set so it runs once
#   tbase-hostname.sh --first-boot
#                                --apply, but only if it has never run and the current
#                                hostname is an image default (raspberrypi, trillium,
#                                tbase, localhost). Used by tbase-hostname.service.
#
# The lab unit keeps its existing name; nothing renames a host that was named by hand.
set -eu
PREFIX=${TBASE_PREFIX:-tbase}
STAMP=/var/lib/trillium/hostname-set

suffix() {
    s=""
    for f in /sys/firmware/devicetree/base/serial-number /proc/device-tree/serial-number; do
        [ -r "$f" ] && s=$(tr -d '\000' < "$f") && break
    done
    [ -n "$s" ] || s=$(awk -F': *' '/^Serial/ {print $2; exit}' /proc/cpuinfo 2>/dev/null || true)
    s=$(printf '%s' "$s" | tr -cd '0-9a-fA-F')
    if [ "${#s}" -lt 4 ] || [ -z "$(printf '%s' "$s" | tr -d 0)" ]; then
        for i in eth0 end0 wlan0; do
            [ -r /sys/class/net/$i/address ] && s=$(tr -d ':\n' < /sys/class/net/$i/address) && break
        done
    fi
    [ "${#s}" -ge 4 ] || { echo "tbase-hostname: no serial or MAC found" >&2; exit 1; }
    printf '%s' "$s" | tail -c 4 | tr 'A-F' 'a-f'
}

name="$PREFIX-$(suffix)"

apply() {
    [ "$(id -u)" = 0 ] || { echo "tbase-hostname: run as root" >&2; exit 1; }
    old=$(hostname)
    if [ "$old" != "$name" ]; then
        hostnamectl set-hostname "$name" 2>/dev/null || { echo "$name" > /etc/hostname; hostname "$name"; }
        if grep -q '^127\.0\.1\.1' /etc/hosts; then
            sed -i "s/^127\.0\.1\.1.*/127.0.1.1\t$name/" /etc/hosts
        else
            printf '127.0.1.1\t%s\n' "$name" >> /etc/hosts
        fi
        systemctl try-restart avahi-daemon.service 2>/dev/null || true
    fi
    mkdir -p "$(dirname "$STAMP")"
    echo "$name" > "$STAMP"
    echo "hostname: $old -> $name"
}

case ${1:-} in
    "") echo "$name" ;;
    --apply) apply ;;
    --first-boot)
        [ -e "$STAMP" ] && exit 0
        case $(hostname) in
            raspberrypi|trillium|tbase|localhost|"") apply ;;
            *) mkdir -p "$(dirname "$STAMP")"; hostname > "$STAMP"; echo "hostname: keeping $(hostname)" ;;
        esac ;;
    *) sed -n '5,18p' "$0"; exit 2 ;;
esac
