#!/bin/sh
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
#
# Build the Trillium Base SD card image (Raspberry Pi 5, Debian 13 trixie arm64).
#
#   ZPOD_SRC=/path/to/zpod-checkout-or-bundle sh image/build.sh [outdir]
#
# Runs on a native arm64 Debian 13 host (a Pi 5, or an arm64 Linux VM/container with
# CAP_SYS_ADMIN) as a normal user. Prerequisites, once:
#   - rpi-image-gen's dependencies:  sudo ./install_deps.sh   (in an rpi-image-gen checkout)
#   - Rust build deps: build-essential pkg-config libjack-jackd2-dev git curl, and cargo
#     (rustup stable) on PATH
# Steps: build pilabd/pilabctl for aarch64 at this commit, fetch the AKWF wavetables (CC0),
# assemble the root filesystem overlay in image/stage/, run rpi-image-gen with
# image/config/trillium-base.yaml, then write <outdir>/trillium-base-<version>.img.xz and
# its .sha256. Nothing is flashed or published.
set -eu
IMAGE_DIR=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$IMAGE_DIR/.." && pwd)
VERSION=$(cat "$IMAGE_DIR/VERSION")
OUT=${1:-$IMAGE_DIR/out}
STAGE=$IMAGE_DIR/stage
CACHE=${TBASE_CACHE:-$HOME/.cache/trillium-image}
# rpi-image-gen, pinned (override with RPI_IMAGE_GEN=/path/to/checkout)
RIG_URL=https://github.com/raspberrypi/rpi-image-gen.git
RIG_REV=${RIG_REV:-56ce7c6180fc2e6ac143f6b43ae978949fe22fe7}
RIG=${RPI_IMAGE_GEN:-$CACHE/rpi-image-gen}
ZPOD_SRC=${ZPOD_SRC:?set ZPOD_SRC to a zpod checkout or a git bundle holding the pinned commit}
COMMIT=$(git -C "$REPO" rev-parse HEAD)
SHORT=$(git -C "$REPO" rev-parse --short=7 HEAD)
DIRTY=$(git -C "$REPO" status --porcelain --untracked-files=no | grep -q . && echo "-dirty" || true)
NAME=trillium-base-$VERSION

[ "$(uname -m)" = aarch64 ] || { echo "build.sh: needs a native arm64 host" >&2; exit 1; }
mkdir -p "$CACHE" "$OUT"

echo "== Trillium Base $VERSION ($SHORT$DIRTY)"

# 1. pilabd + pilabctl
MIRROR=$CACHE/zpod.git
if [ -d "$MIRROR" ]; then git -C "$MIRROR" fetch -q "$ZPOD_SRC" '+refs/heads/*:refs/heads/*'
else git clone -q --mirror "$ZPOD_SRC" "$MIRROR"; fi
export CARGO_TARGET_DIR=$CACHE/target
( cd "$REPO/rust" &&
  CARGO_NET_GIT_FETCH_WITH_CLI=true GIT_CONFIG_COUNT=1 \
  GIT_CONFIG_KEY_0="url.file://$MIRROR.insteadOf" GIT_CONFIG_VALUE_0=https://github.com/nymble/zpod \
  PILAB_COMMIT="$SHORT$DIRTY" cargo build --release --locked -p pilabd )
BIN=$CARGO_TARGET_DIR/release
ls -l "$BIN/pilabd" "$BIN/pilabctl"

# 2. AKWF single-cycle wavetables (CC0), into the base library tier
AKWF=$CACHE/akwf
[ -d "$AKWF/AKWF_0001" ] || PILAB_AKWF_DIR=$AKWF XDG_CACHE_HOME=$CACHE sh "$REPO/scripts/fetch-akwf.sh"

