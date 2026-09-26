//! Part-level raster RTL differential against the independent functional
//! reference. Every scene has a bounded RTL watchdog; value comparison is
//! per event kind because FIFO timing changes cross-kind interleaving.

use super::reference_events;
use crate::hardware::gpu::rastersim::fixed::{S12_4, U0_18};
use crate::hardware::gpu::rastersim::raster;
use crate::hardware::gpu::rastersim::scenes::{scenes, Scene};
use crate::hardware::gpu::rastersim::setup::{self, TriangleSetup};
use std::cell::RefCell;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_RASTER_RUN: AtomicU64 = AtomicU64::new(0);

fn vectors(selected: &[&Scene]) -> String {
    let mut text = String::new();
    for scene in selected {
        text.push_str(&format!("    scene_begin(\"{}\");\n", scene.name));
        for (index, tri) in scene.triangles.iter().enumerate() {
            let raw = |value: i64| format!("32'h{:08x}", value as u32);
            let vertices = tri.iter().flat_map(|v| [v.x, v.y, v.z, v.w]);
            let args = vertices
                .map(|v| raw(v.raw()))
                .collect::<Vec<_>>()
                .join(", ");
            let last = if index + 1 == scene.triangles.len() {
                "1'b1"
            } else {
                "1'b0"
            };
            text.push_str(&format!("    drive_tri({args}, {last});\n"));
        }
    }
    text
}

fn run_rtl_source(vectors: &str, testbench: &str, throttle: u8) -> String {
    run_rtl_source_mode(vectors, testbench, throttle, 2, false)
}

fn run_rtl_source_mode(
    vectors: &str,
    testbench: &str,
    throttle: u8,
    pixels: u8,
    scanline: bool,
) -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = manifest.join("src/hardware/gpu/raster");
    // The candidate matrix and individual viewport tests run concurrently in
    // aggregate validation, even with the same testbench and stall pattern.
    let run = NEXT_RASTER_RUN.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "raster-cosim-{}-{run}-{testbench}-{throttle}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("raster_vectors.vh"), vectors).unwrap();
    let output = directory.join("raster.vvp");
    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let compile = Command::new(iverilog)
        .current_dir(&directory)
        .args(["-g2012", "-DRASTER_COSIM", "-s", "tb", "-I"])
        .arg(&directory)
        .arg(format!("-Ptb.PIXELS_PER_CYCLE={pixels}"))
        .arg(format!("-Ptb.SCANLINE={}", u8::from(scanline)))
        .arg("-o")
        .arg(&output)
        .arg(source.join("raster.v"))
        .arg(source.join("raster_pixel.v"))
        .arg(source.join(testbench))
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "Icarus compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let simulation = Command::new(vvp)
        .current_dir(&directory)
        .arg(&output)
        .arg(format!("+THROTTLE={throttle}"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&simulation.stdout).into_owned();
    assert!(
        simulation.status.success() && stdout.lines().any(|line| line == "DIGITAL_DESIGN_PASS"),
        "RTL simulation failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&simulation.stderr)
    );
    std::fs::remove_dir_all(directory).ok();
    stdout
}

fn run_rtl(selected: &[&Scene], throttle: u8) -> String {
    run_rtl_source(&vectors(selected), "raster_tb.v", throttle)
}

fn assert_bucket(scene: &str, kind: &str, expected: &[String], actual: &[String]) {
    for index in 0..expected.len().max(actual.len()) {
        if expected.get(index) != actual.get(index) {
            panic!(
                "first divergence: scene {scene}, kind {kind}, event {index}\n  expected: {}\n  RTL: {}",
                expected.get(index).map_or("<none>", String::as_str),
                actual.get(index).map_or("<none>", String::as_str)
            );
        }
    }
}

