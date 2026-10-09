//! Bounded natural-photo precision study. Consumes offline coherent RAW565 mips.
//! No scheduling, concurrency, counted model or RTL claims.
use gpu_v2::texture::{ports::*, sim::oracle::*};
use std::{fs, io::Write, path::Path};
#[path = "support/texture_work.rs"]
mod texture_work;
use texture_work::{check_strength_reductions, WorkStudy};

const BASE: u32 = 0x1000;
const MAX_QUADS: usize = 250_000;
const RANDOM_QUADS: usize = 16_384;
const SCENES: [usize; 7] = [640, 400, 256, 160, 80, 20, 3];

struct PhotoMemory {
    bytes: Vec<u8>,
}
impl MemoryPort for PhotoMemory {
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String> {
        if address & 127 != 0 || bytes != 128 {
            return Err("photo reader expects one aligned 128-byte tile".into());
        }
        let start = usize::try_from(
            address
                .checked_sub(u64::from(BASE))
                .ok_or("photo underflow")?,
        )
        .map_err(|_| "photo address width")?;
        let data = self
            .bytes
            .get(start..start + bytes)
            .ok_or("missing photo tile")?;
        Ok(data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect())
    }
}

#[derive(Clone)]
struct Worst {
    case: usize,
    scene: String,
    input: QuadInput,
    lane: u8,
    channel: usize,
    reference: [f64; 3],
    actual: [u8; 3],
}
#[derive(Default)]
struct Stats {
    errors: Vec<f32>,
    sum: f64,
    max: f64,
    over_one: usize,
    worst: Option<Worst>,
}
impl Stats {
    fn add(&mut self, case: usize, scene: &str, q: &QuadInput, pixel: &PixelOutput, rgb: [f64; 3]) {
        for (channel, (&actual, &want)) in pixel.rgb.iter().zip(&rgb).enumerate() {
            let error = (f64::from(actual) - want).abs();
            self.errors.push(error as f32);
            self.sum += error;
            self.over_one += usize::from(error > 1.0);
            if self.worst.is_none() || error > self.max {
                self.max = error;
                self.worst = Some(Worst {
                    case,
                    scene: scene.into(),
                    input: q.clone(),
                    lane: pixel.lane,
                    channel,
                    reference: rgb,
                    actual: pixel.rgb,
                });
            }
        }
    }
    fn report(
        &mut self,
        file: &mut fs::File,
        photo: &str,
        config: &str,
        scope: &str,
    ) -> std::io::Result<()> {
        self.errors.sort_unstable_by(f32::total_cmp);
        let count = self.errors.len();
        let p99 = self.errors[(count * 99 / 100).min(count - 1)];
        writeln!(
            file,
            "{photo},{config},{scope},{count},{},{},{p99},{}",
            self.max,
            self.sum / count as f64,
            self.over_one as f64 / count as f64 * 100.0
        )
    }
}

struct Variant {
    name: &'static str,
    config: Config,
    cache: Cache,
    interior: Stats,
    wrap: Stats,
}

fn input(uv: [f64; 2], dx: [f64; 2], dy: [f64; 2], mask: u8) -> QuadInput {
    QuadInput {
        force_coarsest: false,
        quad_id: 0,
        mask,
        uv: [
            uv,
            [uv[0] + dx[0], uv[1] + dx[1]],
            [uv[0] + dy[0], uv[1] + dy[1]],
            [uv[0] + dx[0] + dy[0], uv[1] + dx[1] + dy[1]],
        ],
        slot: 0,
        material_size_log2: 9,
        filter: Filter::Trilinear,
        lod_bias: 0.0,
    }
}

fn compare(
    case: usize,
    scene: &str,
    q: &QuadInput,
    slot: Slot,
    memory: &mut PhotoMemory,
    variants: &mut [Variant],
    work: &mut WorkStudy,
) -> Result<Vec<[u8; 3]>, String> {
    if case >= MAX_QUADS {
        return Err("photo study quad budget exceeded".into());
    }
    let ideal = reference(q, &[slot], memory, MipSelection::Floor)?;
    let mut colors = Vec::new();
    for (name, rgb) in &ideal {
        assert!(q.mask & (1 << name) != 0);
        colors.push(rgb.map(|v| v.round_ties_even() as u8));
    }
    for variant in variants {
        let out = sample(q, &mut variant.cache, memory, variant.config)?;
        if variant.name == "unorm9" {
            work.observe(scene, &out.prepared);
        }
        for ((pixel, (_, rgb)), prepared) in out.pixels.iter().zip(&ideal).zip(&out.prepared.pixels)
        {
            // Count a pixel as wrap if either contributing physical mip crosses
            // its periodic border. n=0's duplicated constant texel is excluded.
            let wraps = prepared.layers.iter().any(|layer| {
                layer.n > 0
                    && layer
                        .integer
                        .iter()
                        .any(|&i| i < 0 || i + 1 >= 1_i64 << layer.n)
            });
            let stats = if wraps {
                &mut variant.wrap
            } else {
                &mut variant.interior
            };
            stats.add(case, scene, q, pixel, *rgb);
            colors.push(pixel.rgb);
        }
    }
    Ok(colors)
}

