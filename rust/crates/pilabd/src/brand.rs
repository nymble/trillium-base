// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Product naming in one place; the web console, API, discovery records and CLI follow.
//! Trillium Base is the device, Trillium Console the control UI, Trillium Synth the synth.
//! Internal ids (pilabd, pilabctl, pilab-core, `_pilab._tcp`, ~/.config/pilab) are kept.

/// The control UI (web console now; the Mac app later).
pub const PRODUCT_NAME: &str = "Trillium Console";
/// The device this runs on.
pub const DEVICE_NAME: &str = "Trillium Base";
/// mDNS service types: the original one (kept for existing clients) and the product one.
pub const MDNS_TYPES: [&str; 2] = ["_pilab._tcp", "_trillium-base._tcp"];
pub const PRODUCT_TAGLINE: &str = "Trillium, an Ambient Lab project";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Git commit of the deployed tree (set by scripts/install-console.sh from COMMIT).
pub const COMMIT: &str = match option_env!("PILAB_COMMIT") {
    Some(c) => c,
    None => "dev",
};
/// The synth as people see it: the PipeWire/JACK node name, labels and docs. The stable
/// internal id (service id, crate name) stays `pilab-synth`; `LEGACY_SYNTH_CLIENT` is the
/// node name before the rename, accepted wherever a port name is given.
pub const SYNTH_NAME: &str = "Trillium Synth";
pub const SYNTH_ID: &str = "pilab-synth";
pub const LEGACY_SYNTH_CLIENT: &str = "pilab-synth";
pub const DEFAULT_PORT: u16 = 8410;
pub const API_PREFIX: &str = "/api/v1";
