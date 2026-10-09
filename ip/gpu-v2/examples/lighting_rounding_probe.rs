//! Stage-isolated rounding/precision, bias, highlight and degeneracy measurements.
use gpu_v2::lighting::{
    ports::*,
    sim::oracle::{self, Rounding as R, RoundingPolicy as P},
};
use std::{fmt::Write, fs, path::PathBuf};

#[derive(Default)]
struct Metrics {
    n: usize,
    delta_max: [f64; 2],
    delta_sum: [f64; 2],
    delta_sq: [f64; 2],
    ideal_max: [f64; 2],
    ideal_sum: [f64; 2],
    ideal_sq: [f64; 2],
    half_zero_changes: usize,
}
struct Random(u64);
type Case = (PixelInput, Light, Projection);
type Dataset = (&'static str, Vec<Case>);
impl Random {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as u32
    }
    fn unit(&mut self) -> [i16; 3] {
        let v = std::array::from_fn::<_, 3, _>(|_| f64::from(self.next() as i32));
        let length = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| (x / length * 16384.0).round_ties_even() as i16)
    }
}
fn half_zero(g: &oracle::Golden) -> bool {
    g.stages
        .iter()
        .filter(|(name, _)| matches!(name.as_str(), "h.0" | "h.1" | "h.2"))
        .all(|(_, value)| *value == 0)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-lighting/rounding".into()),
    );
    fs::create_dir_all(&root)?;
    let policies = vec![
        ("rne", P::default()),
        ("floor-all", P::floor_all()),
        (
            "magnitude-floor",
            P {
                normalization: R::TowardZero,
                ..P::default()
            },
        ),
        (
            "normalization-floor",
            P {
                normalization: R::Floor,
                ..P::default()
            },
        ),
        (
            "rsqrt-floor",
            P {
                rsqrt: R::Floor,
                ..P::default()
            },
        ),
        (
            "power-floor",
            P {
                power: R::Floor,
                ..P::default()
            },
        ),
        (
            "projection-floor",
            P {
                projection: R::Floor,
                ..P::default()
            },
        ),
        (
            "half-floor",
            P {
                half: R::Floor,
                ..P::default()
            },
        ),
        (
            "dot-floor",
            P {
                dot: R::Floor,
                ..P::default()
            },
        ),
        (
            "output-floor",
            P {
                output: R::Floor,
                ..P::default()
            },
        ),
    ];
    let mut profiles: Vec<_> = policies
        .into_iter()
        .map(|(name, rounding)| {
            (
                name,
                oracle::Config {
                    rounding,
                    ..oracle::Config::default()
                },
            )
        })
        .collect();
    profiles.push((
        "power-rsqrt-floor",
        oracle::Config {
            rounding: P {
                rsqrt: R::Floor,
                power: R::Floor,
                ..P::default()
            },
            ..oracle::Config::default()
        },
    ));
    if std::env::args().nth(2).as_deref() == Some("precision") {
        let default = oracle::Config::default();
        profiles = vec![
            ("baseline", default),
            (
                "rsqrt-work+1",
                oracle::Config {
                    reciprocal_work_extra: 1,
                    ..default
                },
            ),
            (
                "rsqrt-work+3",
                oracle::Config {
                    reciprocal_work_extra: 3,
                    ..default
                },
            ),
            (
                "rsqrt-work+8",
                oracle::Config {
                    reciprocal_work_extra: 8,
                    ..default
                },
            ),
            (
                "rsqrt-rom+1",
                oracle::Config {
                    reciprocal_fraction: 16,
                    ..default
                },
            ),
            (
                "direction+1",
                oracle::Config {
                    direction_fraction: 15,
                    ..default
                },
            ),
            (
                "exact-square",
                oracle::Config {
                    approximate_square: false,
                    ..default
                },
            ),
            (
                "exact-rsqrt",
                oracle::Config {
                    approximate_rsqrt: false,
                    ..default
                },
            ),
            (
                "exact-power",
                oracle::Config {
                    approximate_power: false,
                    ..default
                },
            ),
        ];
    }
    let mut datasets: Vec<Dataset> = Vec::new();
    let mut random = Random(0x218a_4733_29cc_5f01);
    let mut general = Vec::new();
    let mut highlight = Vec::new();
    for index in 0..32768 {
        let normal = std::array::from_fn(|_| random.next() as i16);
        let ndc = std::array::from_fn(|_| (random.next() % 32769) as i32 - 16384);
        let light = Light {
            direction: random.unit(),
            ambient: 32,
            directional: 256,
        };
        let projection = Projection {
            ray_scale: [-12288, -8192],
            k: 10240,
        };
        general.push((PixelInput { normal, ndc }, light, projection));
        if index < 8192 {
            let ray = [
                f64::from(ndc[0]) / 65536.0 * f64::from(projection.ray_scale[0]),
                f64::from(ndc[1]) / 65536.0 * f64::from(projection.ray_scale[1]),
                f64::from(projection.k),
            ];
            let length = ray.iter().map(|x| x * x).sum::<f64>().sqrt();
            let half: [f64; 3] =
                std::array::from_fn(|i| f64::from(light.direction[i]) + ray[i] / length * 16384.0);
            let length = half.iter().map(|x| x * x).sum::<f64>().sqrt();
            let normal = std::array::from_fn(|i| {
                ((half[i] / length + f64::from((random.next() % 129) as i32 - 64) / 16384.0)
                    * 16384.0)
                    .round_ties_even() as i16
            });
            highlight.push((PixelInput { normal, ndc }, light, projection));
        }
    }
    let varied_intensity = general
        .iter()
        .enumerate()
        .map(|(index, &(pixel, mut light, projection))| {
            light.ambient = ((index * 97 + 11) % 257) as u16;
            light.directional = ((index * 73 + 29) % 257) as u16;
            (pixel, light, projection)
        })
        .collect();
    datasets.push(("random", general));
    datasets.push(("random-intensity", varied_intensity));
    datasets.push(("highlight", highlight));
    let mut edges = Vec::new();
    for x in -160_i32..=160 {
        let z = -((16384_i64 * 16384 - i64::from(x * x)) as f64)
            .sqrt()
            .round_ties_even() as i16;
        for normal in [[16384, 0, 0], [-16384, 0, 0], [32767, 0, 32767]] {
            edges.push((
                PixelInput {
                    normal,
                    ndc: [0; 2],
                },
                Light {
                    direction: [x as i16, 0, z],
                    ambient: 32,
                    directional: 256,
                },
                Projection::default(),
            ));
        }
    }
    datasets.push(("half-threshold", edges));
    let mut csv = String::from("dataset,policy,n,max_delta_g_codes,max_delta_h_codes,mean_delta_g_codes,mean_delta_h_codes,rms_delta_g_codes,rms_delta_h_codes,max_ideal_g_codes,max_ideal_h_codes,mean_ideal_g_codes,mean_ideal_h_codes,rms_ideal_g_codes,rms_ideal_h_codes,half_zero_changes\n");
    let mut worst = String::new();
    for (dataset, cases) in datasets {
        let mut metrics: Vec<Metrics> = profiles.iter().map(|_| Metrics::default()).collect();
        for (index, (pixel, light, projection)) in cases.into_iter().enumerate() {
            let material = Material {
                shininess_code: (index % 17) as u8,
                ..Material::default()
            };
            let baseline = oracle::evaluate(
                pixel,
                material,
                light,
                projection,
                oracle::Config::default(),
            )
            .map_err(|e| format!("baseline: {e:?}"))?;
            let ideal = oracle::ideal(pixel, material, light, projection)
                .map_err(|e| format!("ideal: {e:?}"))?;
            for ((name, config), m) in profiles.iter().zip(&mut metrics) {
                let g = oracle::evaluate(pixel, material, light, projection, *config)
                    .map_err(|e| format!("policy {name}: {e:?}"))?;
                m.n += 1;
                m.half_zero_changes += usize::from(half_zero(&g) != half_zero(&baseline));
                for (i, raw) in [g.g, g.h].into_iter().enumerate() {
                    let delta = (raw - [baseline.g, baseline.h][i]) as f64;
                    let error = raw as f64 - ideal[i] * 256.0;
                    if delta.abs() > m.delta_max[i] {
                        writeln!(worst, "dataset={dataset} policy={name} component={i} delta={delta} pixel={pixel:?} light={light:?} material={material:?} baseline={} actual={raw}", [baseline.g,baseline.h][i])?;
                    }
                    m.delta_max[i] = m.delta_max[i].max(delta.abs());
                    m.delta_sum[i] += delta;
                    m.delta_sq[i] += delta * delta;
                    m.ideal_max[i] = m.ideal_max[i].max(error.abs());
                    m.ideal_sum[i] += error;
                    m.ideal_sq[i] += error * error;
                }
            }
        }
        for ((name, _), m) in profiles.iter().zip(metrics) {
            let n = m.n as f64;
            writeln!(csv, "{dataset},{name},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{}", m.n,
                m.delta_max[0],m.delta_max[1],m.delta_sum[0]/n,m.delta_sum[1]/n,
                (m.delta_sq[0]/n).sqrt(),(m.delta_sq[1]/n).sqrt(),m.ideal_max[0],m.ideal_max[1],
                m.ideal_sum[0]/n,m.ideal_sum[1]/n,(m.ideal_sq[0]/n).sqrt(),(m.ideal_sq[1]/n).sqrt(),m.half_zero_changes)?;
        }
        println!("completed dataset={dataset}");
    }
    fs::write(root.join("summary.csv"), &csv)?;
    fs::write(root.join("worst.txt"), worst)?;
    print!("{csv}");
    Ok(())
}
