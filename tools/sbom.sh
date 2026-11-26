#!/usr/bin/env bash
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
#
# Generate the software bill of materials for Trillium Base and scan it for known
# vulnerabilities. Repeatable; run it for every release and attach the results.
#
#   tools/sbom.sh --unit lab@pilab.local                  # Rust + the packages on a running unit
#   tools/sbom.sh --rootfs /mnt/rootfs                    # Rust + an unpacked image rootfs
#   tools/sbom.sh --image-sbom build-meta/filesystem-*.sbom.zst   # + the SBOM rpi-image-gen wrote
#   tools/sbom.sh --no-scan ...                           # skip the vulnerability scan
#
# Outputs go to sbom/ (override with --out), named trillium-base-<version>-*.
# Tools: cargo + cargo-cyclonedx, python3, ssh (for --unit), zstd (for --image-sbom),
#        grype (scan) and osv-scanner (Rust scan, queries api.osv.dev). Missing tools are
#        skipped with a note. Install: cargo install cargo-cyclonedx --locked; grype and
#        osv-scanner are single binaries from their GitHub releases.
# Nothing here is sent anywhere except the scanners' own database or API queries.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=sbom; UNIT=""; ROOTFS=""; IMGSBOM=""; SCAN=1; VERSION=""; IMGVER=""; TARGET=aarch64-unknown-linux-gnu
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --unit) UNIT="$2"; shift 2 ;;
    --rootfs) ROOTFS="$2"; shift 2 ;;
    --image-sbom) IMGSBOM="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --image-version) IMGVER="$2"; shift 2 ;;
    --no-scan) SCAN=0; shift ;;
    -h|--help) sed -n 2,19p "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
[ -n "$VERSION" ] || VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' rust/Cargo.toml | head -1)
mkdir -p "$OUT"
P="$OUT/trillium-base-$VERSION"
[ -n "$IMGVER" ] || IMGVER=$(cat image/VERSION 2>/dev/null || echo "$VERSION")
PI="$OUT/trillium-base-$IMGVER"
have() { command -v "$1" >/dev/null 2>&1; }
note() { echo "[sbom] $*"; }

# 1. Rust workspace. pilabd depends on the other two crates, so its SBOM covers the tree.
if have cargo-cyclonedx; then
  note "Rust: cargo cyclonedx (target $TARGET)"
  (cd rust && cargo cyclonedx --format json --spec-version 1.5 --override-filename sbom-tmp --target "$TARGET" >/dev/null)
  mv rust/crates/pilabd/sbom-tmp.json "$P-rust.cdx.json"
  rm -f rust/crates/*/sbom-tmp.json
else
  note "cargo-cyclonedx not found: skipping the Rust SBOM"
fi

# 2. Debian packages (+ source-built extras) from a unit or an unpacked rootfs.
if [ -n "$UNIT" ] || [ -n "$ROOTFS" ]; then
  tsv=$(mktemp)
  if [ -n "$UNIT" ]; then ssh "$UNIT" 'sh -s' < tools/sbom/collect-dpkg.sh > "$tsv"; label=unit
  else ROOT="$ROOTFS" sh tools/sbom/collect-dpkg.sh > "$tsv"; label=rootfs; fi
  python3 tools/sbom/dpkg2cdx.py "$tsv" "$P-$label-debian.cdx.json" "trillium-base-$label" "$VERSION" tools/sbom/extra-components.json
  rm -f "$tsv"
fi

# 3. The image SBOM that rpi-image-gen writes during a build.
if [ -n "$IMGSBOM" ]; then
  have zstd || { note "zstd not found: skipping the image SBOM"; IMGSBOM=""; }
fi
if [ -n "$IMGSBOM" ]; then
  tmp=$(mktemp); zstd -dc "$IMGSBOM" > "$tmp"
  python3 tools/sbom/slim-spdx.py "$tmp" "$PI-image.spdx.json"
  cp "$tmp" "$OUT/.full-image.spdx.json"   # full copy for the scan; not committed (.gitignore)
  rm -f "$tmp"
fi

# 4. Licence summary.
rust_bom="$P-rust.cdx.json"; deb_bom=$(ls "$P"-*-debian.cdx.json 2>/dev/null | head -1 || true)
if [ -f "$rust_bom" ]; then
  python3 tools/sbom/license-summary.py "$rust_bom" ${deb_bom:+"$deb_bom"} > "$P-licenses.md"
  note "wrote $P-licenses.md"
fi

# 5. Vulnerability scan: grype on each package SBOM (one summary per SBOM, so nothing is
#    counted twice), osv-scanner on Cargo.lock (RustSec and GHSA advisories).
if [ "$SCAN" = 1 ]; then
  if have grype; then
    for f in "$P"-*-debian.cdx.json "$OUT/.full-image.spdx.json"; do
      [ -f "$f" ] || continue
      case "$f" in */.full-image.spdx.json) name="$PI-image" ;; *) name="${f%.cdx.json}" ;; esac
      r="$OUT/.scan.grype.json"
      note "grype: $f"
      if grype "sbom:$f" -o json > "$r" 2>/dev/null; then
        python3 tools/sbom/vuln-summary.py "Vulnerability scan: $(basename "$name") ($(date -u +%Y-%m-%d) UTC, grype database of that day)" "$r" > "$name-vulnerabilities.md"
        note "wrote $name-vulnerabilities.md"
      else
        note "grype failed on $f"
      fi
    done
  else
    note "grype not found: skipping the package scan"
  fi
  if have osv-scanner; then
    note "osv-scanner: rust/Cargo.lock"
    out=$(osv-scanner scan source --lockfile rust/Cargo.lock --format table 2>&1 || true)
    if echo "$out" | grep -q "Error during extraction"; then
      # A network failure prints "Total 0 ... vulnerabilities", which must not be read as clean.
      printf 'NOT COMPLETED: osv-scanner could not reach api.osv.dev. Re-run from a network that can.\n%s\n' "$out" | tail -n 8 > "$P-rust-osv.txt"
      note "osv-scanner did NOT complete (network); $P-rust-osv.txt says so"
    else
      printf '%s\n' "$out" | tail -n 12 > "$P-rust-osv.txt"
      note "wrote $P-rust-osv.txt"
    fi
  else
    note "osv-scanner not found: skipping the Rust advisory scan"
  fi
  rm -f "$OUT/.scan.grype.json" "$OUT/.full-image.spdx.json"
fi
note "done: $(ls "$OUT" | tr '\n' ' ')"
