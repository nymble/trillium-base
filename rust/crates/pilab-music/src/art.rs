// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Cover art: folder images (cover/folder/front.jpg|png) win over embedded pictures.
//! Thumbnails are square JPEGs cached as `<key>-<size>.jpg`; originals are never touched.

use std::path::{Path, PathBuf};

pub const SIZES: &[u32] = &[300, 600];
const NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "0cover"];

pub struct ArtCache {
    pub dir: PathBuf,
}

impl ArtCache {
    pub fn new(dir: PathBuf) -> ArtCache {
        ArtCache { dir }
    }

    pub fn path(&self, key: &str, size: u32) -> PathBuf {
        self.dir.join(format!("{key}-{size}.jpg"))
    }

    pub fn has(&self, key: &str) -> bool {
        SIZES.iter().all(|s| self.path(key, *s).exists())
    }

    /// Nearest cached size at or above `want` (falls back to the largest).
    pub fn best(&self, key: &str, want: u32) -> Option<PathBuf> {
        if !valid_key(key) {
            return None;
        }
        let size = SIZES.iter().copied().find(|s| *s >= want).unwrap_or(*SIZES.last().unwrap());
        let p = self.path(key, size);
        if p.exists() {
            Some(p)
        } else {
            SIZES.iter().map(|s| self.path(key, *s)).find(|p| p.exists())
        }
    }

    /// Decode once and write every thumbnail size. Returns false if the image is unreadable.
    pub fn ensure(&self, key: &str, bytes: &[u8]) -> bool {
        if self.has(key) {
            return true;
        }
        let Ok(img) = image::load_from_memory(bytes) else { return false };
        if std::fs::create_dir_all(&self.dir).is_err() {
            return false;
        }
        for &s in SIZES {
            let t = img.resize_to_fill(s, s, image::imageops::FilterType::Lanczos3).to_rgb8();
            let mut out = Vec::new();
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
            if t.write_with_encoder(enc).is_err() {
                return false;
            }
            let p = self.path(key, s);
            let tmp = p.with_extension("tmp");
            if std::fs::write(&tmp, &out).is_err() || std::fs::rename(&tmp, &p).is_err() {
                return false;
            }
        }
        true
    }
}

pub fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 32 && k.chars().all(|c| c.is_ascii_hexdigit())
}

/// A cover image in an album folder: preferred names first, else the only image there.
/// Failing that, the same in an artwork subfolder (Covers/, Scans/, Artwork/ …), where a
/// numbered set ("1.jpg", "01.jpg") starts with the front.
pub fn folder_image(dir: &Path) -> Option<PathBuf> {
    if let Some(p) = pick_image(dir, false) {
        return Some(p);
    }
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name().and_then(|s| s.to_str()).map(|n| SUBDIRS.iter().any(|s| n.eq_ignore_ascii_case(s))).unwrap_or(false)
        })
        .collect();
    subs.sort();
    subs.iter().find_map(|d| pick_image(d, true))
}

const SUBDIRS: &[&str] = &["covers", "cover", "scans", "artwork", "art", "images", "img"];

fn pick_image(dir: &Path, numbered: bool) -> Option<PathBuf> {
    let mut images = vec![];
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_file() && super::tags::IMAGE_EXT.contains(&super::tags::ext_lower(&p).as_str()) {
            images.push(p);
        }
    }
    images.sort();
    let stem = |p: &PathBuf| p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    for n in NAMES {
        if let Some(p) = images.iter().find(|p| stem(p).eq_ignore_ascii_case(n)) {
            return Some(p.clone());
        }
    }
    if numbered {
        if let Some(p) = images.iter().find(|p| matches!(stem(p).trim_start_matches('0'), "1")) {
            return Some(p.clone());
        }
    }
    if images.len() == 1 {
        return images.pop();
    }
    None
}
