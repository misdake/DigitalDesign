//! Bounded precision study and stage-vector export. No counted/timed work.
#[path = "../tests/support/texture.rs"]
mod support;
use gpu_v2::texture::{ports::*, sim::oracle::*};
use std::{fs::File, io::Write, path::PathBuf};
use support::*;

fn scenarios() -> Vec<QuadInput> {
    let mut inputs = Vec::new();
    for uv in [
        [0.0; 2],
        [0.5; 2],
        [-0.00001, 1.00001],
        [7.5 / 1024.0, 15.5 / 1024.0],
        [0.99999, 0.00001],
        [0.00048828125; 2],
    ] {
        for bias in [0.0, 0.5, 9.5, 10.0] {
            let mut q = input(10, Filter::Trilinear, uv);
            q.uv[1][0] += 1.0 / 1024.0;
            q.uv[2][1] += 1.0 / 1024.0;
            q.uv[3] = [uv[0] + 1.0 / 1024.0, uv[1] + 1.0 / 1024.0];
            q.lod_bias = bias;
            inputs.push(q);
        }
    }
    let mut state = 0x1234_5678_u64;
    for _ in 0..192 {
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 32) as u32
        };
        let uv = [
            f64::from(next()) / f64::from(u32::MAX) * 4.0 - 2.0,
            f64::from(next()) / f64::from(u32::MAX) * 4.0 - 2.0,
        ];
        let slope = 2.0_f64.powf(f64::from(next() % 2560) / 256.0) / 1024.0;
        let mut q = input(10, Filter::Trilinear, uv);
        q.uv[1][0] += slope;
        q.uv[2][1] += slope * 0.7;
        q.uv[3] = [uv[0] + slope, uv[1] + slope * 0.7];
        inputs.push(q);
    }
    inputs
}

