// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! `trillium-base music ...`: the music library and player from the command line.
//! Included into pilabctl; talks to pilabd's /api/v1/music endpoints.

use serde_json::{json, Value};

type Http<'a> = &'a dyn Fn(&str, &str, Option<String>) -> Result<String, String>;

pub const USAGE: &str = "music [status | scan [--wait] | sources | albums | artists | songs | takes | streams | search TEXT.. | playlists | \
play album|artist|track|take|stream|playlist ID | play url URL | play TEXT.. | add KIND ID | next-up KIND ID | queue [clear] | \
toggle | pause | resume | stop | next | prev | seek SECS | vol N | repeat on|off | shuffle on|off | stream add NAME URL | stream rm ID | \
outputs | output dac|NAME.. | take rm ID | trash | trash restore ID | trash empty --yes]";

fn die(e: impl std::fmt::Display) -> ! {
    eprintln!("{e}");
    std::process::exit(1)
}

fn get(http: Http, path: &str) -> Value {
    serde_json::from_str(&http("GET", path, None).unwrap_or_else(|e| die(e))).unwrap_or_default()
}

fn post(http: Http, path: &str, body: Value) -> Value {
    serde_json::from_str(&http("POST", path, Some(body.to_string())).unwrap_or_else(|e| die(e))).unwrap_or_default()
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn mmss(secs: f64) -> String {
    let t = secs.max(0.0) as u64;
    if t >= 3600 {
        format!("{}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
    } else {
        format!("{}:{:02}", t / 60, t % 60)
    }
}

fn qenc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn print_status(v: &Value) {
    let l = &v["library"];
    println!(
        "library: {} albums, {} artists, {} songs ({}), {} recordings, {} streams, {} playlists",
        l["albums"], l["artists"], l["tracks"], mmss(l["duration"].as_f64().unwrap_or(0.0)), l["takes"], l["streams"], l["playlists"]
    );
    let sc = &v["scan"];
    if sc["running"].as_bool() == Some(true) {
        println!("scan:    running ({}, {} files)", s(sc, "phase"), sc["files"]);
    } else if !s(sc, "error").is_empty() {
        println!("scan:    error: {}", s(sc, "error"));
    }
    let p = &v["player"];
    if p["connected"].as_bool() != Some(true) {
        println!("player:  not reachable: {}", s(p, "error"));
        return;
    }
    let cur = &p["current"];
    let what = if cur.is_null() { "-".to_string() } else { format!("{} — {}", s(cur, "title"), s(cur, "subtitle")) };
    println!("player:  {} {}  [{} / {}]  vol {}% (max {}%)", s(p, "state"), what, mmss(p["elapsed"].as_f64().unwrap_or(0.0)), mmss(p["duration"].as_f64().unwrap_or(0.0)), p["volume"], v["volume_max"]);
    if !s(cur, "live").is_empty() {
        println!("         on air: {}", s(cur, "live"));
    }
    if !s(p, "output_path").is_empty() {
        println!("output:  {}", s(p, "output_path"));
    }
    let o = &v["outputs"];
    if let Some(outs) = o["outputs"].as_array() {
        let used: Vec<String> = outs.iter().filter(|x| x["used"].as_bool() == Some(true)).map(|x| format!("{} ({} ch)", s(x, "label"), x["channels"].as_array().map(|a| a.len()).unwrap_or(0))).collect();
        println!("to:      {}", if used.is_empty() { "no output present".to_string() } else { used.join(" + ") });
    }
    if !v["recovered"].is_null() {
        println!("note:    player {} at {}", s(&v["recovered"], "what"), v["recovered"]["at"]);
    }
    println!("queue:   {} items{}{}", p["length"], if p["repeat"].as_bool() == Some(true) { ", repeat" } else { "" }, if p["random"].as_bool() == Some(true) { ", shuffle" } else { "" });
}

fn put(http: Http, path: &str, body: Value) -> Value {
    serde_json::from_str(&http("PUT", path, Some(body.to_string())).unwrap_or_else(|e| die(e))).unwrap_or_default()
}

fn print_outputs(v: &Value) {
    for o in v["outputs"].as_array().cloned().unwrap_or_default() {
        let mark = if o["used"].as_bool() == Some(true) { "*" } else if o["chosen"].as_bool() == Some(true) { "?" } else { " " };
        let ch = o["channels"].as_array().map(|a| a.iter().filter_map(|c| c.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        println!("{mark} {:<10} {:<28} {:<10} {}", s(&o, "kind"), s(&o, "label"), ch, s(&o, "name"));
    }
    for c in v["unclaimed_cards"].as_array().cloned().unwrap_or_default() {
        println!("! card {} {} is attached but PipeWire has not set it up", c["card"], s(&c, "name"));
    }
    if let Some(m) = v["missing"].as_array().filter(|m| !m.is_empty()) {
        println!("chosen but not present: {}", m.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "));
    }
}

fn control(http: Http, action: &str, value: Option<f64>) {
    let v = post(http, "/music/control", json!({ "action": action, "value": value }));
    if v["capped"].as_bool() == Some(true) {
        println!("volume capped at {}%", v["volume_max"]);
    }
    let p = &v["player"];
    println!("{} vol {}%", s(p, "state"), p["volume"]);
}

fn play(http: Http, kind: &str, id: &str, mode: &str) {
    let body = json!({ "kind": kind, "id": id, "mode": mode });
    let r = http("POST", "/music/play", Some(body.to_string())).unwrap_or_else(|e| die(e));
    let v: Value = serde_json::from_str(&r).unwrap_or_default();
    println!("{} {} item(s)", if mode == "replace" { "playing" } else { "queued" }, v["queued"]);
}

pub fn run(args: &[&str], http: Http) {
    match args {
        [] | ["status"] => print_status(&get(http, "/music/status")),
        ["scan"] => println!("{}", post(http, "/music/scan", json!({}))),
        ["scan", "--wait"] => {
            post(http, "/music/scan", json!({}));
            loop {
                std::thread::sleep(std::time::Duration::from_millis(700));
                let v = get(http, "/music/status");
                if v["scan"]["running"].as_bool() != Some(true) {
                    if let Some(last) = v["scan"]["last"].as_object() {
                        for src in last.get("sources").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
                            println!("{:<18} {:<8} files {:<6} added {:<5} changed {:<5} removed {:<5} unreadable {}", s(&src, "id"), s(&src, "state"), src["files"], src["added"], src["changed"], src["removed"], src["unreadable"]);
                        }
                        println!("{} albums, {} songs, {} recordings, {} streams, {} new thumbnails in {:.1} s", last["albums"], last["tracks"], last["takes"], last["streams"], last["art_made"], last["seconds"].as_f64().unwrap_or(0.0));
                    }
                    if !s(&v["scan"], "error").is_empty() {
                        die(s(&v["scan"], "error"));
                    }
                    break;
                }
            }
        }
        ["sources"] => {
            let v = get(http, "/music/sources");
            println!("config: {}", s(&v, "file"));
            for x in v["sources"].as_array().cloned().unwrap_or_default() {
                println!("{:<18} {:<7} {:<10} {:<8} files {:<6} {}", s(&x, "id"), s(&x, "kind"), s(&x, "collection"), x["state"].as_str().unwrap_or(if x["present"].as_bool() == Some(true) { "new" } else { "offline" }), x["files"], s(&x, "path"));
            }
        }
        ["albums"] => {
            for a in get(http, "/music/albums")["albums"].as_array().cloned().unwrap_or_default() {
                println!("{:<20} {:<28} {:<40} {:>4} {:>3} tracks {:>8}  {}", a["id"], s(&a, "artist"), s(&a, "title"), a["year"], a["tracks"], mmss(a["duration"].as_f64().unwrap_or(0.0)), s(&a, "format"));
            }
        }
        ["artists"] => {
            for a in get(http, "/music/artists")["artists"].as_array().cloned().unwrap_or_default() {
                println!("{:<20} {:<32} {} albums, {} songs", a["id"], s(&a, "name"), a["albums"], a["tracks"]);
            }
        }
        ["songs"] => {
            for t in get(http, "/music/tracks?limit=5000")["tracks"].as_array().cloned().unwrap_or_default() {
                println!("{:<8} {:<40} {:<26} {:<30} {:>8}", t["id"], s(&t, "title"), s(&t, "artist"), s(&t, "album"), mmss(t["duration"].as_f64().unwrap_or(0.0)));
            }
        }
        ["takes"] => {
            let v = get(http, "/music/takes");
            println!("location: {}", s(&v, "location"));
            for t in v["takes"].as_array().cloned().unwrap_or_default() {
                println!("{:<28} {:<10} {:<16} {:>8}  {}", s(&t, "id"), s(&t, "session"), s(&t, "title"), mmss(t["duration"].as_f64().unwrap_or(0.0)), s(&t, "scene"));
            }
        }
        ["streams"] => {
            for t in get(http, "/music/streams")["streams"].as_array().cloned().unwrap_or_default() {
                println!("{:<20} {:<32} {}", t["id"], s(&t, "name"), s(&t, "url"));
            }
        }
        ["stream", "add", name, url] => println!("{}", post(http, "/music/streams", json!({ "name": name, "url": url }))),
        ["stream", "rm", id] => println!("{}", http("DELETE", &format!("/music/streams/{}", qenc(id)), None).unwrap_or_else(|e| die(e))),
        ["playlists"] => {
            for p in get(http, "/music/playlists")["playlists"].as_array().cloned().unwrap_or_default() {
                println!("{:<6} {:<32} {} items {}", p["id"], s(&p, "name"), p["count"], mmss(p["duration"].as_f64().unwrap_or(0.0)));
            }
        }
        ["search", q @ ..] if !q.is_empty() => {
            let v = get(http, &format!("/music/search?q={}", qenc(&q.join(" "))));
            for a in v["artists"].as_array().cloned().unwrap_or_default() {
                println!("artist  {:<20} {}", a["id"], s(&a, "name"));
            }
            for a in v["albums"].as_array().cloned().unwrap_or_default() {
                println!("album   {:<20} {} — {}", a["id"], s(&a, "title"), s(&a, "artist"));
            }
            for t in v["tracks"].as_array().cloned().unwrap_or_default() {
                println!("track   {:<20} {} — {} ({})", t["id"], s(&t, "title"), s(&t, "artist"), s(&t, "album"));
            }
            for t in v["takes"].as_array().cloned().unwrap_or_default() {
                println!("take    {:<20} {} ({})", s(&t, "id"), s(&t, "title"), s(&t, "session"));
            }
            for t in v["streams"].as_array().cloned().unwrap_or_default() {
                println!("stream  {:<20} {}", t["id"], s(&t, "name"));
            }
        }
        ["play", "url", url] => play(http, "url", url, "replace"),
        ["play", kind @ ("album" | "artist" | "track" | "take" | "stream" | "playlist"), id] => play(http, kind, id, "replace"),
        ["add", kind, id] => play(http, kind, id, "append"),
        ["next-up", kind, id] => play(http, kind, id, "next"),
        ["play"] => control(http, "play", None),
        ["play", q @ ..] => {
            // Free text: first album, then song, then stream that matches.
            let v = get(http, &format!("/music/search?q={}", qenc(&q.join(" "))));
            if let Some(a) = v["albums"].get(0) {
                println!("{} — {}", s(a, "title"), s(a, "artist"));
                play(http, "album", &a["id"].to_string(), "replace");
            } else if let Some(t) = v["tracks"].get(0) {
                println!("{} — {}", s(t, "title"), s(t, "artist"));
                play(http, "track", &t["id"].to_string(), "replace");
            } else if let Some(t) = v["streams"].get(0) {
                println!("{}", s(t, "name"));
                play(http, "stream", &t["id"].to_string(), "replace");
            } else {
                die(format!("nothing matches {:?}", q.join(" ")));
            }
        }
        ["queue"] => {
            let st = get(http, "/music/status");
            let cur = st["player"]["pos"].as_i64().unwrap_or(-1);
            for e in get(http, "/music/queue")["queue"].as_array().cloned().unwrap_or_default() {
                let mark = if e["pos"].as_i64() == Some(cur) { ">" } else { " " };
                println!("{mark}{:>3}  {:<40} {:<40} {:>8}", e["pos"], s(&e, "title"), s(&e, "subtitle"), mmss(e["duration"].as_f64().unwrap_or(0.0)));
            }
        }
        ["queue", "clear"] => println!("{}", http("DELETE", "/music/queue", None).unwrap_or_else(|e| die(e))),
        [a @ ("toggle" | "pause" | "stop" | "next" | "prev")] => control(http, a, None),
        ["resume"] => control(http, "play", None),
        ["seek", secs] => control(http, "seek", Some(secs.parse().unwrap_or_else(|_| die("seek SECONDS")))),
        ["vol" | "volume", n] => control(http, "volume", Some(n.parse().unwrap_or_else(|_| die("vol 0-100")))),
        ["repeat", v] => control(http, "repeat", Some((*v == "on") as u8 as f64)),
        ["shuffle", v] => control(http, "random", Some((*v == "on") as u8 as f64)),
        ["outputs"] => print_outputs(&get(http, "/music/outputs")),
        ["output", "dac"] => print_outputs(&put(http, "/music/outputs", json!({ "outputs": [] }))),
        ["output", names @ ..] if !names.is_empty() => {
            let all = get(http, "/music/outputs");
            let list = all["outputs"].as_array().cloned().unwrap_or_default();
            let mut pick = vec![];
            for n in names {
                let l = n.to_lowercase();
                let hit = list.iter().find(|x| s(x, "name") == *n).or_else(|| list.iter().find(|x| s(x, "label").to_lowercase().starts_with(&l))).or_else(|| list.iter().find(|x| s(x, "label").to_lowercase().contains(&l)));
                match hit {
                    Some(x) => pick.push(s(x, "name").to_string()),
                    None => die(format!("no output matches {n:?} (see: trillium-base music outputs)")),
                }
            }
            print_outputs(&put(http, "/music/outputs", json!({ "outputs": pick })));
        }
        ["take", "rm" | "delete", id] => {
            let v: Value = serde_json::from_str(&http("DELETE", &format!("/music/takes/{}", qenc(id)), None).unwrap_or_else(|e| die(e))).unwrap_or_default();
            println!("moved to trash: {}  (undo: trillium-base music trash restore {})", s(&v["trash"], "title"), s(&v["trash"], "id"));
        }
        ["trash"] => {
            let v = get(http, "/music/trash");
            for e in v["items"].as_array().cloned().unwrap_or_default() {
                println!("{:<44} {:<18} {:>8.1} MB  {}", s(&e, "id"), s(&e, "title"), e["bytes"].as_f64().unwrap_or(0.0) / 1e6, s(&e, "original"));
            }
            println!("{:.1} MB in the trash", v["bytes"].as_f64().unwrap_or(0.0) / 1e6);
        }
        ["trash", "restore", id] => println!("{}", post(http, &format!("/music/trash/{}/restore", qenc(id)), json!({}))),
        ["trash", "empty", "--yes"] => println!("{}", post(http, "/music/trash/empty", json!({ "confirm": "delete" }))),
        _ => {
            eprintln!("usage: trillium-base {USAGE}");
            std::process::exit(2);
        }
    }
}
