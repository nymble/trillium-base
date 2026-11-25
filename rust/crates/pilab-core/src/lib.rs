// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Console data model: scenes (sound slots + MIDI routing), settings, storage.

pub mod apps;
pub mod devices;
pub mod glob;
pub mod presets;
pub mod routing;
pub mod scene;
pub mod settings;
pub mod store;
pub mod zero;
pub mod pcm;

pub use devices::{classify, DeviceClass, SourceIdentity, Transport};
pub use routing::{compile_routes, compile_routes_with, PortDevices, PortNames};
pub use scene::{Route, Scene, Slot, SCENE_FORMAT, SCENE_VERSION};
pub use settings::Settings;
pub use store::SceneStore;
