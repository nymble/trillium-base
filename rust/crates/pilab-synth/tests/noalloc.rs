// Copyright (C) 2026 Nymble
// SPDX-License-Identifier: GPL-3.0-or-later
//! The render path must not allocate or free.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use pilab_synth::*;

struct Counting;
static ARMED: AtomicBool = AtomicBool::new(false);
static COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ARMED.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if ARMED.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
        }
        System.dealloc(p, l)
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn process_midi_and_apply_do_not_allocate() {
    let mut e = Engine::new(48_000.0);
    let cycle: Vec<f32> = (0..600).map(|i| (i as f32 / 600.0 * 6.283).sin()).collect();
    let table = std::sync::Arc::new(Wavetable::from_cycle("t", &cycle).unwrap());
    let mut rt = Box::new(RouteTable::new());
    rt.push(CompiledRoute { port_mask: u32::MAX, slot: 0, ..Default::default() });
    rt.push(CompiledRoute { port_mask: u32::MAX, slot: 1, ..Default::default() });
    rt.push(CompiledRoute { port_mask: u32::MAX, slot: 2, ..Default::default() });
    let smp: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.0575).sin()).collect();
    let mut z = sampler::Zone::full(0, 4800, 60.0);
    z.loop_mode = sampler::LoopMode::Sustain;
    z.loop_start = 1000;
    z.loop_end = 3000;
    let mut z2 = sampler::Zone::full(0, 4800, 48.0);
    z2.seq_length = 2;
    z2.seq_position = 2;
    z2.group = 1;
    z2.off_by = 1;
    let inst = std::sync::Arc::new(sampler::instrument(
        "t",
        vec![sampler::Sample { data: smp, channels: 1, frames: 4800, rate: 44_100.0 }],
        vec![z, z2],
    ));
    let slot_sampler = SlotParams { wave: Waveform::Sampler, ..Default::default() };
    let slot_table = SlotParams { wave: Waveform::Table, ..Default::default() };
    let (mut l, mut r) = (vec![0f32; 1024], vec![0f32; 1024]);
    let cmds = vec![
        Cmd::SetSlot(1, slot_table),
        Cmd::SetTable(1, Some(table)),
        Cmd::SetSlot(2, slot_sampler),
        Cmd::SetInstrument(2, Some(inst)),
        Cmd::SetRoutes(rt),
        Cmd::Run(true),
        Cmd::SetMaster { db: -12.0 },
    ];
    let mut garbage = Vec::with_capacity(8);
    let mut it = cmds.into_iter();
    ARMED.store(true, Ordering::SeqCst);
    for c in it.by_ref() {
        if let Some(g) = e.apply(c) {
            garbage.push(g);
        }
    }
    for n in 0..40u8 {
        e.midi(1, &[0x90, 30 + n, 100]);
        e.midi(1, &[0xE0, n, 0x40]);
        e.midi(1, &[0xB0, 1, n]);
        e.process(&mut l, &mut r);
        e.midi(1, &[0x80, 30 + n, 0]);
    }
    e.apply(Cmd::Panic);
    e.process(&mut l, &mut r);
    ARMED.store(false, Ordering::SeqCst);
    assert_eq!(COUNT.load(Ordering::SeqCst), 0, "allocations in the realtime path");
    drop(it);
    drop(garbage);
}
