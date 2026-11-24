// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
//! Who is behind a MIDI source port: USB id, ALSA client and port number (the USB MIDI
//! cable), or the BlueZ alias of a BLE MIDI device. Read-only: /proc/asound, `pw-dump`
//! and `bluetoothctl` queries; nothing is changed on any device.
//!
//! The PipeWire MIDI bridge does not always name ports so they can be told apart: a
//! device whose ALSA ports share one name (the Trillium Zero shows two ports both
//! called "Trill XIAO MIDI+PCM (dev) MIDI ") appears through the JACK API as
//! "NAME (capture)" and "NAME (capture)-<serial>", and PipeWire writes "+" as "-".
//! The ALSA address (client and port, i.e. the USB MIDI cable) is therefore taken from
//! the bridge port's `object.path` ("alsa:seq:default:client_36:capture_1") first.

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
    /// PipeWire MIDI bridge capture ports (from `pw-dump`).
    pub bridge: Vec<BridgePort>,
}

/// One PipeWire MIDI bridge capture port and the ALSA sequencer address behind it.
#[derive(Clone, Debug, PartialEq)]
pub struct BridgePort {
    /// PipeWire `object.serial`; pipewire-jack appends it to duplicate port names.
    pub serial: u64,
    /// PipeWire `port.name`, e.g. "Trill XIAO MIDI-PCM (dev) MIDI  (capture)".
    pub name: String,
    /// ALSA sequencer client number.
    pub client: u32,
    /// ALSA sequencer port number (the USB MIDI cable for snd-usb-audio clients).
    pub port: u32,
}

/// Capture ports of the PipeWire ALSA sequencer bridge, from `pw-dump` JSON.
pub fn parse_pw_bridge(json: &str) -> Vec<BridgePort> {
    let Ok(serde_json::Value::Array(objs)) = serde_json::from_str::<serde_json::Value>(json) else {
        return vec![];
    };
    let mut v = vec![];
    for o in objs {
        if !o.get("type").and_then(|t| t.as_str()).is_some_and(|t| t.ends_with(":Port")) {
            continue;
        }
        let Some(p) = o.pointer("/info/props") else { continue };
        let Some(path) = p.get("object.path").and_then(|x| x.as_str()) else { continue };
        let Some(rest) = path.strip_prefix("alsa:seq:") else { continue };
        // "default:client_36:capture_1"
        let mut it = rest.rsplit(':');
        let (Some(cap), Some(cl)) = (it.next(), it.next()) else { continue };
        let (Some(port), Some(client)) = (
            cap.strip_prefix("capture_").and_then(|n| n.parse::<u32>().ok()),
            cl.strip_prefix("client_").and_then(|n| n.parse::<u32>().ok()),
        ) else {
            continue;
        };
        let serial = match p.get("object.serial") {
            Some(serde_json::Value::Number(n)) => n.as_u64(),
            Some(serde_json::Value::String(s)) => s.parse().ok(),
            _ => None,
        };
        let (Some(serial), Some(name)) = (serial, p.get("port.name").and_then(|x| x.as_str())) else { continue };
        v.push(BridgePort { serial, name: name.to_string(), client, port });
    }
    v
}

/// "-<digits>" serial suffix that pipewire-jack adds to a duplicate port name.
fn serial_suffix(rest: &str) -> Option<(&str, u64)> {
    let (head, tail) = rest.rsplit_once('-')?;
    if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    tail.parse().ok().map(|n| (head, n))
}

/// The bridge port behind a JACK-API source name. A plain name is used only when exactly
/// one bridge port with that name is not already claimed by a "-<serial>" sibling.
fn bridge_for<'a>(port: &str, sources: &[String], bridge: &'a [BridgePort]) -> Option<&'a BridgePort> {
    let rest = port.strip_prefix("Midi-Bridge:")?;
    if let Some((head, serial)) = serial_suffix(rest) {
        if let Some(b) = bridge.iter().find(|b| b.serial == serial && b.name == head) {
            return Some(b);
        }
    }
    let claimed: Vec<u64> = sources
        .iter()
        .filter_map(|s| s.strip_prefix("Midi-Bridge:"))
        .filter_map(serial_suffix)
        .map(|(_, n)| n)
        .collect();
    let mut c = bridge.iter().filter(|b| b.name == rest && !claimed.contains(&b.serial));
    let first = c.next()?;
    c.next().is_none().then_some(first)
}