fn parse_rtl_scenes(stdout: &str) -> Vec<(String, Vec<String>, Vec<String>)> {
    let mut rtl_scenes: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("RAST ") {
            let (_, body) = rest.split_once(' ').expect("trace sequence");
            if let Some(name) = body.strip_prefix("SCENE ") {
                rtl_scenes.push((name.to_string(), Vec::new(), Vec::new()));
            } else {
                rtl_scenes
                    .last_mut()
                    .expect("event before scene")
                    .1
                    .push(body.into());
            }
        } else if let Some(body) = line.strip_prefix("RAST_OUT ") {
            rtl_scenes
                .last_mut()
                .expect("output before scene")
                .2
                .push(body.into());
        }
    }
    rtl_scenes
}

fn check(selected: &[&Scene], throttle: u8) {
    let stdout = run_rtl(selected, throttle);
    let rtl_scenes = parse_rtl_scenes(&stdout);
    assert_eq!(rtl_scenes.len(), selected.len(), "scene count");
    for (scene, (name, events, output)) in selected.iter().zip(&rtl_scenes) {
        assert_eq!(name, scene.name);
        let expected: Vec<String> = reference_events(scene.name, &scene.triangles)
            .iter()
            .map(|event| event.body())
            .filter(|body| !body.starts_with("SCENE "))
            .collect();
        let markers: Vec<String> = (0..scene.triangles.len())
            .map(|index| {
                format!(
                    "RETIRE_MARKER {index} DRAW {}",
                    u8::from(index + 1 == scene.triangles.len())
                )
            })
            .collect();
        for kind in [
            "TRI",
            "PREFETCH",
            "QUAD",
            "TILE_END",
            "RETIRE_MARKER",
            "DONE",
        ] {
            let e: Vec<String> = if kind == "RETIRE_MARKER" {
                markers.clone()
            } else {
                expected
                    .iter()
                    .filter(|body| body.starts_with(kind))
                    .cloned()
                    .collect()
            };
            let a: Vec<String> = events
                .iter()
                .filter(|body| body.starts_with(kind))
                .cloned()
                .collect();
            assert_bucket(name, kind, &e, &a);
        }
        let raster_output: Vec<String> = expected
            .iter()
            .filter(|body| body.starts_with("QUAD") || body.starts_with("TILE_END"))
            .cloned()
            .collect();
        let mut expected_output = Vec::new();
        let mut offset = 0;
        for (index, tri) in scene.triangles.iter().enumerate() {
            let count = reference_events(scene.name, &[*tri])
                .iter()
                .filter(|event| {
                    let body = event.body();
                    body.starts_with("QUAD") || body.starts_with("TILE_END")
                })
                .count();
            expected_output.extend_from_slice(&raster_output[offset..offset + count]);
            expected_output.push(markers[index].clone());
            offset += count;
        }
        assert_eq!(offset, raster_output.len(), "reference output partition");
        assert_bucket(name, "output stream", &expected_output, output);
    }
}

#[test]
#[ignore = "explicit raster RTL co-simulation against Icarus"]
fn raster_all_scenes_match_reference() {
    let all = scenes();
    check(&all.iter().collect::<Vec<_>>(), 0);
}

#[test]
#[ignore = "explicit raster RTL co-simulation with output backpressure"]
fn raster_backpressure_preserves_stream() {
    let all = scenes();
    let selected: Vec<&Scene> = all
        .iter()
        .filter(|scene| {
            [
                "shared-edge-quad",
                "clip-near-polygon",
                "clip-multi-consecutive",
                "diag-crossing-tiles",
            ]
            .contains(&scene.name)
        })
        .collect();
    assert_eq!(selected.len(), 4);
    check(&selected, 1);
    check(&selected, 2);
}

type ViewportTriangle = [[i16; 2]; 3];

struct ViewportCase {
    name: String,
    triangles: Vec<ViewportTriangle>,
}

