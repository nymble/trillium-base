// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Music: the library index (pilab-music) and the player.
//!
//! Playback runs in MPD, a separate process (user unit pilab-mpd.service) reached over its
//! private UNIX socket (`$MPD_HOST`, set in pilabd.service). MPD's PipeWire stream is not
//! auto-linked; music_out links it by name to the chosen outputs (the DAC by default)
//! beside the synth. Nothing here touches the DAC level
//! (0.70) or the synth master (-18 dB). Player volume is MPD's own software volume, starts
//! at DEFAULT_VOLUME and is held at or below MAX_VOLUME.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use pilab_music::scan::ScanCtl;
use pilab_music::{ArtCache, Index, ScanReport, Sources};
use serde::Deserialize;
use serde_json::{json, Value};
use zpod_mpd::Client;

use crate::state::App;

pub const DEFAULT_VOLUME: u8 = 40;
pub const MAX_VOLUME: u8 = 80;
/// The PipeWire graph rate the synth runs at; other rates are resampled by MPD (soxr) to it.
pub const GRAPH_RATE: u32 = 48_000;

pub const ENDPOINTS: &[(&str, &str, &str)] = &[
    ("GET", "/api/v1/music/status", "Library counts, scan progress, player state (MPD), now playing, volume and its cap, output format"),
    ("GET", "/api/v1/music/sources", "Library sources (folders, drive, recordings, URL lists) with state and file counts"),
    ("PUT", "/api/v1/music/sources", "{source:[{id, kind, path, collection, name?, optional?, enabled?}]} replace the sources (saved) and rescan"),
    ("POST", "/api/v1/music/scan", "Start a background scan (incremental: only new or changed files are read)"),
    ("GET", "/api/v1/music/albums?sort=artist|title|year|added", "Albums (Music collection)"),
    ("GET", "/api/v1/music/albums/{id}", "One album and its tracks"),
    ("GET", "/api/v1/music/artists", "Album artists"),
    ("GET", "/api/v1/music/artists/{id}", "One artist: albums and appearances"),
    ("GET", "/api/v1/music/tracks?limit=&offset=", "Songs, by title"),
    ("GET", "/api/v1/music/search?q=", "Search albums, artists, songs, recordings and streams (prefix, accent-insensitive)"),
    ("GET", "/api/v1/music/takes", "Recordings collection: takes made on this unit"),
    ("GET", "/api/v1/music/streams", "Internet radio and other URL streams"),
    ("POST", "/api/v1/music/streams", "{name, url} add a stream to the console's list"),
    ("DELETE", "/api/v1/music/streams/{id}", "Remove a stream from the console's list"),
    ("GET", "/api/v1/music/art/{key}?size=300|600", "Cover art thumbnail (JPEG)"),
    ("GET", "/api/v1/music/playlists", "Playlists"),
    ("POST", "/api/v1/music/playlists", "{name} create a playlist"),
    ("GET", "/api/v1/music/playlists/{id}", "A playlist and its items"),
    ("PUT", "/api/v1/music/playlists/{id}", "{name} rename"),
    ("DELETE", "/api/v1/music/playlists/{id}", "Delete a playlist"),
    ("POST", "/api/v1/music/playlists/{id}/items", "{items:[{kind: track|take|url|album, id, title?}]} append"),
    ("DELETE", "/api/v1/music/playlists/{id}/items/{pos}", "Remove one item"),
    ("POST", "/api/v1/music/playlists/{id}/move", "{from, to} reorder"),
    ("POST", "/api/v1/music/play", "{kind: album|artist|track|tracks|take|stream|playlist|url, id|ids, start?, mode?: replace|next|append, shuffle?}"),
    ("GET", "/api/v1/music/queue", "The play queue"),
    ("DELETE", "/api/v1/music/queue", "Clear the queue"),
    ("DELETE", "/api/v1/music/queue/{pos}", "Remove one queue entry"),
    ("POST", "/api/v1/music/queue/{pos}/play", "Jump to a queue entry"),
    ("POST", "/api/v1/music/queue/move", "{from, to} reorder the queue"),
    ("POST", "/api/v1/music/control", "{action: toggle|play|pause|stop|next|prev|seek|volume|repeat|random, value?}"),
    ("GET", "/api/v1/music/outputs", "Where music plays: every audio output (DAC, USB, AirPlay, ...) with channels, chosen/used/active, and sound cards PipeWire has not set up"),
    ("PUT", "/api/v1/music/outputs", "{outputs:[sink node.name, ...]} route music to these outputs (empty = the DAC); never moves the synth"),
    ("DELETE", "/api/v1/music/takes/{id}", "Move a recording to the trash of its recordings folder (restorable; never published music)"),
    ("GET", "/api/v1/music/trash", "Recordings in the trash"),
    ("POST", "/api/v1/music/trash/{id}/restore", "Put a trashed recording back"),
    ("DELETE", "/api/v1/music/trash/{id}", "{confirm:\"delete\"} delete one trashed recording for good"),
    ("POST", "/api/v1/music/trash/empty", "{confirm:\"delete\"} empty the recordings trash for good"),
];

pub struct ScanState {
    pub running: bool,
    pub started: i64,
    pub finished: i64,
    pub last: Option<ScanReport>,
    pub error: String,
    pub ctl: Option<Arc<ScanCtl>>,
}

