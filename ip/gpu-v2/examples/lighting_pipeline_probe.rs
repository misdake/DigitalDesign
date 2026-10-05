//! Matched-budget experiment over reusable typed blocks and existing baselines.
use audited::{lifecycle::RegisterBudget, physical::GowinMemoryBudget};
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, pipeline::BlockKind, timed::*},
};
use std::{fmt::Write, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/lighting-pipeline-blocks-20261005/probe".into()),
    );
    fs::create_dir_all(&root)?;
    let pixels: Vec<_> = (0..16)
        .map(|i| CompactPixelInput {
            normal: [
                [377, -286, 939],
                [-2048, 2047, 1],
                [1, 0, -1],
                [0; 3],
                [1024, 0, 0],
            ][i % 5],
            ndc: [(i as i32 * 7919 % 131073) - 65536, 31457],
        })
        .collect();
    let mut csv=String::from("mode,profile,rom_latency,ii,latency,peak_bits,storage_compacted_bits,cones,blocks_by_shape,normal_alu_sites,increment_or_negate_sites,bsram_blocks,ssram_cells,dsp_tiles_used,capacity_half_slots,finite_cycles,finite_first_result,cone_provisioned,cone_occupied,normal_provisioned,normal_occupied,increment_or_negate_provisioned,increment_or_negate_occupied\n");
    for full in [false, true] {
        let m = Material {
            specular_color: if full { [255; 3] } else { [0; 3] },
            ..Default::default()
        };
        for rom in [1, 2] {
            for (name, depth, measured, lanes) in [
                ("primitive", 0, false, 3),
                ("depth4-two-cycle", 4, false, 3),
                ("measured", 0, true, 3),
                ("measured-two-lanes", 0, true, 2),
                ("functions-with-cones", 4, false, 3),
            ] {
                let h = Hardware {
                    kernel: counted::Config::compact(),
                    cone_depth: depth,
                    measured_blocks: measured,
                    measured_functions: name == "functions-with-cones",
                    cone_latency: if depth == 0 { 1 } else { 2 },
                    cone_lanes_per_shape: lanes,
                    rom_latency: rom,
                    ..Hardware::lighting_architecture_ii2()
                };
                let plan = plan_compact(
                    &pixels,
                    m,
                    Light::default(),
                    Projection::default(),
                    h,
                    Storage::Registers,
                    Strategy::Interleaved,
                )?;
                plan.compare_oracle(
                    &pixels
                        .iter()
                        .map(|p| p.expanded().unwrap())
                        .collect::<Vec<_>>(),
                    m,
                    Light::default(),
                    Projection::default(),
                )?;
                fs::write(
                    root.join(format!("{full}-{name}-rom{rom}-work.txt")),
                    format!(
                        "capacity={h:#?}\nwork={:#?}\ncones={:#?}\n",
                        plan.work(),
                        plan.logic_cones()
                    ),
                )?;
                for ii in [1, 2, 3] {
                    let calendar = match PeriodicSchedule::search(&plan, ii, 16) {
                        Ok(p) => p,
                        Err(e) => {
                            fs::write(
                                root.join(format!("{full}-{name}-rom{rom}-ii{ii}-rejected.txt")),
                                e,
                            )?;
                            continue;
                        }
                    };
                    let phy = calendar.audit_physical(
                        &plan,
                        GowinMemoryBudget {
                            bsram_blocks: 46,
                            ssram_cells: 2048,
                        },
                        &RegisterBudget {
                            total_bits: 1_000_000,
                            by_width: Default::default(),
                        },
                    )?;
                    let compact = calendar.clone().compact_storage_bounded(&plan, 32)?;
                    let compact_bits = compact.retained_values(&plan)?.peak_bits;
                    let adders = calendar.adder_inventory(&plan)?;
                    let normal_sites: usize = adders.normal_sites_by_width.values().sum();
                    let increments: usize = adders.increment_sites_by_width.values().sum();
                    let shape_counts = plan
                        .work()
                        .into_iter()
                        .filter_map(|(kind, n)| match kind {
                            LaneKind::LogicCone { shape, .. } if measured => {
                                Some(format!("{shape}:{n}"))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(";");
                    // Generic shapes contain verbose opcode lists (and commas).
                    // Keep those in work.txt; CSV has only reusable block names.
                    let shape_counts = format!("\"{}\"", shape_counts.replace('"', "\"\""));
                    writeln!(csv,"{},{name},{rom},{ii},{},{},{},{},{shape_counts},{normal_sites},{increments},{},{},{},{},{},{},{},{},{},{},{},{}",if full {"full"} else {"diffuse"},calendar.latency,phy.retained.peak_bits,compact_bits,plan.logic_cones().len(),phy.memory_cells.bsram_blocks,phy.memory_cells.ssram_cells,phy.dsp.tiles,h.multiplier_half_slots(),plan.cycles,plan.writes[0].ready,adders.cone_provisioned,adders.cone_occupied,adders.normal_provisioned.values().sum::<usize>(),adders.normal_occupied.values().sum::<usize>(),adders.increment_provisioned.values().sum::<usize>(),adders.increment_occupied.values().sum::<usize>())?;
                    fs::write(root.join(format!("{full}-{name}-rom{rom}-ii{ii}-calendar.txt")),format!("calendar={calendar:#?}\ncompact={compact:#?}\nadders={adders:#?}\nlayout={:#?}\naccesses={:#?}\nretained={:#?}\n",phy.layout,phy.accesses,phy.retained))?;
                }
            }
        }
    }
    fs::write(root.join("summary.csv"), &csv)?;
    fs::write(
        root.join("blocks.txt"),
        [
            BlockKind::NormalizedOutput { zero_gate: false },
            BlockKind::NormalizedOutput { zero_gate: true },
            BlockKind::ReciprocalTail,
            BlockKind::DiffuseFinish,
            BlockKind::InverseHead,
            BlockKind::InverseTail,
            BlockKind::SquareSum,
            BlockKind::PowerHead,
        ]
        .map(|k| format!("{}: {}", k.label(), k.evidence()))
        .join("\n"),
    )?;
    println!(
        "PASS matched-budget pipeline experiment; detailed evidence: {}",
        root.display()
    );
    Ok(())
}
