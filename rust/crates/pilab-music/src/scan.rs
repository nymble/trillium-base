// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! The scanner: walk folder sources, read tags for new or changed files only (size + mtime),
//! drop vanished files, then rebuild releases, artists, takes, streams, art and search.
//! Files are only read. Runs on a low-priority thread in pilabd; `ScanCtl::pause` lets the
//! caller hold the scan (for example while a recording is running).

use crate::art::{self, ArtCache};
use crate::index::Index;
use crate::sources::SourceCfg;
use crate::tags::{self, Meta};
use crate::{sort_key, stable_id};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Default)]
pub struct ScanCtl {
    pub pause: Option<Box<dyn Fn() -> bool + Send + Sync>>,
    pub cancel: AtomicBool,
    pub files_seen: AtomicUsize,
    pub phase: Mutex<String>,
}

impl ScanCtl {
    fn wait(&self) {
        if let Some(p) = &self.pause {
            while p() && !self.cancel.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }
    fn set_phase(&self, s: &str) {
        if let Ok(mut p) = self.phase.lock() {
            *p = s.to_string();
        }
    }
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct SourceReport {
    pub id: String,
    pub state: String, // ok | offline | error | disabled
    pub error: String,
    pub files: usize,
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
    pub unreadable: usize,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct ScanReport {
    pub sources: Vec<SourceReport>,
    pub albums: i64,
    pub tracks: i64,
    pub takes: i64,
    pub streams: i64,
    pub art_made: usize,
    pub seconds: f64,
}

const MAX_DEPTH: usize = 12;
const BATCH: usize = 100;

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>, ctl: &ScanCtl) {
    if depth > MAX_DEPTH || ctl.cancel.load(Ordering::Relaxed) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut ents: Vec<_> = rd.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    for e in ents {
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n.starts_with('.') || n.starts_with("._") {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() {
            walk(&p, depth + 1, out, ctl);
        } else if (ft.is_file() || ft.is_symlink()) && tags::is_audio(&p) {
            out.push(p);
        }
    }
}

fn mtime_of(md: &std::fs::Metadata) -> i64 {
    md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Scan every source and rebuild the derived tables.
pub fn scan(ix: &Index, sources: &[SourceCfg], cache: &ArtCache, ctl: &ScanCtl) -> Result<ScanReport, String> {
    let t0 = std::time::Instant::now();
    let mut rep = ScanReport::default();
    let db = &ix.db;
    let es = |x: rusqlite::Error| x.to_string();

    // Forget sources that were removed from the config.
    let ids: HashSet<&str> = sources.iter().map(|s| s.id.as_str()).collect();
    let known: Vec<String> = {
        let mut st = db.prepare("SELECT id FROM source").map_err(es)?;
        let v = st.query_map([], |r| r.get(0)).map_err(es)?.flatten().collect();
        v
    };
    for k in known.iter().filter(|k| !ids.contains(k.as_str())) {
        db.execute("DELETE FROM holding WHERE source_id = ?1", [k]).map_err(es)?;
        db.execute("DELETE FROM source WHERE id = ?1", [k]).map_err(es)?;
    }

    for src in sources {
        let mut sr = SourceReport { id: src.id.clone(), ..Default::default() };
        db.execute(
            "INSERT INTO source(id, kind, path, collection, read_only, name) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET kind = ?2, path = ?3, collection = ?4, read_only = ?5, name = ?6",
            params![src.id, src.kind, src.path, src.collection, src.read_only as i64, src.name],
        )
        .map_err(es)?;
        let root = src.abs_path();
        if !src.enabled {
            sr.state = "disabled".into();
        } else if src.kind == "urls" {
            sr.state = if root.exists() { "ok".into() } else { "offline".into() };
            sr.files = std::fs::read_to_string(&root).map(|t| crate::m3u::parse(&t).len()).unwrap_or(0);
        } else if !root.is_dir() {
            // Offline drive: keep what we knew, so the library survives an unplugged SSD
            // until the next scan with it attached. Holdings stay; they are marked offline.
            sr.state = if src.optional { "offline".into() } else { "error".into() };
            if !src.optional {
                sr.error = format!("{} is not a folder", root.display());
            }
        } else {
            ctl.set_phase(&format!("scanning {}", src.id));
            let mut files = vec![];
            walk(&root, 0, &mut files, ctl);
            sr.files = files.len();
            let prev: HashMap<String, (i64, i64, i64)> = {
                let mut st = db.prepare("SELECT path, size, mtime, id FROM holding WHERE source_id = ?1").map_err(es)?;
                let v = st
                    .query_map([&src.id], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))
                    .map_err(es)?
                    .flatten()
                    .collect();
                v
            };
            let mut seen = HashSet::new();
            for chunk in files.chunks(BATCH) {
                ctl.wait();
                if ctl.cancel.load(Ordering::Relaxed) {
                    return Err("scan cancelled".into());
                }
                // Read tags outside the write transaction.
                let mut work = vec![];
                for p in chunk {
                    let ps = p.to_string_lossy().to_string();
                    seen.insert(ps.clone());
                    ctl.files_seen.fetch_add(1, Ordering::Relaxed);
                    let Ok(md) = std::fs::metadata(p) else { continue };
                    let (size, mtime) = (md.len() as i64, mtime_of(&md));
                    match prev.get(&ps) {
                        Some((s, m, _)) if *s == size && *m == mtime => continue,
                        Some(_) => sr.changed += 1,
                        None => sr.added += 1,
                    }
                    let (mut meta, err) = match tags::read(p) {
                        Ok(m) => (m, String::new()),
                        Err(x) => (Meta::default(), x),
                    };
                    if !err.is_empty() {
                        sr.unreadable += 1;
                    }
                    tags::apply_fallbacks(&mut meta, p);
                    work.push((ps, size, mtime, meta, err));
                }
                if work.is_empty() {
                    continue;
                }
                let tx = db.unchecked_transaction().map_err(es)?;
                for (ps, size, mtime, m, err) in work {
                    let js = serde_json::to_string(&m).unwrap_or_else(|_| "{}".into());
                    tx.execute(
                        "INSERT INTO holding(source_id, collection, path, size, mtime, codec, sample_rate, bit_depth, channels, bitrate, duration, meta, error)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                         ON CONFLICT(path) DO UPDATE SET source_id = ?1, collection = ?2, size = ?4, mtime = ?5, codec = ?6, sample_rate = ?7,
                           bit_depth = ?8, channels = ?9, bitrate = ?10, duration = ?11, meta = ?12, error = ?13, sha256 = NULL",
                        params![src.id, src.collection, ps, size, mtime, m.codec, m.sample_rate, m.bit_depth, m.channels, m.bitrate, m.duration, js, err],
                    )
                    .map_err(es)?;
                }
                tx.commit().map_err(es)?;
            }
            let gone: Vec<i64> = prev.iter().filter(|(p, _)| !seen.contains(*p)).map(|(_, v)| v.2).collect();
            sr.removed = gone.len();
            let tx = db.unchecked_transaction().map_err(es)?;
            for id in gone {
                tx.execute("DELETE FROM holding WHERE id = ?1", [id]).map_err(es)?;
            }
            tx.execute("UPDATE holding SET collection = ?2 WHERE source_id = ?1", params![src.id, src.collection]).map_err(es)?;
            tx.commit().map_err(es)?;
            sr.state = "ok".into();
        }
        db.execute(
            "UPDATE source SET state = ?2, error = ?3, last_scan = ?4, files = ?5 WHERE id = ?1",
            params![src.id, sr.state, sr.error, crate::now_unix(), sr.files as i64],
        )
        .map_err(es)?;
        rep.sources.push(sr);
    }

    ctl.set_phase("building library");
    rep.art_made = rebuild(ix, sources, cache, ctl)?;
    let st = ix.stats()?;
    rep.albums = st.albums;
    rep.tracks = st.tracks;
    rep.takes = st.takes;
    rep.streams = st.streams;
    rep.seconds = t0.elapsed().as_secs_f64();
    ctl.set_phase("idle");
    Ok(rep)
}

struct H {
    id: i64,
    source_id: String,
    collection: String,
    path: String,
    size: i64,
    mtime: i64,
    meta: Meta,
}

#[derive(Default)]
struct Rel {
    title: String,
    artists: Vec<String>,
    album_artist: String,
    mbid: String,
    rg_mbid: String,
    mb_artist: String,
    dir: String,
    source_id: String,
    collection: String,
    tracks: Vec<usize>,
}

fn art_key(parts: &[&str]) -> String {
    format!("{:016x}", stable_id(parts) as u64)
}

/// The take card written by the recorder next to each take.
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct Card {
    id: String,
    created: String,
    created_unix: i64,
    duration_s: f64,
    source: String,
    scene: String,
    role: String,
    filed: bool,
    rights: String,
}

fn rebuild(ix: &Index, sources: &[SourceCfg], cache: &ArtCache, ctl: &ScanCtl) -> Result<usize, String> {
    let db = &ix.db;
    let es = |x: rusqlite::Error| x.to_string();
    let hs: Vec<H> = {
        let mut st = db.prepare("SELECT id, source_id, collection, path, size, mtime, meta FROM holding ORDER BY path").map_err(es)?;
        let v = st
            .query_map([], |r| {
                let js: String = r.get(6)?;
                Ok(H {
                    id: r.get(0)?,
                    source_id: r.get(1)?,
                    collection: r.get(2)?,
                    path: r.get(3)?,
                    size: r.get(4)?,
                    mtime: r.get(5)?,
                    meta: serde_json::from_str(&js).unwrap_or_default(),
                })
            })
            .map_err(es)?
            .collect::<Result<_, _>>()
            .map_err(es)?;
        v
    };
    let added: HashMap<i64, i64> = {
        let mut st = db.prepare("SELECT id, added FROM release").map_err(es)?;
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(es)?.flatten().collect();
        v
    };

    // Group music holdings into releases.
    let mut rels: BTreeMap<String, Rel> = BTreeMap::new();
    let mut takes = vec![];
    for (i, h) in hs.iter().enumerate() {
        if h.collection == "recordings" {
            takes.push(i);
            continue;
        }
        let m = &h.meta;
        let dir = Path::new(&h.path).parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        let key = if !m.mb_release.is_empty() {
            format!("mb:{}", m.mb_release)
        } else if !m.album_artist.is_empty() {
            format!("aa:{}\u{1f}{}", m.album.to_lowercase(), m.album_artist.to_lowercase())
        } else {
            format!("dir:{}\u{1f}{}", m.album.to_lowercase(), dir)
        };
        let r = rels.entry(key).or_insert_with(|| Rel {
            title: m.album.clone(),
            dir: dir.clone(),
            source_id: h.source_id.clone(),
            collection: h.collection.clone(),
            ..Default::default()
        });
        if r.album_artist.is_empty() && !m.album_artist.is_empty() {
            r.album_artist = m.album_artist.clone();
        }
        if r.mbid.is_empty() {
            r.mbid = m.mb_release.clone();
            r.rg_mbid = m.mb_release_group.clone();
        }
        if r.mb_artist.is_empty() {
            r.mb_artist = if m.mb_album_artist.is_empty() { m.mb_artist.clone() } else { m.mb_album_artist.clone() };
        }
        r.artists.push(if m.artist.is_empty() { m.album_artist.clone() } else { m.artist.clone() });
        r.tracks.push(i);
    }

    // Artists, by case-folded name.
    let mut artists: HashMap<i64, (String, String)> = HashMap::new();
    let artist_id = |name: &str, mbid: &str, artists: &mut HashMap<i64, (String, String)>| -> i64 {
        let id = stable_id(&["artist", &name.to_lowercase()]);
        let e = artists.entry(id).or_insert_with(|| (name.to_string(), String::new()));
        if e.1.is_empty() && !mbid.is_empty() {
            e.1 = mbid.to_string();
        }
        id
    };

    let mut art_made = 0;
    struct RelRow {
        id: i64,
        r: Rel,
        artist_id: i64,
        year: i64,
        genre: String,
        art: String,
        art_source: String,
        duration: f64,
        format: String,
    }
    let mut rows = vec![];
    for (_, r) in rels {
        ctl.wait();
        let uniq: HashSet<&String> = r.artists.iter().collect();
        let rel_artist = if !r.album_artist.is_empty() {
            r.album_artist.clone()
        } else if uniq.len() == 1 {
            r.artists[0].clone()
        } else {
            "Various Artists".into()
        };
        let aid = artist_id(&rel_artist, &r.mb_artist, &mut artists);
        let id = if r.mbid.is_empty() {
            stable_id(&["release", &r.collection, &r.title.to_lowercase(), &rel_artist.to_lowercase()])
        } else {
            stable_id(&["release-mb", &r.mbid])
        };
        let year = r.tracks.iter().map(|&i| hs[i].meta.year as i64).filter(|y| *y > 0).min().unwrap_or(0);
        let mut gc: HashMap<&str, usize> = HashMap::new();
        for &i in &r.tracks {
            if !hs[i].meta.genre.is_empty() {
                *gc.entry(hs[i].meta.genre.as_str()).or_default() += 1;
            }
        }
        let genre = gc.into_iter().max_by_key(|(g, n)| (*n, std::cmp::Reverse(*g))).map(|(g, _)| g.to_string()).unwrap_or_default();
        let duration: f64 = r.tracks.iter().map(|&i| hs[i].meta.duration).sum();
        let format = release_format(r.tracks.iter().map(|&i| &hs[i].meta));
        // Art: folder image first, then the first embedded picture.
        let (mut art, mut art_source) = (String::new(), String::new());
        if let Some(img) = art::folder_image(Path::new(&r.dir)) {
            if let Ok(md) = std::fs::metadata(&img) {
                let k = art_key(&[&img.to_string_lossy(), &md.len().to_string(), &mtime_of(&md).to_string()]);
                let ok = cache.has(&k) || {
                    let made = std::fs::read(&img).map(|b| cache.ensure(&k, &b)).unwrap_or(false);
                    art_made += made as usize;
                    made
                };
                if ok {
                    art = k;
                    art_source = "folder".into();
                }
            }
        }
        if art.is_empty() {
            if let Some(&i) = r.tracks.iter().find(|&&i| hs[i].meta.has_picture) {
                let h = &hs[i];
                let k = art_key(&[&h.path, &h.size.to_string(), &h.mtime.to_string(), "embedded"]);
                let ok = cache.has(&k) || {
                    let made = tags::embedded_picture(Path::new(&h.path)).map(|b| cache.ensure(&k, &b)).unwrap_or(false);
                    art_made += made as usize;
                    made
                };
                if ok {
                    art = k;
                    art_source = "embedded".into();
                }
            }
        }
        rows.push(RelRow { id, r, artist_id: aid, year, genre, art, art_source, duration, format });
    }

    // Track artists.
    let mut track_artist = HashMap::new();
    for row in &rows {
        for &i in &row.r.tracks {
            let m = &hs[i].meta;
            let n = if m.artist.is_empty() { m.album_artist.clone() } else { m.artist.clone() };
            let a = artist_id(&n, &m.mb_artist, &mut artists);
            track_artist.insert(i, a);
        }
    }

    // Streams from URL sources.
    let mut streams = vec![];
    for s in sources.iter().filter(|s| s.enabled && s.kind == "urls") {
        if let Ok(t) = std::fs::read_to_string(s.abs_path()) {
            for (pos, en) in crate::m3u::parse(&t).into_iter().enumerate() {
                streams.push((stable_id(&["stream", &en.url]), s.id.clone(), en.name, en.url, pos as i64));
            }
        }
    }

    // Write everything in one short transaction.
    let tx = db.unchecked_transaction().map_err(es)?;
    tx.execute_batch("DELETE FROM track; DELETE FROM release; DELETE FROM artist; DELETE FROM take; DELETE FROM stream; DELETE FROM fts;").map_err(es)?;
    for (id, (name, mbid)) in &artists {
        tx.execute("INSERT INTO artist(id, name, sort_name, mbid) VALUES (?1, ?2, ?3, ?4)", params![id, name, sort_key(name), mbid]).map_err(es)?;
    }
    let now = crate::now_unix();
    for row in &rows {
        let artist_name = &artists[&row.artist_id].0;
        tx.execute(
            "INSERT INTO release(id, collection, title, sort_title, artist_id, year, genre, mbid, rg_mbid, art, art_source, dir, source_id, tracks, duration, added, format)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![row.id, row.r.collection, row.r.title, sort_key(&row.r.title), row.artist_id, row.year, row.genre, row.r.mbid, row.r.rg_mbid,
                row.art, row.art_source, row.r.dir, row.r.source_id, row.r.tracks.len() as i64, row.duration, added.get(&row.id).copied().unwrap_or(now), row.format],
        )
        .map_err(es)?;
        tx.execute("INSERT INTO fts(kind, ref, title, artist, album, extra) VALUES ('album', ?1, ?2, ?3, '', ?4)", params![row.id.to_string(), row.r.title, artist_name, row.genre]).map_err(es)?;
        for &i in &row.r.tracks {
            let h = &hs[i];
            let m = &h.meta;
            let ta = track_artist[&i];
            tx.execute(
                "INSERT INTO track(id, release_id, artist_id, title, disc, no, duration, genre, year, mb_recording, mb_track, isrc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![h.id, row.id, ta, m.title, m.disc, m.track, m.duration, m.genre, m.year, m.mb_recording, m.mb_track, m.isrc],
            )
            .map_err(es)?;
            tx.execute("INSERT INTO fts(kind, ref, title, artist, album, extra) VALUES ('track', ?1, ?2, ?3, ?4, '')", params![h.id.to_string(), m.title, artists[&ta].0, row.r.title]).map_err(es)?;
        }
    }
    let release_artists: HashSet<i64> = rows.iter().map(|r| r.artist_id).collect();
    for id in &release_artists {
        tx.execute("INSERT INTO fts(kind, ref, title, artist, album, extra) VALUES ('artist', ?1, '', ?2, '', '')", params![id.to_string(), artists[id].0]).map_err(es)?;
    }
    for &i in &takes {
        let h = &hs[i];
        let p = Path::new(&h.path);
        let card: Card = p
            .parent()
            .map(|d| d.join("record.toml"))
            .and_then(|c| std::fs::read_to_string(c).ok())
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default();
        let id = if !card.id.is_empty() {
            card.id.clone()
        } else {
            p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| h.id.to_string())
        };
        let exists: Option<i64> = tx.query_row("SELECT 1 FROM take WHERE id = ?1", [&id], |r| r.get(0)).optional().map_err(es)?;
        let id = if exists.is_some() { format!("{id}-{}", h.id) } else { id };
        let (session, title) = take_titles(&id, &card.created, &h.meta, p);
        let dur = if card.duration_s > 0.0 { card.duration_s } else { h.meta.duration };
        tx.execute(
            "INSERT INTO take(id, holding_id, title, session, created, created_unix, duration, scene, source, role, filed, rights)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![id, h.id, title, session, card.created, card.created_unix, dur, card.scene, card.source, card.role, card.filed as i64, card.rights],
        )
        .map_err(es)?;
        tx.execute("INSERT INTO fts(kind, ref, title, artist, album, extra) VALUES ('take', ?1, ?2, '', ?3, ?4)", params![id, title, session, format!("{} {}", card.scene, card.source)]).map_err(es)?;
    }
    for (id, sid, name, url, pos) in &streams {
        tx.execute("INSERT OR IGNORE INTO stream(id, source_id, name, url, pos) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, sid, name, url, pos]).map_err(es)?;
        tx.execute("INSERT INTO fts(kind, ref, title, artist, album, extra) VALUES ('stream', ?1, ?2, '', '', ?3)", params![id.to_string(), name, url]).map_err(es)?;
    }
    tx.commit().map_err(es)?;
    Ok(art_made)
}

/// "pilab-rec-20261124-213005" → session "2026-11-24", title "Take 21:30:05".
fn take_titles(id: &str, created: &str, meta: &Meta, path: &Path) -> (String, String) {
    let digits: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    if !meta.title.is_empty() && !meta.title.starts_with("pilab-rec-") && meta.title != id {
        let session = if created.len() >= 10 { created[..10].to_string() } else { "Recordings".into() };
        return (session, meta.title.clone());
    }
    if digits.len() >= 14 {
        let d = &digits[digits.len() - 14..];
        return (format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8]), format!("Take {}:{}:{}", &d[8..10], &d[10..12], &d[12..14]));
    }
    let session = if created.len() >= 10 { created[..10].to_string() } else { "Recordings".into() };
    (session, path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())
}

/// "FLAC 24/96", "MP3", "Vorbis 96 kHz", "Mixed".
fn release_format<'a>(ms: impl Iterator<Item = &'a Meta>) -> String {
    let v: Vec<&Meta> = ms.collect();
    let Some(f) = v.first() else { return String::new() };
    if v.iter().any(|m| m.codec != f.codec) {
        return "Mixed".into();
    }
    let sr = v.iter().map(|m| m.sample_rate).max().unwrap_or(0);
    let bd = v.iter().map(|m| m.bit_depth).max().unwrap_or(0);
    let khz = |sr: u32| if sr % 1000 == 0 { format!("{}", sr / 1000) } else { format!("{:.1}", sr as f64 / 1000.0) };
    match f.codec.as_str() {
        "FLAC" | "WAV" | "AIFF" | "WavPack" | "APE" | "AAC/ALAC" if bd > 0 && sr > 0 => format!("{} {}/{}", f.codec, bd, khz(sr)),
        c if sr > 48000 => format!("{c} {} kHz", khz(sr)),
        c => c.to_string(),
    }
}
