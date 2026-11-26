#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Music page player: install MPD (no recommends), keep Debian's system and user MPD units
# masked so nothing else opens the DAC, render ~/.config/pilab/mpd.conf, enable the
# pilab-mpd user unit, seed the stream list and fetch the demo albums if missing.
# Run by scripts/install-console.sh on the Pi (after the pilab-*.service units are installed).
set -eu
REPO=$(cd "$(dirname "$0")/.." && pwd)
STATE=${XDG_STATE_HOME:-$HOME/.local/state}/pilab
CONF=$HOME/.config/pilab

# Mask first: Debian's mpd postinst would otherwise start a system daemon with an ALSA output.
for u in mpd.service mpd.socket; do
  [ "$(systemctl is-enabled "$u" 2>/dev/null || true)" = masked ] || sudo systemctl mask --now "$u" >/dev/null 2>&1 || true
  [ "$(systemctl --user is-enabled "$u" 2>/dev/null || true)" = masked ] || systemctl --user mask --now "$u" >/dev/null 2>&1 || true
done
dpkg -s mpd >/dev/null 2>&1 || sudo DEBIAN_FRONTEND=noninteractive apt-get -o DPkg::Lock::Timeout=600 install -y -q --no-install-recommends mpd

mkdir -p "$STATE/mpd/empty" "$CONF/music"
TMP=$(mktemp)
sed "s|@STATE@|$STATE|g" "$REPO/home/.config/pilab/mpd.conf.in" > "$TMP"
cmp -s "$TMP" "$CONF/mpd.conf" 2>/dev/null || install -m 644 "$TMP" "$CONF/mpd.conf"
rm -f "$TMP"
[ -f "$CONF/music/streams.m3u" ] || install -m 644 "$REPO/home/.config/pilab/music/streams.m3u" "$CONF/music/streams.m3u"

if [ ! -f "$HOME/.local/share/pilab/music/demo/README.txt" ]; then
  sh "$REPO/scripts/fetch-music-demo.sh" || echo "demo albums not fetched (offline?); run scripts/fetch-music-demo.sh later" >&2
fi

systemctl --user daemon-reload
systemctl --user enable pilab-mpd.service >/dev/null 2>&1
systemctl --user restart pilab-mpd.service