fn viewport_cases() -> Vec<ViewportCase> {
    let mut cases = Vec::new();
    for scene in scenes() {
        let triangles: Vec<ViewportTriangle> = reference_events(scene.name, &scene.triangles)
            .into_iter()
            .filter_map(|event| match event {
                super::RasterEvent::Tri(setup) => Some(std::array::from_fn(|i| {
                    [setup.x[i].raw() as i16, setup.y[i].raw() as i16]
                })),
                _ => None,
            })
            .collect();
        if !triangles.is_empty() {
            cases.push(ViewportCase {
                name: scene.name.into(),
                triangles,
            });
        }
    }
    cases.extend([
        ViewportCase {
            name: "viewport-degenerate".into(),
            triangles: vec![[[1600, 1600]; 3]],
        },
        ViewportCase {
            name: "viewport-backface".into(),
            triangles: vec![[[2400, 1440], [3200, 2400], [4000, 1440]]],
        },
        ViewportCase {
            name: "viewport-offscreen".into(),
            triangles: vec![[[8000, 1600], [9600, 1600], [8800, 3200]]],
        },
        ViewportCase {
            name: "viewport-subpixel-sliver".into(),
            triangles: vec![[[160, 160], [161, 160], [160, 161]]],
        },
        ViewportCase {
            name: "span-offset-shared-edge".into(),
            triangles: vec![
                [[54, 86], [3094, 86], [3094, 1846]],
                [[54, 86], [3094, 1846], [54, 1846]],
            ],
        },
        ViewportCase {
            name: "span-bottom-right-sliver".into(),
            triangles: vec![[[6241, 3745], [6400, 3839], [6240, 3840]]],
        },
        ViewportCase {
            name: "span-guard-band".into(),
            triangles: vec![[[-8192, -8192], [8192, 96], [96, 8192]]],
        },
    ]);
    // Reproducible small triangles exercise prefix offsets, partial tiles,
    // and both increasing/decreasing edges without relying on the demo.
    let mut seed = 0x7193_a0e5u32;
    for index in 0..12 {
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed
        };
        let base_x = (next() % 5500) as i16 - 256;
        let base_y = (next() % 3000) as i16 - 256;
        let mut triangle = std::array::from_fn(|_| {
            [
                base_x + (next() % 1500) as i16,
                base_y + (next() % 1100) as i16,
            ]
        });
        let a = triangle.map(|[x, y]| [i64::from(x), i64::from(y)]);
        if (a[1][0] - a[0][0]) * (a[2][1] - a[0][1]) - (a[2][0] - a[0][0]) * (a[1][1] - a[0][1]) < 0
        {
            triangle.swap(1, 2);
        }
        cases.push(ViewportCase {
            name: format!("span-seeded-{index}"),
            triangles: vec![triangle],
        });
    }
    cases
}

fn viewport_setup(vertices: &ViewportTriangle, id: u32) -> Option<TriangleSetup> {
    let x = std::array::from_fn(|i| S12_4::from_raw(i64::from(vertices[i][0])));
    let y = std::array::from_fn(|i| S12_4::from_raw(i64::from(vertices[i][1])));
    let area2 = setup::area2(&x, &y);
    if area2 <= setup::area2(&[S12_4::zero(); 3], &[S12_4::zero(); 3]) {
        return None;
    }
    let (cx, cy, top_left) = setup::edge_coefficients(&x, &y);
    let quad_aabb = setup::quad_aabb(&x, &y)?;
    Some(TriangleSetup {
        id,
        x,
        y,
        depth: [U0_18::zero(); 3],
        cx,
        cy,
        top_left,
        area2,
        quad_aabb,
    })
}

fn viewport_vectors(cases: &[ViewportCase]) -> String {
    let mut text = String::new();
    for case in cases {
        text.push_str(&format!("    scene_begin(\"{}\");\n", case.name));
        for (index, tri) in case.triangles.iter().enumerate() {
            let packed = tri.map(|[x, y]| (u32::from(y as u16) << 16) | u32::from(x as u16));
            text.push_str(&format!(
                "    drive_tri(32'h{:08x}, 32'h{:08x}, 32'h{:08x}, 1'b{});\n",
                packed[0],
                packed[1],
                packed[2],
                u8::from(index + 1 == case.triangles.len())
            ));
        }
    }
    text
}

