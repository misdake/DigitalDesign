//! Rasterizer part-level cycle emulator, bit-exact against `rastersim`.
//!
//! The emu models pipeline timing, FIFO backpressure, and shared-device
//! arbitration without touching the framebuffer cache, the system, or the
//! `Module` wrapper (that is the RTL step). All values come from the same
//! leaf functions as the functional sim, so the event streams are identical
//! bit for bit; the emu's added value is per-scene cycle counts and
//! backpressure/arbitration behavior.
//!
//! Trace format (same style as `gpu/trace.rs`, own prefix):
//!
//! ```text
//! RAST <seq> SCENE <name>
//! RAST <seq> TRI <id> X .. Y .. Z .. AABB ..
//! RAST <seq> PREFETCH <tile>
//! RAST <seq> QUAD <tri> <x> <y> <mask>
//! RAST <seq> TILE_END <tile>
//! RAST <seq> DONE draw <triangles>
//! ```
//!
//! Comparison buckets per scene: `TRI`, `PREFETCH`, `QUAD`, `TILE_END`,
//! `DONE`; cross-kind interleaving is timing-dependent and not compared.

mod core;
#[cfg(test)]
mod rtl_tests;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;

pub use self::core::{RasterCore, RasterEvent};
use crate::hardware::gpu::rastersim::clip::{clip_triangle, ClipVertex};
use crate::hardware::gpu::rastersim::raster;
use crate::hardware::gpu::rastersim::setup::{SetupUnit, TriangleSetup};

/// Deterministic sink throttle patterns for backpressure injection.
#[derive(Clone, Copy, Debug)]
pub enum Throttle {
    /// Sink always ready.
    Always,
    /// Ready for `run` cycles, then stalled for `stall` cycles, periodically.
    Periodic { run: u32, stall: u32 },
    /// Ready on every third cycle.
    Third,
}

impl Throttle {
    fn ready(self, cycle: u64) -> bool {
        match self {
            Self::Always => true,
            Self::Periodic { run, stall } => cycle % u64::from(run + stall) < u64::from(run),
            Self::Third => cycle.is_multiple_of(3),
        }
    }
}

/// Drives one scene through the core with a fixed cycle order:
/// combine (arbitration) -> advance (clock edge, trace collection).
pub struct Harness {
    pub core: RasterCore,
    pub events: Vec<RasterEvent>,
    pub cycles: u64,
}

impl Harness {
    pub fn new() -> Self {
        Self {
            core: RasterCore::default(),
            events: Vec::new(),
            cycles: 0,
        }
    }

    /// Runs one scene to completion. Returns the cycle count; panics when
    /// `max_cycles` is exceeded (the house rule upper bound).
    pub fn run_scene(
        &mut self,
        name: &str,
        triangles: &[[ClipVertex; 3]],
        throttle: Throttle,
        max_cycles: u64,
    ) -> u64 {
        self.events.push(RasterEvent::Scene(name.to_string()));
        let mut input: VecDeque<[ClipVertex; 3]> = triangles.iter().copied().collect();
        let start = self.cycles;
        loop {
            let grants = self.core.combine();
            let sink_ready = throttle.ready(self.cycles);
            self.core
                .advance(grants, &mut input, sink_ready, &mut self.events);
            self.cycles += 1;
            if input.is_empty() && self.core.idle() {
                break;
            }
            assert!(
                self.cycles - start < max_cycles,
                "scene {name} exceeded {max_cycles} cycles"
            );
        }
        self.events
            .push(RasterEvent::DoneDraw(self.core.walked_triangles()));
        self.cycles - start
    }
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Reference stream from the functional sim.

/// Reference event bodies for one scene, built from `rastersim` (clip +
/// setup + the shared traversal). Bucket order within a kind matches the
/// emu by construction: triangles in emission order, tiles in walk order,
/// quads in per-tile scan order.
pub fn reference_events(name: &str, triangles: &[[ClipVertex; 3]]) -> Vec<RasterEvent> {
    let mut events = vec![RasterEvent::Scene(name.to_string())];
    let mut unit = SetupUnit::default();
    let mut setups: Vec<TriangleSetup> = Vec::new();
    for tri in triangles {
        for clipped in clip_triangle(tri) {
            if let Some(setup) = unit.setup_triangle(&clipped) {
                events.push(RasterEvent::Tri(setup));
                setups.push(setup);
            }
        }
    }
    for setup in &setups {
        // The three traversal callbacks share the event sink through a cell.
        let cell = std::cell::RefCell::new(Vec::new());
        raster::traverse(
            &[*setup],
            |_, tile| cell.borrow_mut().push(RasterEvent::Prefetch(tile.index)),
            |setup, tile, quad| {
                if quad.mask != 0 {
                    cell.borrow_mut()
                        .push(RasterEvent::Quad(core::QuadItem::Quad {
                            tri: setup.id,
                            tile: tile.index,
                            x: quad.x,
                            y: quad.y,
                            mask: quad.mask,
                        }));
                }
            },
            |_, tile| {
                cell.borrow_mut()
                    .push(RasterEvent::Quad(core::QuadItem::TileEnd {
                        tile: tile.index,
                    }));
            },
        );
        events.extend(cell.into_inner());
    }
    events.push(RasterEvent::DoneDraw(setups.len() as u32));
    events
}

// ---------------------------------------------------------------------------
// Trace comparison: per scene, bucketed by event kind; cross-kind
// interleaving is timing-dependent and intentionally not compared.

/// Compares an emu trace against the reference (expected) trace. Per scene,
/// the per-kind event sequences must match exactly.
pub fn compare_traces(expected: &[RasterEvent], actual: &[RasterEvent]) -> Result<(), String> {
    let bucket = |events: &[RasterEvent]| -> Vec<(String, Vec<String>)> {
        let mut scenes: Vec<(String, Vec<String>)> = Vec::new();
        for event in events {
            let body = event.body();
            if body.starts_with("SCENE") {
                scenes.push((body, Vec::new()));
            } else if let Some(scene) = scenes.last_mut() {
                scene.1.push(body);
            }
        }
        scenes
    };
    let expected = bucket(expected);
    let actual = bucket(actual);
    if expected.len() != actual.len() {
        return Err(format!(
            "scene count mismatch: expected {} scenes, emu produced {}",
            expected.len(),
            actual.len()
        ));
    }
    const KINDS: [&str; 5] = ["TRI", "PREFETCH", "QUAD", "TILE_END", "DONE"];
    for ((e_name, e_events), (a_name, a_events)) in expected.iter().zip(&actual) {
        if e_name != a_name {
            return Err(format!(
                "scene mismatch: expected '{e_name}', emu produced '{a_name}'"
            ));
        }
        for kind in KINDS {
            let e: Vec<&String> = e_events.iter().filter(|b| b.starts_with(kind)).collect();
            let a: Vec<&String> = a_events.iter().filter(|b| b.starts_with(kind)).collect();
            for index in 0..e.len().max(a.len()) {
                let matches = match (e.get(index), a.get(index)) {
                    (Some(e), Some(a)) => e == a,
                    _ => false,
                };
                if !matches {
                    return Err(format!(
                        "first divergence: scene '{e_name}' kind {kind} event {index}\n  \
                         expected (sim): {}\n  \
                         actual   (emu): {}",
                        e.get(index).map_or("<none>", |s| s.as_str()),
                        a.get(index).map_or("<none>", |s| s.as_str()),
                    ));
                }
            }
        }
    }
    Ok(())
}
