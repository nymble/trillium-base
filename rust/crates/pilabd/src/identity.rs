// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! Who is behind a MIDI source port: USB id, ALSA client and port number (the USB MIDI
//! cable), or the BlueZ alias of a BLE MIDI device. Read-only: /proc/asound and
//! `bluetoothctl` queries; nothing is changed on any device.

use std::collections::HashMap;
use std::fs;
use std::process::{Command, Stdio};

use pilab_core::devices::cable_from_name;
use pilab_core::{SourceIdentity, Transport};

#[derive(Default, Debug)]
pub struct Alsa {
    /// (client number, client name, [(port number, port name)])
    pub clients: Vec<(u32, String, Vec<(u32, String)>)>,
    /// card short name -> card number
    pub cards: HashMap<String, u32>,
    /// card number -> "vvvv:pppp"
    pub usbids: HashMap<u32, String>,
}

fn quoted(s: &str) -> Option<String> {
    let a = s.find('"')?;
    let b = s.rfind('"')?;
    (b > a).then(|| s[a + 1..b].to_string())
}

/// Parses /proc/asound/seq/clients.
pub fn parse_seq_clients(text: &str) -> Vec<(u32, String, Vec<(u32, String)>)> {
    let mut v: Vec<(u32, String, Vec<(u32, String)>)> = vec![];
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("Client") {
            let num = rest.split(':').next().unwrap_or("").trim();
            if let (Ok(n), Some(name)) = (num.parse::<u32>(), quoted(rest)) {
                v.push((n, name, vec![]));
            }
        } else if let Some(rest) = t.strip_prefix("Port") {
            let num = rest.split(':').next().unwrap_or("").trim();
            if let (Ok(n), Some(name), Some(last)) = (num.parse::<u32>(), quoted(rest), v.last_mut()) {
                last.2.push((n, name));
            }
        }
    }
    v
}

/// Parses /proc/asound/cards: " 2 [Device ]: USB-Audio - USB Composite Device".
pub fn parse_cards(text: &str) -> HashMap<String, u32> {
    let mut m = HashMap::new();
    for line in text.lines() {
        let t = line.trim_start();
        let Some((num, rest)) = t.split_once(' ') else { continue };
        let Ok(n) = num.parse::<u32>() else { continue };
        if let Some((_, short)) = rest.split_once(" - ") {
            m.insert(short.trim().to_string(), n);
        }
    }
    m
}

pub fn read_alsa() -> Alsa {
    let clients = parse_seq_clients(&fs::read_to_string("/proc/asound/seq/clients").unwrap_or_default());
    let cards = parse_cards(&fs::read_to_string("/proc/asound/cards").unwrap_or_default());
    let mut usbids = HashMap::new();
    for &n in cards.values() {
        if let Ok(id) = fs::read_to_string(format!("/proc/asound/card{n}/usbid")) {
            usbids.insert(n, id.trim().to_lowercase());
        }
    }
    Alsa { clients, cards, usbids }
}

/// True for PipeWire's BLE MIDI nodes ("BLE MIDI 1:out").
pub fn is_ble(port: &str) -> bool {
    port.split_once(':').map(|(c, _)| c.starts_with("BLE MIDI")).unwrap_or(false)
}

/// ALSA port name from a graph port name, for the PipeWire MIDI bridge formats
/// "Midi-Bridge:NAME (capture)" and "Midi-Bridge:CLIENT:(capture_N) NAME".
fn alsa_port_name(port: &str) -> Option<String> {
    let rest = port.strip_prefix("Midi-Bridge:")?;
    let rest = match rest.find("(capture_") {
        Some(i) => rest[i..].split_once(") ").map(|(_, n)| n).unwrap_or(rest),
        None => rest,
    };
    Some(rest.trim_end_matches(" (capture)").to_string())
}

/// Builds the identity of each source from ALSA data and the BLE aliases now connected.
pub fn resolve(sources: &[String], alsa: &Alsa, ble_aliases: &[String]) -> Vec<SourceIdentity> {
    sources
        .iter()
        .map(|s| {
            let mut id = SourceIdentity::new(s);
            if is_ble(s) {
                id.transport = Transport::Ble;
                id.ble_aliases = ble_aliases.to_vec();
                return id;
            }
            if let Some(pname) = alsa_port_name(s) {
                id.alsa_port = Some(pname.clone());
                id.cable = cable_from_name(s);
                if let Some((cnum, cname, ports)) = alsa.clients.iter().find(|(_, _, ps)| ps.iter().any(|(_, n)| *n == pname)) {
                    id.product = Some(cname.clone());
                    if let Some((pn, _)) = ports.iter().find(|(_, n)| *n == pname) {
                        id.cable = Some(*pn as u8);
                    }
                    let card = alsa.cards.get(cname).copied().or_else(|| (*cnum >= 16).then(|| (*cnum - 16) / 4));
                    if let Some(u) = card.and_then(|c| alsa.usbids.get(&c)) {
                        id.usb_id = Some(u.clone());
                        id.transport = Transport::Usb;
                    }
                }
            }
            id
        })
        .collect()
}

