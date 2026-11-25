#!/usr/bin/env python3
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""Install the pilab base-pack sample banks onto a device (never into git).

FreePats banks come from library/samples-base.tsv (pinned URL, sha256, size). The VCSL
starter kit comes from library/vcsl-starter.tsv (pinned upstream commit and git blob ids)
and gets a generated General MIDI drum map (starter-kit.sfz). The full GM kit
(library/vcsl-full-kit.tsv) adds toms, ride, crash, splash and Latin percussion, and the
tuned VCSL packs (library/vcsl-tonal.tsv: marimba, vibraphone, glockenspiel, balafon,
mbira) get key-mapped SFZ files from the note names in their file names. All VCSL is CC0.

Every bank gets a pilab.manifest/v1 manifest plus a files.tsv sidecar, and the runtime
index samples/index.json lists what the console can load. Anything whose licence text does
not say CC0 / public domain is skipped.

Usage: pilab-samples.py install [--root DIR] [--only id,id] [--cache DIR] [--no-flac]
       ids: FreePats bank ids, vcsl-starter-kit, vcsl-full-kit, vcsl-tonal (all five tuned packs)
       pilab-samples.py index   [--root DIR]
"""

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
HOST = os.uname().nodename.split(".")[0] + ".local"
VCSL_REPO = "sgossner/VCSL"
VCSL_PAGE = "https://github.com/sgossner/VCSL"
FREEPATS_PAGE = "https://freepats.zenvoid.org/"
LICENSE_RE = re.compile(r"\bCC0\b|creative\s*commons\s*zero|public\s+domain", re.I)
MEDIA = {".flac": "audio/flac", ".wav": "audio/wav", ".sfz": "text/x-sfz", ".txt": "text/plain", ".md": "text/markdown"}


def default_root() -> Path:
    srv = Path("/srv/library")
    if srv.is_dir() and os.access(srv, os.W_OK):
        return srv
    base = os.environ.get("XDG_DATA_HOME") or str(Path.home() / ".local/share")
    return Path(base) / "pilab/library"


def log(*a):
    print(*a, flush=True)


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def git_blob_sha(p: Path) -> str:
    data = p.read_bytes()
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def fetch(url: str, dest: Path):
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")
    req = urllib.request.Request(url, headers={"User-Agent": "pilab-samples/1"})
    with urllib.request.urlopen(req, timeout=120) as r, open(tmp, "wb") as f:
        shutil.copyfileobj(r, f, 1 << 20)
    tmp.rename(dest)


def read_tsv(p: Path, cols):
    rows = []
    for line in p.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        rows.append(dict(zip(cols, line.split("\t"))))
    return rows


def yq(s) -> str:
    return json.dumps(str(s), ensure_ascii=False)


def write_manifest(root: Path, m: dict, bank_dir: Path):
    """Writes manifest/{id}.yaml and manifest/{id}.files.tsv for every file in bank_dir."""
    mdir = root / "manifest"
    mdir.mkdir(parents=True, exist_ok=True)
    lines, total, count = [], 0, 0
    for p in sorted(bank_dir.rglob("*")):
        if p.is_file():
            n = p.stat().st_size
            total += n
            count += 1
            rel = p.relative_to(bank_dir).as_posix()
            lines.append(f"{rel}\t{n}\t{sha256_file(p)}\t{MEDIA.get(p.suffix.lower(), 'application/octet-stream')}")
    files_tsv = mdir / f"{m['id']}.files.tsv"
    files_tsv.write_text("\n".join(lines) + "\n")
    rel_path = bank_dir.relative_to(root).as_posix()
    ident = f"pilab://{HOST}/{m['root']}/{rel_path}"
    y = [
        "schema: pilab.manifest/v1",
        f"id: {m['id']}",
        f"title: {yq(m['title'])}",
        f"source: {m['source']}",
        f"upstream: {m['upstream']}",
        f"version: {yq(m['version'])}",
        f"license: {m['license']}",
        'credits: ""',
        "types: [samples]",
        f"root: {m['root']}",
        f"path: {rel_path}",
        "role: mirror",
        "pinned: true",
        f"bytes: {total}",
        f"file_count: {count}",
        f"files: {files_tsv.name}",
        f"sha256_of_files_tsv: {yq(sha256_file(files_tsv))}",
        'kb_record: ""',
        f"canonical: {m['canonical']}",
        "holdings:",
        f"  - {ident}",
        "dc:",
        f"  title: {yq(m['title'])}",
        "  type: Collection",
        f"  format: {m['format']}",
        f"  rights: {m['license']}",
        f"  source: {m['dc_source']}",
        f"  identifier: {ident}",
        f"  date: {yq(m['version'][:4])}",
        "  subject: [sound, redownloadable, media]",
        "topics: [pilab]",
        "pilab:",
        f"  category: {m['category']}",
        f"  drum: {'true' if m['drum'] else 'false'}",
        f"  sfz: {yq(m['sfz'])}",
        f"  fetched: {dt.datetime.now(dt.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')}",
    ]
    for k, v in m.get("extra", {}).items():
        y.append(f"  {k}: {yq(v)}")
    (mdir / f"{m['id']}.yaml").write_text("\n".join(y) + "\n")
    return total, count


def extract(archive: Path, into: Path):
    into.mkdir(parents=True, exist_ok=True)
    name = archive.name
    if name.endswith(".7z"):
        exe = shutil.which("7z") or shutil.which("7zz") or shutil.which("7za")
        if not exe:
            raise RuntimeError("7z not found: sudo apt install 7zip")
        subprocess.run([exe, "x", "-y", f"-o{into}", str(archive)], check=True, stdout=subprocess.DEVNULL)
    else:
        subprocess.run(["tar", "-xf", str(archive), "-C", str(into)], check=True)


def licence_ok(d: Path) -> str:
    """Returns the licence evidence file name, or '' if no CC0 / public-domain text is found."""
    for p in sorted(d.rglob("*")):
        if p.is_file() and p.suffix.lower() in (".txt", ".md", ".sfz", "") and p.stat().st_size < 200_000:
            try:
                t = p.read_text(errors="replace")
            except OSError:
                continue
            if LICENSE_RE.search(t):
                return p.relative_to(d).as_posix()
    return ""


def install_freepats(root: Path, cache: Path, only):
    rows = read_tsv(REPO / "library/samples-base.tsv", ["id", "title", "category", "drum", "sha256", "bytes", "url"])
    done, skipped = [], []
    for r in rows:
        if only and r["id"] not in only:
            continue
        fn = r["url"].rsplit("/", 1)[1]
        arc = cache / fn
        if not arc.exists() or sha256_file(arc) != r["sha256"]:
            log(f"fetch {r['id']}: {fn}")
            fetch(r["url"], arc)
        got = sha256_file(arc)
        if got != r["sha256"]:
            skipped.append((r["id"], f"sha256 mismatch {got}"))
            arc.unlink()
            continue
        with tempfile.TemporaryDirectory(dir=cache) as td:
            td = Path(td)
            extract(arc, td)
            tops = [p for p in td.iterdir()]
            src = tops[0] if len(tops) == 1 and tops[0].is_dir() else td
            evidence = licence_ok(src)
            if not evidence:
                skipped.append((r["id"], "no CC0 statement found in the bank"))
                continue
            sfzs = sorted(p.relative_to(src).as_posix() for p in src.rglob("*.sfz"))
            if not sfzs:
                skipped.append((r["id"], "no SFZ file"))
                continue
            dest = root / "samples/freepats" / r["id"]
            if dest.exists():
                shutil.rmtree(dest)
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(src), str(dest))
        ver = re.search(r"(\d{8})", fn)
        fmt = "audio/flac" if any(dest.rglob("*.flac")) else "audio/wav"
        total, count = write_manifest(root, {
            "id": f"freepats-{r['id']}", "title": r["title"], "source": "freepats-cc0", "upstream": r["url"],
            "version": ver.group(1) if ver else "", "license": "CC0-1.0", "root": root_id(root),
            "canonical": r["url"], "format": fmt, "dc_source": FREEPATS_PAGE, "category": r["category"],
            "drum": r["drum"] == "yes", "sfz": sfzs[0],
            "extra": {"archive_sha256": r["sha256"], "licence_evidence": evidence},
        }, dest)
        log(f"  ok {r['id']}: {count} files, {total / 1048576:.1f} MiB")
        done.append(r["id"])
    return done, skipped


def root_id(root: Path) -> str:
    s = str(root)
    if s.startswith("/usr/share/pilab"):
        return "base"
    if s.startswith("/srv/library"):
        return "drive"
    return "user"


# ---------------------------------------------------------------- VCSL starter kit

DYN = {"ppp": 1, "pp": 2, "p": 3, "mp": 4, "mf": 5, "f": 6, "ff": 7, "fff": 8}

# note, instrument folder, filename prefix, transpose (semitones), extra opcodes
GM_MAP = [
    (35, "Bass Drum 1", "BDrumNew_hit_", 0, ""),
    (36, "Bass Drum 1", "BDrumNew_hit_", 0, ""),
    (37, "Snare Drum, Modern 2", "Snare3M_Xstick_", 0, ""),
    (38, "Snare Drum, Modern 2", "Snare3M_HitSN_", 0, ""),
    (39, "Claps", "Clap_rr", 0, ""),
    (40, "Snare Drum, Modern 2", "Snare3M_HitNS_", 0, ""),
    (41, "Tom 1", "TomH_HitS_", -7, ""),
    (42, "Hi-Hat Cymbal", "HiHat_HitC_", 0, "group=1"),
    (43, "Tom 1", "TomH_HitS_", -5, ""),
    (44, "Hi-Hat Cymbal", "HiHat_Close_", 0, "group=1"),
    (45, "Tom 1", "TomH_HitS_", -3, ""),
    (46, "Hi-Hat Cymbal", "HiHat_HitO_", 0, "group=2 off_by=1"),
    (47, "Tom 1", "TomH_HitS_", -1, ""),
    (48, "Tom 1", "TomH_HitS_", 0, ""),
    (49, "Clash Cymbals 2", "cymbal_crash2_", 0, ""),
    (50, "Tom 1", "TomH_HitS_", 2, ""),
    (54, "Tambourine 1", "Tamb1_Hit_", 0, ""),
    (56, "Cowbells", "Cowbell1_Hit_", 0, ""),
    (57, "Clash Cymbals 2", "cymbal_crash2_", 2, ""),
    (60, "Bongos", "BongoH_Hit1_", 0, ""),
    (61, "Bongos", "BongoL_Hit1_", 0, ""),
    (62, "Conga", "Conga_HitFM_", 0, ""),
    (63, "Conga", "Conga_HitN_", 0, ""),
    (64, "Conga", "Tumba_HitN_", 0, ""),
    (69, "Shaker, Small", "Mid_ShakerDouble_Down_", 0, ""),
    (70, "Shaker, Small", "Mid_Shaker_Slap_", 0, ""),
    (75, "Claves", "Claves1_Hit_", 0, ""),
    (76, "Woodblock", "wood_click_", 0, ""),
    (77, "Woodblock", "wood_click_", -3, ""),
    (27, "Cajon", "Cajon_hit1_", 0, ""),
    (28, "Cajon", "Cajon_hit2_", 0, ""),
]


def layer_of(name: str, prefix: str):
    """(velocity rank, round-robin index) parsed from a VCSL file name."""
    rest = name[len(prefix):-4]
    m = re.search(r"(?:^|_)v(\d+)(?:_|$)", rest)
    rank = int(m.group(1)) if m else 0
    if not m:
        for tok in re.split(r"[_\d]+", rest):
            if tok.lower() in DYN:
                rank = DYN[tok.lower()]
    m = re.search(r"rr(\d+)", rest)
    if m:
        rr = int(m.group(1))
    else:
        m = re.search(r"[a-z](\d+)$", rest) if prefix.startswith("cymbal") else None
        rr = int(m.group(1)) if m else 1
    return rank, rr


def gm_sfz(files):
    """SFZ text for the GM map from VCSL file paths (relative to the kit folder)."""
    out = [
        "// pilab VCSL starter kit, General MIDI drum map (generated by scripts/pilab-samples.py).",
        "// Samples: Versilian Community Sample Library (VCSL), CC0 1.0.",
        "<control> default_path=",
        "<global> loop_mode=one_shot ampeg_release=0.3 ampeg_attack=0.001",
    ]
    for note, folder, prefix, tr, extra in GM_MAP:
        cand = sorted(f for f in files if f.split("/")[0] == folder and f.rsplit("/", 1)[1].startswith(prefix))
        if prefix == "wood_click_":
            cand = [f for f in cand if re.match(r"wood_click_(f|ff|mp|pp)(_rr\d)?\.wav$", f.rsplit("/", 1)[1])]
        if not cand:
            continue
        layers = {}
        for f in cand:
            rank, rr = layer_of(f.rsplit("/", 1)[1], prefix)
            layers.setdefault(rank, []).append((rr, f))
        ranks = sorted(layers)
        out.append(f"<group> key={note} pitch_keycenter={note - tr} {extra}".rstrip())
        for i, rank in enumerate(ranks):
            lo = 1 + (127 * i) // len(ranks)
            hi = (127 * (i + 1)) // len(ranks)
            rrs = sorted(layers[rank])
            for j, (_, f) in enumerate(rrs):
                seq = f" seq_length={len(rrs)} seq_position={j + 1}" if len(rrs) > 1 else ""
                out.append(f"<region> lovel={lo} hivel={hi}{seq} sample={f}")
    return "\n".join(out) + "\n"


def install_vcsl(root: Path, cache: Path, flac: bool):
    rows = read_tsv(REPO / "library/vcsl-starter.tsv", ["path", "bytes", "blob"])
    commit = next(l.split()[-1] for l in (REPO / "library/vcsl-starter.tsv").read_text().splitlines() if l.startswith("# commit"))
    dest = root / "samples/vcsl-starter"
    stage = cache / "vcsl-starter"
    files = []
    for r in rows:
        local = stage / r["path"]
        if not local.exists() or git_blob_sha(local) != r["blob"]:
            url = f"https://raw.githubusercontent.com/{VCSL_REPO}/{commit}/" + urllib.request.quote(r["path"])
            fetch(url, local)
        if git_blob_sha(local) != r["blob"]:
            raise RuntimeError(f"VCSL blob mismatch for {r['path']}")
        files.append(r["path"])
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    kit = []
    use_flac = flac and shutil.which("flac")
    for f in files:
        folder = f.split("/")[-2] if "/Tom 1/" not in f else "Tom 1"
        name = f.rsplit("/", 1)[1]
        (dest / folder).mkdir(exist_ok=True)
        if use_flac:
            out = dest / folder / (name[:-4] + ".flac")
            subprocess.run(["flac", "-s", "-8", "-f", "-o", str(out), str(stage / f)], check=True)
            kit.append(f"{folder}/{name}")
        else:
            shutil.copy2(stage / f, dest / folder / name)
            kit.append(f"{folder}/{name}")
    sfz = gm_sfz(kit)
    if use_flac:
        sfz = re.sub(r"\.wav$", ".flac", sfz, flags=re.M)
    (dest / "starter-kit.sfz").write_text(sfz)
    (dest / "SOURCE.txt").write_text(
        "Versilian Community Sample Library (VCSL), selected percussion for a GM drum kit.\n"
        f"Source:  {VCSL_PAGE}\nCommit:  {commit}\n"
        "License: CC0 1.0 Universal (public domain dedication), per upstream.\n"
        f"Format:  {'FLAC (converted, same rate and depth)' if use_flac else 'WAV as upstream'}\n"
        "Map:     starter-kit.sfz, General MIDI drum notes (channel 10 by convention).\n"
    )
    total, count = write_manifest(root, {
        "id": "vcsl-starter-kit", "title": "VCSL Starter Kit (GM)", "source": "vcsl", "upstream": VCSL_PAGE,
        "version": "c1ea7bc", "license": "CC0-1.0", "root": root_id(root), "canonical": f"{VCSL_PAGE}/tree/{commit}",
        "format": "audio/flac" if use_flac else "audio/wav", "dc_source": VCSL_PAGE, "category": "drums", "drum": True,
        "sfz": "starter-kit.sfz", "extra": {"upstream_commit": commit},
    }, dest)
    log(f"  ok vcsl-starter-kit: {count} files, {total / 1048576:.1f} MiB ({'FLAC' if use_flac else 'WAV'})")


# ---------------------------------------------------------------- VCSL full kit and tuned packs

# Full GM kit: the starter map, with these notes added or replaced.
# note, folder, filename prefix, transpose, extra opcodes, optional filename regex
GM_FULL_EXTRA = [
    (41, "Tom 2", "TomL_HitS_", -4, "", None),
    (43, "Tom 2", "TomL_HitS_", -2, "", None),
    (45, "Tom 2", "TomL_HitS_", 0, "", None),
    (49, "Clash Cymbals 1", "cymbal_crash1_", 0, "", r"cymbal_crash1_(pp|mp|mf|ff)\d\.wav$"),
    (51, "Suspended Cymbal 1", "susCymb1_hit_stick_", 0, "", None),
    (52, "Suspended Cymbal 1", "susCymb1_hit_", -1, "", r"susCymb1_hit_(pp|mp|f|fff)1\.wav$"),
    (53, "Suspended Cymbal 1", "susCymb1_hit_bell_", 0, "", None),
    (55, "Clash Cymbals 1", "cymbal_crash1_short", 2, "", None),
    (58, "Vibraslap", "vibraslap_", 0, "", None),
    (59, "Suspended Cymbal 1", "susCymb1_hit_stick_", 2, "", None),
    (67, "Agogo Bells", "Agogo_High_", 0, "", None),
    (68, "Agogo Bells", "Agogo_Low_", 0, "", None),
    (69, "Cabasa", "Cabasa1_Hit_", 0, "", None),
    (73, "Guiro", "Guiro_Hit_", 0, "", None),
    (74, "Guiro", "Guiro_Med_", 0, "", None),
    (80, "Triangles", "Triangle6_HitM_", 0, "group=3", None),
    (81, "Triangles", "Triangle6_Hit_", 0, "group=4 off_by=3", None),
    (82, "Shaker, Large", "LShaker_Hit_", 0, "", None),
    (83, "Sleigh Bells", "Sleighbells_Hit_", 0, "", None),
    (84, "Bell Tree", "BellTree_Stroke_", 0, "", None),
    (86, "Slit Drum", "LogDrumLo_", 0, "", None),
    (87, "Slit Drum", "LogDrumHi_", 0, "", None),
]


def vcsl_fetch(tsv: str, cache: Path):
    """Downloads (or reuses) every file of a pinned VCSL list, verified by git blob id."""
    rows = read_tsv(REPO / tsv, ["path", "bytes", "blob"])
    commit = next(l.split()[-1] for l in (REPO / tsv).read_text().splitlines() if l.startswith("# commit"))
    stage = cache / "vcsl"
    for r in rows:
        local = stage / r["path"]
        if not local.exists() or git_blob_sha(local) != r["blob"]:
            url = f"https://raw.githubusercontent.com/{VCSL_REPO}/{commit}/" + urllib.request.quote(r["path"])
            fetch(url, local)
        if git_blob_sha(local) != r["blob"]:
            raise RuntimeError(f"VCSL blob mismatch for {r['path']}")
    return commit, stage, [r["path"] for r in rows]


def vcsl_copy(stage: Path, paths, dest: Path, flac: bool):
    """Copies files into dest/<instrument folder>/, optionally as FLAC. Returns kit-relative names."""
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    use_flac = bool(flac and shutil.which("flac"))
    out = []
    for f in paths:
        folder = f.split("/")[2]
        name = f.rsplit("/", 1)[1]
        (dest / folder).mkdir(exist_ok=True)
        if use_flac:
            subprocess.run(["flac", "-s", "-8", "-f", "-o", str(dest / folder / (name[:-4] + ".flac")), str(stage / f)], check=True)
        else:
            shutil.copy2(stage / f, dest / folder / name)
        out.append(f"{folder}/{name}")
    return out, use_flac


def gm_full_sfz(files):
    base = {n: (n, f, p, t, e, None) for (n, f, p, t, e) in GM_MAP}
    for row in GM_FULL_EXTRA:
        base[row[0]] = row
    out = [
        "// Trillium Base VCSL full kit, General MIDI drum map (generated by scripts/pilab-samples.py).",
        "// Samples: Versilian Community Sample Library (VCSL), CC0 1.0.",
        "<control> default_path=",
        "<global> loop_mode=one_shot ampeg_release=0.4 ampeg_attack=0.001",
    ]
    for note in sorted(base):
        _, folder, prefix, tr, extra, rx = base[note]
        cand = sorted(f for f in files if f.split("/")[0] == folder and f.rsplit("/", 1)[1].startswith(prefix))
        if prefix == "wood_click_":
            cand = [f for f in cand if re.match(r"wood_click_(f|ff|mp|pp)(_rr\d)?\.wav$", f.rsplit("/", 1)[1])]
        if rx:
            cand = [f for f in cand if re.match(rx, f.rsplit("/", 1)[1])]
        if not cand:
            continue
        layers = {}
        for f in cand:
            rank, rr = layer_of(f.rsplit("/", 1)[1], prefix)
            layers.setdefault(rank, []).append((rr, f))
        ranks = sorted(layers)
        out.append(f"<group> key={note} pitch_keycenter={note - tr} {extra}".rstrip())
        for i, rank in enumerate(ranks):
            lo = 1 + (127 * i) // len(ranks)
            hi = (127 * (i + 1)) // len(ranks)
            rrs = sorted(layers[rank])
            for j, (_, f) in enumerate(rrs):
                seq = f" seq_length={len(rrs)} seq_position={j + 1}" if len(rrs) > 1 else ""
                out.append(f"<region> lovel={lo} hivel={hi}{seq} sample={f}")
    return "\n".join(out) + "\n"


def install_vcsl_full(root: Path, cache: Path, flac: bool):
    commit, stage, paths = vcsl_fetch("library/vcsl-full-kit.tsv", cache)
    dest = root / "samples/vcsl-full-kit"
    kit, use_flac = vcsl_copy(stage, paths, dest, flac)
    sfz = gm_full_sfz(kit)
    if use_flac:
        sfz = re.sub(r"\.wav$", ".flac", sfz, flags=re.M)
    (dest / "full-kit.sfz").write_text(sfz)
    (dest / "SOURCE.txt").write_text(
        "Versilian Community Sample Library (VCSL), drums and percussion for a full GM drum kit.\n"
        f"Source:  {VCSL_PAGE}\nCommit:  {commit}\nLicense: CC0 1.0 Universal (public domain dedication), per upstream.\n"
        "Map:     full-kit.sfz, General MIDI drum notes 27-87 where VCSL has a matching instrument (channel 10).\n")
    total, count = write_manifest(root, {
        "id": "vcsl-full-kit", "title": "VCSL Full Kit (GM)", "source": "vcsl", "upstream": VCSL_PAGE,
        "version": commit[:7], "license": "CC0-1.0", "root": root_id(root), "canonical": f"{VCSL_PAGE}/tree/{commit}",
        "format": "audio/flac" if use_flac else "audio/wav", "dc_source": VCSL_PAGE, "category": "drums", "drum": True,
        "sfz": "full-kit.sfz", "extra": {"upstream_commit": commit},
    }, dest)
    log(f"  ok vcsl-full-kit: {count} files, {total / 1048576:.1f} MiB")


NOTE_RE = re.compile(r"(?:^|_)([A-G])(#|b)?(-?\d)(?=_|\.|$)")
VEL_WORDS = {"soft": 2, "medium": 4, "med": 4, "loud": 7, "pp": 2, "p": 3, "mp": 4, "mf": 5, "f": 6, "ff": 7}
TONAL = [
    # id, title, folder, filename filter, category, release
    ("vcsl-marimba", "Marimba (VCSL)", "Marimba", r"Outrigger", "mallets-and-bells", 0.8),
    ("vcsl-vibraphone", "Vibraphone (VCSL)", "Vibraphone", r"^Vibes_hard_", "mallets-and-bells", 1.2),
    ("vcsl-glockenspiel", "Glockenspiel (VCSL)", "Glockenspiel", r"^glock_", "mallets-and-bells", 1.0),
    ("vcsl-balafon", "Balafon (VCSL)", "Balafon", r"_tradM_", "mallets-and-bells", 0.5),
    ("vcsl-mbira", "Mbira, Tanzania (VCSL)", "Kalimba, Tanzania", r"^MBira3_pluck_Main_", "strings-and-plucked", 0.9),
]


def tonal_note(name: str):
    m = NOTE_RE.search(name[:-4] if name.lower().endswith((".wav", ".flac")) else name)
    if not m:
        return None
    pc = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}[m.group(1)] + {"#": 1, "b": -1, None: 0}[m.group(2)]
    return (int(m.group(3)) + 1) * 12 + pc


def tonal_vel(name: str) -> int:
    m = re.search(r"_v[l]?(\d+)_", name)
    if m:
        return int(m.group(1))
    for tok in re.split(r"[_\d.]+", name.lower()):
        if tok in VEL_WORDS:
            return VEL_WORDS[tok]
    return 0


def tonal_sfz(title: str, files, release: float) -> str:
    """Key-mapped SFZ: each sampled note covers the keys halfway to its neighbours."""
    by_note = {}
    for f in files:
        n = tonal_note(f.rsplit("/", 1)[1])
        if n is not None:
            by_note.setdefault(n, []).append(f)
    notes = sorted(by_note)
    out = [f"// Trillium Base {title}: key map generated from VCSL file names (CC0 1.0).",
           f"<control> default_path=", f"<global> ampeg_attack=0.001 ampeg_release={release}"]
    for i, n in enumerate(notes):
        lo = 0 if i == 0 else (notes[i - 1] + n) // 2 + 1
        hi = 127 if i == len(notes) - 1 else (n + notes[i + 1]) // 2
        layers = {}
        for f in by_note[n]:
            layers.setdefault(tonal_vel(f.rsplit("/", 1)[1]), []).append(f)
        ranks = sorted(layers)
        out.append(f"<group> lokey={lo} hikey={hi} pitch_keycenter={n}")
        for j, rank in enumerate(ranks):
            lv = 1 + (127 * j) // len(ranks)
            hv = (127 * (j + 1)) // len(ranks)
            rr = sorted(layers[rank])
            for k, f in enumerate(rr):
                seq = f" seq_length={len(rr)} seq_position={k + 1}" if len(rr) > 1 else ""
                out.append(f"<region> lovel={lv} hivel={hv}{seq} sample={f}")
    return "\n".join(out) + "\n"


def install_vcsl_tonal(root: Path, cache: Path, flac: bool, only=None):
    commit, stage, paths = vcsl_fetch("library/vcsl-tonal.tsv", cache)
    for bid, title, folder, rx, cat, rel in TONAL:
        if only and bid not in only and "vcsl-tonal" not in only:
            continue
        mine = [p for p in paths if p.split("/")[2] == folder and re.search(rx, p.rsplit("/", 1)[1])]
        dest = root / "samples" / bid
        files, use_flac = vcsl_copy(stage, mine, dest, flac)
        sfz = tonal_sfz(title, files, rel)
        if use_flac:
            sfz = re.sub(r"\.wav$", ".flac", sfz, flags=re.M)
        (dest / "instrument.sfz").write_text(sfz)
        (dest / "SOURCE.txt").write_text(f"Versilian Community Sample Library (VCSL): {folder}.\nSource:  {VCSL_PAGE}\nCommit:  {commit}\nLicense: CC0 1.0 Universal, per upstream.\n")
        total, count = write_manifest(root, {
            "id": bid, "title": title, "source": "vcsl", "upstream": VCSL_PAGE, "version": commit[:7], "license": "CC0-1.0",
            "root": root_id(root), "canonical": f"{VCSL_PAGE}/tree/{commit}", "format": "audio/flac" if use_flac else "audio/wav",
            "dc_source": VCSL_PAGE, "category": cat, "drum": False, "sfz": "instrument.sfz", "extra": {"upstream_commit": commit},
        }, dest)
        log(f"  ok {bid}: {count} files, {total / 1048576:.1f} MiB")


def build_index(root: Path):
    items = []
    mdir = root / "manifest"
    for y in sorted(mdir.glob("*.yaml")) if mdir.is_dir() else []:
        kv, section = {}, ""
        for line in y.read_text().splitlines():
            if not line.startswith(" ") and line.endswith(":"):
                section = line[:-1]
                continue
            m = re.match(r"^(\s*)([a-z0-9_]+):\s*(.*)$", line)
            if not m:
                continue
            key = (section + "." if m.group(1) else "") + m.group(2)
            if not m.group(1):
                section = ""
            v = m.group(3)
            if v.startswith('"'):
                v = json.loads(v)
            kv[key] = v
        if "pilab.sfz" not in kv or "samples" not in kv.get("types", ""):
            continue
        path = kv["path"]
        items.append({
            "id": kv["id"], "title": kv["title"], "category": kv.get("pilab.category", "other"),
            "drum": kv.get("pilab.drum") == "true", "sfz": f"{path}/{kv['pilab.sfz']}", "license": kv["license"],
            "bytes": int(kv.get("bytes", 0)), "files": int(kv.get("file_count", 0)), "source": kv.get("source", ""),
        })
    (root / "samples").mkdir(parents=True, exist_ok=True)
    (root / "samples/index.json").write_text(json.dumps({"schema": "pilab.samples-index/v1", "items": items}, indent=1) + "\n")
    log(f"index: {len(items)} banks, {sum(i['bytes'] for i in items) / 1048576:.1f} MiB -> {root / 'samples/index.json'}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["install", "index"])
    ap.add_argument("--root", type=Path, default=None)
    ap.add_argument("--cache", type=Path, default=None)
    ap.add_argument("--only", default="")
    ap.add_argument("--no-flac", action="store_true")
    a = ap.parse_args()
    root = (a.root or default_root()).expanduser()
    cache = (a.cache or Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "pilab/samples-dl").expanduser()
    cache.mkdir(parents=True, exist_ok=True)
    root.mkdir(parents=True, exist_ok=True)
    log(f"library root: {root}")
    if a.cmd == "install":
        only = set(filter(None, a.only.split(",")))
        vcsl_ids = {"vcsl-starter-kit", "vcsl-full-kit", "vcsl-tonal"} | {t[0] for t in TONAL}
        skipped = []
        if not only or only - vcsl_ids:
            done, skipped = install_freepats(root, cache, only - vcsl_ids)
        if not only or "vcsl-starter-kit" in only:
            install_vcsl(root, cache, not a.no_flac)
        if not only or "vcsl-full-kit" in only:
            install_vcsl_full(root, cache, not a.no_flac)
        if not only or only & ({"vcsl-tonal"} | {t[0] for t in TONAL}):
            install_vcsl_tonal(root, cache, not a.no_flac, only)
        for i, why in skipped:
            log(f"  skipped {i}: {why}")
    build_index(root)


if __name__ == "__main__":
    sys.exit(main())
