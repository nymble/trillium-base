#!/bin/sh
# Copyright (C) 2026 Nymble
# SPDX-License-Identifier: GPL-3.0-or-later
# Build and install the Trillium Base service (pilabd + pilabctl, plus the `trillium-base`
# alias for pilabctl) on the Pi, as user lab.
#   sh scripts/install-console.sh [path/to/zpod-pin.bundle]
# Run from the repo root on the Pi (scripts/deploy-console.sh does this from the Mac).
# - installs a minimal Rust toolchain in ~/.rustup and ~/.cargo if cargo is missing
# - resolves the private ZPOD crates from a git bundle (no GitHub credentials on the Pi)
# - installs binaries to ~/.local/bin, the user unit, and the avahi service file (sudo)
# - fetches the AKWF library if it is not there yet
# Does not print the pairing token. Get it with:  pilabctl pair
set -eu
REPO=$(cd "$(dirname "$0")/.." && pwd)
BUNDLE=${1:-$HOME/src/zpod-pin.bundle}
MIRROR=$HOME/.cache/pilab/zpod.git
BIN=$HOME/.local/bin

need_pkgs=""
for p in pkg-config libjack-jackd2-dev; do dpkg -s "$p" >/dev/null 2>&1 || need_pkgs="$need_pkgs $p"; done
[ -z "$need_pkgs" ] || sudo apt-get -o DPkg::Lock::Timeout=600 install -y -q --no-install-recommends $need_pkgs

if ! command -v cargo >/dev/null 2>&1 && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  echo "installing Rust (rustup, minimal profile)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain stable --no-modify-path
fi
PATH=$HOME/.cargo/bin:$PATH

if [ -f "$BUNDLE" ]; then
  if [ -d "$MIRROR" ]; then git -C "$MIRROR" fetch -q "$BUNDLE" '+refs/heads/*:refs/heads/*'
  else mkdir -p "$(dirname "$MIRROR")"; git clone -q --mirror "$BUNDLE" "$MIRROR"; fi
fi
[ -d "$MIRROR" ] || { echo "no ZPOD mirror: pass the zpod bundle path" >&2; exit 1; }

cd "$REPO/rust"
export CARGO_TARGET_DIR=$HOME/.cache/pilab/target
CARGO_NET_GIT_FETCH_WITH_CLI=true GIT_CONFIG_COUNT=1 \
GIT_CONFIG_KEY_0="url.file://$MIRROR.insteadOf" GIT_CONFIG_VALUE_0=https://github.com/nymble/zpod \
  PILAB_COMMIT=$(cat "$REPO/COMMIT" 2>/dev/null || echo unknown) cargo build --release --locked -p pilabd

mkdir -p "$BIN" "$HOME/.config/systemd/user"
install -m 755 "$CARGO_TARGET_DIR/release/pilabd" "$CARGO_TARGET_DIR/release/pilabctl" "$BIN"/
ln -sf pilabctl "$BIN/trillium-base"
install -m 644 "$REPO/home/.config/systemd/user/pilabd.service" "$HOME/.config/systemd/user/pilabd.service"

TMP=$(mktemp)
"$BIN/pilabd" mdns-service > "$TMP"
if ! cmp -s "$TMP" /etc/avahi/services/pilab-console.service 2>/dev/null; then
  sudo install -m 644 "$TMP" /etc/avahi/services/pilab-console.service
fi
rm -f "$TMP"

[ -d "$HOME/.local/share/pilab/library/akwf" ] || sh "$REPO/scripts/fetch-akwf.sh"

systemctl --user daemon-reload
systemctl --user enable pilabd.service >/dev/null 2>&1
systemctl --user restart pilabd.service
sleep 2
systemctl --user --no-pager --lines=0 status pilabd.service | head -5
echo "Trillium Console: http://$(hostname).local:8410/   pairing token: run 'trillium-base pair' on the Pi"
