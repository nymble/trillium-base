#!/usr/bin/env python3
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""Generate library/ATTRIBUTION.md from library/sources.yaml.

Usage: python3 scripts/library-attribution.py > library/ATTRIBUTION.md
Needs PyYAML (python3 -m pip install pyyaml, or apt install python3-yaml).
"""
import pathlib
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent
data = yaml.safe_load((ROOT / "library" / "sources.yaml").read_text())

STATUS = {
    "shippable": "Shipped or shippable",
    "allowed-not-shipped": "Allowed, fetched on request, not in the base image",
    "reference-only": "Reference only: ideas or descriptors, no copied bytes or code",
    "not-allowed": "Not allowed in packs or on shared storage",
}

out = []
w = out.append
w("# Library sources and credits")
w("")
w("Generated from `library/sources.yaml` by `scripts/library-attribution.py`. Do not edit by hand.")
w("")
w(f"Sources checked: {data.get('updated', '')}. Packs installed later add their own CREDITS file")
w("with any per-item credit lines the upstream licence requires.")
w("")
for key, title in STATUS.items():
    rows = [s for s in data["sources"] if s.get("status") == key]
    if not rows:
        continue
    w(f"## {title}")
    w("")
    for s in rows:
        att = s.get("attribution") or {}
        w(f"### {s['name']}")
        w("")
        w(f"- Source: <{s.get('homepage') or s['url']}>")
        if s.get("homepage") and s.get("url") != s.get("homepage"):
            w(f"- Fetched from: <{s['url']}>")
        w(f"- Licence: {s['license']}")
        w(f"- Type: {s['type']}; tier: {s.get('tier', '')}")
        w(f"- Attribution required: {'yes' if att.get('required') else 'no'}")
        if att.get("credit"):
            w(f"- Credit: {att['credit']}")
        if s.get("reason"):
            w(f"- Why: {' '.join(s['reason'].split())}")
        for b in s.get("banks", []) or []:
            lic = f", {b['license']}" if b.get("license") else ""
            w(f"  - {b['name']}{lic} ({b.get('size', '')})")
        w(f"- Checked: {s.get('verified', 'unverified')}")
        w("")
sys.stdout.write("\n".join(out) + "\n")
