// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Console data model: scenes (sound slots + MIDI routing), settings, storage.

pub mod glob;
pub mod routing;
pub mod scene;
pub mod settings;
pub mod store;

pub use routing::{compile_routes, PortNames};
pub use scene::{Route, Scene, Slot, SCENE_FORMAT, SCENE_VERSION};
pub use settings::Settings;
pub use store::SceneStore;