pub struct Music {
    pub db_path: PathBuf,
    pub sources_path: PathBuf,
    pub streams_path: PathBuf,
    pub art: ArtCache,
    pub ix: Mutex<Index>,
    mpd: Mutex<Option<Client>>,
    pub scan: Mutex<ScanState>,
    /// Last automatic player recovery (time, what), shown on the Music page.
    pub recovered: Mutex<Value>,
    app: Arc<App>,
}

static MUSIC: OnceLock<Arc<Music>> = OnceLock::new();

fn svc() -> Result<Arc<Music>, String> {
    MUSIC.get().cloned().ok_or_else(|| "music library not started".to_string())
}

fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".local/state"))
}
fn cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(".cache"))
}

/// Default MPD socket (same path the pilab-mpd unit binds).
pub fn mpd_socket() -> PathBuf {
    state_home().join("pilab/mpd/socket")
}

/// Open the index and start the first scan a little after boot. Never fails startup:
/// on error the Music page shows what went wrong.
pub fn init(app: Arc<App>) {
    // pilabd.service sets MPD_HOST; when run by hand, use the unit's socket path.
    if std::env::var_os("MPD_HOST").is_none() {
        std::env::set_var("MPD_HOST", mpd_socket());
    }
    let cfg = app.paths.config.join("music");
    let db_path = state_home().join("pilab/music/library.sqlite");
    let ix = match Index::open(&db_path) {
        Ok(ix) => ix,
        Err(e) => {
            app.event("error", format!("music library: {e}; using a temporary index"));
            match Index::open_memory() {
                Ok(i) => i,
                Err(_) => return,
            }
        }
    };
    let m = Arc::new(Music {
        db_path,
        sources_path: cfg.join("sources.toml"),
        streams_path: cfg.join("streams.m3u"),
        art: ArtCache::new(cache_home().join("pilab/music-art")),
        ix: Mutex::new(ix),
        mpd: Mutex::new(None),
        scan: Mutex::new(ScanState { running: false, started: 0, finished: 0, last: None, error: String::new(), ctl: None }),
        recovered: Mutex::new(Value::Null),
        app: app.clone(),
    });
    let _ = MUSIC.set(m.clone());
    crate::music_out::init(app.clone());
    spawn_watchdog(m.clone());
    std::thread::Builder::new()
        .name("music-watch".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(15));
            start_scan(&m);
            let mut last_rec = recordings_mtime(&m);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(20));
                // A new take shows up in Recordings without a manual rescan.
                let r = recordings_mtime(&m);
                if r != last_rec && !m.app.recorder.is_recording() {
                    last_rec = r;
                    start_scan(&m);
                }
                // Hold the volume cap even if another MPD client raised it.
                let _ = m.with_mpd(|c| {
                    let st = c.status()?;
                    if st.volume.map(|v| v > MAX_VOLUME).unwrap_or(false) {
                        c.simple(&format!("setvol {MAX_VOLUME}"))?;
                    }
                    Ok(())
                });
            }
        })
        .ok();
}