fn viewport_expected(case: &ViewportCase) -> (Vec<String>, Vec<String>) {
    let mut trace = Vec::new();
    let mut output = Vec::new();
    let mut next_id = 0;
    for (index, vertices) in case.triangles.iter().enumerate() {
        if let Some(setup) = viewport_setup(vertices, next_id) {
            next_id += 1;
            trace.push(setup.fifo_line());
            let events = RefCell::new(Vec::new());
            raster::traverse(
                &[setup],
                |_, tile| events.borrow_mut().push(format!("PREFETCH {}", tile.index)),
                |setup, _, quad| {
                    if quad.mask != 0 {
                        events.borrow_mut().push(format!(
                            "QUAD {} {} {} {:x}",
                            setup.id, quad.x, quad.y, quad.mask
                        ));
                    }
                },
                |_, tile| events.borrow_mut().push(format!("TILE_END {}", tile.index)),
            );
            for event in events.into_inner() {
                if event.starts_with("QUAD") || event.starts_with("TILE_END") {
                    output.push(event.clone());
                }
                trace.push(event);
            }
        }
        let marker = format!(
            "RETIRE_MARKER {index} DRAW {}",
            u8::from(index + 1 == case.triangles.len())
        );
        trace.push(marker.clone());
        output.push(marker);
    }
    trace.push(format!("DONE draw {next_id}"));
    (trace, output)
}

fn viewport_pixels_expected(case: &ViewportCase) -> Vec<String> {
    let mut output = Vec::new();
    let mut next_id = 0;
    for (index, vertices) in case.triangles.iter().enumerate() {
        if let Some(setup) = viewport_setup(vertices, next_id) {
            next_id += 1;
            raster::traverse(
                &[setup],
                |_, _| {},
                |setup, tile, quad| {
                    for lane in 0..4 {
                        if quad.mask & (1 << lane) != 0 {
                            let x = quad.x + (lane & 1);
                            let y = quad.y + (lane >> 1);
                            let color = raster::pixel_color(x as u32, y as u32, setup.id);
                            output.push(format!(
                                "PIXEL {} {} {x} {y} {color:04x}",
                                setup.id, tile.index
                            ));
                        }
                    }
                },
                |_, _| {},
            );
        }
        output.push(format!(
            "RETIRE_MARKER {index} DRAW {}",
            u8::from(index + 1 == case.triangles.len())
        ));
    }
    output
}

fn check_viewport(cases: &[ViewportCase], throttle: u8) {
    let stdout = run_rtl_source(&viewport_vectors(cases), "raster_viewport_tb.v", throttle);
    let actual = parse_rtl_scenes(&stdout);
    assert_eq!(actual.len(), cases.len(), "viewport scene count");
    for (case, (name, trace, output)) in cases.iter().zip(&actual) {
        assert_eq!(name, &case.name);
        let (expected_trace, expected_output) = viewport_expected(case);
        for kind in [
            "TRI",
            "PREFETCH",
            "QUAD",
            "TILE_END",
            "RETIRE_MARKER",
            "DONE",
        ] {
            let e: Vec<String> = expected_trace
                .iter()
                .filter(|event| event.starts_with(kind))
                .cloned()
                .collect();
            let a: Vec<String> = trace
                .iter()
                .filter(|event| event.starts_with(kind))
                .cloned()
                .collect();
            assert_bucket(name, kind, &e, &a);
        }
        assert_bucket(name, "output stream", &expected_output, output);
    }
}

#[test]
#[ignore = "explicit stage-5 viewport raster RTL co-simulation"]
fn viewport_raster_matches_reference() {
    check_viewport(&viewport_cases(), 0);
}

