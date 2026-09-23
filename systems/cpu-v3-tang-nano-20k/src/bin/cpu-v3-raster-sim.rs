//! Standalone reference-rasterizer image and statistics dump.
//!
//! Runs the rastersim functional pipeline (clip -> setup -> rasterize) over
//! the scene registry, writes one PPM per scene, the triangle FIFO text
//! serialization, and the performance/multiplier/depth CSVs. Output goes to
//! `target/rastersim/` or the directory given as the first argument.

use cpu_v3_tang_nano_20k::hardware::rastersim::clip::{classify, clip_triangle, Classify};
use cpu_v3_tang_nano_20k::hardware::rastersim::fixed::{mul_stats_reset, mul_stats_snapshot};
use cpu_v3_tang_nano_20k::hardware::rastersim::raster::rasterize;
use cpu_v3_tang_nano_20k::hardware::rastersim::scenes::{
    depth_precision, performance_scene, scenes,
};
use cpu_v3_tang_nano_20k::hardware::rastersim::setup::SetupUnit;
use std::io::Write;
use std::path::PathBuf;
use std::{env, fs};

fn main() {
    let dir = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/rastersim"));
    fs::create_dir_all(&dir).expect("create output directory");

    for scene in scenes() {
        let mut unit = SetupUnit::default();
        let mut setups = Vec::new();
        let mut fifo = String::new();
        for tri in &scene.triangles {
            for clipped in clip_triangle(tri) {
                if let Some(setup) = unit.setup_triangle(&clipped) {
                    fifo.push_str(&setup.fifo_line());
                    fifo.push('\n');
                    setups.push(setup);
                }
            }
        }
        if setups.is_empty() {
            println!("scene {:40} (no coverage)", scene.name);
            continue;
        }
        let frame = rasterize(&setups);
        frame
            .write_ppm(&dir.join(format!("{}.ppm", scene.name)))
            .expect("write PPM");
        fs::write(dir.join(format!("{}.tri", scene.name)), fifo).expect("write FIFO trace");
        println!(
            "scene {:40} triangles {} covered {}",
            scene.name, frame.stats.triangles, frame.stats.pixels_covered
        );
    }

    // Performance scene statistics.
    mul_stats_reset();
    let scene = performance_scene();
    let mut unit = SetupUnit::default();
    let mut setups = Vec::new();
    let mut clip_accepted = 0u64;
    let mut clip_rejected = 0u64;
    let mut clip_clipped = 0u64;
    let mut clip_output = 0u64;
    for tri in &scene.triangles {
        match classify(tri) {
            Classify::Accept => clip_accepted += 1,
            Classify::Reject => clip_rejected += 1,
            Classify::Clip => clip_clipped += 1,
        }
        for clipped in clip_triangle(tri) {
            clip_output += 1;
            if let Some(setup) = unit.setup_triangle(&clipped) {
                setups.push(setup);
            }
        }
    }
    let frame = rasterize(&setups);
    let s = frame.stats;
    let mut csv = String::from("metric,value\n");
    for (name, value) in [
        ("triangles_input", scene.triangles.len() as u64),
        ("clip_accepted", clip_accepted),
        ("clip_rejected", clip_rejected),
        ("clip_clipped", clip_clipped),
        ("clip_output_triangles", clip_output),
        ("triangles_emitted", s.triangles),
        ("culled_backface", unit.stats.culled_backface),
        ("culled_degenerate", unit.stats.culled_degenerate),
        ("culled_offscreen", unit.stats.culled_offscreen),
        ("tile_visits", s.tile_visits),
        ("quads_visited", s.quads_visited),
        ("pixels_tested", s.pixels_tested),
        ("pixels_covered", s.pixels_covered),
    ] {
        csv.push_str(&format!("{name},{value}\n"));
    }
    fs::write(dir.join("performance.csv"), &csv).expect("write performance CSV");
    print!("performance scene:\n{csv}");

    // Multiplier operand statistics.
    let mut file = fs::File::create(dir.join("mul-stats.csv")).expect("create mul-stats CSV");
    writeln!(
        file,
        "label,calls,max_abs_a,max_abs_b,max_abs_product,overrange_calls"
    )
    .unwrap();
    for (label, stat) in &mul_stats_snapshot() {
        writeln!(
            file,
            "{label},{},{},{},{},{}",
            stat.calls, stat.max_abs_a, stat.max_abs_b, stat.max_abs_product, stat.overrange_calls
        )
        .unwrap();
    }

    // Depth precision experiment.
    let mut text = String::from("variant,far_m,worst_gap_mm,avg_gap_mm_x1000,samples\n");
    for far_m in [150u64, 200] {
        for (name, reverse) in [("reverse", true), ("regular", false)] {
            let r = depth_precision(reverse, far_m * 1000);
            text.push_str(&format!(
                "{name},{far_m},{},{},{}\n",
                r.worst_gap_mm, r.average_gap_mm_x1000, r.samples
            ));
            println!(
                "depth far={far_m}m {name}: worst {} mm avg {}.{:03} mm",
                r.worst_gap_mm,
                r.average_gap_mm_x1000 / 1000,
                r.average_gap_mm_x1000 % 1000,
            );
        }
    }
    fs::write(dir.join("depth-precision.csv"), text).expect("write depth CSV");
}