fn recordings_mtime(m: &Music) -> i64 {
    let d = m.app.recordings_dir();
    std::fs::metadata(&d)
        .and_then(|md| md.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Music {
    pub fn sources(&self) -> Result<Sources, String> {
        let rec = self.app.settings().recording.location.clone();
        let mut s = Sources::load_or_default(&self.sources_path, &rec)?;
        // The console's own stream list is always a source.
        if !s.source.iter().any(|x| x.kind == "urls") {
            s.source.extend(Sources::defaults(&rec).source.into_iter().filter(|x| x.kind == "urls"));
        }
        for x in s.source.iter_mut().filter(|x| x.id == "radio") {
            x.path = self.streams_path.to_string_lossy().to_string();
        }
        Ok(s)
    }

    /// Run `f` on the MPD connection, reconnecting once if the socket went away.
    fn with_mpd<T>(&self, mut f: impl FnMut(&mut Client) -> zpod_mpd::Result<T>) -> Result<T, String> {
        let mut g = self.mpd.lock().unwrap();
        for attempt in 0..2 {
            if g.is_none() {
                if std::env::var_os("MPD_HOST").is_none() {
                    return Err("player not configured (MPD_HOST unset; reinstall the console)".into());
                }
                let mut c = Client::connect_default().map_err(|e| format!("player (MPD) not reachable: {e}"))?;
                // First contact: never start above the cap; a fresh player starts at the default.
                if let Ok(st) = c.status() {
                    if st.volume.map(|v| v > MAX_VOLUME).unwrap_or(false) {
                        let _ = c.simple(&format!("setvol {DEFAULT_VOLUME}"));
                    }
                }
                let _ = c.simple("replay_gain_mode off");
                *g = Some(c);
            }
            match f(g.as_mut().unwrap()) {
                Ok(v) => return Ok(v),
                Err(zpod_mpd::Error::Ack(a)) => return Err(ack_text(&a)),
                Err(e) => {
                    *g = None;
                    if attempt == 1 {
                        return Err(format!("player: {e}"));
                    }
                }
            }
        }
        Err("player unavailable".into())
    }
}

fn ack_text(a: &str) -> String {
    // "ACK [50@0] {add} No such directory" -> "add: No such directory"
    let cmd = a.split('{').nth(1).and_then(|r| r.split('}').next()).unwrap_or("");
    let msg = a.rsplit('}').next().unwrap_or(a).trim();
    if cmd.is_empty() {
        msg.to_string()
    } else {
        format!("player {cmd}: {msg}")
    }
}

pub fn start_scan(m: &Arc<Music>) -> bool {
    {
        let mut s = m.scan.lock().unwrap();
        if s.running {
            return false;
        }
        s.running = true;
        s.started = pilab_music::now_unix();
        s.error.clear();
    }
    let m2 = m.clone();
    let spawned = std::thread::Builder::new().name("music-scan".into()).spawn(move || {
        // Background priority: the synth has CPU 3 to itself; this stays on 0-2 (inherited)
        // and yields to everything else there.
        unsafe {
            let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
            libc::setpriority(libc::PRIO_PROCESS, tid, 15);
        }
        let app = m2.app.clone();
        let ctl = Arc::new(ScanCtl { pause: Some(Box::new(move || app.recorder.is_recording())), ..Default::default() });
        m2.scan.lock().unwrap().ctl = Some(ctl.clone());
        let res = (|| -> Result<ScanReport, String> {
            let srcs = m2.sources()?;
            let ix = Index::open(&m2.db_path)?;
            pilab_music::scan(&ix, &srcs.source, &m2.art, &ctl)
        })();
        let mut s = m2.scan.lock().unwrap();
        s.running = false;
        s.finished = pilab_music::now_unix();
        s.ctl = None;
        match res {
            Ok(r) => {
                m2.app.event("info", format!("music scan: {} albums, {} songs, {} recordings, {} streams in {:.1} s", r.albums, r.tracks, r.takes, r.streams, r.seconds));
                s.last = Some(r);
            }
            Err(e) => {
                m2.app.event("warn", format!("music scan: {e}"));
                s.error = e;
            }
        }
    });
    if spawned.is_err() {
        m.scan.lock().unwrap().running = false;
        return false;
    }
    true
}

// ---- HTTP -------------------------------------------------------------------------------------

type R = Result<Json<Value>, (StatusCode, Json<Value>)>;
type S = State<Arc<App>>;

fn bad(e: impl ToString) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": e.to_string() })))
}

/// Run blocking work (SQLite, MPD socket) off the async workers.
async fn blk(f: impl FnOnce(&Arc<Music>) -> Result<Value, String> + Send + 'static) -> R {
    let m = svc().map_err(bad)?;
    match tokio::task::spawn_blocking(move || f(&m)).await {
        Ok(Ok(v)) => Ok(Json(v)),
        Ok(Err(e)) if e.starts_with("not found") => Err((StatusCode::NOT_FOUND, Json(json!({ "error": e })))),
        Ok(Err(e)) => Err(bad(e)),
        Err(e) => Err(bad(e)),
    }
}

fn js<T: serde::Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/music/status", get(status))
        .route("/music/sources", get(sources_get).put(sources_put))
        .route("/music/scan", post(scan))
        .route("/music/albums", get(albums))
        .route("/music/albums/{id}", get(album))
        .route("/music/artists", get(artists))
        .route("/music/artists/{id}", get(artist))
        .route("/music/tracks", get(tracks))
        .route("/music/search", get(search))
        .route("/music/takes", get(takes))
        .route("/music/streams", get(streams).post(stream_add))
        .route("/music/streams/{id}", delete(stream_del))
        .route("/music/art/{key}", get(art))
        .route("/music/playlists", get(playlists).post(playlist_new))
        .route("/music/playlists/{id}", get(playlist_get).put(playlist_put).delete(playlist_del))
        .route("/music/playlists/{id}/items", post(playlist_add))
        .route("/music/playlists/{id}/items/{pos}", delete(playlist_rm))
        .route("/music/playlists/{id}/move", post(playlist_move))
        .route("/music/play", post(play))
        .route("/music/queue", get(queue).delete(queue_clear))
        .route("/music/queue/move", post(queue_move))
        .route("/music/queue/{pos}", delete(queue_rm))
        .route("/music/queue/{pos}/play", post(queue_play))
        .route("/music/control", post(control))
        .route("/music/outputs", get(outputs_get).put(outputs_put))
        .route("/music/takes/{id}", delete(take_del))
        .route("/music/trash", get(trash_list))
        .route("/music/trash/empty", post(trash_empty))
        .route("/music/trash/{id}", delete(trash_purge))
        .route("/music/trash/{id}/restore", post(trash_restore))
}

/// MPD records: split a pair list at each "file" key.
fn records(p: &[(String, String)]) -> Vec<Vec<(String, String)>> {
    let mut out: Vec<Vec<(String, String)>> = vec![];
    for kv in p {
        if kv.0 == "file" || out.is_empty() {
            out.push(vec![]);
        }
        out.last_mut().unwrap().push(kv.clone());
    }
    out.into_iter().filter(|r| r.iter().any(|(k, _)| k == "file")).collect()
}

fn field<'a>(r: &'a [(String, String)], k: &str) -> Option<&'a str> {
    r.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
}

