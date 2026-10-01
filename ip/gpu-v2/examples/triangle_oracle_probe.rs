//! Bounded precision/work study. Full stage samples and CSV go to the output dir.
#[path = "../tests/support/triangle.rs"]
mod fixtures;
use gpu_v2::triangle::{ports::*, sim::oracle::*};
use std::{fmt::Write as _, fs, path::PathBuf};

fn main() -> Result<(), String> {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-triangle".into()),
    );
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let c = Config {
        max_samples: 1_000_000,
        ..Config::default()
    };
    let mut csv=String::from("scene,model,origin,bits,covered,vertices,fans,intersections,setup_products,setup_reciprocals,pixel_products,uv_texels,depth_codes,normal_codes,rgb_error,lod_error,lod_queries,lod_invalid,source_field_bits,source_det_bits,status\n");
    let mut goldens = String::new();
    let mut total_samples = 0usize;
    for (name, input) in fixtures::pressure() {
        let base = run(&input, c)?;
        let base_samples = base.rasterize()?;
        let positions: Vec<_> = base_samples.iter().map(|(p, _, _)| *p).collect();
        let mut quads = std::collections::BTreeSet::new();
        for p in &positions {
            quads.insert([p[0] & !1, p[1] & !1]);
        }
        writeln!(goldens, "SCENE {name}\n{base:#?}").unwrap();
        for (p, fan, s) in base_samples.iter().take(16) {
            writeln!(
                goldens,
                "PIXEL {p:?} fan={fan} integer_fields={:?}\n{s:#?}\nREFERENCE {:#?}",
                base.integer_pixel_fields(p[0], p[1])?,
                base.reference(s.position)?
            )
            .unwrap();
        }
        for model in [Interpolation::Basis, Interpolation::Planes] {
            for origin in [FieldOrigin::Local, FieldOrigin::Global] {
                for bits in [None, Some(18), Some(24), Some(36)] {
                    let r = run(
                        &input,
                        Config {
                            interpolation: model,
                            field_origin: origin,
                            field_bits: bits,
                            ..c
                        },
                    )?;
                    let samples = r.rasterize();
                    let source = r.source.as_ref().ok_or("empty stress source")?;
                    let field_bits = source
                        .fields
                        .iter()
                        .flatten()
                        .map(|v| 129 - v.unsigned_abs().leading_zeros())
                        .max()
                        .unwrap();
                    let det_bits = 129 - source.determinant.unsigned_abs().leading_zeros();
                    let mut uv = 0.0_f64;
                    let mut depth = 0;
                    let mut normal = 0;
                    let mut rgb = 0.0_f64;
                    let mut lod = 0.0_f64;
                    let mut lod_queries = 0;
                    let mut lod_invalid = 0;
                    let (covered, status) = match samples {
                        Ok(samples) => {
                            total_samples += samples.len();
                            if total_samples > 8_000_000 {
                                return Err("precision probe total sample watchdog".into());
                            }
                            for (_, _, s) in &samples {
                                let expected = r.reference(s.position)?;
                                for (a, b) in s.uv.iter().zip(expected.uv) {
                                    uv = uv.max((a - b).abs() * 1024.0);
                                }
                                for (a, b) in s.normal.iter().zip(expected.normal) {
                                    normal = normal.max(((a - b).abs() * 16384.0).ceil() as u64);
                                }
                                for (a, b) in s.rgb.iter().zip(expected.rgb) {
                                    rgb = rgb.max((a - b).abs());
                                }
                                depth =
                                    depth.max(s.quantized.depth.abs_diff(expected.quantized.depth));
                            }
                            for p in quads.iter().step_by(17) {
                                if let Ok(reference) = base.quad_lod(p[0], p[1], 1024) {
                                    lod_queries += 1;
                                    match r.quad_lod(p[0], p[1], 1024) {
                                        Ok(actual) => lod = lod.max((actual - reference).abs()),
                                        Err(_) => lod_invalid += 1,
                                    }
                                }
                            }
                            (samples.len(), "ok".to_string())
                        }
                        Err(e) => (0, e.replace(',', ";")),
                    };
                    writeln!(csv,"{name},{model:?},{origin:?},{},{covered},{},{},{},{},{},{},{uv:.9},{depth},{normal},{rgb:.9},{lod:.9},{lod_queries},{lod_invalid},{field_bits},{det_bits},{status}",
                        bits.map_or("none".into(),|v|v.to_string()),r.polygon.len(),r.triangles.len(),r.work.intersections,
                        r.work.setup_products(model,origin==FieldOrigin::Local),r.work.reciprocal_requests(),AlgorithmWork::pixel_products(model)).unwrap();
                    if origin == FieldOrigin::Local
                        && (bits.is_none() || bits == Some(36) || bits == Some(18))
                    {
                        println!("{name} {model:?} bits={bits:?}: pixels={covered}, setup={}mul+{}rcp, pixel={}mul+1rcp, UV={uv:.6} texels D16={depth} LOD={lod:.6} [{status}]",
                            r.work.setup_products(model,true),r.work.reciprocal_requests(),AlgorithmWork::pixel_products(model));
                    }
                }
            }
        }
    }
    fs::write(dir.join("precision.csv"), csv).map_err(|e| e.to_string())?;
    let mut thin_csv = String::from("case,offset,thickness,nonconvex,fans,covered,status\n");
    let mut nonconvex = 0;
    let mut faults = 0;
    for case in 0..512 {
        let offset = (case % 64) as f64 / 1024.0;
        let thickness = (case / 64 + 1) as f64 / 256.0;
        let c = fixtures::config();
        let input = fixtures::input(
            [
                [-300.0, 10.0 + offset],
                [300.0, 30.0 + offset],
                [300.0, 30.0 + offset + thickness],
            ],
            [1.0; 3],
            c,
        );
        match run(&input, c) {
            Ok(r) => {
                nonconvex += usize::from(r.snap_nonconvex);
                let (covered, status) = match r.rasterize() {
                    Ok(samples) => (samples.len(), "ok".to_string()),
                    Err(e) => {
                        faults += 1;
                        if faults == 1 {
                            writeln!(goldens, "THIN DIAGNOSTIC {case} {e}\n{r:#?}").unwrap();
                        }
                        (0, e)
                    }
                };
                writeln!(
                    thin_csv,
                    "{case},{offset},{thickness},{},{},{covered},{}",
                    r.snap_nonconvex,
                    r.triangles.len(),
                    status.replace(',', ";")
                )
                .unwrap();
            }
            Err(e) => {
                faults += 1;
                writeln!(
                    thin_csv,
                    "{case},{offset},{thickness},unknown,0,0,{}",
                    e.replace(',', ";")
                )
                .unwrap();
            }
        }
    }
    fs::write(dir.join("thin-diagnostics.csv"), thin_csv).map_err(|e| e.to_string())?;
    fs::write(dir.join("stage-goldens.txt"), goldens).map_err(|e| e.to_string())?;
    println!("THIN512 nonconvex={nonconvex} diagnostics={faults}");
    println!(
        "COMPLETE samples={total_samples}; detailed reports in {}",
        dir.display()
    );
    Ok(())
}
