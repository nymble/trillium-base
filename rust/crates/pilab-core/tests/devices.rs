// Copyright (C) 2026 M-of-N
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
use pilab_core::devices::{cable_from_name, default_classes};
use pilab_core::{classify, compile_routes_with, DeviceClass, PortDevices, PortNames, Route, Scene, Settings, SourceIdentity, Transport};
use pilab_synth::{Hit, MAX_LAYERS};

fn usb(port: &str, usb_id: Option<&str>, product: &str, cable: Option<u8>) -> SourceIdentity {
    SourceIdentity {
        transport: if usb_id.is_some() { Transport::Usb } else { Transport::Other },
        usb_id: usb_id.map(Into::into),
        product: Some(product.into()),
        alsa_port: Some(port.trim_start_matches("Midi-Bridge:").trim_end_matches(" (capture)").into()),
        cable,
        ..SourceIdentity::new(port)
    }
}

fn ble(aliases: &[&str]) -> SourceIdentity {
    SourceIdentity { transport: Transport::Ble, ble_aliases: aliases.iter().map(|s| s.to_string()).collect(), ..SourceIdentity::new("BLE MIDI 1:out") }
}

#[test]
fn trillium_usb_by_id_routes_only_cable_0() {
    let c = default_classes();
    for vid in ["1209:0003", "1209:0002", "1209:0003".to_uppercase().as_str()] {
        let m = classify(&c, &usb("Midi-Bridge:Some Name MIDI 1 (capture)", Some(vid), "Some Name", Some(0))).expect("matched");
        assert_eq!((c[m.class].id.as_str(), m.rule, m.routable), ("trillium", "usb", true));
        let m = classify(&c, &usb("Midi-Bridge:Some Name MIDI 2 (capture)", Some(vid), "Some Name", Some(1))).expect("matched");
        assert!(!m.routable, "cable 1 is config SysEx");
    }
}

#[test]
fn trillium_usb_by_name_prefix() {
    let c = default_classes();
    let m = classify(&c, &usb("Midi-Bridge:Trill XIAO MIDI MIDI 1 (capture)", None, "Trill XIAO MIDI", Some(0))).unwrap();
    assert!(m.routable);
    let m = classify(&c, &usb("Midi-Bridge:Trill XIAO MIDI MIDI 2 (capture)", None, "Trill XIAO MIDI", Some(1))).unwrap();
    assert!(!m.routable);
    // Unknown cable on a class that restricts cables: not routed (safe default).
    let m = classify(&c, &usb("Midi-Bridge:Trill XIAO MIDI (capture)", None, "Trill XIAO MIDI", None)).unwrap();
    assert!(!m.routable);
}

#[test]
fn other_usb_devices_do_not_match() {
    let c = default_classes();
    assert!(classify(&c, &usb("Midi-Bridge:USB Composite Device MIDI 1 (capture)", Some("1915:1025"), "USB Composite Device", Some(0))).is_none());
    assert!(classify(&c, &usb("Midi-Bridge:Keystation 49 MIDI 1 (capture)", Some("0763:2027"), "Keystation 49", Some(0))).is_none());
    // A prefix must be a prefix, not a substring.
    assert!(classify(&c, &usb("Midi-Bridge:My Trill XIAO MIDI 1 (capture)", None, "My Trill XIAO", Some(0))).is_none());
}

#[test]
fn trillium_ble_by_bluez_alias() {
    let c = default_classes();
    let m = classify(&c, &ble(&["Trillium 7F2A"])).unwrap();
    assert_eq!((m.rule, m.routable), ("ble", true));
    assert!(classify(&c, &ble(&[])).is_none(), "'BLE MIDI 1' alone is not Trillium");
    assert!(classify(&c, &ble(&["Living Room Speaker"])).is_none());
    assert!(classify(&c, &ble(&["Trillium"])).is_none(), "glob needs 'Trillium <suffix>'");
    // A BLE node in client role named after the device.
    let id = SourceIdentity { transport: Transport::Ble, ..SourceIdentity::new("Trillium 7F2A:out") };
    assert!(classify(&c, &id).is_some());
}

#[test]
fn classes_match_in_order_and_are_configurable() {
    let mut c = vec![DeviceClass { id: "zero".into(), label: "Trillium Zero".into(), name_prefixes: vec!["Trillium Zero".into()], ..DeviceClass::default() }];
    c.extend(default_classes());
    let m = classify(&c, &usb("Midi-Bridge:Trillium Zero MIDI 1 (capture)", Some("1209:0003"), "Trillium Zero", Some(1))).unwrap();
    assert_eq!(c[m.class].id, "zero", "first matching class wins");
    assert!(m.routable, "no cable restriction on that class");
}

#[test]
fn device_routes_compile_to_matched_ports_only() {
    let mut ports: PortNames = Default::default();
    ports[1] = Some("Midi-Bridge:Trill XIAO MIDI MIDI 1 (capture)".into());
    ports[2] = Some("BLE MIDI 1:out".into());
    ports[3] = Some("Midi-Bridge:Keystation 49 (capture)".into());
    let mut devs: PortDevices = Default::default();
    devs[1] = Some("trillium".into());
    devs[2] = Some("trillium".into());
    let sc = Scene::trillium_sample();
    let t = compile_routes_with(&sc.routes, &ports, &devs);
    let hits = |port: u8, note: u8| {
        let mut h = [Hit::default(); MAX_LAYERS];
        let n = t.resolve(port, 0, note, &mut h);
        h[..n].iter().map(|x| (x.slot, x.note)).collect::<Vec<_>>()
    };
    assert_eq!(hits(1, 48), vec![(2, 36), (3, 48)], "low split an octave down plus layer");
    assert_eq!(hits(2, 72), vec![(1, 72), (3, 72)], "BLE Trillium plays the same map");
    assert!(hits(3, 60).is_empty(), "other controllers are not Trillium");
    // The old name glob does not find a BLE Trillium.
    let old = vec![Route { input: "*Trillium*".into(), slot: 1, ..Route::default() }];
    assert!(compile_routes_with(&old, &ports, &devs).is_empty());
}

#[test]
fn cable_numbers_from_names() {
    assert_eq!(cable_from_name("Midi-Bridge:Trill XIAO MIDI MIDI 1 (capture)"), Some(0));
    assert_eq!(cable_from_name("Midi-Bridge:Trill XIAO MIDI MIDI 2 (capture)"), Some(1));
    assert_eq!(cable_from_name("Midi-Bridge:X:(capture_1) X MIDI 2"), Some(1));
    assert_eq!(cable_from_name("BLE MIDI 1:out"), None);
}

#[test]
fn settings_carry_device_classes() {
    let s = Settings::default();
    assert_eq!(s.midi.devices[0].id, "trillium");
    assert_eq!(s.midi.devices[0].route_cables, vec![0]);
    let back = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
    assert_eq!(back, s);
    // Older settings files without [[midi.devices]] get the defaults.
    let old = Settings::from_toml("[midi]\nautoconnect = true\nignore = []\n").unwrap();
    assert_eq!(old.midi.devices, default_classes());
    let sc = Scene::trillium_sample();
    assert_eq!(Scene::from_toml(&sc.to_toml().unwrap()).unwrap(), sc);
}
