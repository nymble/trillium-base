#!/usr/bin/env python3
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""Shrink the syft SPDX that rpi-image-gen writes for an image rootfs (build-meta/*.sbom.zst):
drop the per-file list and the ~3800 kernel-module entries (kept as a count plus the kernel
packages themselves). Usage: slim-spdx.py in.spdx.json out.spdx.json"""
import json, sys

d = json.load(open(sys.argv[1]))
keep, mods = [], 0
for p in d["packages"]:
    if p["SPDXID"].startswith("SPDXRef-Package-linux-kernel-module"):
        mods += 1
        continue
    keep.append(p)
ids = {p["SPDXID"] for p in keep} | {d["SPDXID"]}
d["packages"] = keep
d.pop("files", None)
d["relationships"] = [r for r in d.get("relationships", []) if r["spdxElementId"] in ids and r["relatedSpdxElement"] in ids]
d["comment"] = "Slimmed by tools/sbom/slim-spdx.py: %d kernel-module entries and the file list removed; the kernel packages stay." % mods
json.dump(d, open(sys.argv[2], "w"), separators=(",", ":"))
print("%s: %d packages (dropped %d kernel modules)" % (sys.argv[2], len(keep), mods))
