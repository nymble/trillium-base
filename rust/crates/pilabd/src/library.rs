// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! AKWF single-cycle library browsing and table loading.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pilab_synth::Wavetable;
use serde::Serialize;

#[derive(Serialize)]
pub struct Group {
    pub name: String,
    pub count: usize,
}

pub struct Library {
    pub root: PathBuf,
    cache: Mutex<HashMap<String, Arc<Wavetable>>>,
}

fn safe_rel(rel: &str) -> bool {
    !rel.is_empty() && !rel.starts_with('/') && !rel.split('/').any(|c| c == ".." || c.is_empty())
}

impl Library {
    pub fn new(root: PathBuf) -> Library {
        Library { root, cache: Mutex::new(HashMap::new()) }
    }

    pub fn available(&self) -> bool {
        self.root.is_dir()
    }

    pub fn groups(&self) -> Vec<Group> {
        let mut v = vec![];
        if let Ok(rd) = fs::read_dir(&self.root) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    let name = e.file_name().to_string_lossy().to_string();
                    let count = fs::read_dir(e.path()).map(|r| r.flatten().filter(|f| is_wav(&f.path())).count()).unwrap_or(0);
                    if count > 0 {
                        v.push(Group { name, count });
                    }
                }
            }
        }
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn files(&self, group: &str) -> Result<Vec<String>, String> {
        if !safe_rel(group) || group.contains('/') {
            return Err("bad group".into());
        }
        let mut v: Vec<String> = fs::read_dir(self.root.join(group))
            .map_err(|e| e.to_string())?
            .flatten()
            .filter(|f| is_wav(&f.path()))
            .map(|f| format!("{group}/{}", f.file_name().to_string_lossy()))
            .collect();
        v.sort();
        Ok(v)
    }

    pub fn load(&self, rel: &str) -> Result<Arc<Wavetable>, String> {
        if !safe_rel(rel) {
            return Err("bad path".into());
        }
        if let Some(t) = self.cache.lock().unwrap().get(rel) {
            return Ok(t.clone());
        }
        let bytes = fs::read(self.root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        let t = Arc::new(Wavetable::from_wav(rel, &bytes)?);
        let mut c = self.cache.lock().unwrap();
        if c.len() > 256 {
            c.clear();
        }
        c.insert(rel.to_string(), t.clone());
        Ok(t)
    }

    /// Raw cycle (decimated) for drawing.
    pub fn preview(&self, rel: &str, points: usize) -> Result<Vec<f32>, String> {
        if !safe_rel(rel) {
            return Err("bad path".into());
        }
        let bytes = fs::read(self.root.join(rel)).map_err(|e| e.to_string())?;
        let s = pilab_synth::wavetable::parse_wav(&bytes)?;
        let n = s.len().max(1);
        Ok((0..points).map(|i| s[i * n / points.max(1)]).collect())
    }

    pub fn source_note(&self) -> String {
        fs::read_to_string(self.root.join("SOURCE.txt")).unwrap_or_default()
    }
}

fn is_wav(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}
