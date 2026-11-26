// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Trillium Base music library.
//!
//! One index over many sources: folders (local, USB SSD, mounted NAS) and URL lists (M3U).
//! Two collections that never mix by default: `music` (published, read-only) and
//! `recordings` (takes made on this unit). Files are never modified; the index is rebuilt
//! from the files, and the tables for later editing, merging and duplicate handling
//! (`override`, `merge`, `dup_candidate`, `dup_decision`) are created empty from v1.
//!
//! Playback is not here: pilabd drives MPD as a separate process over its socket protocol.

pub mod art;
pub mod index;
pub mod m3u;
pub mod scan;
pub mod schema;
pub mod sources;
pub mod tags;

pub use art::ArtCache;
pub use index::Index;
pub use scan::{scan, ScanReport};
pub use sources::{SourceCfg, Sources};

/// FNV-1a 64-bit, kept to 53 bits: stable ids for artists, releases and streams (exact as
/// JSON numbers in a browser), and keys for the art cache.
pub fn stable_id(parts: &[&str]) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            h ^= 0x1f;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for &b in p.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    ((h ^ (h >> 53)) & ((1u64 << 53) - 1)) as i64
}

/// Case-insensitive sort key that ignores a leading "The " (same rule as the ZPOD player).
/// Accented Latin letters fold to their base letter so "Ånne" sorts with "A".
pub fn sort_key(s: &str) -> String {
    let l: String = s.trim().to_lowercase().chars().map(fold).collect();
    l.strip_prefix("the ").map(str::to_string).unwrap_or(l)
}

fn fold(c: char) -> char {
    match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'æ' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è'..='ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
        'ì'..='ï' | 'ī' | 'į' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'ō' | 'ő' => 'o',
        'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        'ś' | 'š' | 'ß' => 's',
        'ź' | 'ż' | 'ž' => 'z',
        'ł' => 'l',
        'đ' | 'ð' => 'd',
        'ř' => 'r',
        'ť' => 't',
        _ => c,
    }
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Expand a leading `~/` against `$HOME`.
pub fn expand(p: &str) -> std::path::PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(h) = std::env::var("HOME") {
            return std::path::PathBuf::from(h).join(rest);
        }
    }
    std::path::PathBuf::from(p)
}

#[cfg(test)]
mod tests;
