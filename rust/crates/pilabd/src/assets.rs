// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Web console files, embedded at build time (no build server on the Pi).

use std::sync::OnceLock;

use crate::brand::{COMMIT, DEVICE_NAME, PRODUCT_NAME, PRODUCT_TAGLINE, VERSION};

macro_rules! files {
    ($($path:literal => $mime:literal),* $(,)?) => {
        const FILES: &[(&str, &str, &[u8])] = &[
            $(($path, $mime, include_bytes!(concat!("../web", $path)))),*
        ];
    };
}

files! {
    "/index.html" => "text/html; charset=utf-8",
    "/app.js" => "text/javascript; charset=utf-8",
    "/app.css" => "text/css; charset=utf-8",
    "/theme.css" => "text/css; charset=utf-8",
    "/data/akwf-catalog.json" => "application/json",
    "/brand/tokens.css" => "text/css; charset=utf-8",
    "/brand/rhodonea-glyph.svg" => "image/svg+xml",
    "/brand/rhodonea-app-dark.svg" => "image/svg+xml",
    "/brand/lockup-primary-on-black.svg" => "image/svg+xml",
    "/brand/lockup-primary.svg" => "image/svg+xml",
    "/manifest.webmanifest" => "application/manifest+json",
    "/brand/app-icon-180.png" => "image/png",
    "/brand/app-icon-192.png" => "image/png",
    "/brand/app-icon-512.png" => "image/png",
    "/brand/ui-icons/axes-calibrate.svg" => "image/svg+xml",
    "/brand/ui-icons/headphones.svg" => "image/svg+xml",
    "/brand/ui-icons/record.svg" => "image/svg+xml",
    "/brand/ui-icons/midi.svg" => "image/svg+xml",
    "/brand/ui-icons/presets.svg" => "image/svg+xml",
    "/brand/ui-icons/instrument.svg" => "image/svg+xml",
    "/brand/ui-icons/live-monitor.svg" => "image/svg+xml",
    "/brand/ui-icons/wireless.svg" => "image/svg+xml",
    "/brand/ui-icons/firmware-update.svg" => "image/svg+xml",
    "/brand/ui-icons/trillium.svg" => "image/svg+xml",
    "/brand/ui-icons/touch.svg" => "image/svg+xml",
    "/fonts/inter-latin-400-normal.woff2" => "font/woff2",
    "/fonts/inter-latin-500-normal.woff2" => "font/woff2",
    "/fonts/inter-latin-600-normal.woff2" => "font/woff2",
    "/fonts/outfit-latin-300-normal.woff2" => "font/woff2",
    "/fonts/outfit-latin-400-normal.woff2" => "font/woff2",
    "/fonts/outfit-latin-500-normal.woff2" => "font/woff2",
    "/fonts/ibm-plex-mono-latin-400-normal.woff2" => "font/woff2",
    "/fonts/ibm-plex-mono-latin-500-normal.woff2" => "font/woff2",
}

pub fn get(path: &str) -> Option<(&'static str, &'static [u8])> {
    FILES.iter().find(|(p, _, _)| *p == path).map(|(_, m, b)| (*m, *b))
}

/// Build id used to version asset URLs: the deployed commit plus a hash of every embedded
/// file, so a rebuild with changed assets always gets new URLs (also for "dev" builds).
pub fn build_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a
        for (p, _, b) in FILES {
            for &x in p.as_bytes().iter().chain(b.iter()) {
                h ^= x as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        let c: String = COMMIT.chars().take(12).collect();
        format!("{c}-{:08x}", (h ^ (h >> 32)) as u32)
    })
}

/// index.html with the product name and build id filled in (the single naming point is
/// `brand.rs`). Asset URLs carry `?v={{BUILD}}`.
pub fn index_html() -> String {
    let raw = get("/index.html").map(|(_, b)| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    raw.replace("{{PRODUCT_NAME}}", PRODUCT_NAME)
        .replace("{{DEVICE_NAME}}", DEVICE_NAME)
        .replace("{{TAGLINE}}", PRODUCT_TAGLINE)
        .replace("{{VERSION}}", VERSION)
        .replace("{{COMMIT}}", COMMIT)
        .replace("{{BUILD}}", build_id())
}

/// Readable AKWF group name from the catalog ("AKWF_fmsynth" -> "FM synth"); unknown groups
/// fall back to the folder name without the prefix.
pub fn akwf_name(group: &str) -> String {
    static CAT: OnceLock<serde_json::Value> = OnceLock::new();
    let cat = CAT.get_or_init(|| get("/data/akwf-catalog.json").and_then(|(_, b)| serde_json::from_slice(b).ok()).unwrap_or_default());
    cat["groups"][group]["name"].as_str().map(str::to_string).unwrap_or_else(|| group.trim_start_matches("AKWF_").replace('_', " "))
}
