#!/bin/sh
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# From the Mac: copy this repo and the pinned ZPOD crates to the Pi, then build and install
# Trillium Base (pilabd, Trillium Console) there. A few ssh sessions; nothing is pushed anywhere else.
#   sh scripts/deploy-console.sh [zpod-checkout-or-bundle]
# The ZPOD source is a local zpod clone (bundled at the pinned commit) or a ready bundle.
set -eu
HOST=${PILAB_HOST:-lab@pilab.local}
REPO=$(cd "$(dirname "$0")/.." && pwd)
ZPOD=${1:-${ZPOD_SRC:-$HOME/cb/tmp/zpod-pin.bundle}}
REV=$(sed -n 's/.*rev = "\([0-9a-f]*\)".*/\1/p' "$REPO/rust/Cargo.toml" | head -1)

if [ -d "$ZPOD/.git" ] || [ -d "$ZPOD/objects" ]; then
  B=$(mktemp -d)/zpod-pin.bundle
  git -C "$ZPOD" branch -f pilab-pin "$REV" >/dev/null
  git -C "$ZPOD" bundle create -q "$B" pilab-pin
  ZPOD=$B
fi
git bundle verify -q "$ZPOD" >/dev/null 2>&1 || true

# tar over ssh: no rsync needed on either side. The build directory lives outside the
# tree on the Pi (~/.cache/pilab/target), so replacing the tree keeps incremental builds.
COPYFILE_DISABLE=1 tar -C "$REPO" --exclude ./.git --exclude ./rust/target --exclude ./rust/.cargo -czf - . |
  ssh "$HOST" 'set -e; mkdir -p ~/src; rm -rf ~/src/pilab.new; mkdir ~/src/pilab.new; tar -xzf - -C ~/src/pilab.new; rm -rf ~/src/pilab; mv ~/src/pilab.new ~/src/pilab'
C=$(git -C "$REPO" rev-parse --short=12 HEAD 2>/dev/null || echo unknown)
[ -z "$(git -C "$REPO" status --porcelain 2>/dev/null)" ] || C="$C-dirty"
echo "$C" | ssh "$HOST" 'cat > ~/src/pilab/COMMIT'
ssh "$HOST" 'cat > ~/src/zpod-pin.bundle' < "$ZPOD"
ssh "$HOST" 'cd ~/src/pilab && sh scripts/install-console.sh ~/src/zpod-pin.bundle'
