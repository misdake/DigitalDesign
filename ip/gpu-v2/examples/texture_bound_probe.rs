//! Universal periodic preparation + bounded cache/color + actual cycle MC.
//! Default MC remains serial; no prefetch or average timing substitution.
#[path = "../tests/support/sdram/physical_texture.rs"]
pub(crate) mod physical;
#[path = "../tests/support/texture.rs"]
#[allow(dead_code)]
pub(crate) mod support;
use gpu_v2::texture::{
    ports::*,
    sim::{oracle, staged::bound, timed},
};
use std::{fs, io::Write, path::Path};
fn configurations() -> Vec<(bound::control::Storage, usize, bool)> {
    [
        bound::control::Storage::Dedicated,
        bound::control::Storage::Packed,
    ]
    .into_iter()
    .flat_map(|storage| {
        [(5, true), (8, false), (8, true)]
            .into_iter()
            .map(move |(contexts, early)| (storage, contexts, early))
    })
    .collect()
}
pub(crate) fn inputs(profile: &str, mask: u8, count: usize) -> Vec<QuadInput> {
    (0..count)
        .map(|i| {
            let i0 = i % 64;
            let uv = match profile {
                "seams" => [0.0; 2],
                "cold-scan" => [(i * 8) as f64 / 512.0 + 0.003, 0.011],
                _ => [
                    (i0 % 16 * 2) as f64 / 512.0 + 0.003,
                    (i0 / 16 * 2) as f64 / 512.0 + 0.003,
                ],
            };
            let mut q = support::input(
                9,
                if profile == "bilinear" || profile == "cold-scan" {
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
            .unwrap_or("target/gpu-v2-texture-bound"),
    );
    fs::create_dir_all(root).unwrap();
    let b = bound::Binding::build().unwrap();
    let mut layout = fs::File::create(root.join("layout.csv")).unwrap();
    writeln!(
        layout,
        "stage,period,value,source_low,width,iteration,physical_low,live_phase_mask,read_offsets"
    )
    .unwrap();
    let mut stages = fs::File::create(root.join("stages.csv")).unwrap();
    writeln!(
        stages,
        "stage,II,primitive_span,registered_transfer,FF_bits,packed_FF_bits,packed_peak_bits,packed_control_FF,packed_write_mux_bits,packed_read_mux_bits,packed_control_gates,DSP_pipeline_bits,RAM16x1_cells,operand_mux_tree_bits,sites"
    )
    .unwrap();
    for (name, p) in [
        ("D", &b.derivative),
        ("LOD", &b.lod),
        ("coord", &b.coordinate),
        ("coeff", &b.coefficient),
        ("membership", &b.plane),
        ("packet", &b.packet),
    ] {
        writeln!(
            stages,
            "{name},{},{},1,{},{},{},{},{},{},{},{},{},{},\"{:?}\"",
            p.ii(),
            p.span(),
            p.fixed_ff_bits,
            p.packed.ff_bits,
            p.packed.peak_live_bits,
            p.packed.control_ff_bits,
            p.packed.write_selector_tree_bits,
            p.packed.read_selector_tree_bits,
            p.packed.control_boolean_gates,
            p.dsp_pipeline_bits,
            p.rom_ram16_cells,
            p.operand_mux_tree_bits,
            p.sites
        )
        .unwrap();
        println!(
            "{name} dedicated={} packed={} periodicPeak={} writeMux={} readMux={}",
            p.fixed_ff_bits,
            p.packed.ff_bits,
            p.packed.peak_live_bits,
            p.packed.write_selector_tree_bits,
            p.packed.read_selector_tree_bits
        );
        for allocation in &p.packed.placements {
            let field = p
                .packed_fields
                .iter()
                .find(|f| f.value == allocation.value && f.source_low == allocation.source_low)
                .unwrap();
            writeln!(
                layout,
                "{name},{},{},{},{},{},{},{:016x},\"{:?}\"",
                p.packed.period,
                allocation.value,
                allocation.source_low,
                allocation.width,
                allocation.iteration,
                allocation.low,
                allocation.live_phases,
                field.read_times
            )
            .unwrap();
        }
    }
    let mut storage = fs::File::create(root.join("storage.csv")).unwrap();
    writeln!(storage, "layout,contexts,early_release,allocation,FF_bits,RAM16SDP4_cells,framework_RAM16x1_cells,BSRAM,hard_pipeline_bits,ports").unwrap();
    let cache = timed::Hardware {
        preparation: timed::PreparationMode::BoundStages,
        prefetch: false,
        ..Default::default()
    };
    for (mode, contexts, early) in configurations() {
        let prep = bound::control::Hardware {
            contexts,
            storage: mode,
            release_after_capture: early,
            ..Default::default()
        };
        let bill = bound::inventory::describe(&b, prep, &cache).unwrap();
        for row in &bill.rows {
            writeln!(
                storage,
                "{mode:?},{contexts},{early},{},{},{},{},{},{},\"{}\"",
                row.name,
                row.ff_bits,
                row.sdp4_cells,
                row.ram16x1_cells,
                row.bsram,
                row.hard_pipeline_bits,
                row.ports
            )
            .unwrap();
        }
        println!("storage layout={mode:?} contexts={contexts} early={early}: FF={} SDP4={} conservativeRAM16x1={} BSRAM={} DSP18={} read_mux_tree_bits={} write_mux_tree_bits={} storage_control_gates={} operand_mux_tree_bits={}",
            bill.ff_bits, bill.sdp4_cells, bill.ram16x1_cells, bill.bsram, bill.dsp18, bill.rotating_read_mux_bits, bill.rotating_write_mux_bits, bill.storage_control_boolean_gates, bill.operand_mux_tree_bits);
    }
    if args.get(1).is_some_and(|a| a == "--storage-only") {
        return;
    }
    let mut csv = fs::File::create(root.join("performance.csv")).unwrap();
    let mut traffic = fs::File::create(root.join("storage_traffic.csv")).unwrap();
    writeln!(traffic, "profile,mask,quads,contexts,early_release,loaded,MC_init_inside_batch,stage,observed_peak_live_bits,bit_writes,bit_reads").unwrap();
    writeln!(csv, "layout,profile,mask,quads,contexts,early_release,loaded,MC_init_inside_batch,init_cycles_excluded,pixels,groups,batch_cycles,batch_cycles_per_pixel,steady_pixels,steady_cycles,steady_cycles_per_pixel,steady_GPU_submissions,refills,demand_wait,group_highwater,contexts_highwater,coordinate_highwater,work_highwater,packet_highwater,live_highwater,coefficient_products").unwrap();
    let slot = support::slot(9, true);
    let bytes = support::asset(slot, support::pattern);
    for (profile, mask, count, loaded, cold_init) in [
        ("bilinear", 15, 128, false, false),
        ("fractional", 15, 128, false, false),
        ("seams", 15, 128, false, false),
        ("fractional", 1, 128, false, false),
        ("fractional", 9, 128, false, false),
        ("cold-scan", 15, 64, false, false),
        ("bilinear", 15, 128, true, false),
        ("bilinear", 15, 1, false, false),
        ("fractional", 15, 4, false, false),
        ("bilinear", 15, 1, false, true),
    ] {
        let qs = inputs(profile, mask, count);
        let mut image = support::Image {
            bytes: bytes.clone(),
            requests: vec![],
        };
        let mut gold_cache = oracle::Cache::new(vec![slot]).unwrap();
        let expected: Vec<_> = qs
            .iter()
            .flat_map(|q| {
                oracle::sample(q, &mut gold_cache, &mut image, Config::counted())
                    .unwrap()
                    .pixels
                    .into_iter()
                    .map(|p| timed::PixelResult {
                        quad_id: q.quad_id,
                        lane: p.lane,
                        rgb: p.rgb,
                    })
            })
            .collect();
        for (mode, contexts, early) in configurations() {
            let prep = bound::control::Hardware {
                contexts,
                storage: mode,
                release_after_capture: early,
                ..Default::default()
            };
            let mut memory = physical::Physical::new(
                u64::from(support::BASE),
                bytes.clone(),
                !cold_init,
                loaded,
            );
            let r = bound::system::run(&qs, &[slot], &mut memory, prep, cache.clone(), |_| {
                Default::default()
            })
            .unwrap();
            assert_eq!(r.cache.pixels, expected);
            if mode == bound::control::Storage::Packed {
                for (name, t) in ["D", "LOD", "coord", "coeff", "membership", "packet"]
                    .into_iter()
                    .zip(bound::storage::audit_trace(&r).unwrap())
                {
                    writeln!(traffic, "{profile},{mask},{count},{contexts},{early},{loaded},{cold_init},{name},{},{},{}",
                        t.peak_live_bits, t.bit_writes, t.bit_reads).unwrap();
                }
            }
            let commits: Vec<_> = r
                .cache
                .steps
                .iter()
                .flat_map(|s| {
                    s.events.iter().filter_map(move |e| {
                        matches!(e, timed::Event::Commit { .. }).then_some(s.cycle)
                    })
                })
                .collect();
            let (steady_pixels, steady_cycles, submissions) = if count >= 128 {
                // Middle 32 quads of the repeated second pass (80..111).
                // Commit edges exclude batch fill and final drain. Actual
                // refill submissions qualify warmth; no inferred cache hit.
                let start = commits[commits.len() * 5 / 8 - 1];
                let end = commits[commits.len() * 7 / 8 - 1];
                let submissions = r
                    .cache
                    .steps
                    .iter()
                    .filter(|s| s.cycle > start && s.cycle <= end)
                    .flat_map(|s| &s.events)
                    .filter(|e| matches!(e, timed::Event::Submitted { .. }))
                    .count();
                (commits.len() / 4, end - start, submissions)
            } else {
                (0, 0, 0)
            };
            let s = &r.cache.stats;
            let p = &r.stats;
            let cpp = s.wall_cycles as f64 / expected.len() as f64;
            let steady = if steady_pixels == 0 {
                f64::NAN
            } else {
                steady_cycles as f64 / steady_pixels as f64
            };
            let products: usize = r
                .programs
                .iter()
                .flat_map(|p| &p.preparation().lanes)
                .map(|l| {
                    l.coefficient
                        .frame
                        .events
                        .iter()
                        .filter(|e| e.operation == audited::Operation::Multiply)
                        .count()
                })
                .sum();
            writeln!(csv, "{mode:?},{profile},{mask},{count},{contexts},{early},{loaded},{cold_init},{},{},{},{},{cpp:.6},{steady_pixels},{steady_cycles},{steady:.6},{submissions},{},{},{},{},{},{},{},{},{products}",
                memory.init_cycles, expected.len(), s.reads, s.wall_cycles, s.refills, s.demand_wait_cycles,
                s.peak_groups, p.peak_contexts, p.peak_coordinates, p.peak_work, p.peak_packet, p.peak_live).unwrap();
            println!("{mode:?} {profile} mask={mask} n={count} context={contexts}/{early} loaded={loaded} coldInit={cold_init}: batch={cpp:.4} steady={steady:.4} hotSubmits={submissions} refills={}", s.refills);
        }
    }
}
