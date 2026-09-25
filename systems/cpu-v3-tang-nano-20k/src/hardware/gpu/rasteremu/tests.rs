//! Emu-vs-rastersim differential tests plus backpressure/arbitration
//! focus tests. Every test locks the rcp mode to Lerp256 (thread-local) and
//! every run carries a cycle upper bound (house rule).

use super::*;
use crate::hardware::gpu::rastersim::fixed::{set_rcp_mode, RcpMode};
use crate::hardware::gpu::rastersim::raster::rasterize;
use crate::hardware::gpu::rastersim::scenes::{performance_scene, scenes};

/// Per-scene cycle budget for the small scenes.
const MAX_SCENE_CYCLES: u64 = 2_000_000;
/// Budget for the performance-subset run.
const MAX_PERF_CYCLES: u64 = 5_000_000;

/// Reconstructs the coverage set from an emu QUAD stream.
fn emu_coverage(events: &[RasterEvent]) -> Vec<bool> {
    let width = crate::FRAMEBUFFER_WIDTH as usize;
    let height = crate::FRAMEBUFFER_HEIGHT as usize;
    let mut covered = vec![false; width * height];
    for event in events {
        if let RasterEvent::Quad(core::QuadItem::Quad { x, y, mask, .. }) = event {
            for dy in 0..2 {
                for dx in 0..2 {
                    if mask & (1 << (dy * 2 + dx)) != 0 {
                        let px = (x + dx) as usize;
                        let py = (y + dy) as usize;
                        covered[py * width + px] = true;
                    }
                }
            }
        }
    }
    covered
}

/// Runs one scene through both sides and compares everything: TRI records,
/// QUAD/TILE_END/PREFETCH sequences, DONE, and the coverage mask.
fn check_scene(name: &str, triangles: &[[ClipVertex; 3]], throttle: Throttle) -> u64 {
    let reference = reference_events(name, triangles);
    let mut harness = Harness::default();
    let cycles = harness.run_scene(name, triangles, throttle, MAX_SCENE_CYCLES);
    compare_traces(&reference, &harness.events).expect("emu/sim trace divergence");
    // Coverage mask from the emu quad stream equals the reference frame.
    let setups: Vec<TriangleSetup> = reference
        .iter()
        .filter_map(|event| match event {
            RasterEvent::Tri(setup) => Some(*setup),
            _ => None,
        })
        .collect();
    let frame = rasterize(&setups);
    assert_eq!(
        emu_coverage(&harness.events),
        frame.covered,
        "coverage mask diverges in scene {name}"
    );
    cycles
}

#[test]
fn differential_all_scenes() {
    set_rcp_mode(RcpMode::Lerp256);
    let _guard = crate::hardware::gpu::rastersim::fixed::stats_lock()
        .lock()
        .unwrap();
    for scene in scenes() {
        let cycles = check_scene(scene.name, &scene.triangles, Throttle::Always);
        println!("emu scene {:40} {cycles} cycles", scene.name);
    }
}

#[test]
fn differential_performance_subset() {
    set_rcp_mode(RcpMode::Lerp256);
    let _guard = crate::hardware::gpu::rastersim::fixed::stats_lock()
        .lock()
        .unwrap();
    let scene = performance_scene();
    let subset = &scene.triangles[..400];
    let reference = reference_events("perf-subset", subset);
    let mut harness = Harness::default();
    let cycles = harness.run_scene("perf-subset", subset, Throttle::Always, MAX_PERF_CYCLES);
    compare_traces(&reference, &harness.events).expect("emu/sim trace divergence (perf subset)");
    println!("emu perf-subset (400 triangles): {cycles} cycles");
}

#[test]
fn backpressure_sequences_are_bit_identical() {
    set_rcp_mode(RcpMode::Lerp256);
    let all = scenes();
    let representative = [
        "shared-edge-quad",
        "clip-near-polygon",
        "clip-multi-consecutive",
        "diag-crossing-tiles",
    ];
    let throttles = [
        Throttle::Periodic { run: 3, stall: 2 },
        Throttle::Periodic { run: 1, stall: 5 },
        Throttle::Third,
    ];
    for name in representative {
        let scene = all.iter().find(|s| s.name == name).unwrap();
        let reference = reference_events(name, &scene.triangles);
        let mut baseline = Harness::default();
        baseline.run_scene(name, &scene.triangles, Throttle::Always, MAX_SCENE_CYCLES);
        compare_traces(&reference, &baseline.events).unwrap();
        for throttle in throttles {
            let mut harness = Harness::default();
            let cycles = harness.run_scene(name, &scene.triangles, throttle, MAX_SCENE_CYCLES);
            // Bucketed comparison: per-kind sequences are bit-identical;
            // cross-kind interleaving is timing-dependent by design.
            compare_traces(&baseline.events, &harness.events)
                .expect("backpressure changed the event sequence");
            println!("scene {name} throttle {throttle:?}: {cycles} cycles");
        }
    }
}

#[test]
fn arbitration_under_clip_load_preserves_values() {
    set_rcp_mode(RcpMode::Lerp256);
    // Clip-heavy scenes force RcpUnit/Multiplier36x18 contention between the
    // clip and setup stages; the streams must stay bit-exact regardless.
    let all = scenes();
    for name in [
        "clip-near-polygon",
        "clip-near-two-out",
        "clip-multi-consecutive",
        "clip-guard-band-only",
        "clip-near-sliver",
    ] {
        let scene = all.iter().find(|s| s.name == name).unwrap();
        check_scene(
            name,
            &scene.triangles,
            Throttle::Periodic { run: 2, stall: 1 },
        );
    }
}

#[test]
fn trace_render_format() {
    let events = [
        RasterEvent::Scene("A".to_string()),
        RasterEvent::Prefetch(7),
        RasterEvent::Quad(core::QuadItem::TileEnd { tile: 7 }),
        RasterEvent::DoneDraw(3),
    ];
    let bodies: Vec<String> = events.iter().map(RasterEvent::body).collect();
    assert_eq!(
        bodies,
        vec!["SCENE A", "PREFETCH 7", "TILE_END 7", "DONE draw 3"]
    );
}
