#!/usr/bin/env python3
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""Summarise one or more grype JSON reports as Markdown, de-duplicated by (source package, CVE).

Grype reports a match once per binary package, so a source package that builds twenty
binaries inflates the count. The kernel is reported separately: Debian's tracker follows its
own 6.12 kernel, not the Raspberry Pi 6.18 RT kernel, so its matches are leads, not findings.

Usage: vuln-summary.py "title" report.grype.json [more.json ...] > summary.md"""
import json, sys, collections

SEV = ["Critical", "High", "Medium", "Low", "Negligible", "Unknown"]

def load(paths):
    rows = {}
    for p in paths:
        g = json.load(open(p))
        for m in g.get("matches", []):
            a, v = m["artifact"], m["vulnerability"]
            q = dict(x.split("=", 1) for x in a.get("purl", "").split("?", 1)[1].split("&")) if "?" in a.get("purl", "") else {}
            src = q.get("upstream", a["name"]).split("%40")[0].split("@")[0]
            if a["name"].startswith("linux-") or src == "linux":
                src = "linux (kernel)"
            k = (src, v["id"])
            r = rows.setdefault(k, {"sev": v["severity"], "fix": v["fix"]["state"], "fv": v["fix"].get("versions") or [],
                                    "pk": set(), "kev": False, "inst": a["version"]})
            r["pk"].add(a["name"])
            r["kev"] = r["kev"] or bool(v.get("knownExploited"))
    return rows

def main():
    title, paths = sys.argv[1], sys.argv[2:]
    rows = load(paths)
    kern = {k: r for k, r in rows.items() if k[0].startswith("linux")}
    rest = {k: r for k, r in rows.items() if k not in kern}
    print("# " + title + "\n")
    print("Unique (source package, CVE) pairs after de-duplication: %d (kernel: %d, everything else: %d).\n" % (len(rows), len(kern), len(rest)))
    for label, d in (("Everything except the kernel", rest), ("Kernel (leads only)", kern)):
        print("## " + label + "\n")
        print("| Severity | Fix in the archive | No fix yet | Will not fix |\n| --- | ---: | ---: | ---: |")
        for s in SEV:
            c = collections.Counter(r["fix"] for r in d.values() if r["sev"] == s)
            print("| %s | %d | %d | %d |" % (s, c["fixed"], c["not-fixed"], c["wont-fix"]))
        print()
    print("## Known-exploited (CISA KEV) matches\n")
    kev = sorted(k for k, r in rows.items() if r["kev"])
    print("\n".join("- %s %s (%s, %s)" % (k[0], k[1], rows[k]["sev"], rows[k]["fix"]) for k in kev) or "None.")
    print("\n## Critical and High with a fix already in the archive (upgrade clears these)\n")
    print("| Source package | Installed | Fixed in | CVEs |\n| --- | --- | --- | ---: |")
    grp = collections.defaultdict(list)
    for k, r in rest.items():
        if r["fix"] == "fixed" and r["sev"] in ("Critical", "High"):
            grp[(k[0], r["inst"], ", ".join(r["fv"]))].append(k[1])
    for (s, i, f), cves in sorted(grp.items()):
        print("| %s | %s | %s | %d |" % (s, i, f, len(cves)))
    print("\n## Critical and High by source package (no fix or will not fix), top 15\n")
    c = collections.Counter(k[0] for k, r in rest.items() if r["sev"] in ("Critical", "High") and r["fix"] != "fixed")
    print("| Source package | Critical+High |\n| --- | ---: |")
    for s, n in c.most_common(15):
        print("| %s | %d |" % (s, n))

main()
