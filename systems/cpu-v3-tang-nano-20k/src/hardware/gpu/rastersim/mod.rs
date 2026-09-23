//! Standalone functional reference for the GPU geometry/raster pipeline.
//!
//! This module is deliberately independent of the `GpuCore` cycle model: it
//! computes coverage directly from triangle geometry (pure functions, no
//! cycle state machine) so that both the future rasterizer emu and the RTL
//! can be checked against an oracle that shares no logic with them.
//!
//! Pipeline: clip-space triangles (Q16.16) -> outcode/trivial accept-reject
//! -> homogeneous clip (near/far + guard band) -> rcp-based NDC -> viewport
//! transform and subpixel snap (s12.4) -> area/backface cull -> triangle
//! setup (edge coefficients, tile/quad AABB) -> tile-first reference
//! rasterizer -> RGB565 frame + coverage/performance statistics.

pub mod clip;
pub mod depth;
pub mod fixed;
pub mod raster;
pub mod scenes;
pub mod setup;

#[cfg(test)]
mod tests {
    use super::clip::{clip_triangle, ClipVertex};
    use super::fixed::{mul_stats_reset, mul_stats_snapshot, q16_from_rational as q, MulStat};
    use super::raster::rasterize;
    use super::scenes::{depth_precision, performance_scene, scenes};
    use super::setup::{SetupStats, SetupUnit, TriangleSetup};
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn output_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/rastersim")
    }

    /// Clip-stage counters for one scene.
    #[derive(Clone, Copy, Debug, Default)]
    struct ClipStats {
        accepted: u64,
        rejected: u64,
        clipped: u64,
        output_triangles: u64,
    }

    /// Runs the full pipeline over one scene and returns the setups.
    fn process(scene: &super::scenes::Scene) -> (Vec<TriangleSetup>, SetupStats, ClipStats) {
        let mut unit = SetupUnit::default();
        let mut clip_stats = ClipStats::default();
        let mut setups = Vec::new();
        for tri in &scene.triangles {
            let clipped = clip_triangle(tri);
            match super::clip::classify(tri) {
                super::clip::Classify::Accept => clip_stats.accepted += 1,
                super::clip::Classify::Reject => clip_stats.rejected += 1,
                super::clip::Classify::Clip => clip_stats.clipped += 1,
            }
            clip_stats.output_triangles += clipped.len() as u64;
            for c in clipped {
                if let Some(setup) = unit.setup_triangle(&c) {
                    setups.push(setup);
                }
            }
        }
        (setups, unit.stats, clip_stats)
    }

    #[test]
    fn reference_images_for_key_scenes() {
        let dir = output_dir();
        let wanted = ["diag-45", "shared-edge-quad", "clip-near-polygon"];
        for scene in scenes() {
            if !wanted.contains(&scene.name) {
                continue;
            }
            let (setups, ..) = process(&scene);
            assert!(!setups.is_empty(), "scene {} produced nothing", scene.name);
            let frame = rasterize(&setups);
            assert!(frame.stats.pixels_covered > 0);
            frame
                .write_ppm(&dir.join(format!("{}.ppm", scene.name)))
                .expect("write reference image");
        }
    }

    #[test]
    fn triangle_fifo_serializes() {
        let scene = scenes()
            .into_iter()
            .find(|s| s.name == "shared-edge-quad")
            .unwrap();
        let (setups, ..) = process(&scene);
        let mut text = String::new();
        for setup in &setups {
            text.push_str(&setup.fifo_line());
            text.push('\n');
        }
        assert_eq!(text.lines().count(), 2);
        assert!(text.starts_with("TRI 0 X "));
        std::fs::create_dir_all(output_dir()).unwrap();
        std::fs::write(output_dir().join("triangle-fifo.txt"), text).unwrap();
    }

    #[test]
    fn ab_ba_property_on_scenes() {
        // Clipping every scene with reversed winding must produce the same
        // vertex sets (backface culling happens later, in setup).
        let mut steps = 0u64;
        for scene in scenes() {
            for tri in &scene.triangles {
                let fwd = clip_triangle(tri);
                let mut rev = *tri;
                rev.reverse();
                let bwd = clip_triangle(&rev);
                assert_eq!(fwd.len(), bwd.len(), "scene {}", scene.name);
                let mut a: Vec<ClipVertex> = fwd.into_iter().flatten().collect();
                let mut b: Vec<ClipVertex> = bwd.into_iter().flatten().collect();
                a.sort_by_key(|v| (v.x, v.y, v.z, v.w));
                b.sort_by_key(|v| (v.x, v.y, v.z, v.w));
                // Fan triangulation depends on winding; compare the sets of
                // distinct vertices, which must be bit-identical (AB == BA).
                a.dedup();
                b.dedup();
                assert_eq!(a, b, "scene {}", scene.name);
                steps += 1;
                assert!(steps < 1_000_000);
            }
        }
    }

    #[test]
    fn performance_scene_stats_and_csv() {
        mul_stats_reset();
        let scene = performance_scene();
        let (setups, setup_stats, clip_stats) = process(&scene);
        let frame = rasterize(&setups);
        let s = frame.stats;
        let dir = output_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let mut csv = String::from("metric,value\n");
        let mut row = |name: &str, value: u64| {
            csv.push_str(&format!("{name},{value}\n"));
        };
        row("triangles_input", scene.triangles.len() as u64);
        row("clip_accepted", clip_stats.accepted);
        row("clip_rejected", clip_stats.rejected);
        row("clip_clipped", clip_stats.clipped);
        row("clip_output_triangles", clip_stats.output_triangles);
        row("triangles_emitted", s.triangles);
        row("culled_backface", setup_stats.culled_backface);
        row("culled_degenerate", setup_stats.culled_degenerate);
        row("culled_offscreen", setup_stats.culled_offscreen);
        row("tile_visits", s.tile_visits);
        row("quads_visited", s.quads_visited);
        row("pixels_tested", s.pixels_tested);
        row("pixels_covered", s.pixels_covered);
        std::fs::write(dir.join("performance.csv"), &csv).unwrap();
        println!("performance scene stats:\n{csv}");
        // Sanity bounds, also acting as the step limit.
        assert_eq!(s.triangles as usize, setups.len());
        assert!(s.pixels_tested < 100_000_000, "pixel budget exceeded");
        assert!(s.pixels_covered > 0);
    }

    #[test]
    fn multiplier_stats_csv() {
        mul_stats_reset();
        let scene = performance_scene();
        let (setups, ..) = process(&scene);
        let _ = rasterize(&setups);
        let stats = mul_stats_snapshot();
        let dir = output_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let mut file = std::fs::File::create(dir.join("mul-stats.csv")).unwrap();
        writeln!(
            file,
            "label,calls,max_abs_a,max_abs_b,max_abs_product,overrange_calls"
        )
        .unwrap();
        for (label, stat) in &stats {
            let MulStat {
                calls,
                max_abs_a,
                max_abs_b,
                max_abs_product,
                overrange_calls,
            } = stat;
            writeln!(
                file,
                "{label},{calls},{max_abs_a},{max_abs_b},{max_abs_product},{overrange_calls}"
            )
            .unwrap();
        }
        drop(file);
        println!("multiplier stats written: {} call sites", stats.len());
        assert!(!stats.is_empty());
    }

    #[test]
    fn depth_precision_csv() {
        let dir = output_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let mut text = String::from("variant,far_m,worst_gap_mm,avg_gap_mm_x1000,samples\n");
        for far_m in [150u64, 200] {
            for (name, reverse) in [("reverse", true), ("regular", false)] {
                let r = depth_precision(reverse, far_m * 1000);
                text.push_str(&format!(
                    "{name},{far_m},{},{},{}\n",
                    r.worst_gap_mm, r.average_gap_mm_x1000, r.samples
                ));
            }
        }
        std::fs::write(dir.join("depth-precision.csv"), text).unwrap();
    }

    #[test]
    fn rcp_vs_exact_snap_consistency_on_scenes() {
        // For every emitted setup vertex the rcp-based NDC must stay within
        // 1/32 px of exact division (checked inside setup tests per sweep);
        // here we assert snapped coordinates agree on all scene vertices,
        // under both LUT+lerp configurations.
        use super::setup::{ndc_exact, ndc_rcp, snap_s12_4};
        use crate::hardware::gpu::rastersim::clip::W_MIN_RAW;
        use crate::hardware::gpu::rastersim::fixed::{rcp_q16, set_rcp_mode, RcpMode};
        for (mode, bound) in [(RcpMode::Lerp256, 1), (RcpMode::Lerp128, 2)] {
            set_rcp_mode(mode);
            let mut worst = 0i32;
            let mut steps = 0u64;
            for scene in scenes() {
                for tri in &scene.triangles {
                    for v in tri {
                        let w_raw = v.w.max(W_MIN_RAW);
                        let (mag, shift) = rcp_q16(w_raw as u32);
                        let approx = i64::from(ndc_rcp(v.x, mag, shift));
                        let exact = ndc_exact(v.x, w_raw);
                        // Viewport path: high 17 significant bits (>> 14) of
                        // the s2.29 NDC feed the 18-bit multiply by the
                        // half-width. Scene inputs may sit outside the guard
                        // band (clip removes those first); skip them here.
                        if exact.abs() > (1 << 29) * 64 / 25 {
                            continue;
                        }
                        let hi_a = (approx >> 14).clamp(-131071, 131071);
                        let hi_e = (exact >> 14).clamp(-131071, 131071);
                        let x_a = snap_s12_4((hi_a * 200) << 1);
                        let x_e = snap_s12_4((hi_e * 200) << 1);
                        let diff = (i32::from(x_a) - i32::from(x_e)).abs();
                        worst = worst.max(diff);
                        assert!(
                            diff <= bound,
                            "{mode:?} scene {} vertex {v:?}: rcp snap {x_a} vs exact {x_e}",
                            scene.name
                        );
                        steps += 1;
                    }
                }
            }
            assert!(steps < 1_000_000);
            println!(
                "{mode:?} scene snap consistency: worst {worst} subpixel over {steps} vertices"
            );
        }
        set_rcp_mode(RcpMode::Lerp256);
    }

    #[test]
    fn q16_rational_basics() {
        assert_eq!(q(1, 4), 1 << 14);
    }

    #[test]
    fn post_clip_ndc_is_within_guard_band() {
        // Contract: after clip + divide, |ndc| <= 2.56 for every vertex, so
        // the NDC/viewport datapath fits 32-bit s2.30. Check all scenes plus
        // constructed cases straddling the guard-band boundary.
        use super::setup::ndc_exact;
        let limit = (1 << 29) * 64 / 25; // 2.56 in s2.29
        let epsilon = limit / 256 + 1; // rcp/interpolation slack
        let extra: Vec<[ClipVertex; 3]> = vec![
            // Triangle straddling the right guard plane (x = 2.56*w).
            [
                ClipVertex::new(q(2, 1), q(0, 1), q(1, 2), q(1, 1)),
                ClipVertex::new(q(3, 1), q(1, 1), q(1, 2), q(1, 1)),
                ClipVertex::new(q(2, 1), q(1, 1), q(1, 2), q(1, 1)),
            ],
            // Triangle straddling the top guard plane (y = -2.56*w).
            [
                ClipVertex::new(q(0, 1), q(-2, 1), q(1, 2), q(1, 1)),
                ClipVertex::new(q(1, 1), q(-3, 1), q(1, 2), q(1, 1)),
                ClipVertex::new(q(-1, 1), q(-2, 1), q(1, 2), q(1, 1)),
            ],
            // Vertex exactly on the guard plane (x = 64/25 * w, w = 25).
            [
                ClipVertex::new(q(64, 1), q(0, 1), q(25, 2), q(25, 1)),
                ClipVertex::new(q(1, 2), q(1, 1), q(1, 2), q(1, 1)),
                ClipVertex::new(q(1, 2), q(-1, 1), q(1, 2), q(1, 1)),
            ],
        ];
        let mut steps = 0u64;
        let mut check = |tri: &[ClipVertex; 3]| {
            for clipped in clip_triangle(tri) {
                for v in &clipped {
                    for component in [v.x, v.y] {
                        let ndc = ndc_exact(component, v.w);
                        assert!(
                            ndc.abs() <= limit + epsilon,
                            "ndc {ndc} out of guard band for {v:?}"
                        );
                        steps += 1;
                    }
                }
            }
        };
        for scene in scenes() {
            for tri in &scene.triangles {
                check(tri);
            }
        }
        for tri in &extra {
            check(tri);
        }
        assert!(steps < 1_000_000);
    }

    #[test]
    fn ppm_written_to_target() {
        let path: &Path = &output_dir().join("diag-45.ppm");
        if !path.exists() {
            reference_images_for_key_scenes();
        }
        assert!(path.exists());
    }
}
