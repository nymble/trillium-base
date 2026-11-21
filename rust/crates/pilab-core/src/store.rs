// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Scene files in a directory (`~/.config/pilab/scenes/<name>.toml`), written atomically.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::scene::{valid_name, Scene};

pub struct SceneStore {
    dir: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct SceneInfo {
    pub name: String,
    pub description: String,
    pub slots: usize,
    pub routes: usize,
    pub modified: u64,
    pub error: Option<String>,
}

impl SceneStore {
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<SceneStore> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(SceneStore { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, name: &str) -> Result<PathBuf, String> {
        if !valid_name(name) {
            return Err(format!("invalid scene name {name:?}"));
        }
        Ok(self.dir.join(format!("{name}.toml")))
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).map(|p| p.exists()).unwrap_or(false)
    }

    pub fn list(&self) -> Vec<SceneInfo> {
        let mut out = vec![];
        if let Ok(rd) = fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) != Some("toml") {
                    continue;
                }
                let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                let modified = e
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                match fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| Scene::from_toml(&s)) {
                    Ok(sc) => out.push(SceneInfo {
                        name,
                        description: sc.description,
                        slots: sc.slots.len(),
                        routes: sc.routes.len(),
                        modified,
                        error: None,
                    }),
                    Err(err) => out.push(SceneInfo { name, description: String::new(), slots: 0, routes: 0, modified, error: Some(err) }),
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn load(&self, name: &str) -> Result<Scene, String> {
        let p = self.path(name)?;
        let s = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let mut sc = Scene::from_toml(&s)?;
        sc.name = name.to_string();
        Ok(sc)
    }

    /// Saves under `name` (the scene's own name is set to match). Keeps the previous
    /// file as `<name>.toml.bak`.
    pub fn save(&self, scene: &Scene, name: &str) -> Result<(), String> {
        let p = self.path(name)?;
        let mut sc = scene.clone();
        sc.name = name.to_string();
        sc.validate()?;
        let body = sc.to_toml()?;
        let tmp = p.with_extension("toml.tmp");
        fs::write(&tmp, body).map_err(|e| e.to_string())?;
        if p.exists() {
            let _ = fs::copy(&p, p.with_extension("toml.bak"));
        }
        fs::rename(&tmp, &p).map_err(|e| e.to_string())
    }

    pub fn duplicate(&self, from: &str, to: &str) -> Result<(), String> {
        if self.exists(to) {
            return Err(format!("scene {to:?} already exists"));
        }
        let sc = self.load(from)?;
        self.save(&sc, to)
    }

    pub fn delete(&self, name: &str) -> Result<(), String> {
        let p = self.path(name)?;
        fs::remove_file(&p).map_err(|e| e.to_string())
    }
}