fn entry_json(m: &Music, r: &[(String, String)]) -> Value {
    let file = field(r, "file").unwrap_or("");
    let known = m.ix.lock().unwrap().lookup_uri(file).ok().flatten();
    let dur = field(r, "duration").or(field(r, "Time")).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let pos = field(r, "Pos").and_then(|v| v.parse::<i64>().ok()).unwrap_or(-1);
    let id = field(r, "Id").and_then(|v| v.parse::<i64>().ok()).unwrap_or(-1);
    let is_stream = file.starts_with("http://") || file.starts_with("https://");
    // Streams: "Name" is the station, "Title" is the live ICY title.
    let live = if is_stream { field(r, "Title").unwrap_or("").to_string() } else { String::new() };
    match known {
        Some(k) => json!({
            "pos": pos, "songid": id, "kind": k.kind, "ref": k.id, "title": k.title, "subtitle": k.subtitle,
            "album_id": k.album_id.to_string(), "art": k.art, "duration": if dur > 0.0 { dur } else { k.duration }, "live": live, "stream": is_stream
        }),
        None => {
            let title = field(r, "Name").or(field(r, "Title")).map(str::to_string).unwrap_or_else(|| file.rsplit('/').next().unwrap_or(file).to_string());
            json!({
                "pos": pos, "songid": id, "kind": if is_stream { "stream" } else { "file" }, "ref": "", "title": title,
                "subtitle": if is_stream { "Internet radio".to_string() } else { field(r, "Artist").unwrap_or("").to_string() },
                "album_id": "0", "art": "", "duration": dur, "live": live, "stream": is_stream
            })
        }
    }
}

fn player_json(m: &Music) -> Value {
    let res = m.with_mpd(|c| {
        let st = c.cmd("status")?;
        let cur = c.cmd("currentsong")?;
        Ok((st, cur, c.version.clone()))
    });
    match res {
        Ok((st, cur, version)) => {
            let s = zpod_mpd::Status::from_pairs(&st);
            let audio = field(&st, "audio").unwrap_or("").to_string(); // "44100:16:2" or "44100:f:2"
            let rate: u32 = audio.split(':').next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let current = records(&cur).first().map(|r| entry_json(m, r)).unwrap_or(Value::Null);
            let vol = s.volume.map(|v| v as i64).unwrap_or(-1);
            let path = if rate == 0 {
                String::new()
            } else if rate == GRAPH_RATE && vol == 100 {
                format!("{} → PipeWire {} kHz float (unchanged)", audio, GRAPH_RATE / 1000)
            } else if rate == GRAPH_RATE {
                format!("{} → PipeWire {} kHz float (player volume {vol}%)", audio, GRAPH_RATE / 1000)
            } else {
                format!("{} → resampled to {} kHz by the player (soxr), the synth's rate", audio, GRAPH_RATE / 1000)
            };
            json!({
                "connected": true, "version": version, "state": s.state, "volume": vol,
                "repeat": s.repeat, "random": s.random, "single": s.single,
                "pos": s.song.map(|v| v as i64).unwrap_or(-1), "length": s.playlist_len,
                "elapsed": s.elapsed, "duration": s.duration, "error": s.error,
                "bitrate": field(&st, "bitrate").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0),
                "audio": audio, "output_path": path, "current": current,
            })
        }
        Err(e) => json!({ "connected": false, "error": e, "state": "stop", "volume": -1 }),
    }
}

async fn status() -> R {
    blk(|m| {
        let stats = m.ix.lock().unwrap().stats()?;
        let sc = m.scan.lock().unwrap();
        let scan = json!({
            "running": sc.running, "started": sc.started, "finished": sc.finished, "error": sc.error,
            "files": sc.ctl.as_ref().map(|c| c.files_seen.load(Ordering::Relaxed)).unwrap_or(0),
            "phase": sc.ctl.as_ref().and_then(|c| c.phase.lock().ok().map(|p| p.clone())).unwrap_or_default(),
            "last": sc.last.as_ref().map(js),
        });
        drop(sc);
        Ok(json!({
            "library": js(stats), "scan": scan, "player": player_json(m),
            "outputs": crate::music_out::get().map(|o| o.json()).unwrap_or(Value::Null),
            "recovered": m.recovered.lock().unwrap().clone(),
            "volume_default": DEFAULT_VOLUME, "volume_max": MAX_VOLUME, "graph_rate": GRAPH_RATE,
        }))
    })
    .await
}

async fn sources_get() -> R {
    blk(|m| {
        let s = m.sources()?;
        let st = m.ix.lock().unwrap().sources_state()?;
        let list: Vec<Value> = s
            .source
            .iter()
            .map(|c| {
                let state = st.iter().find(|x| x["id"] == c.id.as_str()).cloned().unwrap_or(json!({}));
                json!({
                    "id": c.id, "kind": c.kind, "path": c.path, "collection": c.collection, "name": c.name,
                    "optional": c.optional, "enabled": c.enabled, "read_only": c.read_only,
                    "present": c.abs_path().exists(), "state": state["state"], "files": state["files"],
                    "last_scan": state["last_scan"], "error": state["error"],
                })
            })
            .collect();
        Ok(json!({ "file": m.sources_path.display().to_string(), "sources": list }))
    })
    .await
}

async fn sources_put(Json(body): Json<Sources>) -> R {
    blk(move |m| {
        let mut ids = std::collections::HashSet::new();
        for s in &body.source {
            s.validate()?;
            if !ids.insert(s.id.clone()) {
                return Err(format!("duplicate source id {}", s.id));
            }
        }
        crate::paths::write_atomic(&m.sources_path, body.to_toml().as_bytes()).map_err(|e| e.to_string())?;
        let started = start_scan(m);
        Ok(json!({ "ok": true, "scan": started }))
    })
    .await
}

async fn scan() -> R {
    blk(|m| Ok(json!({ "ok": true, "started": start_scan(m) }))).await
}

#[derive(Deserialize, Default)]
struct ListQ {
    sort: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
    q: Option<String>,
}

fn pid(s: &str) -> Result<i64, String> {
    s.parse().map_err(|_| format!("not found: bad id {s:?}"))
}

