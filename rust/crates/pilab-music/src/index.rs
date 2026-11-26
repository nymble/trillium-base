// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! The SQLite index and its read API (albums, artists, songs, search, takes, streams,
//! playlists). One connection per thread; WAL lets the console read during a scan.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::path::Path;

pub struct Index {
    pub db: Connection,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub artist_id: i64,
    pub year: i64,
    pub genre: String,
    pub tracks: i64,
    pub duration: f64,
    pub art: String,
    pub format: String,
    pub collection: String,
    pub source_id: String,
    pub mbid: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Track {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub artist_id: i64,
    pub album: String,
    pub album_id: i64,
    pub disc: i64,
    pub no: i64,
    pub duration: f64,
    pub art: String,
    pub codec: String,
    pub sample_rate: i64,
    pub bit_depth: i64,
    pub mb_recording: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub albums: i64,
    pub tracks: i64,
    pub art: String,
    pub mbid: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Take {
    pub id: String,
    pub title: String,
    pub session: String,
    pub created: String,
    pub created_unix: i64,
    pub duration: f64,
    pub scene: String,
    pub source: String,
    pub role: String,
    pub filed: bool,
    pub rights: String,
    pub sample_rate: i64,
    pub channels: i64,
    pub codec: String,
    pub source_id: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Stream {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub source_id: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub count: i64,
    pub duration: f64,
    pub art: String,
    pub updated: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PlaylistItem {
    pub pos: i64,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub duration: f64,
    pub art: String,
    pub track_id: Option<i64>,
    pub take_id: Option<String>,
    pub url: Option<String>,
    pub missing: bool,
}

/// Something that can be queued: what the queue shows for a URI MPD reports back.
#[derive(Clone, Debug, Serialize, PartialEq, Default)]
pub struct Playable {
    pub kind: String, // track | take | stream
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub album_id: i64,
    pub art: String,
    pub duration: f64,
    pub uri: String,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Stats {
    pub albums: i64,
    pub artists: i64,
    pub tracks: i64,
    pub takes: i64,
    pub streams: i64,
    pub playlists: i64,
    pub holdings: i64,
    pub errors: i64,
    pub duration: f64,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct SearchResult {
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    pub tracks: Vec<Track>,
    pub takes: Vec<Take>,
    pub streams: Vec<Stream>,
}

const ALBUM_SQL: &str = "SELECT r.id, r.title, a.name, r.artist_id, r.year, r.genre, r.tracks, r.duration, r.art, r.format, r.collection, r.source_id, r.mbid FROM release r JOIN artist a ON a.id = r.artist_id";
const TRACK_SQL: &str = "SELECT t.id, t.title, a.name, t.artist_id, r.title, t.release_id, t.disc, t.no, t.duration, r.art, h.codec, h.sample_rate, h.bit_depth, t.mb_recording FROM track t JOIN artist a ON a.id = t.artist_id JOIN release r ON r.id = t.release_id JOIN holding h ON h.id = t.id";
const TAKE_SQL: &str = "SELECT k.id, k.title, k.session, k.created, k.created_unix, k.duration, k.scene, k.source, k.role, k.filed, k.rights, h.sample_rate, h.channels, h.codec, h.source_id FROM take k JOIN holding h ON h.id = k.holding_id";
const ARTIST_SQL: &str = "SELECT a.id, a.name, (SELECT COUNT(*) FROM release r WHERE r.artist_id = a.id), (SELECT COUNT(*) FROM track t WHERE t.artist_id = a.id), COALESCE((SELECT r.art FROM release r WHERE r.artist_id = a.id AND r.art != '' ORDER BY r.year, r.sort_title LIMIT 1), ''), a.mbid FROM artist a";

fn album_row(r: &Row) -> rusqlite::Result<Album> {
    Ok(Album {
        id: r.get(0)?,
        title: r.get(1)?,
        artist: r.get(2)?,
        artist_id: r.get(3)?,
        year: r.get(4)?,
        genre: r.get(5)?,
        tracks: r.get(6)?,
        duration: r.get(7)?,
        art: r.get(8)?,
        format: r.get(9)?,
        collection: r.get(10)?,
        source_id: r.get(11)?,
        mbid: r.get(12)?,
    })
}
fn track_row(r: &Row) -> rusqlite::Result<Track> {
    Ok(Track {
        id: r.get(0)?,
        title: r.get(1)?,
        artist: r.get(2)?,
        artist_id: r.get(3)?,
        album: r.get(4)?,
        album_id: r.get(5)?,
        disc: r.get(6)?,
        no: r.get(7)?,
        duration: r.get(8)?,
        art: r.get(9)?,
        codec: r.get(10)?,
        sample_rate: r.get(11)?,
        bit_depth: r.get(12)?,
        mb_recording: r.get(13)?,
    })
}
fn take_row(r: &Row) -> rusqlite::Result<Take> {
    Ok(Take {
        id: r.get(0)?,
        title: r.get(1)?,
        session: r.get(2)?,
        created: r.get(3)?,
        created_unix: r.get(4)?,
        duration: r.get(5)?,
        scene: r.get(6)?,
        source: r.get(7)?,
        role: r.get(8)?,
        filed: r.get::<_, i64>(9)? != 0,
        rights: r.get(10)?,
        sample_rate: r.get(11)?,
        channels: r.get(12)?,
        codec: r.get(13)?,
        source_id: r.get(14)?,
    })
}
fn artist_row(r: &Row) -> rusqlite::Result<Artist> {
    Ok(Artist { id: r.get(0)?, name: r.get(1)?, albums: r.get(2)?, tracks: r.get(3)?, art: r.get(4)?, mbid: r.get(5)? })
}
fn stream_row(r: &Row) -> rusqlite::Result<Stream> {
    Ok(Stream { id: r.get(0)?, name: r.get(1)?, url: r.get(2)?, source_id: r.get(3)? })
}

type R<T> = Result<T, String>;
fn e(x: rusqlite::Error) -> String {
    x.to_string()
}

/// FTS5 prefix query from free text: every word must match the start of a token.
pub fn fts_query(q: &str) -> Option<String> {
    let toks: Vec<String> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .take(8)
        .map(|t| format!("\"{}\"*", t.to_lowercase()))
        .collect();
    if toks.is_empty() {
        None
    } else {
        Some(toks.join(" "))
    }
}

impl Index {
    pub fn open(path: &Path) -> R<Index> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|x| x.to_string())?;
        }
        let db = Connection::open(path).map_err(e)?;
        Self::init(db)
    }

    pub fn open_memory() -> R<Index> {
        Self::init(Connection::open_in_memory().map_err(e)?)
    }

    fn init(db: Connection) -> R<Index> {
        db.busy_timeout(std::time::Duration::from_secs(5)).map_err(e)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=OFF;").map_err(e)?;
        db.execute_batch(crate::schema::SQL).map_err(e)?;
        db.pragma_update(None, "user_version", crate::schema::VERSION).map_err(e)?;
        Ok(Index { db })
    }

    pub fn tables(&self) -> R<Vec<String>> {
        let mut st = self.db.prepare("SELECT name FROM sqlite_master WHERE type IN ('table') ORDER BY name").map_err(e)?;
        let v = st.query_map([], |r| r.get(0)).map_err(e)?.flatten().collect();
        Ok(v)
    }

    pub fn stats(&self) -> R<Stats> {
        let c = |sql: &str| -> R<i64> { self.db.query_row(sql, [], |r| r.get(0)).map_err(e) };
        Ok(Stats {
            albums: c("SELECT COUNT(*) FROM release")?,
            artists: c("SELECT COUNT(DISTINCT artist_id) FROM release")?,
            tracks: c("SELECT COUNT(*) FROM track")?,
            takes: c("SELECT COUNT(*) FROM take")?,
            streams: c("SELECT COUNT(*) FROM stream")?,
            playlists: c("SELECT COUNT(*) FROM playlist")?,
            holdings: c("SELECT COUNT(*) FROM holding")?,
            errors: c("SELECT COUNT(*) FROM holding WHERE error != ''")?,
            duration: self.db.query_row("SELECT COALESCE(SUM(duration),0) FROM track", [], |r| r.get(0)).map_err(e)?,
        })
    }

    pub fn albums(&self, sort: &str) -> R<Vec<Album>> {
        let order = match sort {
            "title" => "r.sort_title, r.year",
            "year" => "r.year DESC, r.sort_title",
            "added" => "r.added DESC, r.sort_title",
            _ => "a.sort_name, r.year, r.sort_title",
        };
        let mut st = self.db.prepare(&format!("{ALBUM_SQL} ORDER BY {order}")).map_err(e)?;
        let v = st.query_map([], album_row).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        Ok(v)
    }

    pub fn album(&self, id: i64) -> R<Option<(Album, Vec<Track>)>> {
        let a = self.db.query_row(&format!("{ALBUM_SQL} WHERE r.id = ?1"), [id], album_row).optional().map_err(e)?;
        let Some(a) = a else { return Ok(None) };
        let mut st = self.db.prepare(&format!("{TRACK_SQL} WHERE t.release_id = ?1 ORDER BY t.disc, t.no, t.title")).map_err(e)?;
        let t = st.query_map([id], track_row).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        Ok(Some((a, t)))
    }

    /// Album artists (Apple Music style: an artist appears once they have an album).
    pub fn artists(&self) -> R<Vec<Artist>> {
        let mut st = self
            .db
            .prepare(&format!("{ARTIST_SQL} WHERE EXISTS (SELECT 1 FROM release r WHERE r.artist_id = a.id) ORDER BY a.sort_name"))
            .map_err(e)?;
        let v = st.query_map([], artist_row).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        Ok(v)
    }

    pub fn artist(&self, id: i64) -> R<Option<(Artist, Vec<Album>, Vec<Track>)>> {
        let a = self.db.query_row(&format!("{ARTIST_SQL} WHERE a.id = ?1"), [id], artist_row).optional().map_err(e)?;
        let Some(a) = a else { return Ok(None) };
        let mut st = self.db.prepare(&format!("{ALBUM_SQL} WHERE r.artist_id = ?1 ORDER BY r.year, r.sort_title")).map_err(e)?;
        let albums = st.query_map([id], album_row).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        let mut st = self
            .db
            .prepare(&format!("{TRACK_SQL} WHERE t.artist_id = ?1 AND r.artist_id != ?1 ORDER BY r.sort_title, t.disc, t.no"))
            .map_err(e)?;
        let tracks = st.query_map([id], track_row).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        Ok(Some((a, albums, tracks)))
    }

    pub fn tracks(&self, limit: i64, offset: i64) -> R<Vec<Track>> {
        let mut st = self
            .db
            .prepare(&format!("{TRACK_SQL} ORDER BY t.title COLLATE NOCASE, a.sort_name LIMIT ?1 OFFSET ?2"))
            .map_err(e)?;
        let v = st.query_map(params![limit, offset], track_row).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        Ok(v)
    }

    pub fn track(&self, id: i64) -> R<Option<Track>> {
        self.db.query_row(&format!("{TRACK_SQL} WHERE t.id = ?1"), [id], track_row).optional().map_err(e)
    }

    pub fn takes(&self) -> R<Vec<Take>> {
        let mut st = self.db.prepare(&format!("{TAKE_SQL} ORDER BY k.created_unix DESC, k.id DESC")).map_err(e)?;
        let v = st.query_map([], take_row).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        Ok(v)
    }

    /// A take's file and the source it was found in (for moving it to the trash).
    pub fn take_file(&self, id: &str) -> R<Option<(String, String)>> {
        self.db
            .query_row("SELECT h.path, h.source_id FROM take k JOIN holding h ON h.id = k.holding_id WHERE k.id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(e)
    }

    pub fn take(&self, id: &str) -> R<Option<Take>> {
        self.db.query_row(&format!("{TAKE_SQL} WHERE k.id = ?1"), [id], take_row).optional().map_err(e)
    }

    pub fn streams(&self) -> R<Vec<Stream>> {
        let mut st = self.db.prepare("SELECT id, name, url, source_id FROM stream ORDER BY source_id, pos").map_err(e)?;
        let v = st.query_map([], stream_row).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        Ok(v)
    }

    pub fn stream(&self, id: i64) -> R<Option<Stream>> {
        self.db.query_row("SELECT id, name, url, source_id FROM stream WHERE id = ?1", [id], stream_row).optional().map_err(e)
    }

    pub fn search(&self, q: &str, limit: i64) -> R<SearchResult> {
        let mut out = SearchResult::default();
        let Some(fq) = fts_query(q) else { return Ok(out) };
        let mut st = self
            .db
            .prepare("SELECT kind, ref FROM fts WHERE fts MATCH ?1 ORDER BY bm25(fts, 0, 0, 4.0, 2.0, 1.0, 0.5) LIMIT 400")
            .map_err(e)?;
        let hits: Vec<(String, String)> = st.query_map([fq], |r| Ok((r.get(0)?, r.get(1)?))).map_err(e)?.flatten().collect();
        for (kind, rf) in hits {
            match kind.as_str() {
                "album" if (out.albums.len() as i64) < limit => {
                    if let Some((a, _)) = self.album(rf.parse().unwrap_or(0))? {
                        out.albums.push(a);
                    }
                }
                "artist" if (out.artists.len() as i64) < limit => {
                    if let Some((a, _, _)) = self.artist(rf.parse().unwrap_or(0))? {
                        out.artists.push(a);
                    }
                }
                "track" if (out.tracks.len() as i64) < limit => {
                    if let Some(t) = self.track(rf.parse().unwrap_or(0))? {
                        out.tracks.push(t);
                    }
                }
                "take" if (out.takes.len() as i64) < limit => {
                    if let Some(t) = self.take(&rf)? {
                        out.takes.push(t);
                    }
                }
                "stream" if (out.streams.len() as i64) < limit => {
                    if let Some(s) = self.stream(rf.parse().unwrap_or(0))? {
                        out.streams.push(s);
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    // ---- playable items (URIs handed to MPD) ------------------------------------------------

    pub fn path_of_holding(&self, id: i64) -> R<Option<String>> {
        self.db.query_row("SELECT path FROM holding WHERE id = ?1", [id], |r| r.get(0)).optional().map_err(e)
    }

    pub fn track_playable(&self, id: i64) -> R<Option<Playable>> {
        let Some(t) = self.track(id)? else { return Ok(None) };
        let Some(p) = self.path_of_holding(id)? else { return Ok(None) };
        Ok(Some(Playable {
            kind: "track".into(),
            id: t.id.to_string(),
            title: t.title,
            subtitle: format!("{} — {}", t.artist, t.album),
            album_id: t.album_id,
            art: t.art,
            duration: t.duration,
            uri: file_uri(&p),
        }))
    }

    pub fn take_playable(&self, id: &str) -> R<Option<Playable>> {
        let row: Option<(String, i64)> = self
            .db
            .query_row("SELECT k.id, k.holding_id FROM take k WHERE k.id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(e)?;
        let Some((_, hid)) = row else { return Ok(None) };
        let Some(t) = self.take(id)? else { return Ok(None) };
        let Some(p) = self.path_of_holding(hid)? else { return Ok(None) };
        Ok(Some(Playable {
            kind: "take".into(),
            id: t.id.clone(),
            title: t.title,
            subtitle: format!("Recording · {}", t.session),
            album_id: 0,
            art: String::new(),
            duration: t.duration,
            uri: file_uri(&p),
        }))
    }

    pub fn stream_playable(&self, id: i64) -> R<Option<Playable>> {
        Ok(self.stream(id)?.map(|s| Playable {
            kind: "stream".into(),
            id: s.id.to_string(),
            title: s.name,
            subtitle: "Internet radio".into(),
            uri: s.url,
            ..Default::default()
        }))
    }

    pub fn album_playables(&self, id: i64) -> R<Vec<Playable>> {
        let Some((_, tracks)) = self.album(id)? else { return Ok(vec![]) };
        let mut v = vec![];
        for t in tracks {
            if let Some(p) = self.track_playable(t.id)? {
                v.push(p);
            }
        }
        Ok(v)
    }

    pub fn artist_playables(&self, id: i64) -> R<Vec<Playable>> {
        let Some((_, albums, tracks)) = self.artist(id)? else { return Ok(vec![]) };
        let mut v = vec![];
        for a in albums {
            v.extend(self.album_playables(a.id)?);
        }
        for t in tracks {
            if let Some(p) = self.track_playable(t.id)? {
                v.push(p);
            }
        }
        Ok(v)
    }

    pub fn playlist_playables(&self, id: i64) -> R<Vec<Playable>> {
        let mut v = vec![];
        for it in self.playlist_items(id)? {
            let p = match (&it.track_id, &it.take_id, &it.url) {
                (Some(t), _, _) => self.track_playable(*t)?,
                (_, Some(k), _) => self.take_playable(k)?,
                (_, _, Some(u)) => Some(Playable { kind: "stream".into(), title: it.title.clone(), uri: u.clone(), ..Default::default() }),
                _ => None,
            };
            if let Some(p) = p {
                v.push(p);
            }
        }
        Ok(v)
    }

    /// What the queue shows for a URI MPD reports (file:// path or stream URL).
    pub fn lookup_uri(&self, uri: &str) -> R<Option<Playable>> {
        if let Some(path) = path_from_uri(uri) {
            let row: Option<(i64, String)> = self
                .db
                .query_row("SELECT id, collection FROM holding WHERE path = ?1", [&path], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()
                .map_err(e)?;
            if let Some((hid, coll)) = row {
                if coll == "recordings" {
                    let tid: Option<String> =
                        self.db.query_row("SELECT id FROM take WHERE holding_id = ?1", [hid], |r| r.get(0)).optional().map_err(e)?;
                    if let Some(t) = tid {
                        return self.take_playable(&t);
                    }
                } else {
                    return self.track_playable(hid);
                }
            }
            return Ok(None);
        }
        let s: Option<Stream> = self
            .db
            .query_row("SELECT id, name, url, source_id FROM stream WHERE url = ?1 LIMIT 1", [uri], stream_row)
            .optional()
            .map_err(e)?;
        Ok(s.map(|s| Playable { kind: "stream".into(), id: s.id.to_string(), title: s.name, subtitle: "Internet radio".into(), uri: s.url, ..Default::default() }))
    }

    pub fn log_play(&self, kind: &str, rf: &str) -> R<()> {
        self.db.execute("INSERT INTO play_event(at, kind, ref) VALUES (?1, ?2, ?3)", params![crate::now_unix(), kind, rf]).map_err(e)?;
        Ok(())
    }

    // ---- playlists ----------------------------------------------------------------------------

    pub fn playlists(&self) -> R<Vec<Playlist>> {
        let mut st = self
            .db
            .prepare(
                "SELECT p.id, p.name, (SELECT COUNT(*) FROM playlist_item i WHERE i.playlist_id = p.id),
                        COALESCE((SELECT SUM(t.duration) FROM playlist_item i JOIN track t ON t.id = i.track_id WHERE i.playlist_id = p.id), 0),
                        COALESCE((SELECT r.art FROM playlist_item i JOIN track t ON t.id = i.track_id JOIN release r ON r.id = t.release_id
                                  WHERE i.playlist_id = p.id AND r.art != '' ORDER BY i.pos LIMIT 1), ''),
                        p.updated
                 FROM playlist p ORDER BY p.name COLLATE NOCASE",
            )
            .map_err(e)?;
        let v = st
            .query_map([], |r| Ok(Playlist { id: r.get(0)?, name: r.get(1)?, count: r.get(2)?, duration: r.get(3)?, art: r.get(4)?, updated: r.get(5)? }))
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?;
        Ok(v)
    }

    pub fn playlist(&self, id: i64) -> R<Option<Playlist>> {
        Ok(self.playlists()?.into_iter().find(|p| p.id == id))
    }

    pub fn playlist_create(&self, name: &str) -> R<i64> {
        let n = clean_name(name)?;
        let now = crate::now_unix();
        self.db.execute("INSERT INTO playlist(name, created, updated) VALUES (?1, ?2, ?2)", params![n, now]).map_err(e)?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn playlist_rename(&self, id: i64, name: &str) -> R<bool> {
        let n = clean_name(name)?;
        Ok(self.db.execute("UPDATE playlist SET name = ?2, updated = ?3 WHERE id = ?1", params![id, n, crate::now_unix()]).map_err(e)? > 0)
    }

    pub fn playlist_delete(&self, id: i64) -> R<bool> {
        self.db.execute("DELETE FROM playlist_item WHERE playlist_id = ?1", [id]).map_err(e)?;
        Ok(self.db.execute("DELETE FROM playlist WHERE id = ?1", [id]).map_err(e)? > 0)
    }

    pub fn playlist_items(&self, id: i64) -> R<Vec<PlaylistItem>> {
        let mut st = self
            .db
            .prepare("SELECT pos, track_id, take_id, url, title FROM playlist_item WHERE playlist_id = ?1 ORDER BY pos")
            .map_err(e)?;
        let raw: Vec<(i64, Option<i64>, Option<String>, Option<String>, String)> = st
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?;
        let mut out = vec![];
        for (pos, tr, tk, url, title) in raw {
            let mut it = PlaylistItem { pos, kind: String::new(), title: title.clone(), subtitle: String::new(), duration: 0.0, art: String::new(), track_id: tr, take_id: tk.clone(), url: url.clone(), missing: false };
            if let Some(t) = tr {
                it.kind = "track".into();
                match self.track(t)? {
                    Some(t) => {
                        it.title = t.title;
                        it.subtitle = format!("{} — {}", t.artist, t.album);
                        it.duration = t.duration;
                        it.art = t.art;
                    }
                    None => it.missing = true,
                }
            } else if let Some(k) = tk {
                it.kind = "take".into();
                match self.take(&k)? {
                    Some(t) => {
                        it.title = t.title;
                        it.subtitle = format!("Recording · {}", t.session);
                        it.duration = t.duration;
                    }
                    None => it.missing = true,
                }
            } else {
                it.kind = "stream".into();
                it.subtitle = url.clone().unwrap_or_default();
            }
            out.push(it);
        }
        Ok(out)
    }

    /// Append items: `("track", "12")`, `("take", "pilab-rec-…")`, `("url", "https://…")`.
    pub fn playlist_add(&self, id: i64, items: &[(String, String, String)]) -> R<usize> {
        if self.playlist(id)?.is_none() {
            return Err("no such playlist".into());
        }
        let mut pos: i64 = self.db.query_row("SELECT COALESCE(MAX(pos)+1, 0) FROM playlist_item WHERE playlist_id = ?1", [id], |r| r.get(0)).map_err(e)?;
        let mut n = 0;
        for (kind, rf, title) in items {
            match kind.as_str() {
                "track" => {
                    let t: i64 = rf.parse().map_err(|_| "bad track id")?;
                    if self.track(t)?.is_none() {
                        return Err(format!("no track {t}"));
                    }
                    self.db.execute("INSERT INTO playlist_item(playlist_id, pos, track_id) VALUES (?1, ?2, ?3)", params![id, pos, t]).map_err(e)?;
                }
                "take" => {
                    if self.take(rf)?.is_none() {
                        return Err(format!("no take {rf}"));
                    }
                    self.db.execute("INSERT INTO playlist_item(playlist_id, pos, take_id) VALUES (?1, ?2, ?3)", params![id, pos, rf]).map_err(e)?;
                }
                "url" => {
                    if !crate::m3u::valid_url(rf) {
                        return Err("url must be http(s)".into());
                    }
                    self.db.execute("INSERT INTO playlist_item(playlist_id, pos, url, title) VALUES (?1, ?2, ?3, ?4)", params![id, pos, rf, title]).map_err(e)?;
                }
                _ => return Err(format!("unknown item kind {kind}")),
            }
            pos += 1;
            n += 1;
        }
        self.touch(id)?;
        Ok(n)
    }

    pub fn playlist_remove(&self, id: i64, pos: i64) -> R<bool> {
        let n = self.db.execute("DELETE FROM playlist_item WHERE playlist_id = ?1 AND pos = ?2", params![id, pos]).map_err(e)?;
        self.renumber(id)?;
        self.touch(id)?;
        Ok(n > 0)
    }

    pub fn playlist_move(&self, id: i64, from: i64, to: i64) -> R<bool> {
        let items = self.playlist_items(id)?;
        if from < 0 || to < 0 || from as usize >= items.len() || to as usize >= items.len() {
            return Ok(false);
        }
        let tx = self.db.unchecked_transaction().map_err(e)?;
        tx.execute("UPDATE playlist_item SET pos = -1 WHERE playlist_id = ?1 AND pos = ?2", params![id, from]).map_err(e)?;
        if from < to {
            tx.execute("UPDATE playlist_item SET pos = pos - 1 WHERE playlist_id = ?1 AND pos > ?2 AND pos <= ?3", params![id, from, to]).map_err(e)?;
        } else {
            tx.execute("UPDATE playlist_item SET pos = pos + 1 WHERE playlist_id = ?1 AND pos >= ?3 AND pos < ?2", params![id, from, to]).map_err(e)?;
        }
        tx.execute("UPDATE playlist_item SET pos = ?2 WHERE playlist_id = ?1 AND pos = -1", params![id, to]).map_err(e)?;
        tx.commit().map_err(e)?;
        self.touch(id)?;
        Ok(true)
    }

    fn renumber(&self, id: i64) -> R<()> {
        let tx = self.db.unchecked_transaction().map_err(e)?;
        let rows: Vec<i64> = {
            let mut st = tx.prepare("SELECT rowid FROM playlist_item WHERE playlist_id = ?1 ORDER BY pos").map_err(e)?;
            let v = st.query_map([id], |r| r.get(0)).map_err(e)?.flatten().collect();
            v
        };
        for (i, rid) in rows.iter().enumerate() {
            tx.execute("UPDATE playlist_item SET pos = ?1 WHERE rowid = ?2", params![i as i64, rid]).map_err(e)?;
        }
        tx.commit().map_err(e)
    }

    fn touch(&self, id: i64) -> R<()> {
        self.db.execute("UPDATE playlist SET updated = ?2 WHERE id = ?1", params![id, crate::now_unix()]).map_err(e)?;
        Ok(())
    }

    pub fn sources_state(&self) -> R<Vec<serde_json::Value>> {
        let mut st = self.db.prepare("SELECT id, state, error, last_scan, files FROM source ORDER BY id").map_err(e)?;
        let v = st
            .query_map([], |r| {
                Ok(serde_json::json!({"id": r.get::<_, String>(0)?, "state": r.get::<_, String>(1)?, "error": r.get::<_, String>(2)?,
                    "last_scan": r.get::<_, i64>(3)?, "files": r.get::<_, i64>(4)?}))
            })
            .map_err(e)?
            .flatten()
            .collect();
        Ok(v)
    }
}

fn clean_name(n: &str) -> R<String> {
    let n = n.trim();
    if n.is_empty() || n.chars().count() > 120 {
        return Err("playlist name must be 1-120 characters".into());
    }
    Ok(n.replace(['\n', '\r', '\t'], " "))
}

/// `file://` + the absolute path, as MPD expects it from a local (socket) client: MPD does
/// not percent-decode file URIs, so the path goes in verbatim (the protocol layer quotes it).
pub fn file_uri(path: &str) -> String {
    format!("file://{path}")
}

/// MPD reports local files back as plain absolute paths (or the file:// URI we sent).
pub fn path_from_uri(uri: &str) -> Option<String> {
    if uri.starts_with('/') {
        return Some(uri.to_string());
    }
    uri.strip_prefix("file://").filter(|p| p.starts_with('/')).map(str::to_string)
}