#[test]
#[ignore = "explicit tile/scanline resource and throughput candidate differential"]
fn viewport_raster_candidate_matrix() {
    let cases = viewport_cases();
    let mut csv = String::from("traversal,pixels_per_cycle,throttle,scene,cycles,quads,visited,peak_quad_gap,cycles_per_quad\n");
    for scanline in [false, true] {
        for pixels in [1, 2] {
            for throttle in [0, 1, 2] {
                let stdout = run_rtl_source_mode(
                    &viewport_vectors(&cases),
                    "raster_viewport_tb.v",
                    throttle,
                    pixels,
                    scanline,
                );
                let actual = parse_rtl_scenes(&stdout);
                assert!(stdout.contains(&format!("RAST_CONFIG {pixels} {}", u8::from(scanline))));
                assert_eq!(actual.len(), cases.len());
                for (case, (name, _, output)) in cases.iter().zip(&actual) {
                    assert_eq!(name, &case.name);
                    let (_, expected) = viewport_expected(case);
                    // Traversal differs, but each quad and lane mask must be
                    // exactly equal, including multiplicity and triangle ID.
                    for kind in ["QUAD", "RETIRE_MARKER"] {
                        let mut e: Vec<_> = expected
                            .iter()
                            .filter(|s| s.starts_with(kind))
                            .cloned()
                            .collect();
                        let mut a: Vec<_> = output
                            .iter()
                            .filter(|s| s.starts_with(kind))
                            .cloned()
                            .collect();
                        if kind == "QUAD" {
                            e.sort();
                            a.sort();
                        }
                        assert_bucket(name, kind, &e, &a);
                    }
                }
                let metrics: Vec<_> = stdout
                    .lines()
                    .filter_map(|s| s.strip_prefix("RAST_METRICS "))
                    .collect();
                assert_eq!(metrics.len(), cases.len());
                for line in metrics {
                    let fields: Vec<_> = line.split_whitespace().collect();
                    assert_eq!(fields.len(), 5);
                    let cycles: u64 = fields[1].parse().unwrap();
                    let quads: u64 = fields[2].parse().unwrap();
                    let average = if quads == 0 {
                        String::new()
                    } else {
                        format!("{:.6}", cycles as f64 / quads as f64)
                    };
                    let peak: u64 = fields[4].parse().unwrap();
                    if throttle == 0 && fields[0] == "screen-borders" {
                        assert_eq!(peak, 4 / u64::from(pixels), "dense producer peak");
                    }
                    let traversal = if scanline { "scanline" } else { "tile" };
                    let peak = if quads < 2 {
                        String::new()
                    } else {
                        peak.to_string()
                    };
                    csv.push_str(&format!(
                        "{traversal},{pixels},{throttle},{},{cycles},{quads},{},{peak},{average}\n",
                        fields[0], fields[3]
                    ));
                }
            }
        }
    }
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-raster-candidates");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("throughput.csv"), csv).unwrap();
}

#[test]
#[ignore = "explicit stage-5 viewport raster output backpressure"]
fn viewport_raster_backpressure_preserves_stream() {
    let cases: Vec<ViewportCase> = viewport_cases()
        .into_iter()
        .filter(|case| {
            [
                "shared-edge-quad",
                "diag-crossing-tiles",
                "viewport-degenerate",
                "viewport-subpixel-sliver",
            ]
            .contains(&case.name.as_str())
        })
        .collect();
    assert_eq!(cases.len(), 4);
    check_viewport(&cases, 1);
    check_viewport(&cases, 2);
}

#[test]
#[ignore = "explicit stage-5 pixel stream RTL co-simulation"]
fn viewport_pixel_stream_matches_reference() {
    let cases: Vec<ViewportCase> = viewport_cases()
        .into_iter()
        .filter(|case| {
            [
                "shared-edge-quad",
                "screen-borders",
                "viewport-degenerate",
                "viewport-backface",
                "viewport-offscreen",
                "viewport-subpixel-sliver",
            ]
            .contains(&case.name.as_str())
        })
        .collect();
    for throttle in [0, 1, 2] {
        let stdout = run_rtl_source(&viewport_vectors(&cases), "raster_pixel_tb.v", throttle);
        let actual = parse_rtl_scenes(&stdout);
        assert_eq!(actual.len(), cases.len(), "pixel scene count");
        for (case, (name, _, output)) in cases.iter().zip(&actual) {
            assert_eq!(name, &case.name);
            assert_bucket(
                name,
                "pixel stream",
                &viewport_pixels_expected(case),
                output,
            );
        }
    }
}