async fn albums(Query(q): Query<ListQ>) -> R {
    blk(move |m| Ok(json!({ "albums": m.ix.lock().unwrap().albums(q.sort.as_deref().unwrap_or("artist"))? }))).await
}

async fn album(Path(id): Path<String>) -> R {
    blk(move |m| match m.ix.lock().unwrap().album(pid(&id)?)? {
        Some((a, t)) => Ok(json!({ "album": a, "tracks": t })),
        None => Err("not found: no such album".into()),
    })
    .await
}

async fn artists() -> R {
    blk(|m| Ok(json!({ "artists": m.ix.lock().unwrap().artists()? }))).await
}

async fn artist(Path(id): Path<String>) -> R {
    blk(move |m| match m.ix.lock().unwrap().artist(pid(&id)?)? {
        Some((a, al, t)) => Ok(json!({ "artist": a, "albums": al, "tracks": t })),
        None => Err("not found: no such artist".into()),
    })
    .await
}

async fn tracks(Query(q): Query<ListQ>) -> R {
    blk(move |m| {
        let ix = m.ix.lock().unwrap();
        let total = ix.stats()?.tracks;
        Ok(json!({ "total": total, "tracks": ix.tracks(q.limit.unwrap_or(500).clamp(1, 5000), q.offset.unwrap_or(0).max(0))? }))
    })
    .await
}

async fn search(Query(q): Query<ListQ>) -> R {
    blk(move |m| Ok(js(m.ix.lock().unwrap().search(q.q.as_deref().unwrap_or(""), q.limit.unwrap_or(25).clamp(1, 200))?))).await
}

async fn takes() -> R {
    blk(|m| Ok(json!({ "location": m.app.recordings_dir().display().to_string(), "takes": m.ix.lock().unwrap().takes()? }))).await
}

async fn streams() -> R {
    blk(|m| Ok(json!({ "file": m.streams_path.display().to_string(), "streams": m.ix.lock().unwrap().streams()? }))).await
}

#[derive(Deserialize)]
struct StreamBody {
    name: String,
    url: String,
}

fn read_streams(m: &Music) -> Vec<pilab_music::m3u::Entry> {
    std::fs::read_to_string(&m.streams_path).map(|t| pilab_music::m3u::parse(&t)).unwrap_or_default()
}

/// Streams change rarely; rebuild synchronously so the list is current on return.
fn rescan_now(m: &Arc<Music>) -> Result<(), String> {
    let srcs = m.sources()?;
    let ix = m.ix.lock().unwrap();
    pilab_music::scan(&ix, &srcs.source, &m.art, &ScanCtl::default()).map(|_| ())
}

async fn stream_add(Json(b): Json<StreamBody>) -> R {
    blk(move |m| {
        let url = b.url.trim().to_string();
        if !pilab_music::m3u::valid_url(&url) {
            return Err("url must start with http:// or https://".into());
        }
        let name = b.name.trim();
        let name = if name.is_empty() { url.clone() } else { name.chars().take(120).collect() };
        let mut v = read_streams(m);
        if v.iter().any(|e| e.url == url) {
            return Err("that stream is already in the list".into());
        }
        v.push(pilab_music::m3u::Entry { name, url });
        crate::paths::write_atomic(&m.streams_path, pilab_music::m3u::render(&v).as_bytes()).map_err(|e| e.to_string())?;
        if m.scan.lock().unwrap().running {
            return Ok(json!({ "ok": true, "pending": true }));
        }
        rescan_now(m)?;
        Ok(json!({ "ok": true }))
    })
    .await
}

async fn stream_del(Path(id): Path<String>) -> R {
    blk(move |m| {
        let s = m.ix.lock().unwrap().stream(pid(&id)?)?.ok_or("not found: no such stream")?;
        let v: Vec<_> = read_streams(m).into_iter().filter(|e| e.url != s.url).collect();
        crate::paths::write_atomic(&m.streams_path, pilab_music::m3u::render(&v).as_bytes()).map_err(|e| e.to_string())?;
        rescan_now(m)?;
        Ok(json!({ "ok": true }))
    })
    .await
}

#[derive(Deserialize)]
struct ArtQ {
    size: Option<u32>,
}

async fn art(Path(key): Path<String>, Query(q): Query<ArtQ>) -> Response {
    let Ok(m) = svc() else { return StatusCode::SERVICE_UNAVAILABLE.into_response() };
    let Some(p) = m.art.best(&key, q.size.unwrap_or(300)) else { return StatusCode::NOT_FOUND.into_response() };
    match tokio::task::spawn_blocking(move || std::fs::read(&p)).await {
        Ok(Ok(b)) => ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "private, max-age=31536000, immutable")], b).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

async fn playlists() -> R {
    blk(|m| Ok(json!({ "playlists": m.ix.lock().unwrap().playlists()? }))).await
}

async fn playlist_new(Json(b): Json<NameBody>) -> R {
    blk(move |m| {
        let ix = m.ix.lock().unwrap();
        let id = ix.playlist_create(&b.name)?;
        Ok(json!({ "ok": true, "id": id.to_string(), "playlist": ix.playlist(id)? }))
    })
    .await
}

async fn playlist_get(Path(id): Path<String>) -> R {
    blk(move |m| {
        let ix = m.ix.lock().unwrap();
        let id = pid(&id)?;
        let p = ix.playlist(id)?.ok_or("not found: no such playlist")?;
        Ok(json!({ "playlist": p, "items": ix.playlist_items(id)? }))
    })
    .await
}

