// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! pilabctl: local command line for the Trillium Base service (also installed as
//! `trillium-base`, a symlink to this binary).
//!
//! ```text
//! pilabctl pair [--rotate]      show (or replace) the pairing token and console URLs
//! pilabctl pair --qr            one-time pairing code (5 min, single use) as a QR code + URL
//! pilabctl status               engine and machine status
//! pilabctl note NOTE [VEL] [SECONDS]   play a test note through the console port
//! pilabctl gain DB              set the master level (capped at -6 dBFS)
//! pilabctl run on|off           fade the synth in or out
//! pilabctl panic                all sound off
//! pilabctl scenes | load NAME | save NAME
//! pilabctl record on|off
//! pilabctl devices show [KEY]   Trillium Zero sensors (present / not fitted / running), note path, settings
//! pilabctl measure strike [--rehearse]   one strike: Zero note-on, synth onset, acoustic onset (JSON result)
//! pilabctl apps [list | show ID | validate FILE | enable ID | disable ID]   app manifests
//! ```

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

#[path = "../apps_cli.rs"]
mod apps_cli;

const PORT: u16 = 8410;

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join(".config"))
        .join("pilab")
}

fn token() -> String {
    std::fs::read_to_string(config_dir().join("console-token")).map(|s| s.trim().to_string()).unwrap_or_else(|_| {
        eprintln!("no pairing token yet: start the service first (systemctl --user start pilabd)");
        std::process::exit(1)
    })
}

fn port() -> u16 {
    std::env::var("PILAB_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(PORT)
}

fn http(method: &str, path: &str, body: Option<String>) -> Result<String, String> {
    http_timeout(method, path, body, 10)
}

fn http_timeout(method: &str, path: &str, body: Option<String>, secs: u64) -> Result<String, String> {
    let mut s = TcpStream::connect(("127.0.0.1", port())).map_err(|e| format!("cannot reach pilabd on port {}: {e}", port()))?;
    s.set_read_timeout(Some(Duration::from_secs(secs))).ok();
    let body = body.unwrap_or_default();
    let req = format!(
        "{method} /api/v1{path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        token(),
        body.len()
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut resp = String::new();
    s.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    let (head, rest) = resp.split_once("\r\n\r\n").unwrap_or((&resp, ""));
    let code: u16 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") { dechunk(rest) } else { rest.to_string() };
    if (200..300).contains(&code) {
        Ok(body)
    } else {
        Err(format!("HTTP {code}: {body}"))
    }
}

fn dechunk(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some((len, tail)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(len.trim(), 16).unwrap_or(0);
        if n == 0 || tail.len() < n {
            break;
        }
        out.push_str(&tail[..n]);
        rest = tail[n..].trim_start_matches("\r\n");
    }
    out
}

fn addrs() -> Vec<String> {
    std::process::Command::new("hostname")
        .arg("-I")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().filter(|a| !a.contains(':')).map(|s| s.to_string()).collect())
        .unwrap_or_default()
}

/// Writes a one-time pairing code (8 characters, no look-alikes, valid 5 minutes, single
/// use) and shows it as a QR code pointing at the console. The console exchanges it for
/// the token, so the token itself never has to be typed or shown.
fn pair_qr() {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
    const TTL: u64 = 300;
    let mut buf = [0u8; 8];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).expect("urandom");
    let code: String = buf.iter().map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect();
    let shown = format!("{}-{}", &code[..4], &code[4..]);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = config_dir().join("pair-code");
    let tmp = path.with_extension("tmp");
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).expect("write pairing code");
        writeln!(f, "{code} {}", now + TTL).unwrap();
    }
    std::fs::rename(&tmp, &path).expect("rename pairing code");
    let url = format!("http://{}.local:{}/#pair={shown}", hostname(), port());
    let qr = std::process::Command::new("qrencode").args(["-t", "ansiutf8", "-m", "2", &url]).output();
    match qr {
        Ok(o) if o.status.success() => println!("{}", String::from_utf8_lossy(&o.stdout)),
        _ => println!("(install qrencode to show a scannable QR code here)\n"),
    }
    println!("Scan with a phone camera, or open:\n\n    {url}\n");
    println!("Or type the code into the console's pairing box:  {shown}");
    println!("Valid for {} minutes, works once. A new `pair --qr` replaces it.", TTL / 60);
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").map(|s| s.trim().to_string()).unwrap_or_else(|_| "pilab".into())
}

