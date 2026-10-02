//! Bounded end-to-end reservation and prepared-input cache/color measurements.
#[path = "../tests/support/sdram/cycle.rs"]
mod cycle;
#[path = "../tests/support/texture.rs"]
mod support;
use digital_design_hardware_gowin::sdram_memory_controller::{
    ports::OracleImage,
    sim::{average, oracle as memory, traffic::*},
};
use gpu_v2::texture::{
    ports::*,
    sim::{oracle, timed::*},
};
use std::{fs, io::Write, path::Path};
use support::*;

fn image(bytes: &[u8]) -> OracleImage {
    // SAFETY: independent external asset, outside the audited computation.
    unsafe {
        OracleImage::from_host(
            u64::from(BASE),
            bytes.to_vec(),
            "texture timing probe asset",
        )
        .unwrap()
    }
}
fn inputs(profile: &str, count: usize) -> Vec<QuadInput> {
    (0..count)
        .map(|i| {
            let uv = if profile == "seams" {
                [0.0; 2]
            } else if profile == "cold_scan" {
                [(i % 64 * 8) as f64 / 512.0 + 0.003, 0.003]
            } else {
                [
                    (i % 16 * 2) as f64 / 512.0 + 0.003,
                    (i / 16 * 2) as f64 / 512.0 + 0.003,
                ]
            };
            let mut q = input(
                9,
                if profile == "bilinear" || profile == "cold_scan" {
                    Filter::Bilinear
                } else {
                    Filter::Trilinear
                },
                uv,
            );
            q.quad_id = (i % 16) as u8;
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
fn golden(inputs: &[QuadInput], slot: Slot, bytes: &[u8]) -> Vec<PixelResult> {
    let mut direct = Image {
        bytes: bytes.to_vec(),
        requests: vec![],
    };
    let mut cache = oracle::Cache::new(vec![slot]).unwrap();
    inputs
        .iter()
        .flat_map(|q| {
            oracle::sample(q, &mut cache, &mut direct, Config::counted())
                .unwrap()
                .pixels
                .into_iter()
                .map(|p| PixelResult {
                    quad_id: q.quad_id,
                    lane: p.lane,
                    rgb: p.rgb,
                })
        })
        .collect()
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(
        args.first()
            .map(String::as_str)
            .unwrap_or("target/gpu-v2-texture-timed"),
    );
    fs::create_dir_all(root).unwrap();
    let slot = slot(9, true);
    let bytes = asset(slot, pattern);
    let mut csv = fs::File::create(root.join("summary.csv")).unwrap();
    writeln!(csv, "profile,memory,preparation,fifo,prefetch,result_capacity,quads,pixels,cycles,cycles_per_pixel,groups,group_util,prep_cycles,prep_dsp_util,peak_live_bits,producer_stalls,demand_wait,result_stalls,refills,overlap_reads,beat_read_edges,peak_fifo,peak_descriptors,hint_drops,promotions").unwrap();
    let mut evidence = fs::File::create(root.join("overlap.csv")).unwrap();
    writeln!(evidence, "profile,cycle,read_line,refill_line,beat").unwrap();
    for profile in ["bilinear", "fractional", "cold_scan", "seams"] {
        let inputs = inputs(profile, 64);
        let expected = golden(&inputs, slot, &bytes);
        let candidates = [
            (PreparationMode::Reserved, 32, true, 16, false),
            (PreparationMode::PreparedGroups, 16, false, 16, false),
            (PreparationMode::PreparedGroups, 16, true, 16, false),
            (PreparationMode::PreparedGroups, 32, false, 16, false),
            (PreparationMode::PreparedGroups, 32, true, 16, false),
            (PreparationMode::PreparedGroups, 32, true, 4, false),
            (PreparationMode::PreparedGroups, 32, true, 16, true),
        ];
        for (preparation, group_capacity, prefetch, result_capacity, loaded) in candidates {
            let h = Hardware {
                preparation,
                group_capacity,
                prefetch,
                result_capacity,
                ..Default::default()
            };
            let mut service: Box<dyn RefillPort> = if loaded {
                Box::new(cycle::Adapter::new(
                    memory::Memory::new(
                        image(&bytes),
                        memory::Config {
                            load: Load::display_and_cpu(50),
                            chain: ChainPolicy::ExistingUnchained,
                            ..Default::default()
                        },
                    )
                    .unwrap(),
                ))
            } else {
                Box::new(cycle::Adapter::new(
                    average::Memory::new(
                        image(&bytes),
                        average::Profile::gpu_default().unwrap(),
                        Default::default(),
                    )
                    .unwrap(),
                ))
            };
            let report = run(&inputs, &[slot], &mut *service, h, |_| Control::default()).unwrap();
            assert_eq!(report.pixels, expected);
            let s = &report.stats;
            let products: usize = report
                .programs
                .iter()
                .map(|p| p.arithmetic().multiplier_issues.len())
                .sum();
            let peak_live = report
                .programs
                .iter()
                .map(|p| p.arithmetic().live_bits)
                .max()
                .unwrap();
            let prep_work: u64 = report
                .programs
                .iter()
                .map(|p| p.arithmetic().schedule.cycles + 1)
                .sum();
            let mut overlaps = 0;
            for step in &report.steps {
                let beat = step.events.iter().find_map(|e| {
                    if let Event::Beat { line, index, .. } = e {
                        Some((*line, *index))
                    } else {
                        None
                    }
                });
                let read = step.events.iter().find_map(|e| {
                    if let Event::Read { line, .. } = e {
                        Some(*line)
                    } else {
                        None
                    }
                });
                if let (Some((line, index)), Some(read)) = (beat, read) {
                    overlaps += 1;
                    if preparation == PreparationMode::PreparedGroups
                        && group_capacity == 32
                        && prefetch
                        && result_capacity == 16
                        && !loaded
                    {
                        writeln!(evidence, "{profile},{},{read},{line},{index}", step.cycle)
                            .unwrap();
                    }
                }
            }
            let mem = if loaded {
                "display_cpu_unchained"
            } else {
                "calibrated_average"
            };
            writeln!(csv, "{profile},{mem},{preparation:?},{group_capacity},{prefetch},{result_capacity},{},{},{},{:.6},{},{:.6},{},{:.6},{},{},{},{},{},{},{},{},{},{},{}",
                inputs.len(), report.pixels.len(), s.wall_cycles, s.wall_cycles as f64 / report.pixels.len() as f64,
                s.reads, s.reads as f64 / s.enabled_cycles as f64, prep_work,
                products as f64 / (report.hardware.coefficient_lanes as u64 * prep_work) as f64,
                peak_live, s.producer_stalls, s.demand_wait_cycles, s.result_credit_stalls,
                s.refills, s.hits_during_refill, overlaps, s.peak_groups, s.peak_descriptors, s.hints_dropped, s.promotions).unwrap();
            println!("{profile:10} {preparation:?} fifo={group_capacity} prefetch={prefetch} result={result_capacity} {mem}: {:.3} cyc/pixel; {} refills; {overlaps} beat/read edges", s.wall_cycles as f64 / report.pixels.len() as f64, s.refills);
        }
    }
    // Optional natural images use the same addresses/stalls but independent data.
    if let Some(assets) = args.get(1) {
        for name in ["peppers", "mandrill", "sailboat", "airplane"] {
            let bytes = fs::read(Path::new(assets).join(format!("{name}.raw565"))).unwrap();
            let inputs = inputs("fractional", 64);
            let expected = golden(&inputs, slot, &bytes);
            let mut service = cycle::Adapter::new(
                average::Memory::new(
                    image(&bytes),
                    average::Profile::gpu_default().unwrap(),
                    Default::default(),
                )
                .unwrap(),
            );
            let report = run(
                &inputs,
                &[slot],
                &mut service,
                Hardware {
                    preparation: PreparationMode::PreparedGroups,
                    ..Default::default()
                },
                |_| Control::default(),
            )
            .unwrap();
            assert_eq!(report.pixels, expected);
            println!(
                "photo {name}: {} pixels match, {} cycles",
                report.pixels.len(),
                report.stats.wall_cycles
            );
        }
    }
}
