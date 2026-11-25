#!/usr/bin/env python3
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""License summary for the Rust CycloneDX SBOM and a Debian CycloneDX SBOM (Markdown on stdout).
Usage: license-summary.py rust.cdx.json debian.cdx.json"""
import json, sys, re, collections

OWN = re.compile(r"GPL-3\.0-or-later OR LicenseRef-Commercial")
COPYLEFT_IN_RUST = re.compile(r"(?<![L])GPL|AGPL|LGPL|MPL|EPL|CDDL|SSPL|BUSL")

def names(c):
    out = []
    for e in c.get("licenses", []):
        if "expression" in e:
            out.append(e["expression"])
        else:
            l = e.get("license", {})
            out.append(l.get("id") or l.get("name") or "")
    return out

def rust(path):
    b = json.load(open(path))
    print("## Rust workspace (%d components, own crates included)\n" % len(b["components"]))
    c, own, flagged, none = collections.Counter(), [], [], []
    for x in b["components"]:
        n = names(x)
        if not n:
            none.append(x["name"]); continue
        s = " AND ".join(n)
        c[s] += 1
        if OWN.search(s):
            own.append("%s %s" % (x["name"], x["version"]))
        elif COPYLEFT_IN_RUST.search(s) and "OR MIT" not in s and "OR Apache" not in s:
            flagged.append("%s %s: %s" % (x["name"], x["version"], s))
    print("| Licence expression | Crates |\n| --- | ---: |")
    for k, v in c.most_common():
        print("| `%s` | %d |" % (k, v))
    print()
    print("Own code (GPL-3.0-or-later OR LicenseRef-Commercial): " + ", ".join(own) + "\n")
    print("**Copyleft-only or unlicensed third-party crates (would conflict with the commercial option):** " + ("; ".join(flagged + none) if flagged or none else "none.") + "\n")

CLASS = [("AGPL", re.compile(r"(?i)agpl|affero")), ("GPL", re.compile(r"(?i)(?<!l)gpl")), ("LGPL", re.compile(r"(?i)lgpl")),
         ("Review (non-free, custom, other, firmware)", re.compile(r"(?i)non-?free|proprietary|redistrib|freeware|custom|other|firmware|unknown|restricted"))]

def debian(path):
    b = json.load(open(path))
    cs = b["components"]
    print("## Debian packages and source-built extras (%d components)\n" % len(cs))
    buck, lists = collections.Counter(), collections.defaultdict(list)
    for x in cs:
        raw = " ; ".join(names(x))
        if not raw:
            buck["No machine-readable licence in the package"] += 1
            continue
        hit = None
        for label, rx in CLASS:
            if rx.search(raw):
                hit = label
                break
        hit = hit or "Permissive only"
        buck[hit] += 1
        lists[hit].append(x["name"])
    print("| Strongest licence class named by the package | Packages |\n| --- | ---: |")
    for k, v in buck.most_common():
        print("| %s | %d |" % (k, v))
    print("\nPackages in the review class: " + (", ".join(sorted(lists["Review (non-free, custom, other, firmware)"])[:60]) or "none") + "\n")
    print("AGPL packages: " + (", ".join(sorted(lists["AGPL"])) or "none") + "\n")
    print("Source-built extras:\n")
    for x in cs:
        if x.get("bom-ref", "").startswith("src:"):
            print("- %s %s: %s" % (x["name"], x["version"], "; ".join(names(x))))

def main():
    print("# Licence summary\n")
    rust(sys.argv[1])
    if len(sys.argv) > 2:
        debian(sys.argv[2])

main()