fn ppm(path: &Path, side: usize, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::File::create(path)?;
    write!(file, "P6\n{side} {side}\n255\n")?;
    file.write_all(bytes)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    check_strength_reductions();
    let directory = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/gpu-v2-texture-photos".into());
    let root = Path::new(&directory);
    fs::create_dir_all(root)?;
    let slot = Slot {
        base_address: BASE,
        has_full_mip: true,
        max_size_log2: 9,
        valid: true,
    };
    let baseline = Config {
        uv_fraction: Some(18),
        mip_selection: MipSelection::Floor,
        max_quads: MAX_QUADS,
        ..Config::default()
    };
    let configs = [
        ("baseline", baseline),
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
    ];
    let mut report = fs::File::create(root.join("precision.csv"))?;
    writeln!(report, "photo,configuration,scope,channels,max_rgb_code_error,mean_rgb_code_error,p99_rgb_code_error,over_one_percent")?;
    let mut worst_csv = fs::File::create(root.join("worst.csv"))?;
    writeln!(worst_csv, "photo,configuration,scope,case,scene,lane,channel,error,u,v,ideal_lod,actual_lod,reference_r,reference_g,reference_b,actual_r,actual_g,actual_b,q0u,q0v,q1u,q1v,q2u,q2v,q3u,q3v")?;
    let mut trace = fs::File::create(root.join("worst-detail.txt"))?;
    for photo in ["peppers", "mandrill", "sailboat", "airplane"] {
        let mut work = WorkStudy::default();
        let data = fs::read(root.join("assets").join(format!("{photo}.raw565")))?;
        if data.len() != 699392 {
            return Err(format!("{photo}: incomplete mip asset").into());
        }
        let mut memory = PhotoMemory { bytes: data };
        let mut variants = configs.map(|(name, config)| Variant {
            name,
            config,
            cache: Cache::new(vec![slot]).unwrap(),
            interior: Stats::default(),
            wrap: Stats::default(),
        });
        let mut case = 0;
        for side in SCENES {
            let scene = format!("image{side}");
            let mut images = vec![vec![0_u8; side * side * 3]; variants.len() + 1];
            for y in (0..side).step_by(2) {
                for x in (0..side).step_by(2) {
                    let mut mask = 0;
                    for lane in 0..4 {
                        if x + lane % 2 < side && y + lane / 2 < side {
                            mask |= 1 << lane;
                        }
                    }
                    let q = input(
                        [
                            (x as f64 + 0.31) / side as f64,
                            (y as f64 + 0.67) / side as f64,
                        ],
                        [1.0 / side as f64, 0.0],
                        [0.0, 1.0 / side as f64],
                        mask,
                    );
                    let colors = compare(
                        case,
                        &scene,
                        &q,
                        slot,
                        &mut memory,
                        &mut variants,
                        &mut work,
                    )?;
                    let lanes = (0..4)
                        .filter(|lane| mask & (1 << lane) != 0)
                        .collect::<Vec<_>>();
                    for (image, colors) in images.iter_mut().zip(colors.chunks_exact(lanes.len())) {
                        for (&lane, rgb) in lanes.iter().zip(colors) {
                            let offset = ((y + lane / 2) * side + x + lane % 2) * 3;
                            image[offset..offset + 3].copy_from_slice(rgb);
                        }
                    }
                    case += 1;
                }
            }
            for (index, image) in images.iter().enumerate() {
                let name = if index == 0 {
                    "reference"
                } else {
                    variants[index - 1].name
                };
                ppm(
                    &root.join(format!("{photo}-{scene}-{name}.ppm")),
                    side,
                    image,
                )?;
            }
        }
        let mut state = 0x4164_0315_u64;
        for _ in 0..RANDOM_QUADS {
            let mut next = || {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                (state >> 32) as f64 / f64::from(u32::MAX)
            };
            let uv = [next(), next()];
            let slope = 2.0_f64.powf(-1.0 + next() * 9.75) / 512.0;
            let q = input(
                uv,
                [slope * 0.93, slope * 0.19],
                [-slope * 0.31, slope * 0.77],
                15,
            );
            compare(
                case,
                "random_affine",
                &q,
                slot,
                &mut memory,
                &mut variants,
                &mut work,
            )?;
            case += 1;
        }
        assert_eq!(case, 183272);
        work.write(&root.join(format!("{photo}-work.csv")))?;
        for variant in &mut variants {
            for (scope, stats) in [
                ("interior", &mut variant.interior),
                ("repeat_border", &mut variant.wrap),
            ] {
                stats.report(&mut report, photo, variant.name, scope)?;
                let w = stats.worst.as_ref().unwrap();
                let uv = w.input.uv[usize::from(w.lane)];
                let out = sample(&w.input, &mut variant.cache, &mut memory, variant.config)?;
                write!(
                    worst_csv,
                    "{photo},{},{scope},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    variant.name,
                    w.case,
                    w.scene,
                    w.lane,
                    w.channel,
                    stats.max,
                    uv[0],
                    uv[1],
                    out.prepared.lod.ideal,
                    out.prepared.lod.selected,
                    w.reference[0],
                    w.reference[1],
                    w.reference[2],
                    w.actual[0],
                    w.actual[1],
                    w.actual[2]
                )?;
                for uv in w.input.uv {
                    write!(worst_csv, ",{},{}", uv[0], uv[1])?;
                }
                writeln!(worst_csv)?;
                writeln!(
                    trace,
                    "{photo}/{} {scope}: input {:#?}\nreference {:?}\noutput {out:#?}\n",
                    variant.name, w.input, w.reference
                )?;
                println!(
                    "{photo}/{} {scope}: max {:.6}, mean {:.6}, channels {}",
                    variant.name,
                    stats.max,
                    stats.sum / stats.errors.len() as f64,
                    stats.errors.len()
                );
            }
        }
        report.flush()?;
        worst_csv.flush()?;
        println!("{photo}: {case} bounded quads complete");
    }
    Ok(())
}
