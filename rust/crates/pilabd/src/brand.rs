// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Product naming in one place. The product name is not decided yet; change it here and
//! the web console, API, discovery TXT records and CLI all follow.

pub const PRODUCT_NAME: &str = "pilab console";
pub const PRODUCT_TAGLINE: &str = "Trillium, an Ambient Lab project";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_PORT: u16 = 8410;
pub const API_PREFIX: &str = "/api/v1";
