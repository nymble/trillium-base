#!/usr/bin/env python3
# Copyright (C) 2026 M-of-N
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""MIDI and telemetry timing with the Zero microphone stream off vs on.

Collects host-side arrival deltas for instrument MIDI (ALSA sequencer port 0)
and telemetry cadence, plus CPU/xrun snapshots. Does not write to the device
except when --mic on|off is passed (uses trillium-base).

Usage (on the Pi, Zero plugged in):
  python3 tools/measure-midi-responsiveness.py --seconds 20
  python3 tools/measure-midi-responsiveness.py --seconds 20 --label mic-on
"""
from __future__ import annotations

import argparse, json, os, re, statistics, subprocess, sys, time
from pathlib import Path

def sh(args, timeout=30):
    return subprocess.check_output(args, text=True, timeout=timeout)

def status():
    return json.loads(sh(["trillium-base", "status"]))

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seconds", type=float, default=15)
    ap.add_argument("--client", type=int, default=36, help="ALSA sequencer client of the Zero")
    ap.add_argument("--label", default="run")
    args = ap.parse_args()

    st = status()
    x0 = st.get("xruns", 0)
    # telemetry via CLI if a key is known
    try:
        dev = json.loads(sh(["trillium-base", "devices"]))
        keys = [z["key"] for z in dev.get("zeros", []) if z.get("found")]
    except Exception:
        keys = []

    midi_ts = []
    proc = subprocess.Popen(
        ["stdbuf", "-oL", "aseqdump", "-t", "-p", f"{args.client}:0"],
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
    )
    t_end = time.monotonic() + args.seconds
    t0 = time.monotonic()
    try:
        assert proc.stdout
        while time.monotonic() < t_end:
            line = proc.stdout.readline()
            if not line:
                break
            # aseqdump -t prefixes a timestamp like " 20:15:01.123456"
            if "Waiting" in line:
                continue
            midi_ts.append(time.monotonic() - t0)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            proc.kill()

    tel = {"n": 0}
    if keys:
        try:
            raw = sh(["trillium-base", "devices", "telemetry", keys[0], "20", "2000"], timeout=8)
            frames = json.loads(raw).get("frames") or []
            ts = [f["time_ms"] for f in frames]
            dts = [ts[i] - ts[i - 1] for i in range(1, len(ts)) if 0 < ts[i] - ts[i - 1] < 1000]
            tel = {
                "n": len(frames),
                "dt_mean": round(sum(dts) / len(dts), 2) if dts else None,
                "dt_min": min(dts) if dts else None,
                "dt_max": max(dts) if dts else None,
            }
        except Exception as e:
            tel = {"error": str(e)}

    st2 = status()
    dts = [midi_ts[i] - midi_ts[i - 1] for i in range(1, len(midi_ts))]
    dts_ms = [d * 1000 for d in dts if 0 < d < 2]
    out = {
        "label": args.label,
        "seconds": args.seconds,
        "midi_events": len(midi_ts),
        "midi_dt_ms_mean": round(statistics.mean(dts_ms), 3) if dts_ms else None,
        "midi_dt_ms_stdev": round(statistics.pstdev(dts_ms), 3) if len(dts_ms) > 1 else None,
        "midi_dt_ms_min": round(min(dts_ms), 3) if dts_ms else None,
        "midi_dt_ms_max": round(max(dts_ms), 3) if dts_ms else None,
        "xruns_start": x0,
        "xruns_end": st2.get("xruns"),
        "telemetry": tel,
    }
    print(json.dumps(out, indent=2))

if __name__ == "__main__":
    main()
