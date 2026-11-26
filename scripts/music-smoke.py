#!/usr/bin/env python3
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
"""API smoke test for the Music page endpoints against a running pilabd.

    python3 scripts/music-smoke.py [--port 8410] [--no-play] [--keep-playing]

Reads the console token from ~/.config/pilab/console-token (never printed). Plays at most
a few seconds at the default volume unless --no-play, then stops and restores the queue
state it found (stopped). Exit status 0 when every check passes.
"""
import json, os, sys, time, urllib.request, urllib.error

PORT = int(sys.argv[sys.argv.index("--port") + 1]) if "--port" in sys.argv else 8410
PLAY = "--no-play" not in sys.argv
CFG = os.environ.get("XDG_CONFIG_HOME", os.path.expanduser("~/.config"))
TOKEN = open(os.path.join(CFG, "pilab", "console-token")).read().strip()
BASE = f"http://127.0.0.1:{PORT}/api/v1"
fails = []


def call(method, path, body=None, raw=False):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=data, method=method, headers={"Authorization": "Bearer " + TOKEN, "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            b = r.read()
            return r.status, (b if raw else json.loads(b or b"null")), r.headers
    except urllib.error.HTTPError as e:
        b = e.read()
        try:
            return e.code, json.loads(b), e.headers
        except Exception:
            return e.code, b, e.headers


def check(name, cond, detail=""):
    print(("ok   " if cond else "FAIL ") + name + (f"  ({detail})" if detail and not cond else ""))
    if not cond:
        fails.append(name)


st, s, _ = call("GET", "/music/status")
check("status 200", st == 200, s)
try:
    urllib.request.urlopen(BASE + "/music/status", timeout=5)
    check("no token -> 401", False, "served without a token")
except urllib.error.HTTPError as e:
    check("no token -> 401", e.code == 401, e.code)

st, v, _ = call("POST", "/music/scan", {})
check("scan starts", st == 200, v)
for _ in range(240):
    time.sleep(0.5)
    _, s, _ = call("GET", "/music/status")
    if not s["scan"]["running"]:
        break
last = s["scan"]["last"] or {}
check("scan finished without error", not s["scan"]["error"], s["scan"]["error"])
print("     scan:", json.dumps({k: last.get(k) for k in ("albums", "tracks", "takes", "streams", "art_made", "seconds")}))
lib = s["library"]
check("library has >= 3 albums", lib["albums"] >= 3, lib)
check("library has streams", lib["streams"] >= 2, lib)

_, src, _ = call("GET", "/music/sources")
ids = {x["id"]: x for x in src["sources"]}
check("sources include demo, recordings, radio", all(k in ids for k in ("demo", "recordings", "radio")), list(ids))
check("demo source ok", ids.get("demo", {}).get("state") == "ok", ids.get("demo"))

_, al, _ = call("GET", "/music/albums")
albums = al["albums"]
check("albums listed", len(albums) >= 3)
a = next((x for x in albums if x["art"]), albums[0])
st, d, _ = call("GET", f"/music/albums/{a['id']}")
check("album detail has tracks", st == 200 and len(d["tracks"]) == a["tracks"], d if st != 200 else "")
st, img, h = call("GET", f"/music/art/{a['art']}?size=300", raw=True)
check("cover art is a JPEG", st == 200 and img[:2] == b"\xff\xd8" and h.get("Content-Type") == "image/jpeg", st)
st, _, _ = call("GET", "/music/art/zzzz", raw=True)
check("bad art key -> 404", st == 404, st)
st, _, _ = call("GET", "/music/albums/123", raw=True)
check("unknown album -> 404", st == 404, st)

_, ar, _ = call("GET", "/music/artists")
check("artists listed", len(ar["artists"]) >= 3)
st, d, _ = call("GET", f"/music/artists/{ar['artists'][0]['id']}")
check("artist detail", st == 200 and len(d["albums"]) >= 1)
_, tr, _ = call("GET", "/music/tracks?limit=500")
check("songs listed", tr["total"] == lib["tracks"] and len(tr["tracks"]) == min(500, lib["tracks"]))
word = a["title"].split()[0][:4]
_, sr, _ = call("GET", "/music/search?q=" + urllib.request.quote(word))
check(f"search '{word}' finds the album", any(x["id"] == a["id"] for x in sr["albums"]), sr)
_, sr, _ = call("GET", "/music/search?q=radio")
check("search finds a stream", len(sr["streams"]) >= 1)

_, tk, _ = call("GET", "/music/takes")
print(f"     recordings: {len(tk['takes'])} in {tk['location']}")

# A run that was cut short may have left its test stream behind.
for x in call("GET", "/music/streams")[1]["streams"]:
    if x["url"] == "https://example.invalid/smoke.mp3":
        call("DELETE", f"/music/streams/{x['id']}")
st, v, _ = call("POST", "/music/streams", {"name": "Smoke test stream", "url": "https://example.invalid/smoke.mp3"})
check("add stream", st == 200, v)
st, v, _ = call("POST", "/music/streams", {"name": "bad", "url": "file:///etc/passwd"})
check("non-http stream refused", st == 400, v)
_, sl, _ = call("GET", "/music/streams")
mine = [x for x in sl["streams"] if x["url"] == "https://example.invalid/smoke.mp3"]
check("stream listed", len(mine) == 1)
if mine:
    st, _, _ = call("DELETE", f"/music/streams/{mine[0]['id']}")
    _, sl, _ = call("GET", "/music/streams")
    check("stream removed", st == 200 and not any(x["url"] == "https://example.invalid/smoke.mp3" for x in sl["streams"]))

st, p, _ = call("POST", "/music/playlists", {"name": "Smoke test"})
check("create playlist", st == 200, p)
pid = p.get("id")
st, v, _ = call("POST", f"/music/playlists/{pid}/items", {"items": [{"kind": "album", "id": a["id"]}, {"kind": "url", "id": "https://stream.radioparadise.com/aac-128", "title": "RP"}]})
check("add album + url to playlist", st == 200 and v["added"] == a["tracks"] + 1, v)
st, v, _ = call("POST", f"/music/playlists/{pid}/move", {"from": a["tracks"], "to": 0})
_, pl, _ = call("GET", f"/music/playlists/{pid}")
check("reorder playlist", pl["items"][0]["kind"] == "stream", [i["kind"] for i in pl["items"]][:3])
st, _, _ = call("DELETE", f"/music/playlists/{pid}/items/0")
st2, _, _ = call("PUT", f"/music/playlists/{pid}", {"name": "Smoke test 2"})
_, pl, _ = call("GET", f"/music/playlists/{pid}")
check("remove item + rename", pl["playlist"]["name"] == "Smoke test 2" and pl["playlist"]["count"] == a["tracks"])

player = s["player"]
check("player (MPD) connected", player.get("connected") is True, player.get("error"))
if PLAY and player.get("connected"):
    st, v, _ = call("POST", "/music/play", {"kind": "playlist", "id": pid})
    check("play playlist", st == 200 and v["queued"] == a["tracks"], v)
    time.sleep(2.5)
    _, s2, _ = call("GET", "/music/status")
    p2 = s2["player"]
    check("playing", p2["state"] == "play" and p2["elapsed"] > 0.5, p2.get("state"))
    check("volume within cap", 0 <= p2["volume"] <= s2["volume_max"], p2["volume"])
    print("     output:", p2.get("output_path"))
    _, q, _ = call("GET", "/music/queue")
    check("queue mirrors playlist", len(q["queue"]) == a["tracks"] and q["queue"][0]["kind"] == "track", len(q["queue"]))
    _, v, _ = call("POST", "/music/control", {"action": "volume", "value": 100})
    check("volume 100 is capped", v["capped"] is True and v["player"]["volume"] == s2["volume_max"], v["player"]["volume"])
    _, v, _ = call("POST", "/music/control", {"action": "volume", "value": s2["volume_default"]})
    check("volume back to default", v["player"]["volume"] == s2["volume_default"])
    _, v, _ = call("POST", "/music/control", {"action": "seek", "value": 10})
    _, v, _ = call("POST", "/music/control", {"action": "next"})
    _, s3, _ = call("GET", "/music/status")
    check("next track", s3["player"]["pos"] == 1, s3["player"]["pos"])
    _, v, _ = call("POST", "/music/control", {"action": "toggle"})
    check("pause", v["player"]["state"] == "pause", v["player"]["state"])
    if "--keep-playing" not in sys.argv:
        call("POST", "/music/control", {"action": "stop"})
        call("DELETE", "/music/queue")
call("DELETE", f"/music/playlists/{pid}")

# Outputs: listed, the DAC is among them, choosing "DAC only" works.
st, o, _ = call("GET", "/music/outputs")
check("outputs listed", st == 200 and isinstance(o.get("outputs"), list), st)
if o.get("outputs"):
    check("dac is an output", any(x["kind"] == "dac" for x in o["outputs"]), [x.get("name") for x in o["outputs"]])
else:
    print("skip dac is an output (no PipeWire here)")
st, o2, _ = call("PUT", "/music/outputs", {"outputs": o.get("chosen", [])})
check("outputs choice saved", st == 200 and o2.get("chosen") == o.get("chosen"), o2)
st, _, _ = call("PUT", "/music/outputs", {"outputs": ["a", "b", "c", "d", "e"]})
check("more than 4 outputs refused", st == 400, st)
call("PUT", "/music/outputs", {"outputs": o.get("chosen", [])})

# Recordings trash: a throwaway take (0.2 s of silence) is trashed, restored and purged.
import struct, time, wave
_, tk, _ = call("GET", "/music/takes")
loc = tk.get("location", "")
if loc and os.path.isdir(loc):
    tid = time.strftime("pilab-smoke-%Y%m%d-%H%M%S")
    d = os.path.join(loc, tid)
    os.makedirs(d, exist_ok=True)
    with wave.open(os.path.join(d, tid + ".wav"), "wb") as w:
        w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000); w.writeframes(bytes(48000 * 4 // 5))
    open(os.path.join(d, "record.toml"), "w").write(f'id = "{tid}"\nscene = "smoke test"\nrole = "test"\n')
    def scan_wait():
        call("POST", "/music/scan", {})
        for _ in range(60):
            time.sleep(0.5)
            _, ss, _ = call("GET", "/music/status")
            if not ss["scan"]["running"]:
                return
    scan_wait()
    _, tk, _ = call("GET", "/music/takes")
    check("smoke take indexed", any(t["id"] == tid for t in tk["takes"]))
    st, r, _ = call("DELETE", f"/music/takes/{tid}")
    check("take moved to trash", st == 200 and not os.path.exists(d), r)
    trash_id = r.get("trash", {}).get("id", "")
    _, tr, _ = call("GET", "/music/trash")
    check("trash lists it", any(e["id"] == trash_id for e in tr["items"]))
    st, _, _ = call("DELETE", f"/music/trash/{trash_id}", {})
    check("purge without confirm refused", st == 400, st)
    st, r, _ = call("POST", f"/music/trash/{trash_id}/restore", {})
    check("restore puts it back", st == 200 and os.path.exists(os.path.join(d, tid + ".wav")), r)
    scan_wait()
    st, r, _ = call("DELETE", f"/music/takes/{tid}")
    check("trash again after restore", st == 200, r)
    st, r, _ = call("DELETE", f"/music/trash/{r.get('trash', {}).get('id', 'x')}", {"confirm": "delete"})
    check("purge with confirm", st == 200 and r.get("deleted") == 1, r)
    scan_wait()
    _, tk, _ = call("GET", "/music/takes")
    check("smoke take gone", not any(t["id"] == tid for t in tk["takes"]))
_, al, _ = call("GET", "/music/albums")
if al["albums"]:
    _, ad, _ = call("GET", f"/music/albums/{al['albums'][0]['id']}")
    st, _, _ = call("DELETE", f"/music/takes/{ad['tracks'][0]['id']}")
    check("a music track cannot be deleted as a take", st in (400, 404), st)

_, ep, _ = call("GET", "/endpoints")
check("endpoints list has music routes", sum(1 for e in ep if e["path"].startswith("/api/v1/music/")) >= 25)

print(f"\n{'PASS' if not fails else 'FAIL'}: {len(fails)} failed")
sys.exit(1 if fails else 0)
