#!/usr/bin/env python3
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""Turn the TSV from collect-dpkg.sh into a CycloneDX 1.5 JSON SBOM (and optionally a
license summary). Usage: dpkg2cdx.py packages.tsv out.cdx.json NAME VERSION [extra.json]"""
import json, sys, re, uuid, datetime, hashlib, urllib.parse

SPDX = {
    "GPL-1": "GPL-1.0-only", "GPL-1+": "GPL-1.0-or-later", "GPL-2": "GPL-2.0-only", "GPL-2+": "GPL-2.0-or-later",
    "GPL-3": "GPL-3.0-only", "GPL-3+": "GPL-3.0-or-later", "LGPL-2": "LGPL-2.0-only", "LGPL-2+": "LGPL-2.0-or-later",
    "LGPL-2.1": "LGPL-2.1-only", "LGPL-2.1+": "LGPL-2.1-or-later", "LGPL-3": "LGPL-3.0-only", "LGPL-3+": "LGPL-3.0-or-later",
    "AGPL-3": "AGPL-3.0-only", "AGPL-3+": "AGPL-3.0-or-later", "Apache-2.0": "Apache-2.0", "Apache-2": "Apache-2.0",
    "BSD-2-clause": "BSD-2-Clause", "BSD-3-clause": "BSD-3-Clause", "BSD-4-clause": "BSD-4-Clause", "MIT": "MIT",
    "Expat": "MIT", "ISC": "ISC", "Zlib": "Zlib", "MPL-2.0": "MPL-2.0", "MPL-1.1": "MPL-1.1", "Artistic": "Artistic-1.0-Perl",
    "Artistic-2.0": "Artistic-2.0", "public-domain": "LicenseRef-public-domain", "CC0-1.0": "CC0-1.0",
    "BSL-1.0": "BSL-1.0", "OFL-1.1": "OFL-1.1", "X11": "X11", "FSFAP": "FSFAP", "GFDL-1.3": "GFDL-1.3-only",
}

def lic_entries(raw):
    out = []
    for tok in [t.strip() for t in raw.split(";") if t.strip()]:
        # skip fragments that are not names (free-text lines)
        if len(tok) > 80:
            continue
        sp = SPDX.get(tok)
        out.append({"license": {"id": sp}} if sp and not sp.startswith("LicenseRef") else {"license": {"name": sp or tok}})
    seen, res = set(), []
    for e in out:
        k = json.dumps(e, sort_keys=True)
        if k not in seen:
            seen.add(k); res.append(e)
    return res

def main():
    tsv, out, name, version = sys.argv[1:5]
    extra = json.load(open(sys.argv[5])) if len(sys.argv) > 5 else []
    comps = []
    for line in open(tsv, encoding="utf-8", errors="replace"):
        f = line.rstrip("\n").split("\t")
        if len(f) < 5:
            continue
        pkg, ver, arch, src, sver = f[:5]
        lic = f[5] if len(f) > 5 else ""
        ns = "raspberrypi" if re.search(r"\+(rpt|rpi)", ver) or ver.endswith("rpt1") else "debian"
        # purl convention for deb: the epoch is a qualifier, not part of the version
        epoch, pver = ver.split(":", 1) if ":" in ver else (None, ver)
        q = {"arch": arch, "distro": "debian-13"}
        if epoch:
            q["epoch"] = epoch
        if src and (src != pkg or sver != ver):
            # grype matches Debian advisories on the source package at the source version
            q["upstream"] = src + ("@" + sver if sver else "")
        purl = "pkg:deb/%s/%s@%s?%s" % (ns, urllib.parse.quote(pkg), urllib.parse.quote(pver, safe="+~"), urllib.parse.urlencode(q))
        c = {"type": "library", "bom-ref": purl, "name": pkg, "version": ver, "purl": purl,
             "properties": [{"name": "debian:arch", "value": arch}, {"name": "debian:source", "value": src}, {"name": "debian:source-version", "value": sver}]}
        l = lic_entries(lic)
        if l:
            c["licenses"] = l
        comps.append(c)
    comps.extend(extra)
    comps.sort(key=lambda c: (c["name"], c.get("version", "")))
    bom = {
        "bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
        "serialNumber": "urn:uuid:" + str(uuid.uuid5(uuid.NAMESPACE_URL, name + version + hashlib.sha256(open(tsv,'rb').read()).hexdigest())),
        "metadata": {
            "timestamp": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "tools": {"components": [{"type": "application", "name": "trillium-base tools/sbom/dpkg2cdx.py"}]},
            "component": {"type": "operating-system", "name": name, "version": version,
                          "description": "Debian 13 (trixie) arm64 packages installed on the unit, plus source-built extras"},
        },
        "components": comps,
    }
    json.dump(bom, open(out, "w"), separators=(",", ":"), sort_keys=False)
    print("%s: %d components" % (out, len(comps)))

main()
