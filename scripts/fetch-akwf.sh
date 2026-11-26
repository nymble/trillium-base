#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Fetch the Adventure Kid Waveforms (AKWF, CC0 1.0) single-cycle library for the pilab
# synth's wavetable oscillator. The library is never committed to this repo.
# Run on the Pi as user lab:  sh scripts/fetch-akwf.sh        (about 60 MB, one time)
# Result: ~/.local/share/pilab/library/akwf/AKWF_*/AKWF_*.wav plus SOURCE.txt.
# Re-running updates to the latest upstream commit.
set -eu
REPO=https://github.com/KristofferKarlAxelEkstrand/AKWF-FREE
DEST="${PILAB_AKWF_DIR:-$HOME/.local/share/pilab/library/akwf}"
SRC="${XDG_CACHE_HOME:-$HOME/.cache}/pilab/akwf-src"

mkdir -p "$(dirname "$SRC")" "$(dirname "$DEST")"
if [ -d "$SRC/.git" ]; then
  git -C "$SRC" pull -q --ff-only
else
  git clone -q --depth 1 --filter=blob:none --sparse "$REPO" "$SRC"
  git -C "$SRC" sparse-checkout set AKWF
fi
[ -d "$SRC/AKWF" ] || { echo "upstream layout changed: no AKWF/ folder" >&2; exit 1; }

COMMIT=$(git -C "$SRC" rev-parse HEAD)
TMP="$DEST.new"
rm -rf "$TMP"
mkdir -p "$TMP"
cp -R "$SRC"/AKWF/AKWF_* "$TMP"/
for f in LICENSE LICENSE.md LICENSE.txt license.txt README.md; do
  [ -f "$SRC/$f" ] && cp "$SRC/$f" "$TMP/UPSTREAM-$f"
done
cat > "$TMP/SOURCE.txt" <<SRC_EOF
Adventure Kid Waveforms (AKWF), single-cycle waveforms.
Source:  $REPO
Commit:  $COMMIT
Fetched: $(date -u +%Y-%m-%dT%H:%M:%SZ)
License: CC0 1.0 Universal (public domain dedication), per upstream.
Not part of the pilab repo; fetched by scripts/fetch-akwf.sh.
SRC_EOF
rm -rf "$DEST.old"
[ -d "$DEST" ] && mv "$DEST" "$DEST.old"
mv "$TMP" "$DEST"
rm -rf "$DEST.old"
GROUPS=$(find "$DEST" -mindepth 1 -maxdepth 1 -type d | wc -l)
WAVS=$(find "$DEST" -name '*.wav' | wc -l)
echo "AKWF ready: $GROUPS groups, $WAVS waveforms in $DEST (commit ${COMMIT%"${COMMIT#????????????}"})"
