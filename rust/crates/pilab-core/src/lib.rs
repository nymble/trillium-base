// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Console data model: scenes (sound slots + MIDI routing), settings, storage.

pub mod devices;
pub mod glob;
pub mod routing;
pub mod scene;
pub mod settings;
pub mod store;

pub use devices::{classify, DeviceClass, SourceIdentity, Transport};
pub use routing::{compile_routes, compile_routes_with, PortDevices, PortNames};
pub use scene::{Route, Scene, Slot, SCENE_FORMAT, SCENE_VERSION};
pub use settings::Settings;
pub use store::SceneStore;
