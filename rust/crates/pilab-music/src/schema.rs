// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! SQLite schema (v1). Holdings are files; tracks, releases, artists, takes and the search
//! table are derived from them on every scan. Track ids are holding ids, so playlists keep
//! pointing at the same file across rescans. Release, artist and stream ids are stable hashes.

pub const VERSION: i32 = 1;

pub const SQL: &str = r#"
CREATE TABLE IF NOT EXISTS source (
  id TEXT PRIMARY KEY, kind TEXT NOT NULL, path TEXT NOT NULL, collection TEXT NOT NULL,
  read_only INTEGER NOT NULL DEFAULT 1, name TEXT NOT NULL DEFAULT '',
  state TEXT NOT NULL DEFAULT 'new', error TEXT NOT NULL DEFAULT '',
  last_scan INTEGER NOT NULL DEFAULT 0, files INTEGER NOT NULL DEFAULT 0
);
-- One row per audio file. meta is the tag read (JSON), reused while size and mtime match.
CREATE TABLE IF NOT EXISTS holding (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source_id TEXT NOT NULL, collection TEXT NOT NULL, path TEXT NOT NULL UNIQUE,
  size INTEGER NOT NULL, mtime INTEGER NOT NULL,
  sha256 TEXT,                      -- filled by the future duplicate pass
  fingerprint TEXT,                 -- acoustic fingerprint, future (Chromaprint as a separate process)
  codec TEXT NOT NULL DEFAULT '', sample_rate INTEGER NOT NULL DEFAULT 0,
  bit_depth INTEGER NOT NULL DEFAULT 0, channels INTEGER NOT NULL DEFAULT 0,
  bitrate INTEGER NOT NULL DEFAULT 0, duration REAL NOT NULL DEFAULT 0,
  meta TEXT NOT NULL DEFAULT '{}', error TEXT NOT NULL DEFAULT '', seen INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS holding_source ON holding(source_id);
CREATE TABLE IF NOT EXISTS artist (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL, sort_name TEXT NOT NULL, mbid TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS release (
  id INTEGER PRIMARY KEY, collection TEXT NOT NULL, title TEXT NOT NULL, sort_title TEXT NOT NULL,
  artist_id INTEGER NOT NULL, year INTEGER NOT NULL DEFAULT 0, genre TEXT NOT NULL DEFAULT '',
  mbid TEXT NOT NULL DEFAULT '', rg_mbid TEXT NOT NULL DEFAULT '', art TEXT NOT NULL DEFAULT '',
  art_source TEXT NOT NULL DEFAULT '', dir TEXT NOT NULL DEFAULT '', source_id TEXT NOT NULL DEFAULT '',
  tracks INTEGER NOT NULL DEFAULT 0, duration REAL NOT NULL DEFAULT 0, added INTEGER NOT NULL DEFAULT 0,
  format TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS track (
  id INTEGER PRIMARY KEY,           -- = holding.id
  release_id INTEGER NOT NULL, artist_id INTEGER NOT NULL, title TEXT NOT NULL,
  disc INTEGER NOT NULL DEFAULT 0, no INTEGER NOT NULL DEFAULT 0, duration REAL NOT NULL DEFAULT 0,
  genre TEXT NOT NULL DEFAULT '', year INTEGER NOT NULL DEFAULT 0,
  mb_recording TEXT NOT NULL DEFAULT '', mb_track TEXT NOT NULL DEFAULT '', isrc TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS track_release ON track(release_id);
CREATE INDEX IF NOT EXISTS track_artist ON track(artist_id);
-- Takes recorded on this unit (Recordings collection). Never mixed with releases.
CREATE TABLE IF NOT EXISTS take (
  id TEXT PRIMARY KEY, holding_id INTEGER NOT NULL, title TEXT NOT NULL, session TEXT NOT NULL,
  created TEXT NOT NULL DEFAULT '', created_unix INTEGER NOT NULL DEFAULT 0, duration REAL NOT NULL DEFAULT 0,
  scene TEXT NOT NULL DEFAULT '', source TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '',
  filed INTEGER NOT NULL DEFAULT 0, rights TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS stream (
  id INTEGER PRIMARY KEY, source_id TEXT NOT NULL, name TEXT NOT NULL, url TEXT NOT NULL, pos INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS playlist (
  id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, created INTEGER NOT NULL,
  updated INTEGER NOT NULL, rules TEXT NOT NULL DEFAULT ''   -- smart playlist rules (JSON), future
);
CREATE TABLE IF NOT EXISTS playlist_item (
  playlist_id INTEGER NOT NULL, pos INTEGER NOT NULL,
  track_id INTEGER, take_id TEXT, url TEXT, title TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS playlist_item_pl ON playlist_item(playlist_id, pos);
CREATE TABLE IF NOT EXISTS play_event (
  id INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER NOT NULL, kind TEXT NOT NULL, ref TEXT NOT NULL
);
-- Future library management (empty in v1; see the library plan).
CREATE TABLE IF NOT EXISTS override (
  id INTEGER PRIMARY KEY AUTOINCREMENT, entity TEXT NOT NULL, entity_id TEXT NOT NULL,
  field TEXT NOT NULL, value TEXT NOT NULL, origin TEXT NOT NULL DEFAULT 'console',
  batch TEXT NOT NULL DEFAULT '', at INTEGER NOT NULL, undone INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS merge (
  id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, from_id INTEGER NOT NULL,
  into_id INTEGER NOT NULL, at INTEGER NOT NULL, undone INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS dup_candidate (
  id INTEGER PRIMARY KEY AUTOINCREMENT, a INTEGER NOT NULL, b INTEGER NOT NULL,
  signals TEXT NOT NULL DEFAULT '', score REAL NOT NULL DEFAULT 0, state TEXT NOT NULL DEFAULT 'open'
);
CREATE TABLE IF NOT EXISTS dup_decision (
  id INTEGER PRIMARY KEY AUTOINCREMENT, candidate_id INTEGER NOT NULL, kept_holding INTEGER NOT NULL,
  action TEXT NOT NULL DEFAULT 'hide', confirmed_at INTEGER NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
  kind UNINDEXED, ref UNINDEXED, title, artist, album, extra,
  tokenize = 'unicode61 remove_diacritics 2'
);
"#;
