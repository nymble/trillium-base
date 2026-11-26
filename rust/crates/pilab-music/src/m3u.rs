// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! M3U / M3U8 lists of URLs (internet radio, HTTP file lists). `#EXTINF:-1,Name` names the next URL.

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub url: String,
}

pub fn parse(text: &str) -> Vec<Entry> {
    let mut out = vec![];
    let mut name = String::new();
    for line in text.lines() {
        let l = line.trim().trim_start_matches('\u{feff}');
        if l.is_empty() {
            continue;
        }
        if let Some(info) = l.strip_prefix("#EXTINF:") {
            name = info.split_once(',').map(|(_, n)| n.trim().to_string()).unwrap_or_default();
            continue;
        }
        if l.starts_with('#') {
            continue;
        }
        if l.starts_with("http://") || l.starts_with("https://") {
            let n = if name.is_empty() { host_of(l) } else { std::mem::take(&mut name) };
            out.push(Entry { name: n, url: l.to_string() });
        }
        name.clear();
    }
    out
}

pub fn render(entries: &[Entry]) -> String {
    let mut s = String::from("#EXTM3U\n");
    for e in entries {
        s.push_str(&format!("#EXTINF:-1,{}\n{}\n", e.name.replace(['\n', '\r'], " "), e.url));
    }
    s
}

fn host_of(url: &str) -> String {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url).to_string()
}

pub fn valid_url(u: &str) -> bool {
    (u.starts_with("http://") || u.starts_with("https://")) && u.len() < 2048 && !u.chars().any(|c| c.is_whitespace() || c.is_control())
}
