// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Tag and stream-property reading (lofty), with folder and file-name fallbacks for
//! untagged files: "Artist - Album" folders, "NN - Title" / "NN. Title" files.

use lofty::file::{AudioFile, FileType, TaggedFileExt};
use lofty::prelude::{Accessor, ItemKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const AUDIO_EXT: &[&str] = &["flac", "mp3", "ogg", "oga", "opus", "m4a", "mp4", "aac", "wav", "aif", "aiff", "wv", "ape", "mpc"];
pub const IMAGE_EXT: &[&str] = &["jpg", "jpeg", "png"];

pub fn ext_lower(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

pub fn is_audio(p: &Path) -> bool {
    AUDIO_EXT.contains(&ext_lower(p).as_str())
}

/// What the scanner keeps per file (stored as JSON in `holding.meta`).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub album_artist: String,
    #[serde(default)]
    pub track: u32,
    #[serde(default)]
    pub disc: u32,
    #[serde(default)]
    pub year: u32,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub mb_recording: String,
    #[serde(default)]
    pub mb_track: String,
    #[serde(default)]
    pub mb_release: String,
    #[serde(default)]
    pub mb_release_group: String,
    #[serde(default)]
    pub mb_artist: String,
    #[serde(default)]
    pub mb_album_artist: String,
    #[serde(default)]
    pub isrc: String,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub sample_rate: u32,
    #[serde(default)]
    pub bit_depth: u32,
    #[serde(default)]
    pub channels: u32,
    #[serde(default)]
    pub bitrate: u32,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub has_picture: bool,
}

fn codec_name(ft: &FileType) -> &'static str {
    match ft {
        FileType::Flac => "FLAC",
        FileType::Mpeg => "MP3",
        FileType::Vorbis => "Vorbis",
        FileType::Opus => "Opus",
        FileType::Mp4 => "AAC/ALAC",
        FileType::Aac => "AAC",
        FileType::Wav => "WAV",
        FileType::Aiff => "AIFF",
        FileType::WavPack => "WavPack",
        FileType::Ape => "APE",
        FileType::Mpc => "Musepack",
        FileType::Speex => "Speex",
        _ => "audio",
    }
}

/// Read tags and properties. Errors are returned as text; the caller still indexes the file
/// with fallbacks so a damaged tag never hides music.
pub fn read(path: &Path) -> Result<Meta, String> {
    let f = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let p = f.properties();
    let mut m = Meta {
        codec: codec_name(&f.file_type()).to_string(),
        sample_rate: p.sample_rate().unwrap_or(0),
        bit_depth: p.bit_depth().unwrap_or(0) as u32,
        channels: p.channels().unwrap_or(0) as u32,
        bitrate: p.audio_bitrate().or(p.overall_bitrate()).unwrap_or(0),
        duration: p.duration().as_secs_f64(),
        ..Default::default()
    };
    if let Some(t) = f.primary_tag().or_else(|| f.first_tag()) {
        let s = |k: &ItemKey| t.get_string(k).map(|v| v.trim().to_string()).unwrap_or_default();
        m.title = t.title().map(|v| v.trim().to_string()).unwrap_or_default();
        m.artist = t.artist().map(|v| v.trim().to_string()).unwrap_or_default();
        m.album = t.album().map(|v| v.trim().to_string()).unwrap_or_default();
        m.genre = t.genre().map(|v| v.trim().to_string()).unwrap_or_default();
        m.track = t.track().unwrap_or(0);
        m.disc = t.disk().unwrap_or(0);
        m.year = t.year().unwrap_or(0);
        if m.year == 0 {
            for k in [ItemKey::RecordingDate, ItemKey::ReleaseDate, ItemKey::OriginalReleaseDate] {
                if let Some(y) = s(&k).get(..4).and_then(|y| y.parse().ok()) {
                    m.year = y;
                    break;
                }
            }
        }
        m.album_artist = s(&ItemKey::AlbumArtist);
        m.mb_recording = s(&ItemKey::MusicBrainzRecordingId);
        m.mb_track = s(&ItemKey::MusicBrainzTrackId);
        m.mb_release = s(&ItemKey::MusicBrainzReleaseId);
        m.mb_release_group = s(&ItemKey::MusicBrainzReleaseGroupId);
        m.mb_artist = s(&ItemKey::MusicBrainzArtistId);
        m.mb_album_artist = s(&ItemKey::MusicBrainzReleaseArtistId);
        m.isrc = s(&ItemKey::Isrc);
    }
    m.has_picture = f.tags().iter().any(|t| !t.pictures().is_empty());
    Ok(m)
}

/// The front cover (or first picture) embedded in a file.
pub fn embedded_picture(path: &Path) -> Option<Vec<u8>> {
    let f = lofty::read_from_path(path).ok()?;
    let mut best: Option<Vec<u8>> = None;
    for t in f.tags() {
        for p in t.pictures() {
            if p.pic_type() == lofty::picture::PictureType::CoverFront {
                return Some(p.data().to_vec());
            }
            if best.is_none() {
                best = Some(p.data().to_vec());
            }
        }
    }
    best
}

/// "03 - Title", "03. Title", "03_Title", "1.Title" → (3, "Title"). No leading digits → (0, stem).
pub fn title_from_filename(path: &Path) -> (u32, String) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").trim().to_string();
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() && digits.len() <= 3 {
        let rest = &stem[digits.len()..];
        let trimmed = rest.trim_start_matches([' ', '.', '-', '_', ')']).trim();
        if !trimmed.is_empty() && rest.len() != trimmed.len() {
            return (digits.parse().unwrap_or(0), trimmed.replace('_', " "));
        }
    }
    (0, stem.replace('_', " "))
}

/// "Artist - Album" → (Some(artist), album); otherwise (None, folder name).
pub fn album_from_folder(dir: &Path) -> (Option<String>, String) {
    let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("").trim();
    if let Some((a, b)) = name.split_once(" - ") {
        if !a.trim().is_empty() && !b.trim().is_empty() {
            return (Some(a.trim().to_string()), b.trim().to_string());
        }
    }
    (None, name.to_string())
}

/// Fill missing title, track number, album and artist from the file and folder names.
pub fn apply_fallbacks(m: &mut Meta, path: &Path) {
    if m.title.is_empty() || m.track == 0 {
        let (n, t) = title_from_filename(path);
        if m.title.is_empty() {
            m.title = t;
        }
        if m.track == 0 {
            m.track = n;
        }
    }
    if m.album.is_empty() || (m.artist.is_empty() && m.album_artist.is_empty()) {
        if let Some(dir) = path.parent() {
            let (a, album) = album_from_folder(dir);
            if m.album.is_empty() {
                m.album = album;
            }
            if m.artist.is_empty() && m.album_artist.is_empty() {
                if let Some(a) = a {
                    m.artist = a;
                }
            }
        }
    }
    if m.artist.is_empty() && m.album_artist.is_empty() {
        m.artist = "Unknown artist".into();
    }
}
