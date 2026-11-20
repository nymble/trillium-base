// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
use pilab_core::{compile_routes, PortNames, Route, Scene, SceneStore, Settings};
use pilab_synth::{Hit, MAX_LAYERS, PORT_CONSOLE, PORT_VIRTUAL};

fn ports() -> PortNames {
    let mut p: PortNames = Default::default();
    p[PORT_VIRTUAL as usize] = Some("virtual".into());
    p[1] = Some("Midi-Bridge:Trillium Zero MIDI 1 (capture)".into());
    p[2] = Some("Midi-Bridge:Keystation 49 (capture)".into());
    p[PORT_CONSOLE as usize] = Some("console".into());
    p
}

fn hits(t: &pilab_synth::RouteTable, port: u8, ch0: u8, note: u8) -> Vec<(u8, u8)> {
    let mut h = [Hit::default(); MAX_LAYERS];
    let n = t.resolve(port, ch0, note, &mut h);
    h[..n].iter().map(|x| (x.slot, x.note)).collect()
}

#[test]
fn split_layer_channel_transpose() {
    let routes = vec![
        // Keyboard split at middle C: bass on slot 1 an octave down, lead on slot 2.
        Route { input: "*Keystation*".into(), note_hi: 59, transpose: -12, slot: 1, ..Route::default() },
        Route { input: "*Keystation*".into(), note_lo: 60, slot: 2, ..Route::default() },
        // Layer: the upper half also plays the pad.
        Route { input: "*Keystation*".into(), note_lo: 60, slot: 3, ..Route::default() },
        // Trillium channel 2 only plays slot 4.
        Route { input: "*trillium*".into(), channel: 2, slot: 4, ..Route::default() },
        Route { input: "virtual".into(), slot: 5, ..Route::default() },
        Route { enabled: false, input: "*".into(), slot: 6, ..Route::default() },
    ];
    let t = compile_routes(&routes, &ports());
    assert_eq!(hits(&t, 2, 0, 48), vec![(0, 36)]);
    assert_eq!(hits(&t, 2, 0, 64), vec![(1, 64), (2, 64)]);
    assert_eq!(hits(&t, 1, 1, 60), vec![(3, 60)]); // wire channel 1 = MIDI channel 2
    assert!(hits(&t, 1, 0, 60).is_empty());
    assert_eq!(hits(&t, PORT_VIRTUAL, 9, 70), vec![(4, 70)]);
    assert!(hits(&t, PORT_CONSOLE, 0, 70).is_empty());
}

#[test]
fn any_input_includes_console_and_transpose_clips() {
    let routes = vec![Route { input: "*".into(), transpose: 24, slot: 1, ..Route::default() }];
    let t = compile_routes(&routes, &ports());
    assert_eq!(hits(&t, PORT_CONSOLE, 0, 60), vec![(0, 84)]);
    assert!(hits(&t, 3, 0, 120).is_empty(), "transposed above 127 is dropped");
    assert!(hits(&t, pilab_synth::PORT_AUDITION, 0, 60).is_empty(), "audition port bypasses the map");
}

#[test]
fn unmatched_glob_compiles_to_nothing() {
    let routes = vec![Route { input: "*Nothing*".into(), slot: 1, ..Route::default() }];
    assert!(compile_routes(&routes, &ports()).is_empty());
}

#[test]
fn scene_round_trip() {
    let dir = std::env::temp_dir().join(format!("pilab-scenes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = SceneStore::new(&dir).unwrap();
    let mut sc = Scene::factory();
    sc.routes.push(Route { input: "*Trillium*".into(), channel: 3, note_lo: 36, note_hi: 72, transpose: -5, velocity_scale: 0.8, slot: 2, label: "tri split".into(), ..Route::default() });
    store.save(&sc, "live set").unwrap();
    let back = store.load("live set").unwrap();
    let mut want = sc.clone();
    want.name = "live set".into();
    assert_eq!(back, want);
    store.duplicate("live set", "live set 2").unwrap();
    assert!(store.duplicate("live set", "live set 2").is_err());
    let names: Vec<String> = store.list().into_iter().map(|i| i.name).collect();
    assert_eq!(names, vec!["live set".to_string(), "live set 2".to_string()]);
    store.delete("live set 2").unwrap();
    assert!(store.load("../etc/passwd").is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn validation_rejects_bad_scenes() {
    let mut sc = Scene::factory();
    sc.routes.push(Route { slot: 9, ..Route::default() });
    assert!(sc.validate().is_err());
    let mut sc = Scene::factory();
    sc.version = 99;
    assert!(sc.validate().is_err());
    let mut sc = Scene::factory();
    sc.slots[0].table = "../../secret".into();
    assert!(sc.validate().is_err());
    assert!(Scene::from_toml("format = \"other\"").is_err());
}

#[test]
fn settings_defaults_are_safe() {
    let s = Settings::default();
    assert!(!s.archive.enabled, "archival is off by default");
    assert!(!s.archive.configured);
    let back = Settings::from_toml(&s.to_toml().unwrap()).unwrap();
    assert_eq!(back, s);
    let mut on = s.clone();
    on.archive.enabled = true;
    assert_eq!(on.archive_status().0, "not-configured");
}
