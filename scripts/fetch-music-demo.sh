#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Fetch the demo albums for the Music page (CC0 1.0, credits in library/ATTRIBUTION.md,
# source id music-demo) into ~/.local/share/pilab/music/demo. About 81 MB. Checks SHA-256.
#   sh scripts/fetch-music-demo.sh [DEST]
set -eu
DEST=${1:-$HOME/.local/share/pilab/music/demo}
BASE=https://archive.org/download
TMP=$DEST/.partial
mkdir -p "$TMP"

# item|archive.org file name|album folder|sha256
LIST='gt459AnchoreState-ChangesOfLifeEp|0cover.jpg|Anchore State - Changes of Life EP|dee34ea7dc74eef8d11527b0c574f0b34eddf3bb0d458b0b027280bc269dfe49
gt459AnchoreState-ChangesOfLifeEp|1.ItsNotAToy.mp3|Anchore State - Changes of Life EP|158d20716e7d5fa3d3e6e04da6bdd5779014e52f368ce739133e27c78ec699ad
gt459AnchoreState-ChangesOfLifeEp|2.Divingrefixed.mp3|Anchore State - Changes of Life EP|9bbc6cffa1a6c404932c83b9dcf00117d0c90336b3571e4d2dda52372bcb8dee
gt459AnchoreState-ChangesOfLifeEp|3.DeepForest.mp3|Anchore State - Changes of Life EP|4e1a170f135966990372b7f5df48843ab6bc264b44d37a8d73178e75136e8d20
gt458SteamFlow-WithTrainsAndBones|cover.JPG|Steam Flow - With Trains and Bones|253ba0381bc8b382ce2c0d1c8ca82f6809393a8119a8704fb3e4e8b251daeba0
gt458SteamFlow-WithTrainsAndBones|1.RailwayToNowhere.mp3|Steam Flow - With Trains and Bones|1fe17f46cc39c5076996ccec2a68bb6f6cdbf222e1e6cb77394a0ecfcf468812
gt458SteamFlow-WithTrainsAndBones|2.TrainAtNight.mp3|Steam Flow - With Trains and Bones|1230bd9c3b8410751dc409bbe782910b8c2195811230174ed2fea39f06af7e54
gt458SteamFlow-WithTrainsAndBones|3.DeadSecret.mp3|Steam Flow - With Trains and Bones|c7e60479a4f43df8844a8863bd16684e929396811ac2c8e614175f3ba36c1098
musopen-chopin|Ballade no. 1 - Op. 23.ogg|Chopin - Ballades (Musopen)|456f26631e9c18f0ae9c9ec944c763fea342a6ad6d62952fea7c164a090ad16a
musopen-chopin|Ballade no. 2 - Op. 38.ogg|Chopin - Ballades (Musopen)|7ee17e362f62ee80f6f34a3e914ed03812edc2fb29b18e6420c8df530af312af
musopen-chopin|Ballade no. 3 - Op. 47.ogg|Chopin - Ballades (Musopen)|dcdccefe009122c6e1d537f0100df5a456a6e07cfc48139637de20c0468b224e
musopen-chopin|Ballade no. 4 - Op. 52.ogg|Chopin - Ballades (Musopen)|aa81ac5327768e71da85c404850585b2afea8ff87afdc036cf9fef915eb8e861'

urlenc() { python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "$1"; }

echo "$LIST" | while IFS='|' read -r item name album sum; do
  out="$DEST/$album/$name"
  if [ -f "$out" ] && echo "$sum  $out" | sha256sum -c --status; then continue; fi
  mkdir -p "$DEST/$album"
  part="$TMP/$(echo "$album-$name" | tr ' /' '__')"
  curl -fsSL --retry 3 -o "$part" "$BASE/$item/$(urlenc "$name")"
  if echo "$sum  $part" | sha256sum -c --status; then
    mv "$part" "$out"
    echo "ok   $album/$name"
  else
    rm -f "$part"; echo "BAD  $album/$name (checksum)" >&2; exit 1
  fi
done
rmdir "$TMP" 2>/dev/null || true
cat > "$DEST/README.txt" <<'TXT'
Demo albums for the Trillium Base Music page. All CC0 1.0 (public domain dedication),
from the Internet Archive. Credits: library/ATTRIBUTION.md in the trillium-base repo
(source id music-demo). Remove this folder to remove the demo; the next scan drops it.
TXT
echo "demo albums in $DEST"
