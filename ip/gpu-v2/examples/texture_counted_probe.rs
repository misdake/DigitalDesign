//! Bounded stage-golden audit and work ledger; no cycle estimates.
use gpu_v2::texture::{
    ports::*,
    sim::{counted, oracle},
};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

struct Image {
    bytes: Vec<u8>,
}
impl MemoryPort for Image {
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String> {
        let start = address.checked_sub(0x1000).ok_or("below image")? as usize;
        if address & 127 != 0 || bytes != 128 {
            return Err("one aligned tile required".into());
        }
        Ok(self
            .bytes
            .get(start..start + bytes)
            .ok_or("missing tile")?
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect())
    }
}
fn image(n: u8) -> Image {
    let mut bytes = Vec::new();
    for size in 0..=n {
        let logical = 1_usize << size;
        let tiles = logical.div_ceil(8);
        for ty in 0..tiles {
            for tx in 0..tiles {
                for y in 0..8 {
                    for x in 0..8 {
                        let x = (tx * 8 + x) % logical;
                        let y = (ty * 8 + y) % logical;
                        let word = (((x * 3 + y * 7 + size as usize * 5) % 32) as u16) << 11
                            | (((x * 11 + y * 5 + size as usize * 13) % 64) as u16) << 5
                            | ((x * 13 + y * 3 + size as usize * 17) % 32) as u16;
                        bytes.extend(word.to_le_bytes());
                    }
                }
            }
        }
    }
    Image { bytes }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(
        args.first()
            .map(String::as_str)
            .unwrap_or("target/gpu-v2-texture-counted"),
    );
    fs::create_dir_all(root).unwrap();
    let mut summary = fs::File::create(root.join("summary.csv")).unwrap();
    writeln!(summary,"profile,quads,pixels,groups,coefficient_products,color_products,prepare_adds,color_adds,prepare_compares,fifo_write_bits,payload_read_bits,texel_read_bits,refills,beats,max_error,mean_error").unwrap();
    let mut operations = fs::File::create(root.join("operations.csv")).unwrap();
    writeln!(operations, "profile,stage,operation,count").unwrap();
    let mut profiles = vec![
        "bilinear",
        "integer_lod",
        "fractional_lod",
        "affine",
        "perspective",
        "seams",
    ];
    if args.len() > 1 {
        profiles.extend(["peppers", "mandrill", "sailboat", "airplane"]);
    }
    for profile in profiles {
        let slot = Slot {
            base_address: 0x1000,
            has_full_mip: true,
            max_size_log2: 9,
            valid: true,
        };
        let photo = ["peppers", "mandrill", "sailboat", "airplane"].contains(&profile);
        let mut memory = if photo {
            Image {
                bytes: fs::read(Path::new(&args[1]).join(format!("{profile}.raw565"))).unwrap(),
            }
        } else {
            image(9)
        };
        let mut cache = oracle::Cache::new(vec![slot]).unwrap();
        let mut reference_memory = Image {
            bytes: memory.bytes.clone(),
        };
        let mut reference_cache = oracle::Cache::new(vec![slot]).unwrap();
        let mut pixels = 0;
        let mut groups = 0;
        let mut prep = BTreeMap::<&str, u64>::new();
        let mut color = BTreeMap::<&str, u64>::new();
        let mut coefficient = 0;
        let mut products = 0;
        let mut writes = 0;
        let mut payload_reads = 0;
        let mut texel_reads = 0;
        let mut max_error = 0.0_f64;
        let mut sum_error = 0.0_f64;
        for case in 0..128 {
            let mut uv = [[0.0; 2]; 4];
            for (lane, value) in uv.iter_mut().enumerate() {
                let x = case as f64 * 0.037 + (lane & 1) as f64 / 256.0;
                let y = case as f64 * 0.023 + (lane >> 1) as f64 / 256.0;
                *value = if profile == "perspective" {
                    [x / (1.0 + x * 0.7 + y * 0.3), y / (1.0 + x * 0.7 + y * 0.3)]
                } else {
                    [x, y]
                };
            }
            if profile == "seams" {
                uv = [[0.0; 2]; 4];
                uv[3][0] = 1.0 / 262144.0;
            }
            let q = QuadInput {
                quad_id: (case % 16) as u8,
                mask: if profile == "perspective" {
                    (case % 15 + 1) as u8
                } else {
                    15
                },
                uv,
                slot: 0,
                material_size_log2: 9,
                filter: if profile == "bilinear" {
                    Filter::Bilinear
                } else {
                    Filter::Trilinear
                },
                lod_bias: match profile {
                    "integer_lod" | "bilinear" => 0.0,
                    "fractional_lod" => 0.5,
                    "seams" => 9.5,
                    _ => case as f64 % 4.0 / 8.0,
                },
            };
            let expected = oracle::sample(
                &q,
                &mut reference_cache,
                &mut reference_memory,
                Config::counted(),
            )
            .unwrap();
            let actual = counted::sample(&q, &mut cache, &mut memory).unwrap();
            let continuous =
                oracle::reference(&q, &[slot], &mut reference_memory, MipSelection::Floor).unwrap();
            assert_eq!(
                actual.preparation.groups,
                expected
                    .prepared
                    .pixels
                    .iter()
                    .flat_map(|p| p.groups.iter().cloned())
                    .collect::<Vec<_>>()
            );
            pixels += actual.pixels.len();
            groups += actual.preparation.groups.len();
            let p = &actual.preparation.frame;
            p.audit().unwrap();
            coefficient += p.counts.logical_products.values().sum::<u64>();
            writes += p.counts.write_bits.values().sum::<u64>();
            for (op, count) in &p.counts.operations {
                *prep.entry(op).or_default() += count;
            }
            for ((got, golden), (_, ideal)) in
                actual.pixels.iter().zip(&expected.pixels).zip(continuous)
            {
                assert_eq!(got.rgb, golden.rgb);
                got.frame.audit().unwrap();
                for (actual, reference) in got.rgb.iter().zip(ideal) {
                    let error = (f64::from(*actual) - reference).abs();
                    max_error = max_error.max(error);
                    sum_error += error;
                }
                let c = &got.frame;
                products += c.counts.logical_products.values().sum::<u64>();
                for (op, count) in &c.counts.operations {
                    *color.entry(op).or_default() += count;
                }
                for (store, bits) in &c.counts.read_bits {
                    match c.memories[*store].name.as_str() {
                        "Group4" => payload_reads += bits,
                        "cache_RAW565" => texel_reads += bits,
                        _ => {}
                    }
                }
            }
            if case == 0 {
                fs::write(
                    root.join(format!("{profile}-golden.txt")),
                    format!(
                        "input={q:?}\npreparation={:#?}\ncolor={:#?}",
                        p.outputs,
                        actual
                            .pixels
                            .iter()
                            .map(|p| &p.frame.outputs)
                            .collect::<Vec<_>>()
                    ),
                )
                .unwrap();
            }
        }
        assert_eq!(cache.stats, reference_cache.stats);
        writeln!(summary,"{profile},128,{pixels},{groups},{coefficient},{products},{},{},{},{writes},{payload_reads},{texel_reads},{},{},{max_error},{}",prep.get("add").copied().unwrap_or(0)+prep.get("subtract").copied().unwrap_or(0),color.get("add").copied().unwrap_or(0)+color.get("subtract").copied().unwrap_or(0),prep.get("compare").copied().unwrap_or(0),cache.stats.refills,cache.stats.beats,sum_error/(pixels*3) as f64).unwrap();
        for (stage, counts) in [("prepare", prep), ("color", color)] {
            for (op, count) in counts {
                writeln!(operations, "{profile},{stage},{op},{count}").unwrap();
            }
        }
    }
    println!(
        "Audited {} quads; stage goldens and work summaries: {}",
        if args.len() > 1 { 1280 } else { 768 },
        root.display()
    );
}