async fn playlist_put(Path(id): Path<String>, Json(b): Json<NameBody>) -> R {
    blk(move |m| {
        if !m.ix.lock().unwrap().playlist_rename(pid(&id)?, &b.name)? {
            return Err("not found: no such playlist".into());
        }
        Ok(json!({ "ok": true }))
    })
    .await
}

async fn playlist_del(Path(id): Path<String>) -> R {
    blk(move |m| Ok(json!({ "ok": m.ix.lock().unwrap().playlist_delete(pid(&id)?)? }))).await
}

#[derive(Deserialize)]
struct ItemRef {
    kind: String,
    id: Value,
    #[serde(default)]
    title: String,
}
#[derive(Deserialize)]
struct ItemsBody {
    items: Vec<ItemRef>,
}

fn id_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

async fn playlist_add(Path(id): Path<String>, Json(b): Json<ItemsBody>) -> R {
    blk(move |m| {
        let ix = m.ix.lock().unwrap();
        let mut items = vec![];
        for it in &b.items {
            let rf = id_str(&it.id);
            if it.kind == "album" {
                let (_, tr) = ix.album(pid(&rf)?)?.ok_or("not found: no such album")?;
                items.extend(tr.into_iter().map(|t| ("track".to_string(), t.id.to_string(), String::new())));
            } else {
                items.push((it.kind.clone(), rf, it.title.clone()));
            }
        }
        let n = ix.playlist_add(pid(&id)?, &items)?;
        Ok(json!({ "ok": true, "added": n }))
    })
    .await
}

async fn playlist_rm(Path((id, pos)): Path<(String, i64)>) -> R {
    blk(move |m| Ok(json!({ "ok": m.ix.lock().unwrap().playlist_remove(pid(&id)?, pos)? }))).await
}

#[derive(Deserialize)]
struct MoveBody {
    from: i64,
    to: i64,
}

async fn playlist_move(Path(id): Path<String>, Json(b): Json<MoveBody>) -> R {
    blk(move |m| Ok(json!({ "ok": m.ix.lock().unwrap().playlist_move(pid(&id)?, b.from, b.to)? }))).await
}

#[derive(Deserialize)]
struct PlayBody {
    kind: String,
    #[serde(default)]
    id: Value,
    #[serde(default)]
    ids: Vec<Value>,
    #[serde(default)]
    start: usize,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    shuffle: bool,
    #[serde(default)]
    title: String,
}

fn playables(m: &Music, b: &PlayBody) -> Result<Vec<pilab_music::index::Playable>, String> {
    let ix = m.ix.lock().unwrap();
    let rf = id_str(&b.id);
    let v = match b.kind.as_str() {
        "album" => ix.album_playables(pid(&rf)?)?,
        "artist" => ix.artist_playables(pid(&rf)?)?,
        "playlist" => ix.playlist_playables(pid(&rf)?)?,
        "track" => ix.track_playable(pid(&rf)?)?.into_iter().collect(),
        "tracks" => {
            let mut v = vec![];
            for i in &b.ids {
                if let Some(p) = ix.track_playable(pid(&id_str(i))?)? {
                    v.push(p);
                }
            }
            v
        }
        "take" => ix.take_playable(&rf)?.into_iter().collect(),
        "takes" => {
            let mut v = vec![];
            for i in &b.ids {
                if let Some(p) = ix.take_playable(&id_str(i))? {
                    v.push(p);
                }
            }
            v
        }
        "stream" => ix.stream_playable(pid(&rf)?)?.into_iter().collect(),
        "url" => {
            if !pilab_music::m3u::valid_url(&rf) {
                return Err("url must start with http:// or https://".into());
            }
            vec![pilab_music::index::Playable { kind: "stream".into(), title: b.title.clone(), uri: rf.clone(), ..Default::default() }]
        }
        k => return Err(format!("unknown kind {k:?}")),
    };
    if v.is_empty() {
        return Err("not found: nothing to play".into());
    }
    Ok(v)
}

/// Volume before sound: never start a play above the cap.
fn guard_volume(c: &mut Client) -> zpod_mpd::Result<()> {
    let st = c.status()?;
    match st.volume {
        Some(v) if v <= MAX_VOLUME => Ok(()),
        _ => c.simple(&format!("setvol {DEFAULT_VOLUME}")).or(Ok(())),
    }
}

async fn play(Json(b): Json<PlayBody>) -> R {
    blk(move |m| {
        let items = playables(m, &b)?;
        let uris: Vec<String> = items.iter().map(|p| p.uri.clone()).collect();
        let start = b.start.min(uris.len().saturating_sub(1));
        let mode = if b.mode.is_empty() { "replace" } else { b.mode.as_str() };
        m.with_mpd(|c| {
            guard_volume(c)?;
            match mode {
                "append" => c.add_uris(&uris),
                "next" => {
                    let st = c.status()?;
                    if st.song.is_none() {
                        return c.add_uris(&uris);
                    }
                    let mut batch = String::from("command_list_begin\n");
                    for (i, u) in uris.iter().enumerate() {
                        batch.push_str(&format!("add {} +{i}\n", zpod_mpd::quote(u)));
                    }
                    batch.push_str("command_list_end");
                    c.simple(&batch)
                }
                _ => {
                    c.simple(&format!("random {}", if b.shuffle { 1 } else { 0 }))?;
                    c.play_uris(&uris, if b.shuffle { 0 } else { start })?;
                    guard_volume(c)
                }
            }
        })?;
        let rf = if b.kind == "tracks" || b.kind == "takes" { format!("{} items", b.ids.len()) } else { id_str(&b.id) };
        let _ = m.ix.lock().unwrap().log_play(&b.kind, &rf);
        Ok(json!({ "ok": true, "queued": uris.len(), "mode": mode }))
    })
    .await
}

