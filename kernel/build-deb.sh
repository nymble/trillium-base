#!/bin/sh
# Reproducible build recipe for the Trillium Base RT kernel as Debian packages.
# Status: recipe only, not run yet. Runs on any Linux build machine (cross-compile for
# arm64) or natively on a Pi 5 with ~10 GB free. It never installs or flashes anything.
#   sh kernel/build-deb.sh /path/to/workdir
# Output: linux-image-*-trillium-rt_*.deb and linux-headers-*.deb in the workdir.
# Copyright (C) 2026 Nymble. GPL-3.0-or-later.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=${1:-$PWD/kbuild}
# Source: the Raspberry Pi kernel tree. The exact commit behind Debian package
# 1:6.18.60-1+rpt1 still has to be pinned (see kernel/README.md); until then use the
# branch head and record the commit in the build log.
REPO=${KERNEL_REPO:-https://github.com/raspberrypi/linux}
REF=${KERNEL_REF:-rpi-6.18.y}
BASE="$HERE/configs/6.18.60+rpt-rpi-v8-rt.config"
FRAG="$HERE/trillium-rt.fragment"

mkdir -p "$WORK"; cd "$WORK"
[ -d linux ] || git clone --depth 1 --branch "$REF" "$REPO" linux
cd linux
echo "kernel source commit: $(git rev-parse HEAD)"
ARCH=arm64; export ARCH
case "$(uname -m)" in aarch64) ;; *) CROSS_COMPILE=${CROSS_COMPILE:-aarch64-linux-gnu-}; export CROSS_COMPILE ;; esac
# Reproducibility: fixed timestamps and build identity.
KBUILD_BUILD_TIMESTAMP=${KBUILD_BUILD_TIMESTAMP:-$(git log -1 --format=%cd)}
KBUILD_BUILD_USER=trillium KBUILD_BUILD_HOST=build
export KBUILD_BUILD_TIMESTAMP KBUILD_BUILD_USER KBUILD_BUILD_HOST

cp "$BASE" .config
./scripts/kconfig/merge_config.sh -m .config "$FRAG"
make olddefconfig
# Fail if a requested option did not survive olddefconfig.
grep -v '^#' "$FRAG" | grep '=' | while read -r l; do grep -qx "$l" .config || { echo "missing after merge: $l"; exit 1; }; done
make -j"$(nproc)" bindeb-pkg
ls -1 ../*.deb