# 3. Root filesystem overlay
rm -rf "$STAGE"
R=$STAGE/rootfs
mkdir -p "$R"
cp -a "$IMAGE_DIR/rootfs/." "$R/"
install -D -m 755 "$BIN/pilabd" "$R/usr/local/bin/pilabd"
install -D -m 755 "$BIN/pilabctl" "$R/usr/local/bin/pilabctl"
ln -sf pilabctl "$R/usr/local/bin/trillium-base"
install -D -m 755 "$REPO/bin/pilab-bench" "$R/usr/local/bin/pilab-bench"
install -D -m 755 "$REPO/tools/tbase-hostname.sh" "$R/usr/local/sbin/tbase-hostname"
install -D -m 755 "$REPO/kernel/cpu-isolation.sh" "$R/usr/local/sbin/trillium-cpu-isolation"
install -D -m 755 "$REPO/tools/ble-midi-advertise.py" "$R/usr/local/sbin/trillium-ble-midi"
install -D -m 644 "$REPO/etc/systemd/system/tbase-hostname.service" "$R/etc/systemd/system/tbase-hostname.service"
install -D -m 644 "$REPO/etc/systemd/system/cpu-performance.service" "$R/etc/systemd/system/cpu-performance.service"
install -D -m 644 "$REPO/etc/systemd/system.conf.d/10-cpu-affinity.conf" "$R/etc/systemd/system.conf.d/10-cpu-affinity.conf"
install -D -m 644 "$REPO/etc/security/limits.d/95-audio.conf" "$R/etc/security/limits.d/95-audio.conf"
install -D -m 644 "$REPO/etc/pipewire/pipewire.conf.d/10-lowlatency.conf" "$R/etc/pipewire/pipewire.conf.d/10-lowlatency.conf"
# Same unit as kernel/cpu-isolation.sh installs with `apply` (the command line part is set
# at build time by the layer's cmdline.d fragment).
cat > "$R/etc/systemd/system/trillium-cpu-isolation.service" <<UNIT
# Installed by the Trillium Base image (same unit as kernel/cpu-isolation.sh apply)
[Unit]
Description=Trillium Base: keep IRQs and kernel work off the audio core (CPU 3)
After=systemd-modules-load.service systemd-udev-trigger.service

[Service]
Type=oneshot
Environment=AUDIO_CPU=3 HOUSEKEEPING=0-2
ExecStart=/usr/local/sbin/trillium-cpu-isolation runtime
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
UNIT
# pilabd as a user service of lab, binaries in /usr/local/bin
mkdir -p "$R/etc/systemd/user"
sed -e 's|%h/.local/bin/pilabd|/usr/local/bin/pilabd|' \
    -e 's|^# Installed by scripts/install-console.sh.*|# Installed by the Trillium Base image; enabled for lab (linger on).|' \
    "$REPO/home/.config/systemd/user/pilabd.service" > "$R/etc/systemd/user/pilabd.service"
grep -q '/usr/local/bin/pilabd serve' "$R/etc/systemd/user/pilabd.service"
mkdir -p "$R/usr/share/pilab/library"
cp -a "$AKWF" "$R/usr/share/pilab/library/akwf"
rm -rf "$R/usr/share/pilab/library/akwf/.git"
D=$R/usr/share/doc/trillium-base
install -D -m 644 "$REPO/LICENSE" "$D/LICENSE"
install -m 644 "$REPO/LICENSE-COMMERCIAL.md" "$REPO/README.md" "$REPO/docs/image.md" "$REPO/docs/bluetooth-midi.md" "$D/"
cat > "$R/etc/trillium-release" <<REL
NAME="Trillium Base"
VERSION="$VERSION"
VERSION_ID=$VERSION
BUILD_COMMIT=$COMMIT$DIRTY
BUILD_DATE=$(date -u +%Y-%m-%dT%H:%M:%SZ)
BASE="Debian GNU/Linux 13 (trixie), Raspberry Pi archive"
HOME_URL=https://github.com/nymble/trillium-base
REL

# 4. rpi-image-gen
if [ ! -d "$RIG/.git" ]; then git clone -q "$RIG_URL" "$RIG"; fi
git -C "$RIG" fetch -q origin 2>/dev/null || true
git -C "$RIG" checkout -q "$RIG_REV"
WORK=$CACHE/work
mkdir -p "$WORK"
( cd "$RIG" && ./rpi-image-gen build -S "$IMAGE_DIR" -c trillium-base.yaml -B "$WORK" )
IMG=$(ls "$WORK"/image-*/trillium-base.img)

# 5. Compress and checksum
xz -T0 -9 -c "$IMG" > "$OUT/$NAME.img.xz"
( cd "$OUT" && sha256sum "$NAME.img.xz" > "$NAME.img.xz.sha256" )
echo "== $OUT/$NAME.img.xz"
ls -l "$IMG" "$OUT/$NAME.img.xz"
cat "$OUT/$NAME.img.xz.sha256"