fn out(r: Result<String, String>) {
    match r {
        Ok(b) => println!("{b}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match a.as_slice() {
        ["pair", rest @ ..] if rest.contains(&"--qr") => pair_qr(),
        ["pair", rest @ ..] => {
            let path = config_dir().join("console-token");
            if rest.contains(&"--rotate") {
                let mut buf = [0u8; 24];
                std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).expect("urandom");
                let t: String = buf.iter().map(|b| format!("{b:02x}")).collect();
                let tmp = path.with_extension("tmp");
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).expect("write token");
                    writeln!(f, "{t}").unwrap();
                }
                std::fs::rename(&tmp, &path).expect("rename token");
                println!("New token written; paired browsers must re-enter it (pilabd picks it up within 2 s).");
            }
            let t = token();
            println!("Pairing token (keep it private; type it into the console once per browser):\n\n    {t}\n");
            println!("Console:");
            println!("    http://{}.local:{}/", hostname(), port());
            for ip in addrs() {
                println!("    http://{ip}:{}/", port());
            }
        }
        ["status"] => out(http("GET", "/status", None)),
        ["system"] => out(http("GET", "/system", None)),
        ["note", n, rest @ ..] => {
            let note: u8 = n.parse().unwrap_or(69).min(127);
            let vel: u8 = rest.first().and_then(|v| v.parse().ok()).unwrap_or(90).clamp(1, 127);
            let secs: f32 = rest.get(1).and_then(|v| v.parse().ok()).unwrap_or(1.0f32).clamp(0.05, 30.0);
            out(http("POST", "/midi", Some(format!("{{\"bytes\":[144,{note},{vel}]}}"))));
            std::thread::sleep(Duration::from_secs_f32(secs));
            out(http("POST", "/midi", Some(format!("{{\"bytes\":[128,{note},0]}}"))));
        }
        ["gain", db] => out(http("POST", "/master", Some(format!("{{\"db\":{}}}", db.parse::<f32>().unwrap_or(-18.0))))),
        ["run", v] => out(http("POST", "/engine", Some(format!("{{\"run\":{}}}", *v == "on")))),
        ["panic"] => out(http("POST", "/panic", Some("{}".into()))),
        ["scenes"] => out(http("GET", "/scenes", None)),
        ["load", name] => out(http("POST", &format!("/scenes/{}/load", name.replace(' ', "%20")), Some("{}".into()))),
        ["save", name] => out(http("POST", &format!("/scenes/{}/save", name.replace(' ', "%20")), Some("{}".into()))),
        ["record", v] => out(http("POST", "/record", Some(format!("{{\"on\":{}}}", *v == "on")))),
        ["devices"] => out(http("GET", "/devices", None)),
        ["devices", "show"] => devices_show(None),
        ["devices", "show", key] => devices_show(Some(key)),
        ["devices", "snapshot", key] => out(http("POST", &format!("/devices/{}/snapshot", key.replace(' ', "%20")), Some("{}".into()))),
        ["devices", "reapply", key] => out(http("POST", &format!("/devices/{}/reapply", key.replace(' ', "%20")), Some("{\"confirm\":true}".into()))),
        ["devices", "telemetry", key] => out(http("GET", &format!("/devices/{}/telemetry?hz=20&ms=1000", key.replace(' ', "%20")), None)),
        ["devices", "telemetry", key, hz] => out(http("GET", &format!("/devices/{}/telemetry?hz={}&ms=1000", key.replace(' ', "%20"), hz), None)),
        ["devices", "telemetry", key, hz, ms] => out(http("GET", &format!("/devices/{}/telemetry?hz={}&ms={}", key.replace(' ', "%20"), hz, ms), None)),
        ["devices", "mic", key, "on"] => out(http("POST", &format!("/devices/{}/mic", key.replace(' ', "%20")), Some("{\"on\":true,\"confirm\":true}".into()))),
        ["devices", "mic", key, "off"] => out(http("POST", &format!("/devices/{}/mic", key.replace(' ', "%20")), Some("{\"on\":false}".into()))),
        ["measure", "strike"] | ["measure", "strike", "--rehearse"] => {
            let rehearse = a.len() == 3;
            // Print the cue immediately; the HTTP call blocks for up to 60 s.
            let cue = if rehearse {
                "REHEARSAL — no strike needed: the Base injects one note on the Zero's input and times it to the synth.".to_string()
            } else {
                http("GET", "/measure/strike/cue", None)
                    .ok()
                    .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok())
                    .and_then(|v| v["cue"].as_str().map(|s| s.to_string()))
                    .unwrap_or_else(|| "GO — strike the Trillium Zero once, then keep still.".into())
            };
            println!("{cue}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            out(http_timeout("POST", "/measure/strike", Some(format!("{{\"timeout_s\":60,\"rehearse\":{rehearse}}}")), 75))
        }
        ["apps", rest @ ..] => {
            if let Err(e) = apps_cli::run(rest) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        ["inputs"] => out(http("GET", "/midi/inputs", None)),
        ["input"] => out(http("GET", "/input", None)),
        ["input", "source", glob] => out(http("POST", "/input", Some(format!("{{\"source\":{}}}", json_str(glob))))),
        ["input", "gain", db] => out(http("POST", "/input", Some(format!("{{\"gain_db\":{}}}", db.parse::<f32>().unwrap_or(0.0))))),
        ["input", "record", v] => out(http("POST", "/input", Some(format!("{{\"record\":{}}}", *v == "on")))),
        ["input", "monitor", "off"] => out(http("POST", "/input", Some("{\"monitor\":false}".into()))),
        _ => {
            eprintln!("usage: {} pair [--rotate|--qr] | status | system | note N [VEL] [SECS] | gain DB | run on|off | panic | scenes | load NAME | save NAME | record on|off | devices [snapshot KEY | reapply KEY | telemetry KEY [HZ [MS]] | mic KEY on|off | show [KEY]] | measure strike [--rehearse] | apps [list | show ID | validate FILE | enable ID | disable ID] | inputs | input [source GLOB | gain DB | record on|off | monitor off]", prog());
            std::process::exit(2);
        }
    }
}

