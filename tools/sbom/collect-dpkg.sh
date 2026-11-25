#!/bin/sh
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Print one TSV line per installed Debian package:
#   name  version  arch  source  source-version  licenses(;-joined, from the DEP-5 copyright file)
# Read-only. ROOT=/path/to/rootfs reads an unpacked image instead of the running system.
# Run locally, or on a unit with:  ssh lab@host 'sh -s' < tools/sbom/collect-dpkg.sh
ROOT="${ROOT:-}"
ADMIN="$ROOT/var/lib/dpkg"
dpkg-query --admindir="$ADMIN" -W -f='${Package}\t${Version}\t${Architecture}\t${source:Package}\t${source:Version}\t${db:Status-Abbrev}\n' |
while IFS="$(printf '\t')" read -r name ver arch src sver st; do
  case "$st" in ii*) ;; *) continue ;; esac
  base="${name%%:*}"
  c="$ROOT/usr/share/doc/$base/copyright"
  lic=""
  if [ -r "$c" ]; then
    lic=$(grep -h '^License:' "$c" 2>/dev/null | sed 's/^License:[[:space:]]*//' | sort -u | tr '\n' ';' | sed 's/;$//')
  fi
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$ver" "$arch" "${src:-$name}" "${sver:-$ver}" "$lic"
done
