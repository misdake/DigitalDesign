//! Bounded first-principles shared-ray/normal approximation stress survey.
use gpu_v2::lighting::{ports::*, sim::oracle};
use std::{fmt::Write, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-lighting/architecture-upgrade".into()),
    );
    fs::create_dir_all(&root)?;
    let mut summary = String::from(
        "width,code,samples,max_h_shared_H,max_g_shared_N,max_h_shared_N,max_h_joint\n",
    );
    for width in [128, 320, 640, 1920] {
        for code in [0, 8, 16] {
            let step = 131072 / width;
            let mut maxima = [0i128; 4];
            let mut samples = 0;
            for center in [
                -60000, -20000, -1024, -513, -512, -511, -256, -128, 0, 128, 256, 511, 512, 513,
                1024, 20000, 60000,
            ] {
                for ldir in [[0, 0, 16384], [0, 0, -16384], [9459, 9459, 9459]] {
                    for base_n in [
                        [0i16, 0, 16384],
                        [16384, 0, 0],
                        [127, 0, 16383],
                        [63, -64, 65],
                        [16384, 0, -64],
                    ] {
                        for (dx, dy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
                            let normal = [
                                base_n[0].saturating_add(dx * 64),
                                base_n[1].saturating_add(dy * 64),
                                base_n[2],
                            ];
                            let p = PixelInput {
                                normal,
                                ndc: [
                                    (center + i32::from(dx) * step / 2) / 4,
                                    (i32::from(dy) * step / 2) / 4,
                                ],
                            };
                            let m = Material {
                                shininess_code: code,
                                ..Material::default()
                            };
                            let l = Light {
                                direction: ldir,
                                ..Light::default()
                            };
                            let cfg = oracle::Config {
                                rounding: oracle::RoundingPolicy {
                                    power: oracle::Rounding::Floor,
                                    ..Default::default()
                                },
                                ..Default::default()
                            };
                            let baseline =
                                oracle::evaluate(p, m, l, Projection::default(), cfg).unwrap();
                            let h = oracle::evaluate(
                                p,
                                m,
                                l,
                                Projection::default(),
                                oracle::Config {
                                    half_ndc_override: Some([center, 0]),
                                    ..cfg
                                },
                            )
                            .unwrap();
                            let n = oracle::evaluate(
                                p,
                                m,
                                l,
                                Projection::default(),
                                oracle::Config {
                                    normal_override: Some(base_n),
                                    ..cfg
                                },
                            )
                            .unwrap();
                            let joint = oracle::evaluate(
                                p,
                                m,
                                l,
                                Projection::default(),
                                oracle::Config {
                                    half_ndc_override: Some([center, 0]),
                                    normal_override: Some(base_n),
                                    ..cfg
                                },
                            )
                            .unwrap();
                            for (a, b) in maxima.iter_mut().zip([
                                (baseline.h - h.h).abs(),
                                (baseline.g - n.g).abs(),
                                (baseline.h - n.h).abs(),
                                (baseline.h - joint.h).abs(),
                            ]) {
                                *a = (*a).max(b);
                            }
                            samples += 1;
                        }
                    }
                }
            }
            writeln!(
                summary,
                "{width},{code},{samples},{},{},{},{}",
                maxima[0], maxima[1], maxima[2], maxima[3]
            )?;
        }
    }
    fs::write(root.join("sharing.csv"), &summary)?;
    print!("{summary}");
    Ok(())
}