/// Human-readable Trillium Zero summary: sensors, which sensor path makes notes, key settings.
fn devices_show(only: Option<&str>) {
    let body = match http("GET", "/devices", None) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let zeros = v["zeros"].as_array().cloned().unwrap_or_default();
    let mut shown = 0;
    for z in zeros.iter().filter(|z| only.map_or(true, |k| z["key"] == k)) {
        shown += 1;
        let up = if z["found"].is_null() { "disconnected" } else { "connected" };
        println!("Trillium Zero {}  ({up}, firmware {}, profile {})", z["key"].as_str().unwrap_or("?"), z["hello"]["firmware"].as_str().unwrap_or("?"), z["profile_state"].as_str().unwrap_or("?"));
        println!("Sensors:");
        for s in z["sensors"].as_array().cloned().unwrap_or_default() {
            println!("  {:<20} {:<12} {}", s["label"].as_str().unwrap_or(""), s["state"].as_str().unwrap_or("").replace('_', " "), s["detail"].as_str().unwrap_or(""));
        }
        let np = &z["note_path"];
        println!("Notes: {}{}", np["summary"].as_str().unwrap_or("?"), if np["strike_ready"] == true { "  [strike ready]" } else { "" });
        if let Some(h) = np["hint"].as_str().filter(|h| !h.is_empty()) {
            println!("  {h}");
        }
        let get = |id: u64| z["params"].as_array().and_then(|a| a.iter().find(|p| p["id"] == id)).map(|p| p["value"].to_string()).unwrap_or_else(|| "?".into());
        println!("Settings: mode {} · tap percussion {} (ch {} note {}) · strike directions {} · tap threshold {} centi-g · mic {}", get(14), get(11), get(12), get(13), get(31), get(25), if z["mic"] == true { "on" } else { "off" });
    }
    if shown == 0 {
        println!("No Trillium Zero found.");
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Name this binary was run as (`pilabctl` or the `trillium-base` alias).
fn prog() -> String {
    std::env::args().next().and_then(|a| std::path::Path::new(&a).file_name().map(|f| f.to_string_lossy().to_string())).unwrap_or_else(|| "pilabctl".into())
}
