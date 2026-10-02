//! Bounded precision study and stage-vector export. No counted/timed work.
#[path = "../tests/support/texture.rs"]
mod support;
use gpu_v2::texture::{ports::*, sim::oracle::*};
use std::{fs::File, io::Write, path::PathBuf};
use support::*;

fn scenarios(size_log2: u8) -> Vec<QuadInput> {
    let size = f64::from(1_u16 << size_log2);
    let mut inputs = Vec::new();
    for uv in [
        [0.0; 2],
        [0.5; 2],
        [-0.00001, 1.00001],
        [7.5 / size, 15.5 / size],
        [0.99999, 0.00001],
        [0.5 / size; 2],
    ] {
        for bias in [0.0, 0.5, f64::from(size_log2) - 0.5, f64::from(size_log2)] {
            let mut q = input(size_log2, Filter::Trilinear, uv);
            q.uv[1][0] += 1.0 / size;
            q.uv[2][1] += 1.0 / size;
            q.uv[3] = [uv[0] + 1.0 / size, uv[1] + 1.0 / size];
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
        let slope = 2.0_f64.powf(f64::from(next() % 2560) / 256.0) / size;
        let mut q = input(size_log2, Filter::Trilinear, uv);
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
    stage(
        "config.coefficient_scale",
        out.prepared.config.coefficient_scale().to_string(),
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

// Diagnostic ideal weights are evaluated from continuous coordinates, independent
// of the oracle split. The reference RGB still comes from reference(), not this
// reporting helper. The asset's small mip layers have periodic padding.
fn dump_taps(
    file: &mut File,
    name: &str,
    q: &QuadInput,
    out: &Output,
    lane: u8,
) -> std::io::Result<()> {
    let pixel = &out.prepared.pixels[usize::from(lane)];
    let scale = f64::from(out.prepared.config.coefficient_scale());
    for layer in &pixel.layers {
        for (tap, ([x, y], weight)) in layer.taps.iter().zip(layer.coefficients).enumerate() {
            let logical = 1_u16 << layer.n;
            let rgb = expand565(pattern(
                layer.n,
                usize::from(x % logical),
                usize::from(y % logical),
            ));
            writeln!(
                file,
                "{name},actual,{lane},{},{tap},{x},{y},{},{},{},{}",
                layer.n,
                f64::from(weight) / scale,
                rgb[0],
                rgb[1],
                rgb[2]
            )?;
        }
    }
    let lod = out.prepared.lod.ideal;
    let level = lod.floor() as u8;
    let lambda = lod - lod.floor();
    for (level, parent) in [(level, 1.0 - lambda), (level + 1, lambda)] {
        if parent == 0.0 {
            continue;
        }
        let n = q.material_size_log2 - level;
        let size = 1_i64 << n.max(1);
        let p = q.uv[usize::from(lane)].map(|v| v.rem_euclid(1.0) * size as f64 - 0.5);
        let frac = p.map(|v| v - v.floor());
        for tap in 0..4 {
            let dx = tap % 2;
            let dy = tap / 2;
            let x = (p[0].floor() as i64 + dx).rem_euclid(size) as u16;
            let y = (p[1].floor() as i64 + dy).rem_euclid(size) as u16;
            let weight = parent
                * if dx == 0 { 1.0 - frac[0] } else { frac[0] }
                * if dy == 0 { 1.0 - frac[1] } else { frac[1] };
            let logical = 1_u16 << n;
            let rgb = expand565(pattern(
                n,
                usize::from(x % logical),
                usize::from(y % logical),
            ));
            writeln!(
                file,
                "{name},ideal,{lane},{n},{tap},{x},{y},{weight},{},{},{}",
                rgb[0], rgb[1], rgb[2]
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
    let size_log2 = std::env::args()
        .nth(2)
        .map(|v| v.parse::<u8>())
        .transpose()?
        .unwrap_or(10);
    if !(9..=10).contains(&size_log2) {
        return Err("probe size_log2 must be 9 or 10".into());
    }
    let s = slot(size_log2, true);
    let bytes = asset(s, pattern);
    let inputs = scenarios(size_log2);
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
            "uv18_unorm9",
            Config {
                uv_fraction: Some(18),
                coefficient_fraction: 9,
                coefficient_encoding: CoefficientEncoding::Unorm,
                ..baseline
            },
        ),
        (
            "uv18_zero_mask9",
            Config {
                uv_fraction: Some(18),
                coefficient_fraction: 9,
                ..baseline
            },
        ),
        (
            "unorm9",
            Config {
                coefficient_fraction: 9,
                coefficient_encoding: CoefficientEncoding::Unorm,
                ..baseline
            },
        ),
        (
            "zero_mask9",
            Config {
                coefficient_fraction: 9,
                ..baseline
            },
        ),
        (
            "exact_lod_unorm9",
            Config {
                coefficient_fraction: 9,
                coefficient_encoding: CoefficientEncoding::Unorm,
                lod_method: LodMethod::Exact,
                ..baseline
            },
        ),
        (
            "uv18",
            Config {
                uv_fraction: Some(18),
                ..baseline
            },
        ),
        (
            "coordinate9",
            Config {
                coordinate_fraction: 9,
                ..baseline
            },
        ),
        (
            "coefficient9",
            Config {
                coefficient_fraction: 9,
                ..baseline
            },
        ),
        (
            "coordinate_coefficient9",
            Config {
                coordinate_fraction: 9,
                coefficient_fraction: 9,
                ..baseline
            },
        ),
        (
            "uv18_coordinate_coefficient9",
            Config {
                uv_fraction: Some(18),
                coordinate_fraction: 9,
                coefficient_fraction: 9,
                ..baseline
            },
        ),
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
        (
            "continuous_uv",
            Config {
                uv_fraction: None,
                ..baseline
            },
        ),
        (
            "exact_lod_coefficient9",
            Config {
                lod_method: LodMethod::Exact,
                coefficient_fraction: 9,
                ..baseline
            },
        ),
    ];
    let mut report = File::create(directory.join("precision.csv"))?;
    writeln!(report,"configuration,quads,channels,max_rgb_code_error,mean_rgb_code_error,max_lod_error,groups,refills,beats")?;
    let mut golden = File::create(directory.join("stages.csv"))?;
    writeln!(golden, "case,stage,value")?;
    let mut worst_report = File::create(directory.join("worst.csv"))?;
    writeln!(worst_report, "configuration,case,lane,channel,error,reference_r,reference_g,reference_b,actual_r,actual_g,actual_b,ideal_lod,actual_lod,u,v,quantized_u,quantized_v,accumulator_r,accumulator_g,accumulator_b,coefficient_scale")?;
    let mut taps = File::create(directory.join("worst_taps.csv"))?;
    writeln!(taps, "configuration,kind,lane,n,tap,x,y,weight,r,g,b")?;
    let mut detail = File::create(directory.join("worst-detail.txt"))?;
    let mut baseline_worst = None;
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
        let mut worst_position = (0, 0_u8, 0);
        for (case, (q, ideal)) in inputs.iter().zip(&references).enumerate() {
            let out = sample(q, &mut cache, &mut source, config)?;
            lod_error = lod_error.max((out.prepared.lod.selected - out.prepared.lod.ideal).abs());
            for (p, (_, rgb)) in out.pixels.iter().zip(ideal) {
                groups += p.groups.len();
                for (channel, (actual, want)) in p.rgb.iter().zip(rgb).enumerate() {
                    let error = (f64::from(*actual) - want).abs();
                    if error > worst {
                        worst = error;
                        worst_position = (case, p.lane, channel);
                    }
                    sum += error;
                    count += 1;
                }
            }
            if name == "baseline" {
                dump(&mut golden, case, &out)?;
            }
        }
        let (case, lane, channel) = worst_position;
        if name == "baseline" {
            baseline_worst = Some((case, lane));
        }
        let q = &inputs[case];
        let out = sample(q, &mut Cache::new(vec![s])?, &mut source, config)?;
        let rgb = references[case][usize::from(lane)].1;
        let actual = &out.pixels[usize::from(lane)];
        let acc = actual.groups.last().unwrap().accumulator;
        let uv = q.uv[usize::from(lane)];
        let quantized_uv = out.prepared.lod.uv[usize::from(lane)];
        writeln!(worst_report, "{name},{case},{lane},{channel},{worst},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}", rgb[0], rgb[1], rgb[2], actual.rgb[0], actual.rgb[1], actual.rgb[2], out.prepared.lod.ideal, out.prepared.lod.selected, uv[0], uv[1], quantized_uv[0], quantized_uv[1], acc[0], acc[1], acc[2], config.coefficient_scale())?;
        dump_taps(&mut taps, name, q, &out, lane)?;
        writeln!(detail, "{name}: case {case}, lane {lane}, channel {channel}\ninput: {q:#?}\noutput: {out:#?}\nreference: {rgb:?}\n")?;
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
    // Hold the baseline argmax input fixed: changing a setting can move the
    // maximum, so aggregate maxima alone cannot attribute the original error.
    let (case, lane) = baseline_worst.unwrap();
    let mut fixed = File::create(directory.join("fixed_case.csv"))?;
    writeln!(fixed, "configuration,case,lane,actual_r,actual_g,actual_b,pre_round_r,pre_round_g,pre_round_b,ideal_lod,actual_lod,rho,table_index")?;
    for (name, config) in configurations {
        let out = sample(
            &inputs[case],
            &mut Cache::new(vec![s])?,
            &mut source,
            config,
        )?;
        let p = &out.pixels[usize::from(lane)];
        let acc = p.groups.last().unwrap().accumulator;
        let scale = f64::from(config.coefficient_scale());
        writeln!(
            fixed,
            "{name},{case},{lane},{},{},{},{},{},{},{},{},{},{}",
            p.rgb[0],
            p.rgb[1],
            p.rgb[2],
            acc[0] as f64 / scale,
            acc[1] as f64 / scale,
            acc[2] as f64 / scale,
            out.prepared.lod.ideal,
            out.prepared.lod.selected,
            out.prepared.lod.rho,
            out.prepared.lod.table_index.map_or(-1, i32::from)
        )?;
    }
    println!(
        "{} bounded quads; reports: {}",
        inputs.len(),
        directory.display()
    );
    Ok(())
}
