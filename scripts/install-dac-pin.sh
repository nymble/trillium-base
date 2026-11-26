#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
#
# Keep the DAC in charge when other audio interfaces are plugged in. Writes a WirePlumber
# rule that gives the configured DAC sink (settings: audio.dac_sink, matched by node name,
# never by ALSA card number) the highest session and driver priority, so it stays the
# default sink and, whenever graphs join (a USB interface linked to the synth's meters, or
# music sent to two outputs), the DAC's clock drives and the other device follows.
# Does not restart WirePlumber (that briefly drops every device); the rule applies the
# next time WirePlumber starts. pilabd re-links the synth to the DAC by name when the DAC
# node comes back.
set -eu
CONF=${XDG_CONFIG_HOME:-$HOME/.config}
DAC=$(python3 - "$CONF/pilab/settings.toml" <<'PY'
import sys, tomllib
try:
    s = tomllib.load(open(sys.argv[1], "rb"))
    print(s.get("audio", {}).get("dac_sink", ""))
except Exception:
    print("")
PY
)
[ -n "$DAC" ] || DAC="alsa_output.platform-soc_107c000000_sound.stereo-fallback"
case "$DAC" in *[!A-Za-z0-9._:-]*) echo "install-dac-pin: unexpected sink name, skipped" >&2; exit 0;; esac
DIR="$CONF/wireplumber/wireplumber.conf.d"
mkdir -p "$DIR"
TMP=$(mktemp)
cat > "$TMP" <<EOT
# Written by scripts/install-dac-pin.sh (Trillium Base). The DAC stays the default sink and
# the graph driver whatever else is plugged in.
monitor.alsa.rules = [
  {
    matches = [ { node.name = "$DAC" } ]
    actions = { update-props = { priority.session = 3000, priority.driver = 3000 } }
  }
]
EOT
if cmp -s "$TMP" "$DIR/51-pilab-dac.conf" 2>/dev/null; then
  rm -f "$TMP"
  echo "DAC pin: unchanged ($DAC)"
else
  install -m 644 "$TMP" "$DIR/51-pilab-dac.conf"
  rm -f "$TMP"
  echo "DAC pin: written for $DAC (applies the next time WirePlumber starts)"
fi
