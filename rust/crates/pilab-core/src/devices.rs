// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Known MIDI controllers ("device classes"), matched by USB id, ALSA/USB product name or
//! BlueZ alias, and which of their ports may reach the synth.
//!
//! The patterns live in settings (`[[midi.devices]]`), not in code: development ids change.
//! Routes refer to a class with `input = "device:<id>"`.

use serde::{Deserialize, Serialize};

use crate::glob;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceClass {
    /// Short id used in routes as `device:<id>`.
    pub id: String,
    /// Shown in the console.
    pub label: String,
    /// USB `VID:PID`, hex, case-insensitive (e.g. "1209:0003").
    pub usb_ids: Vec<String>,
    /// USB/ALSA product, client or port name prefixes (case-insensitive).
    pub name_prefixes: Vec<String>,
    /// Globs on the BlueZ alias of a connected BLE MIDI device (e.g. "Trillium *").
    pub ble_aliases: Vec<String>,
    /// USB MIDI cables (0-based ALSA port numbers) that may be routed. Empty = all.
    pub route_cables: Vec<u8>,
    /// Free-form note shown in the console (e.g. why other cables are skipped).
    pub note: String,
}

impl Default for DeviceClass {
    fn default() -> Self {
        DeviceClass {
            id: String::new(),
            label: String::new(),
            usb_ids: vec![],
            name_prefixes: vec![],
            ble_aliases: vec![],
            route_cables: vec![],
            note: String::new(),
        }
    }
}

/// Default classes. The Trillium ids are development values; expect them to change
/// (a future product string is likely "Trillium Zero"), so edit settings, not code.
pub fn default_classes() -> Vec<DeviceClass> {
    vec![DeviceClass {
        id: "trillium".into(),
        label: "Trillium".into(),
        usb_ids: vec!["1209:0003".into(), "1209:0002".into()],
        name_prefixes: vec!["Trill XIAO MIDI".into(), "Trillium Zero".into()],
        ble_aliases: vec!["Trillium *".into()],
        route_cables: vec![0],
        note: "USB cable 0 is the instrument; cable 1 is configuration SysEx and is never \
               connected; the CDC ACM serial ports are not MIDI and are ignored."
            .into(),
    }]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Usb,
    Ble,
    Other,
}

/// What is known about one MIDI source port in the graph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceIdentity {
    /// Full graph port name, e.g. "Midi-Bridge:Trill XIAO MIDI MIDI 1 (capture)".
    pub port: String,
    pub transport: Transport,
    /// USB `VID:PID` of the sound card behind an ALSA source.
    pub usb_id: Option<String>,
    /// ALSA sequencer client / USB product name.
    pub product: Option<String>,
    /// ALSA port name.
    pub alsa_port: Option<String>,
    /// USB MIDI cable (ALSA port number), 0-based.
    pub cable: Option<u8>,
    /// BlueZ aliases of the BLE device(s) behind a BLE MIDI port.
    pub ble_aliases: Vec<String>,
}

impl SourceIdentity {
    pub fn new(port: &str) -> SourceIdentity {
        SourceIdentity {
            port: port.into(),
            transport: Transport::Other,
            usb_id: None,
            product: None,
            alsa_port: None,
            cable: None,
            ble_aliases: vec![],
        }
    }

    /// One-line description for the console.
    pub fn describe(&self) -> String {
        let mut parts = vec![];
        match self.transport {
            Transport::Usb => parts.push("USB".to_string()),
            Transport::Ble => parts.push("BLE".to_string()),
            Transport::Other => {}
        }
        if let Some(u) = &self.usb_id {
            parts.push(u.clone());
        }
        if let Some(p) = &self.product {
            parts.push(p.clone());
        }
        if let Some(c) = self.cable {
            parts.push(format!("cable {c}"));
        }
        if !self.ble_aliases.is_empty() {
            parts.push(self.ble_aliases.join(", "));
        }
        parts.join(" · ")
    }
}

/// Result of matching one source against the device classes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Match {
    /// Index into the class list.
    pub class: usize,
    /// "usb" or "ble": which rule matched.
    pub rule: &'static str,
    /// Whether this port may be connected and routed.
    pub routable: bool,
    /// Why not, when `routable` is false.
    pub reason: String,
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    !prefix.is_empty() && s.to_lowercase().starts_with(&prefix.to_lowercase())
}

/// The graph port name without the client part ("Midi-Bridge:", "BLE MIDI 1:").
fn port_part(port: &str) -> &str {
    port.split_once(':').map(|(_, p)| p).unwrap_or(port)
}

/// First class that matches, in list order. Within a class the USB rule is tried before BLE.
pub fn classify(classes: &[DeviceClass], id: &SourceIdentity) -> Option<Match> {
    for (i, c) in classes.iter().enumerate() {
        if id.transport != Transport::Ble {
            let by_id = id.usb_id.as_deref().is_some_and(|u| c.usb_ids.iter().any(|x| x.eq_ignore_ascii_case(u)));
            let names = [id.product.as_deref(), id.alsa_port.as_deref(), Some(port_part(&id.port))];
            let by_name = names.iter().flatten().any(|n| c.name_prefixes.iter().any(|p| starts_with_ci(n, p)));
            if by_id || by_name {
                let (routable, reason) = match (c.route_cables.is_empty(), id.cable) {
                    (true, _) => (true, String::new()),
                    (false, Some(k)) if c.route_cables.contains(&k) => (true, String::new()),
                    (false, Some(k)) => (false, format!("{} cable {k} is not routed (only {:?})", c.label, c.route_cables)),
                    (false, None) => (false, format!("{}: cable number unknown, not routed", c.label)),
                };
                return Some(Match { class: i, rule: "usb", routable, reason });
            }
        }
        let client = id.port.split_once(':').map(|(c, _)| c).unwrap_or("");
        let by_alias = id.ble_aliases.iter().any(|a| c.ble_aliases.iter().any(|g| glob::matches(g, a)));
        // A BLE MIDI node in client role may carry the device name as its client name.
        let by_client = id.transport != Transport::Usb && !client.is_empty() && c.ble_aliases.iter().any(|g| glob::matches(g, client));
        if by_alias || by_client {
            return Some(Match { class: i, rule: "ble", routable: true, reason: String::new() });
        }
    }
    None
}

/// Cable number from a port name: "(capture_1)" or a trailing "MIDI 2" (1-based) suffix.
pub fn cable_from_name(name: &str) -> Option<u8> {
    if let Some(i) = name.find("(capture_") {
        let rest = &name[i + 9..];
        let n: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(v) = n.parse() {
            return Some(v);
        }
    }
    let base = name.trim_end_matches(" (capture)").trim_end_matches(" (playback)").trim_end();
    let digits: String = base.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    if !digits.is_empty() {
        let head = base[..base.len() - digits.len()].trim_end();
        if head.to_lowercase().ends_with("midi") {
            return digits.parse::<u8>().ok().and_then(|n| n.checked_sub(1));
        }
    }
    None
}
