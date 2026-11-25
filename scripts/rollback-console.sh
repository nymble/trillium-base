#!/bin/sh
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Put back the previous pilabd and pilabctl (kept by install-console.sh) and restart the service.
# Run on the Pi as lab. Settings and scenes are not touched. Swapping twice returns to the newer build.
set -eu
BIN=$HOME/.local/bin
[ -x "$BIN/pilabd.prev" ] || { echo "no previous build kept in $BIN (pilabd.prev)" >&2; exit 1; }
for b in pilabd pilabctl; do
  [ -x "$BIN/$b.prev" ] || continue
  mv "$BIN/$b" "$BIN/$b.swap"
  mv "$BIN/$b.prev" "$BIN/$b"
  mv "$BIN/$b.swap" "$BIN/$b.prev"
done
systemctl --user restart pilabd.service
sleep 2
"$BIN/pilabctl" status >/dev/null 2>&1 && echo "rolled back: $("$BIN/pilabd" --version 2>&1 | head -1)" || echo "service did not answer; check: journalctl --user -u pilabd -n 50"
