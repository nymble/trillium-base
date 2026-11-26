// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Library sources: `~/.config/pilab/music/sources.toml`.
//!
//! ```toml
//! [[source]]
//! id = "drive"
//! kind = "folder"            # folder | urls
//! path = "/srv/library/music"
//! collection = "music"       # music | recordings
//! optional = true            # missing path is "offline", not an error
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SourceCfg {
    pub id: String,
    #[serde(default = "folder")]
    pub kind: String,
    pub path: String,
    #[serde(default = "music")]
    pub collection: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub read_only: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub optional: bool,
}

fn folder() -> String {
    "folder".into()
}
fn music() -> String {
    "music".into()
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Sources {
    #[serde(default)]
    pub source: Vec<SourceCfg>,
}

impl SourceCfg {
    pub fn abs_path(&self) -> PathBuf {
        crate::expand(&self.path)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || !self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(format!("source id {:?}: use letters, digits, - or _", self.id));
        }
        if !matches!(self.kind.as_str(), "folder" | "urls") {
            return Err(format!("source {}: kind must be folder or urls", self.id));
        }
        if !matches!(self.collection.as_str(), "music" | "recordings") {
            return Err(format!("source {}: collection must be music or recordings", self.id));
        }
        if self.path.is_empty() {
            return Err(format!("source {}: path is empty", self.id));
        }
        Ok(())
    }
}

impl Sources {
    /// Defaults for a Trillium Base unit. `recordings` is the recorder's own folder.
    pub fn defaults(recordings_dir: &str) -> Sources {
        let s = |id: &str, kind: &str, path: &str, coll: &str, name: &str, optional: bool| SourceCfg {
            id: id.into(),
            kind: kind.into(),
            path: path.into(),
            collection: coll.into(),
            name: name.into(),
            read_only: true,
            enabled: true,
            optional,
        };
        Sources {
            source: vec![
                s("demo", "folder", "~/.local/share/pilab/music/demo", "music", "Demo albums (CC0)", true),
                s("local", "folder", "~/Music", "music", "Local music", true),
                s("drive", "folder", "/srv/library/music", "music", "Library drive", true),
                s("recordings", "folder", recordings_dir, "recordings", "Recordings on this unit", true),
                s("drive-recordings", "folder", "/srv/library/recordings", "recordings", "Recordings on the drive", true),
                s("radio", "urls", "~/.config/pilab/music/streams.m3u", "music", "Internet radio", true),
            ],
        }
    }

    pub fn load_or_default(path: &Path, recordings_dir: &str) -> Result<Sources, String> {
        match std::fs::read_to_string(path) {
            Ok(t) => {
                let s: Sources = toml::from_str(&t).map_err(|e| format!("{}: {e}", path.display()))?;
                for c in &s.source {
                    c.validate()?;
                }
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Sources::defaults(recordings_dir)),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn to_toml(&self) -> String {
        format!(
            "# Trillium Base music sources. Edited by the console (Music > Sources) or by hand.\n{}",
            toml::to_string_pretty(self).unwrap_or_default()
        )
    }

    pub fn get(&self, id: &str) -> Option<&SourceCfg> {
        self.source.iter().find(|s| s.id == id)
    }
}
