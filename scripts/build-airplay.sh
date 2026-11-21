#!/bin/sh
# Copyright (C) 2026 Nymble
# SPDX-License-Identifier: GPL-3.0-or-later
# Build and install the AirPlay 2 receiver (nqptp + shairport-sync with the PipeWire
# backend) on the Pi. Debian's shairport-sync 4.3.7 package is classic AirPlay only and
# Debian has no nqptp package, so both are built from source.
# Run on the Pi as user lab:  sh build-airplay.sh     (about 1 minute on a Pi 5)
# Installs to /usr/local/bin; nqptp.service lands in /usr/lib/systemd/system, disabled.
# Nothing is enabled or started; use "pilab-airplay receiver on" afterwards.
set -eu
NQPTP_REF=1.2.9
SPS_REF=5.6.3
SRC="$HOME/src"

sudo apt-get -o DPkg::Lock::Timeout=600 install -y -q --no-install-recommends \
  build-essential git autoconf automake libtool pkg-config xxd systemd-dev \
  libpopt-dev libconfig-dev libasound2-dev avahi-daemon avahi-utils libavahi-client-dev \
  libssl-dev libsoxr-dev libplist-dev libplist-utils libsodium-dev uuid-dev libgcrypt20-dev \
  libavutil-dev libavcodec-dev libavformat-dev libswresample-dev \
  libpipewire-0.3-dev libspa-0.2-dev

mkdir -p "$SRC"
[ -d "$SRC/nqptp/.git" ] || git clone -q --depth 1 --branch "$NQPTP_REF" https://github.com/mikebrady/nqptp "$SRC/nqptp"
[ -d "$SRC/shairport-sync/.git" ] || git clone -q --depth 1 --branch "$SPS_REF" https://github.com/mikebrady/shairport-sync "$SRC/shairport-sync"

cd "$SRC/nqptp"
autoreconf -fi
./configure --with-systemd-startup
make -j3
sudo make install
sudo systemctl disable --now nqptp.service 2>/dev/null || true

cd "$SRC/shairport-sync"
autoreconf -fi
# No --with-systemd-startup: it must run as a user service next to PipeWire.
./configure --sysconfdir=/etc --with-airplay-2 --with-pipewire --with-alsa \
  --with-soxr --with-avahi --with-ssl=openssl --with-metadata
make -j3
# make install also writes /etc/shairport-sync.conf(.sample) if none exists;
# the repo's etc/shairport-sync.conf replaces it.
sudo make install
/usr/local/bin/shairport-sync -V