/// ALSA names compared the way PipeWire rewrites them ("+" becomes "-").
fn same_name(alsa: &str, graph: &str) -> bool {
    alsa == graph || alsa.replace('+', "-") == graph
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
    let bridge = Command::new("pw-dump")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|o| parse_pw_bridge(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    Alsa { clients, cards, usbids, bridge }
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
            // Exact ALSA address from the bridge port, when PipeWire reports it.
            if let Some(b) = bridge_for(s, sources, &alsa.bridge) {
                if let Some((cnum, cname, ports)) = alsa.clients.iter().find(|(n, _, _)| *n == b.client) {
                    id.product = Some(cname.clone());
                    id.alsa_port = ports.iter().find(|(n, _)| *n == b.port).map(|(_, n)| n.clone());
                    id.cable = u8::try_from(b.port).ok();
                    let card = alsa.cards.get(cname).copied().or_else(|| (*cnum >= 16).then(|| (*cnum - 16) / 4));
                    if let Some(u) = card.and_then(|c| alsa.usbids.get(&c)) {
                        id.usb_id = Some(u.clone());
                        id.transport = Transport::Usb;
                    }
                    return id;
                }
            }
            if let Some(pname) = alsa_port_name(s) {
                id.alsa_port = Some(pname.clone());
                id.cable = cable_from_name(s);
                if let Some((cnum, cname, ports)) = alsa.clients.iter().find(|(_, _, ps)| ps.iter().any(|(_, n)| same_name(n, &pname))) {
                    id.product = Some(cname.clone());
                    if let Some((pn, _)) = ports.iter().find(|(_, n)| same_name(n, &pname)) {
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
        let mut a = Alsa { clients: parse_seq_clients(CLIENTS), cards: parse_cards(CARDS), usbids: HashMap::new(), bridge: vec![] };
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

    // Recorded on a Pi 5 (PipeWire 1.4.2) with a Trillium Zero on firmware 0.8.0.
    const T0_CLIENTS: &str = r#"Client info
Client  14 : "Midi Through" [Kernel Legacy]
  Port   0 : "Midi Through Port-0" (RWe-) [In/Out]
Client  36 : "Trill XIAO MIDI+PCM (dev)" [Kernel Legacy]
  Port   0 : "Trill XIAO MIDI+PCM (dev) MIDI " (RWeX) [In/Out]
  Port   1 : "Trill XIAO MIDI+PCM (dev) MIDI " (RWeX) [In/Out]
"#;
    const T0_CARDS: &str = " 3 [Katana         ]: Allo_Katana - Allo Katana\n 5 [MIDIPCMdev     ]: USB-Audio - Trill XIAO MIDI+PCM (dev)\n                      nymble/trill Trill XIAO MIDI+PCM (dev) at usb-xhci-hcd.0-1, full speed\n";
    const T0_PWDUMP: &str = r#"[
 {"id":153,"type":"PipeWire:Interface:Port","info":{"props":{"object.serial":129982,"object.path":"alsa:seq:default:client_36:playback_0","port.name":"Trill XIAO MIDI-PCM (dev) MIDI  (playback)"}}},
 {"id":154,"type":"PipeWire:Interface:Port","info":{"props":{"object.serial":129983,"object.path":"alsa:seq:default:client_36:capture_0","port.name":"Trill XIAO MIDI-PCM (dev) MIDI  (capture)"}}},
 {"id":156,"type":"PipeWire:Interface:Port","info":{"props":{"object.serial":129985,"object.path":"alsa:seq:default:client_36:capture_1","port.name":"Trill XIAO MIDI-PCM (dev) MIDI  (capture)"}}},
 {"id":75,"type":"PipeWire:Interface:Node","info":{"props":{"node.name":"bluez_midi.server"}}}
]"#;

    #[test]
    fn duplicate_bridge_names_resolve_to_cables() {
        let mut a = Alsa { clients: parse_seq_clients(T0_CLIENTS), cards: parse_cards(T0_CARDS), usbids: HashMap::new(), bridge: parse_pw_bridge(T0_PWDUMP) };
        a.usbids.insert(5, "1209:0003".into());
        assert_eq!(a.bridge.len(), 2);
        let src = vec![
            "Midi-Bridge:Trill XIAO MIDI-PCM (dev) MIDI  (capture)".to_string(),
            "Midi-Bridge:Trill XIAO MIDI-PCM (dev) MIDI  (capture)-129985".to_string(),
        ];
        let ids = resolve(&src, &a, &[]);
        assert_eq!(ids[0].cable, Some(0));
        assert_eq!(ids[1].cable, Some(1));
        for id in &ids {
            assert_eq!(id.usb_id.as_deref(), Some("1209:0003"));
            assert_eq!(id.transport, Transport::Usb);
            assert_eq!(id.product.as_deref(), Some("Trill XIAO MIDI+PCM (dev)"));
        }
        // With the default classes, cable 0 is routed and cable 1 (configuration) is not.
        let classes = pilab_core::devices::default_classes();
        assert!(pilab_core::devices::classify(&classes, &ids[0]).unwrap().routable);
        assert!(!pilab_core::devices::classify(&classes, &ids[1]).unwrap().routable);
    }

    #[test]
    fn plus_written_as_minus_still_matches_alsa_names() {
        // No pw-dump data: fall back to names, where PipeWire wrote "+" as "-".
        let mut a = Alsa { clients: parse_seq_clients(T0_CLIENTS), cards: parse_cards(T0_CARDS), usbids: HashMap::new(), bridge: vec![] };
        a.usbids.insert(5, "1209:0003".into());
        let ids = resolve(&["Midi-Bridge:Trill XIAO MIDI-PCM (dev) MIDI  (capture)".to_string()], &a, &[]);
        assert_eq!(ids[0].usb_id.as_deref(), Some("1209:0003"));
        assert_eq!(ids[0].product.as_deref(), Some("Trill XIAO MIDI+PCM (dev)"));
    }

    #[test]
    fn ambiguous_plain_name_is_not_guessed() {
        let bridge = parse_pw_bridge(T0_PWDUMP);
        // Both bridge ports share the name and neither is claimed by a "-<serial>" sibling.
        let only = ["Midi-Bridge:Trill XIAO MIDI-PCM (dev) MIDI  (capture)".to_string()];
        assert!(bridge_for(&only[0], &only, &bridge).is_none());
        assert_eq!(serial_suffix("Midi Through Port-0 (capture)"), None);
        assert_eq!(serial_suffix("X (capture)-129985"), Some(("X (capture)", 129985)));
    }
}
