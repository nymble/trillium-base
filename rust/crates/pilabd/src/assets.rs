// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Web console files, embedded at build time (no build server on the Pi).

use crate::brand::{PRODUCT_NAME, PRODUCT_TAGLINE, VERSION};

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

/// index.html with the product name filled in (the single naming point is `brand.rs`).
pub fn index_html() -> String {
    let raw = get("/index.html").map(|(_, b)| String::from_utf8_lossy(b).to_string()).unwrap_or_default();
    raw.replace("{{PRODUCT_NAME}}", PRODUCT_NAME).replace("{{TAGLINE}}", PRODUCT_TAGLINE).replace("{{VERSION}}", VERSION)
}
