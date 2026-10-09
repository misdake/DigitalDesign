//! Native 400x240 scene/motion comparison. No enlarged error map or hidden gain.
use gpu_v2::lighting::{ports::*, sim::oracle};
use std::fmt::Write;

const WIDTH: usize = 400;
const HEIGHT: usize = 240;
const FRAMES: usize = 12;
fn quantize(x: f64) -> i16 {
    (x * 16384.0).round_ties_even().clamp(-32768.0, 32767.0) as i16
}
fn unit(v: [f64; 3]) -> [i16; 3] {
    let length = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    v.map(|x| quantize(x / length))
}
fn rgb(g: f64, h: f64) -> [u8; 3] {
    [0.24, 0.43, 0.65]
        .map(|base| ((base * g + 0.75 * h).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0).round() as u8)
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/gpu-v2-lighting/system-study/visual".into());
    std::fs::create_dir_all(&dir).unwrap();
    let policy = oracle::RoundingPolicy {
        power: oracle::Rounding::Floor,
        ..Default::default()
    };
    let variants = [
        oracle::Config {
            rounding: policy,
            ..Default::default()
        },
        oracle::Config {
            scalar_norm: true,
            scalar_normal: true,
            exact_normal_gate: true,
            direct_all_squares: true,
            rounding: policy,
            ..Default::default()
        },
        oracle::Config {
            scalar_norm: true,
            scalar_normal: true,
            direct_all_squares: true,
            square9: true,
            rounding: policy,
            ..Default::default()
        },
    ];
    let labels = ["ideal", "original", "square18", "mixed9"];
    let mut csv=String::from("scene,frame,variant,pixels,max_h_error_codes,mean_h_error_codes,over4_codes,max_temporal_residual_codes,mean_temporal_residual_codes\n");
    for scene in ["sphere", "NL-stress", "H-stress"] {
        let mut previous = vec![[0_f64; 3]; WIDTH * HEIGHT];
        for frame in 0..FRAMES {
            let time = frame as f64 / (FRAMES - 1) as f64;
            let mut images =
                std::array::from_fn::<_, 4, _>(|_| Vec::<u8>::with_capacity(WIDTH * HEIGHT * 3));
            let mut count = 0;
            let mut max = [0_f64; 3];
            let mut sum = [0_f64; 3];
            let mut over = [0; 3];
            let mut temporal_max = [0_f64; 3];
            let mut temporal_sum = [0_f64; 3];
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let sx = (x as f64 + 0.5) / WIDTH as f64 * 2.0 - 1.0;
                    let sy = 1.0 - (y as f64 + 0.5) / HEIGHT as f64 * 2.0;
                    let mut pixel = PixelInput {
                        normal: [8192, 0, 0],
                        ndc: [
                            ((sx * 65536.0).round() as i32) / 4,
                            ((sy * 65536.0).round() as i32) / 4,
                        ],
                    };
                    let mut light = Light {
                        direction: unit([0.35 + 0.25 * (time - 0.5), -0.25, 0.9]),
                        ambient: 32,
                        directional: 224,
                    };
                    let mut covered = true;
                    match scene {
                        "sphere" => {
                            let nx = sx * 1.65;
                            let ny = sy * 1.08;
                            let rr = nx * nx + ny * ny;
                            covered = rr <= 1.0;
                            if covered {
                                let scale = 0.65 + 0.18 * (time * 0.8 + sy * 0.6).sin();
                                pixel.normal =
                                    [nx, ny, (1.0 - rr).sqrt()].map(|v| quantize(v * scale));
                            }
                        }
                        "NL-stress" => {
                            let lx = 64.0 + 448.0 * time;
                            let lz = -(268435456.0 - lx * lx).sqrt();
                            light = Light {
                                direction: [lx.round() as i16, 0, lz.round() as i16],
                                ambient: 32,
                                directional: 224,
                            };
                            let magnitude = (256.0 + (sx + 1.0) * 0.5 * 32000.0).round() as i16;
                            let z = (f64::from(magnitude) * f64::from(light.direction[0])
                                / f64::from(-light.direction[2])
                                + sy * 2.5)
                                .round_ties_even() as i16;
                            pixel.normal = [magnitude, 0, z];
                            pixel.ndc = [0, 0];
                        }
                        "H-stress" => {
                            let lx = 128.0 + 64.0 * time;
                            let lz = -(268435456.0 - lx * lx).sqrt();
                            light = Light {
                                direction: [lx.round() as i16, 0, lz.round() as i16],
                                ambient: 32,
                                directional: 224,
                            };
                            pixel.ndc =
                                [(sx * 4000.0).round() as i32, (sy * 3000.0).round() as i32];
                        }
                        _ => unreachable!(),
                    }
                    if !covered {
                        for image in &mut images {
                            image.extend([12, 16, 24]);
                        }
                        continue;
                    }
                    count += 1;
                    let material = Material {
                        shininess_code: 16,
                        ..Default::default()
                    };
                    let ideal =
                        oracle::ideal(pixel, material, light, Projection::default()).unwrap();
                    images[0].extend(rgb(ideal[0], ideal[1]));
                    for (v, config) in variants.iter().enumerate() {
                        let result = oracle::evaluate(
                            pixel,
                            material,
                            light,
                            Projection::default(),
                            *config,
                        )
                        .unwrap();
                        let error = result.h as f64 - ideal[1] * 256.0;
                        max[v] = max[v].max(error.abs());
                        sum[v] += error.abs();
                        over[v] += usize::from(error.abs() > 4.0);
                        if frame > 0 {
                            let residual = (error - previous[y * WIDTH + x][v]).abs();
                            temporal_max[v] = temporal_max[v].max(residual);
                            temporal_sum[v] += residual;
                        }
                        previous[y * WIDTH + x][v] = error;
                        images[v + 1].extend(rgb(result.g as f64 / 256.0, result.h as f64 / 256.0));
                    }
                }
            }
            for (v, bytes) in images.iter().enumerate() {
                let mut ppm = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
                ppm.extend(bytes);
                std::fs::write(format!("{dir}/{scene}-{frame:02}-{}.ppm", labels[v]), ppm).unwrap();
            }
            for v in 0..3 {
                writeln!(
                    csv,
                    "{scene},{frame},{},{count},{:.6},{:.6},{},{:.6},{:.6}",
                    labels[v + 1],
                    max[v],
                    sum[v] / count as f64,
                    over[v],
                    temporal_max[v],
                    temporal_sum[v] / count as f64
                )
                .unwrap();
            }
            println!("rendered {scene} frame {frame}/{FRAMES}");
        }
    }
    std::fs::write(format!("{dir}/metrics.csv"), csv).unwrap();
}