fn run(args: &[&str]) -> String {
    Command::new("bluetoothctl")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// Aliases of BLE devices connected now (BlueZ, through bluetoothctl). The Pi's BLE MIDI
/// node in server role does not name its peer, so every connected device is a candidate;
/// class globs (e.g. "Trillium *") pick the right one.
pub fn ble_connected_aliases() -> Vec<String> {
    let mut v = vec![];
    for line in run(&["devices", "Connected"]).lines() {
        let mut it = line.split_whitespace();
        if it.next() != Some("Device") {
            continue;
        }
        let Some(mac) = it.next() else { continue };
        let info = run(&["info", mac]);
        let alias = info
            .lines()
            .find_map(|l| l.trim().strip_prefix("Alias: "))
            .or_else(|| info.lines().find_map(|l| l.trim().strip_prefix("Name: ")))
            .map(|s| s.trim().to_string());
        if let Some(a) = alias {
            v.push(a);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENTS: &str = r#"Client info
  cur  clients : 5
Client   0 : "System" [Kernel Legacy]
  Port   0 : "Timer" (Rwe-) [In/Out]
Client  14 : "Midi Through" [Kernel Legacy]
  Port   0 : "Midi Through Port-0" (RWe-) [In/Out]
Client  24 : "Trill XIAO MIDI" [Kernel Legacy]
  Port   0 : "Trill XIAO MIDI MIDI 1" (RWeX) [In/Out]
  Port   1 : "Trill XIAO MIDI MIDI 2" (RWeX) [In/Out]
Client  28 : "USB Composite Device" [Kernel Legacy]
  Port   0 : "USB Composite Device MIDI 1" (RWeX) [In/Out]
"#;
    const CARDS: &str = " 0 [vc4hdmi0       ]: vc4-hdmi - vc4-hdmi-0\n                      vc4-hdmi-0\n 2 [Device         ]: USB-Audio - USB Composite Device\n                      USB Composite Device at usb-xhci-hcd.1-2, full speed\n 3 [MIDI           ]: USB-Audio - Trill XIAO MIDI\n                      Seeed Trill XIAO MIDI at usb-xhci-hcd.1-1, full speed\n";

    fn alsa() -> Alsa {
        let mut a = Alsa { clients: parse_seq_clients(CLIENTS), cards: parse_cards(CARDS), usbids: HashMap::new() };
        a.usbids.insert(3, "1209:0003".into());
        a.usbids.insert(2, "1915:1025".into());
        a
    }

    #[test]
    fn resolves_usb_cables_and_ids() {
        let src = vec![
            "Midi-Bridge:Trill XIAO MIDI MIDI 1 (capture)".to_string(),
            "Midi-Bridge:Trill XIAO MIDI MIDI 2 (capture)".to_string(),
            "Midi-Bridge:USB Composite Device MIDI 1 (capture)".to_string(),
            "Midi-Bridge:Midi Through Port-0 (capture)".to_string(),
            "BLE MIDI 1:out".to_string(),
        ];
        let ids = resolve(&src, &alsa(), &["Trillium 7F2A".to_string()]);
        assert_eq!(ids[0].usb_id.as_deref(), Some("1209:0003"));
        assert_eq!(ids[0].cable, Some(0));
        assert_eq!(ids[0].transport, Transport::Usb);
        assert_eq!(ids[1].cable, Some(1));
        assert_eq!(ids[2].usb_id.as_deref(), Some("1915:1025"));
        assert_eq!(ids[3].transport, Transport::Other);
        assert_eq!(ids[4].transport, Transport::Ble);
        assert_eq!(ids[4].ble_aliases, vec!["Trillium 7F2A".to_string()]);
        assert_eq!(ids[0].describe(), "USB · 1209:0003 · Trill XIAO MIDI · cable 0");
    }

    #[test]
    fn alternate_bridge_format() {
        assert_eq!(alsa_port_name("Midi-Bridge:Trill XIAO MIDI:(capture_1) Trill XIAO MIDI MIDI 2").as_deref(), Some("Trill XIAO MIDI MIDI 2"));
        assert_eq!(cable_from_name("Midi-Bridge:Trill XIAO MIDI:(capture_1) Trill XIAO MIDI MIDI 2"), Some(1));
    }
}
