#!/usr/bin/env python3
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""Strike-to-sound helper.

Arms a console recording, prints a one-line cue, waits, stops the take, and
reports onsets in the WAV (synth + input strip if the input is connected).

One-line instruction for a person:
  When this script prints GO, strike the Trillium Zero once and keep still (needs tap percussion on if no touch strip is fitted; `trillium-base measure strike` explains this from the Zero's sensors).

Usage (on the Pi):
  python3 tools/measure-strike-to-sound.py --pre 1 --listen 2
"""
from __future__ import annotations

import argparse, json, math, struct, subprocess, sys, time
from pathlib import Path

def sh(args, timeout=30):
    return subprocess.check_output(args, text=True, timeout=timeout)

def read_wav(path: Path):
    b = path.read_bytes()
    i = b.find(b"data")
    n = struct.unpack_from("<I", b, i + 4)[0]
    raw = b[i + 8:i + 8 + n]
    s = struct.unpack("<%df" % (len(raw) // 4), raw[: len(raw) // 4 * 4])
    # stereo interleaved
    l = s[0::2]
    r = s[1::2]
    return 48000, l, r

def onset(ch, sr, thresh=10 ** (-45 / 20), pre=0.0):
    skip = int(pre * sr)
    peak = 0.0
    first = None
    for i, v in enumerate(ch[skip:], skip):
        a = abs(v)
        if a > peak:
            peak = a
        if first is None and a >= thresh:
            first = i
    return first, peak

def newest_take():
    root = Path.home() / ".local/share/pilab/recordings"
    takes = sorted(root.glob("pilab-rec-*/pilab-rec-*.wav"), key=lambda p: p.stat().st_mtime)
    return takes[-1] if takes else None

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--pre", type=float, default=1.0, help="seconds of silence before GO")
    ap.add_argument("--listen", type=float, default=2.0, help="seconds after GO")
    ap.add_argument("--keep", action="store_true", help="do not delete the take")
    args = ap.parse_args()

    print("Arming the recorder. Input strip (Yeti and/or Trillium Zero Mic) should already be connected, monitor off.", flush=True)
    sh(["trillium-base", "record", "on"])
    time.sleep(args.pre)
    print("GO — strike the Trillium Zero once and keep still (needs tap percussion on if no touch strip is fitted; `trillium-base measure strike` explains this from the Zero's sensors).", flush=True)
    t_go = time.monotonic()
    time.sleep(args.listen)
    sh(["trillium-base", "record", "off"])
    wav = newest_take()
    if not wav:
        print("no take found", file=sys.stderr)
        sys.exit(1)
    sr, l, r = read_wav(wav)
    # The take is post-master stereo: if the input is mixed and the synth
    # also sounds, both appear in the same channels. A physical strike
    # picked up by the mic should precede the synth if both are recorded.
    mid = [(a + b) * 0.5 for a, b in zip(l, r)]
    first, peak = onset(mid, sr, pre=0)
    go_sample = int(args.pre * sr)
    out = {
        "wav": str(wav),
        "duration_s": round(len(mid) / sr, 3),
        "peak_db": round(20 * math.log10(peak + 1e-12), 2),
        "first_onset_s": None if first is None else round(first / sr, 5),
        "first_onset_after_go_ms": None if first is None else round((first - go_sample) / sr * 1000, 2),
        "note": "A single-channel mix cannot split mic vs synth; for a two-source measurement, record the Yeti directly (pw-record) in parallel or use a loopback.",
    }
    print(json.dumps(out, indent=2))
    if not args.keep and wav:
        # leave deletion to the operator unless they passed --keep; default keep for a human test
        pass

if __name__ == "__main__":
    main()
