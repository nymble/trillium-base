// Copyright (C) 2026 Paul Lambert
// SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial

use pilab_core::{presets, Scene};
use pilab_synth::Waveform;

#[test]
fn presets_are_unique_samplers_with_banks() {
    let all = presets::all();
    for (i, p) in all.iter().enumerate() {
        assert!(all[i + 1..].iter().all(|q| q.id != p.id), "duplicate {}", p.id);
        assert_eq!(p.sound.wave, Waveform::Sampler);
        assert!(!p.bank.is_empty());
        assert!(p.sound.level_db <= 0.0);
    }
    assert!(presets::find("starter-kit").unwrap().drum);
}

#[test]
fn samples_scene_validates_and_routes_drums_on_ch10() {
    let sc = Scene::samples_demo();
    sc.validate().unwrap();
    let kit = sc.slots.iter().find(|s| s.sample == "vcsl-starter-kit").unwrap();
    assert!(sc.routes.iter().any(|r| r.slot == kit.id && r.channel == 10));
    let toml = sc.to_toml().unwrap();
    let back = Scene::from_toml(&toml).unwrap();
    assert_eq!(back.slots[0].sample, "freepats-upright-piano-kw");
}

#[test]
fn bad_sample_ids_are_rejected() {
    let mut sc = Scene::samples_demo();
    sc.slots[0].sample = "../etc".into();
    assert!(sc.validate().is_err());
}

#[test]
fn trillium_kit_scene_routes_ch10_to_the_kit() {
    let sc = Scene::trillium_kit("full-kit");
    sc.validate().unwrap();
    assert_eq!(sc.name, "trillium kit");
    let r = sc.routes.iter().find(|r| r.channel == 10).unwrap();
    assert_eq!(sc.slots.iter().find(|s| s.id == r.slot).unwrap().sample, "vcsl-full-kit");
    let fallback = Scene::trillium_kit("nope");
    assert_eq!(fallback.slots[0].sample, "vcsl-starter-kit");
}