async fn queue() -> R {
    blk(|m| {
        let pairs = m.with_mpd(|c| c.cmd("playlistinfo"))?;
        let items: Vec<Value> = records(&pairs).iter().map(|r| entry_json(m, r)).collect();
        Ok(json!({ "queue": items }))
    })
    .await
}

async fn queue_clear() -> R {
    blk(|m| m.with_mpd(|c| c.simple("clear")).map(|_| json!({ "ok": true }))).await
}

async fn queue_rm(Path(pos): Path<usize>) -> R {
    blk(move |m| m.with_mpd(|c| c.simple(&format!("delete {pos}"))).map(|_| json!({ "ok": true }))).await
}

async fn queue_play(Path(pos): Path<usize>) -> R {
    blk(move |m| {
        m.with_mpd(|c| {
            guard_volume(c)?;
            c.simple(&format!("play {pos}"))
        })
        .map(|_| json!({ "ok": true }))
    })
    .await
}

#[derive(Deserialize)]
struct MoveQ {
    from: usize,
    to: usize,
}

async fn queue_move(Json(b): Json<MoveQ>) -> R {
    blk(move |m| m.with_mpd(|c| c.simple(&format!("move {} {}", b.from, b.to))).map(|_| json!({ "ok": true }))).await
}

#[derive(Deserialize)]
struct ControlBody {
    action: String,
    #[serde(default)]
    value: Option<f64>,
}

async fn control(State(_app): S, Json(b): Json<ControlBody>) -> R {
    blk(move |m| {
        let a = b.action.as_str();
        let mut capped = false;
        m.with_mpd(|c| match a {
            "toggle" => {
                let st = c.status()?;
                guard_volume(c)?;
                match st.state.as_str() {
                    "play" => c.simple("pause 1"),
                    "pause" => c.simple("pause 0"),
                    _ => c.simple("play"),
                }
            }
            "play" => {
                guard_volume(c)?;
                c.simple("play")
            }
            "pause" => c.simple("pause 1"),
            "stop" => c.simple("stop"),
            "next" => c.simple("next"),
            "prev" => c.simple("previous"),
            "seek" => c.simple(&format!("seekcur {:.2}", b.value.unwrap_or(0.0).max(0.0))),
            "volume" => {
                let want = b.value.unwrap_or(DEFAULT_VOLUME as f64).round().clamp(0.0, 100.0) as u8;
                capped = want > MAX_VOLUME;
                c.simple(&format!("setvol {}", want.min(MAX_VOLUME)))
            }
            "repeat" => c.simple(&format!("repeat {}", (b.value.unwrap_or(0.0) != 0.0) as u8)),
            "random" => c.simple(&format!("random {}", (b.value.unwrap_or(0.0) != 0.0) as u8)),
            _ => Err(zpod_mpd::Error::Ack(format!("ACK [2@0] {{{a}}} unknown action"))),
        })?;
        Ok(json!({ "ok": true, "capped": capped, "volume_max": MAX_VOLUME, "player": player_json(m) }))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mpd_records_and_ack_text() {
        let p: Vec<(String, String)> = [("file", "/a.flac"), ("Pos", "0"), ("Id", "5"), ("file", "https://x/y"), ("Name", "Radio"), ("Pos", "1")]
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        let r = records(&p);
        assert_eq!(r.len(), 2);
        assert_eq!(field(&r[1], "Name"), Some("Radio"));
        assert_eq!(ack_text("ACK [50@0] {add} No such directory"), "player add: No such directory");
        assert!(DEFAULT_VOLUME < MAX_VOLUME && MAX_VOLUME < 100);
    }
}

// ---- player watchdog --------------------------------------------------------------------------

/// MPD says "play" but the position has not moved: the PipeWire stream is stuck (seen on the
/// Pi after a long idle: the stream errored every cycle and nothing played). First reopen the
/// output; if that does not help, restart the player process and resume. At most one restart
/// every two minutes.
fn spawn_watchdog(m: Arc<Music>) {
    std::thread::Builder::new()
        .name("music-dog".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(20));
            let mut last = (String::new(), -1.0f32, 0usize);
            let mut stuck = 0u32;
            let mut last_restart = std::time::Instant::now() - std::time::Duration::from_secs(600);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let Ok(st) = m.with_mpd(|c| c.status()) else {
                    stuck = 0;
                    continue;
                };
                let key = (st.state.clone(), st.elapsed, st.song.unwrap_or(usize::MAX));
                if st.state == "play" && key == last {
                    stuck += 2;
                } else {
                    stuck = 0;
                }
                last = key;
                if stuck == 8 {
                    let _ = m.with_mpd(|c| {
                        c.simple("disableoutput 0")?;
                        c.simple("enableoutput 0")
                    });
                    m.app.event("warn", "music: playback was stuck; reopened the player output");
                    *m.recovered.lock().unwrap() = json!({ "at": pilab_music::now_unix(), "what": "reopened the output" });
                } else if stuck >= 20 && last_restart.elapsed() > std::time::Duration::from_secs(120) {
                    last_restart = std::time::Instant::now();
                    stuck = 0;
                    let pos = st.song;
                    let at = st.elapsed;
                    let ok = std::process::Command::new("systemctl").args(["--user", "restart", "pilab-mpd.service"]).status().map(|s| s.success()).unwrap_or(false);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    *m.mpd.lock().unwrap() = None;
                    if let Some(p) = pos {
                        let _ = m.with_mpd(|c| c.simple(&format!("seek {p} {at:.1}")));
                    }
                    let _ = m.with_mpd(|c| c.simple("play"));
                    m.app.event("warn", format!("music: playback was still stuck; restarted the player{}", if ok { "" } else { " (restart failed)" }));
                    *m.recovered.lock().unwrap() = json!({ "at": pilab_music::now_unix(), "what": "restarted the player" });
                }
            }
        })
        .ok();
}

