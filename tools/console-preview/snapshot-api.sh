#!/bin/sh
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# On the device: save every read-only API answer as JSON so the console can be reviewed
# without the device (mock-server.js). The token is read from its file and never printed.
#   sh snapshot-api.sh [out-dir]      then copy the directory to the machine that runs the preview
set -eu
OUT=${1:-./api-snap}
TOKEN=$(cat "${XDG_CONFIG_HOME:-$HOME/.config}/pilab/console-token")
mkdir -p "$OUT"
for e in info status system versions storage audio/devices scene scenes midi/inputs library/samples \
         library/presets library/akwf graph mixer meters settings recordings input devices endpoints \
         zero/schema zero/presets firmware setup; do
  curl -fsS -m 20 -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:8410/api/v1/$e" > "$OUT/$(echo "$e" | tr / _).json"
done
echo "saved $(ls "$OUT" | wc -l) files to $OUT (they contain this device's addresses and names; do not commit them)"
