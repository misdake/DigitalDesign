//! Preparation-control proposals and independently checked structural work.
//! These measurements intentionally exclude cache/SDRAM/color; no sampler II.
#[path = "../tests/support/texture.rs"]
#[allow(dead_code)]
mod support;
use gpu_v2::texture::{
    ports::*,
    sim::staged::{self, binding::Evidence, stream},
};
use std::{fs, io::Write, path::Path};
fn inputs(profile: &str, mask: u8) -> Vec<QuadInput> {
    (0..64)
        .map(|i| {
            let uv = if profile == "seams" {
                [0.0; 2]
            } else {
                [
                    (i % 16 * 2) as f64 / 512.0 + 0.003,
                    (i / 16 * 2) as f64 / 512.0 + 0.003,
                ]
            };
            let mut q = support::input(
                9,
                if profile == "bilinear" {
                    Filter::Bilinear
                } else {
                    Filter::Trilinear
                },
                uv,
            );
            q.quad_id = (i % 16) as u8;
            q.mask = mask;
            if profile == "seams" {
                q.uv[3][0] += 1.0 / 262144.0;
                q.lod_bias = 9.5;
            } else {
                q.uv[1][0] += 1.0 / 512.0;
                q.uv[2][1] += 1.0 / 512.0;
                q.uv[3][0] += 1.0 / 512.0;
                q.uv[3][1] += 1.0 / 512.0;
                if profile == "fractional" {
                    q.lod_bias = 0.5;
                }
            }
            q
        })
        .collect()
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(
        args.first()
            .map(String::as_str)
            .unwrap_or("target/gpu-v2-texture-staged"),
    );
    fs::create_dir_all(root).unwrap();
    let mut csv = fs::File::create(root.join("control.csv")).unwrap();
    writeln!(csv, "profile,latency_certified,mask,contexts,work_credits,pressure,release_after_capture,quads,covered_pixels,packets,cycles,enabled,control_cycles_per_covered_pixel,peak_contexts,peak_coordinates,peak_work,peak_live_quads,peak_boundary_record_bits,lane_capture_bits,plane_capture_bits,coordinate_stalls,coefficient_stalls,output_stalls").unwrap();
    let mut work = fs::File::create(root.join("binding.csv")).unwrap();
    writeln!(work, "profile,stage,events,wiring_adds,equalities,primitive_dependency_cycles,output_bits,physical_primitive,operations").unwrap();
    for profile in ["bilinear", "fractional", "seams"] {
        for (mask, contexts, work_credits, pressure) in [
            (15, 1, 8, false),
            (15, 2, 8, false),
            (15, 4, 8, false),
            (15, 5, 8, false),
            (15, 8, 8, false),
            (15, 4, 2, false),
            (15, 4, 8, true),
            (1, 4, 8, false),
            (0, 4, 8, false),
        ] {
            for release_after_capture in [false, true] {
                let report = stream::run(
                    &inputs(profile, mask),
                    &[support::slot(9, true)],
                    stream::Hardware {
                        contexts,
                        work_credits,
                        release_after_capture,
                        ..Default::default()
                    },
                    |c| {
                        if pressure {
                            (!c.is_multiple_of(17), c >= 128 && c % 43 >= 11)
                        } else {
                            (true, true)
                        }
                    },
                )
                .unwrap();
                let s = &report.stats;
                let pixels = 64 * mask.count_ones();
                let rate = if pixels == 0 {
                    String::new()
                } else {
                    format!("{:.6}", s.cycles as f64 / f64::from(pixels))
                };
                writeln!(csv, "{profile},false,{mask},{contexts},{work_credits},{pressure},{release_after_capture},64,{pixels},{},{},{},{rate},{},{},{},{},{},{},{},{},{},{}",
                s.packets,s.cycles,s.enabled,s.peak_contexts,s.peak_coordinates,s.peak_work,s.peak_live_quads,s.peak_boundary_record_bits,s.lane_captures*116,s.plane_captures*92,s.coordinate_stalls,s.coefficient_stalls,s.output_stalls).unwrap();
                if mask == 15
                    && contexts == 4
                    && work_credits == 8
                    && !pressure
                    && !release_after_capture
                {
                    let p = report.programs[0].preparation();
                    let lane = &p.lanes[0];
                    let mut frames = vec![
                        ("derivative", &p.derivative.frame),
                        ("lod", &p.lod.frame),
                        ("coordinate", &lane.coordinate.frame),
                        ("rows", &lane.rows.frame),
                        ("columns", &lane.columns.frame),
                    ];
                    frames.extend(lane.planes.iter().map(|p| {
                        (
                            if p.which == 0 {
                                "fine_plane"
                            } else {
                                "coarse_plane"
                            },
                            &p.frame,
                        )
                    }));
                    for (stage, frame) in frames {
                        let e = Evidence::build(frame).unwrap();
                        e.audit(frame).unwrap();
                        for (r, count) in &e.work {
                            writeln!(
                                work,
                                "{profile},{stage},{},{},{},{},{},{r:?},{count}",
                                frame.events.len(),
                                e.lowering.wiring_adds.len(),
                                e.lowering.equalities.len(),
                                e.cycles,
                                e.output_bits
                            )
                            .unwrap();
                        }
                    }
                    // Retain a compact real multi-output closure counterexample.
                    let cone = staged::binding::lod_shared_h_cone(&p.lod.frame).unwrap();
                    assert!(cone.audit(&p.lod.frame).is_err());
                }
            }
        }
    }
    println!(
        "54 bounded preparation-control proposals and checked binding profiles: {}",
        root.display()
    );
    println!("latency_certified=false: these are not end-to-end or cache throughput measurements");
}
