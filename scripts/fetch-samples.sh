#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Install the pilab base-pack sample banks (CC0 only) onto this device, never into git:
# 28 small FreePats SFZ banks and the VCSL starter drum kit with a General MIDI map.
# Run on the Pi as user lab:  sh scripts/fetch-samples.sh      (about 120 MiB download)
# Needs: python3, tar, xz, and 7z (Debian: sudo apt install 7zip); flac is optional and
# converts the VCSL kit from WAV to FLAC (about 40 % smaller).
# Result: ~/.local/share/pilab/library (or /srv/library when present and writable) with
#   samples/freepats/{id}/, samples/vcsl-starter/, manifest/{id}.yaml + files.tsv,
#   samples/index.json. Then reload the console library: pilabctl samples reload (or restart).
# Options are passed through: --only kalimba,vcsl-starter-kit  --root DIR  --no-flac
set -eu
cd "$(dirname "$0")/.."
command -v 7z >/dev/null 2>&1 || command -v 7zz >/dev/null 2>&1 || command -v 7za >/dev/null 2>&1 || {
  echo "7z is missing. Install it with: sudo apt install 7zip" >&2
  exit 1
}
exec python3 scripts/pilab-samples.py install "$@"