// ---- outputs ----------------------------------------------------------------------------------

async fn outputs_get() -> R {
    let o = crate::music_out::get().ok_or_else(|| bad("outputs not started"))?;
    tokio::task::spawn_blocking(move || {
        o.apply();
        Json(o.json())
    })
    .await
    .map_err(bad)
    .map(Ok)?
}

#[derive(Deserialize)]
struct OutBody {
    #[serde(default)]
    outputs: Vec<String>,
}

async fn outputs_put(Json(b): Json<OutBody>) -> R {
    let o = crate::music_out::get().ok_or_else(|| bad("outputs not started"))?;
    match tokio::task::spawn_blocking(move || o.set(b.outputs)).await {
        Ok(Ok(v)) => Ok(Json(v)),
        Ok(Err(e)) => Err(bad(e)),
        Err(e) => Err(bad(e)),
    }
}

// ---- recordings trash -------------------------------------------------------------------------

/// Folders of the Recordings collection (the only places a delete may touch).
fn recording_roots(m: &Music) -> Result<Vec<(String, PathBuf)>, String> {
    Ok(m.sources()?.source.into_iter().filter(|s| s.collection == "recordings" && s.kind == "folder").map(|s| (s.id.clone(), pilab_music::expand(&s.path))).collect())
}

async fn take_del(Path(id): Path<String>) -> R {
    blk(move |m| {
        if m.app.recorder.is_recording() {
            return Err("a recording is running; stop it first".into());
        }
        let take = m.ix.lock().unwrap().take(&id)?.ok_or_else(|| "not found: no such recording".to_string())?;
        let (file, source) = m.ix.lock().unwrap().take_file(&id)?.ok_or_else(|| "not found: no such recording".to_string())?;
        let roots = recording_roots(m)?;
        let Some((_, root)) = roots.iter().find(|(sid, _)| *sid == source) else {
            return Err("only recordings can be deleted here (published music is never deleted)".into());
        };
        // Stop the player first if it is playing this take.
        let fu = pilab_music::index::file_uri(&file);
        let _ = m.with_mpd(|c| {
            let cur = c.cmd("currentsong")?;
            if field(&cur, "file") == Some(fu.as_str()) {
                c.simple("stop")?;
            }
            Ok(())
        });
        let e = crate::music_trash::trash(std::path::Path::new(&file), root, &take.id, &take.title, pilab_music::now_unix())?;
        m.app.event("info", format!("recording \"{}\" moved to the trash ({})", take.title, e.id));
        start_scan(m);
        Ok(json!({ "ok": true, "trash": e }))
    })
    .await
}

async fn trash_list() -> R {
    blk(|m| {
        let roots: Vec<PathBuf> = recording_roots(m)?.into_iter().map(|x| x.1).collect();
        let items = crate::music_trash::list(&roots);
        let bytes: u64 = items.iter().map(|e| e.bytes).sum();
        Ok(json!({ "items": items, "bytes": bytes }))
    })
    .await
}

async fn trash_restore(Path(id): Path<String>) -> R {
    blk(move |m| {
        let roots: Vec<PathBuf> = recording_roots(m)?.into_iter().map(|x| x.1).collect();
        let e = crate::music_trash::restore(&roots, &id)?;
        m.app.event("info", format!("recording \"{}\" restored from the trash", e.title));
        start_scan(m);
        Ok(json!({ "ok": true, "restored": e }))
    })
    .await
}

#[derive(Deserialize, Default)]
struct ConfirmBody {
    #[serde(default)]
    confirm: String,
}

fn purge_common(m: &Music, id: Option<String>, confirm: &str) -> Result<Value, String> {
    if confirm != crate::music_trash::PURGE_CONFIRM {
        return Err(format!("permanent delete needs {{\"confirm\": \"{}\"}}", crate::music_trash::PURGE_CONFIRM));
    }
    let roots: Vec<PathBuf> = recording_roots(m)?.into_iter().map(|x| x.1).collect();
    let (n, bytes) = crate::music_trash::purge(&roots, id.as_deref())?;
    m.app.event("info", format!("recordings trash: {n} item(s) deleted for good ({:.1} MB)", bytes as f64 / 1e6));
    Ok(json!({ "ok": true, "deleted": n, "bytes": bytes }))
}

async fn trash_purge(Path(id): Path<String>, body: Option<Json<ConfirmBody>>) -> R {
    let c = body.map(|b| b.0.confirm).unwrap_or_default();
    blk(move |m| purge_common(m, Some(id), &c)).await
}

async fn trash_empty(body: Option<Json<ConfirmBody>>) -> R {
    let c = body.map(|b| b.0.confirm).unwrap_or_default();
    blk(move |m| purge_common(m, None, &c)).await
}