fn dump(file: &mut File, case: usize, out: &Output) -> std::io::Result<()> {
    let mut stage = |name: &str, value: String| writeln!(file, "{case},{name},{value}");
    stage(
        "config.coefficient_fraction",
        out.prepared.config.coefficient_fraction.to_string(),
    )?;
    stage("lod.rho", out.prepared.lod.rho.to_string())?;
    stage("lod.raw", out.prepared.lod.raw.to_string())?;
    stage(
        "config.coordinate_fraction",
        out.prepared.config.coordinate_fraction.to_string(),
    )?;
    stage(
        "config.lod_fraction",
        out.prepared.config.lod_fraction.to_string(),
    )?;
    stage(
        "config.uv_fraction",
        out.prepared
            .config
            .uv_fraction
            .map_or(-1, i32::from)
            .to_string(),
    )?;
    for (lane, uv) in out.prepared.lod.uv.iter().enumerate() {
        for (axis, v) in uv.iter().enumerate() {
            stage(&format!("uv.lane{lane}.{axis}"), v.to_string())?;
            if let Some(f) = out.prepared.config.uv_fraction {
                stage(
                    &format!("uv.lane{lane}.raw{axis}"),
                    ((*v * 2.0_f64.powi(i32::from(f))).round_ties_even() as i64).to_string(),
                )?;
            }
        }
    }
    stage(
        "lod.overflow",
        u8::from(out.prepared.lod.overflow).to_string(),
    )?;
    stage("lod.exponent", out.prepared.lod.exponent.to_string())?;
    stage(
        "lod.table_index",
        out.prepared
            .lod
            .table_index
            .map_or(-1, i32::from)
            .to_string(),
    )?;
    for (edge, d) in out.prepared.lod.derivatives.iter().enumerate() {
        for (axis, value) in d.iter().enumerate() {
            stage(&format!("lod.derivative{edge}.{axis}"), value.to_string())?;
        }
    }
    for pixel in &out.prepared.pixels {
        let name = format!("lane{}", pixel.lane);
        stage(&format!("{name}.lambda"), pixel.lambda.to_string())?;
        for (i, layer) in pixel.layers.iter().enumerate() {
            let name = format!("{name}.layer{i}");
            stage(&format!("{name}.n"), layer.n.to_string())?;
            stage(&format!("{name}.parent"), layer.parent.to_string())?;
            for axis in 0..2 {
                stage(&format!("{name}.p{axis}"), layer.p[axis].to_string())?;
                stage(
                    &format!("{name}.integer{axis}"),
                    layer.integer[axis].to_string(),
                )?;
                stage(
                    &format!("{name}.fraction{axis}"),
                    layer.fraction[axis].to_string(),
                )?;
            }
            for t in 0..4 {
                stage(
                    &format!("{name}.weight{t}"),
                    layer.coefficients[t].to_string(),
                )?;
                for axis in 0..2 {
                    stage(
                        &format!("{name}.tap{t}.{axis}"),
                        layer.taps[t][axis].to_string(),
                    )?;
                }
            }
        }
    }
    for pixel in &out.pixels {
        for (i, g) in pixel.groups.iter().enumerate() {
            let name = format!("lane{}.group{i}", pixel.lane);
            stage(
                &format!("{name}.record72"),
                g.group.pack72().map_err(std::io::Error::other)?.to_string(),
            )?;
            for t in 0..4 {
                stage(&format!("{name}.texel{t}"), g.texels[t].to_string())?;
                for c in 0..3 {
                    stage(
                        &format!("{name}.expanded{t}.{c}"),
                        g.expanded[t][c].to_string(),
                    )?;
                }
            }
            for c in 0..3 {
                stage(&format!("{name}.partial{c}"), g.partial[c].to_string())?;
                stage(
                    &format!("{name}.accumulator{c}"),
                    g.accumulator[c].to_string(),
                )?;
            }
        }
        for c in 0..3 {
            stage(
                &format!("lane{}.rgb{c}", pixel.lane),
                pixel.rgb[c].to_string(),
            )?;
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-texture".into()),
    );
    std::fs::create_dir_all(&directory)?;
    let s = slot(10, true);
    let bytes = asset(s, pattern);
    let inputs = scenarios();
    let mut source = Image {
        bytes: bytes.clone(),
        requests: vec![],
    };
    let references = inputs
        .iter()
        .map(|q| reference(q, &[s], &mut source, MipSelection::Nearest))
        .collect::<Result<Vec<_>, _>>()?;
    let baseline = Config::default();
    let configurations = [
        ("baseline", baseline),
        (
            "exact_lod",
            Config {
                lod_method: LodMethod::Exact,
                ..baseline
            },
        ),
        (
            "uv16",
            Config {
                uv_fraction: Some(16),
                ..baseline
            },
        ),
        (
            "uv20",
            Config {
                uv_fraction: Some(20),
                ..baseline
            },
        ),
        (
            "coordinate12",
            Config {
                coordinate_fraction: 12,
                ..baseline
            },
        ),
        (
            "coefficient12",
            Config {
                coefficient_fraction: 12,
                ..baseline
            },
        ),
        (
            "lod12",
            Config {
                lod_fraction: 12,
                ..baseline
            },
        ),
        (
            "high_precision",
            Config {
                uv_fraction: None,
                coordinate_fraction: 16,
                coefficient_fraction: 16,
                lod_fraction: 16,
                lod_method: LodMethod::Exact,
                ..baseline
            },
        ),
    ];
    let mut report = File::create(directory.join("precision.csv"))?;
    writeln!(report,"configuration,quads,channels,max_rgb_code_error,mean_rgb_code_error,max_lod_error,groups,refills,beats")?;
    let mut golden = File::create(directory.join("stages.csv"))?;
    writeln!(golden, "case,stage,value")?;
    for (name, config) in configurations {
        let mut source = Image {
            bytes: bytes.clone(),
            requests: vec![],
        };
        let mut cache = Cache::new(vec![s])?;
        let mut worst = 0.0_f64;
        let mut sum = 0.0;
        let mut count = 0;
        let mut lod_error = 0.0_f64;
        let mut groups = 0;
        for (case, (q, ideal)) in inputs.iter().zip(&references).enumerate() {
            let out = sample(q, &mut cache, &mut source, config)?;
            lod_error = lod_error.max((out.prepared.lod.selected - out.prepared.lod.ideal).abs());
            for (p, (_, rgb)) in out.pixels.iter().zip(ideal) {
                groups += p.groups.len();
                for (actual, want) in p.rgb.iter().zip(rgb) {
                    let error = (f64::from(*actual) - want).abs();
                    worst = worst.max(error);
                    sum += error;
                    count += 1;
                }
            }
            if name == "baseline" {
                dump(&mut golden, case, &out)?;
            }
        }
        writeln!(
            report,
            "{name},{},{count},{worst},{},{lod_error},{groups},{},{}",
            inputs.len(),
            sum / count as f64,
            cache.stats.refills,
            cache.stats.beats
        )?;
        println!(
            "{name}: max RGB {worst:.6}, mean {:.6}, max LOD {lod_error:.6}",
            sum / count as f64
        );
    }
    println!(
        "{} bounded quads; reports: {}",
        inputs.len(),
        directory.display()
    );
    Ok(())
}
