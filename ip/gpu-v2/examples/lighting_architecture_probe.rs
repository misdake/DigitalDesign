//! Bounded dataflow, resource-sensitivity and uniform-mode experiment.
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, timed::*},
};
use std::{fmt::Write, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-lighting/architecture-upgrade".into()),
    );
    fs::create_dir_all(&root)?;
    let base = Hardware::lighting_optimized_ii2();
    let data = Hardware {
        kernel: counted::Config::architecture(),
        ..base
    };
    let prepared = Hardware {
        kernel: counted::Config::prepared(),
        ..base
    };
    let diffuse = Material {
        specular_color: [0; 3],
        ..Material::default()
    };
    let pixel = PixelInput {
        normal: [7123, -519, 13567],
        ndc: [5308, 3086],
    };
    let mut out=String::from("profile,II,latency,half_slots,macros,bsram_pixel,ssram_pixel,retained_bits,normal_add18,normal_add36,increment18,logic_cones\n");
    for (name, hardware, material, ii) in [
        ("baseline", base, Material::default(), 2),
        ("dataflow", data, Material::default(), 2),
        ("prepared-ray", prepared, Material::default(), 2),
        (
            "dataflow+read",
            Hardware {
                normalize_reads: 7,
                ..data
            },
            Material::default(),
            2,
        ),
        (
            "dataflow+18x18",
            Hardware {
                large_multiply: 8,
                ..data
            },
            Material::default(),
            2,
        ),
        (
            "prepared+read",
            Hardware {
                normalize_reads: 7,
                ..prepared
            },
            Material::default(),
            2,
        ),
        (
            "prepared+18x18",
            Hardware {
                large_multiply: 8,
                ..prepared
            },
            Material::default(),
            2,
        ),
        ("diffuse", data, diffuse, 1),
        (
            "flat-full",
            Hardware {
                kernel: counted::Config {
                    flat_normal: true,
                    ..counted::Config::architecture()
                },
                ..Hardware::lighting_architecture_ii2()
            },
            Material::default(),
            2,
        ),
        (
            "flat-diffuse",
            Hardware {
                kernel: counted::Config {
                    flat_normal: true,
                    ..counted::Config::architecture()
                },
                ..Hardware::lighting_architecture_ii2()
            },
            diffuse,
            1,
        ),
        (
            "cones2",
            Hardware {
                cone_depth: 2,
                ..data
            },
            Material::default(),
            2,
        ),
        (
            "cones3",
            Hardware {
                cone_depth: 3,
                ..data
            },
            Material::default(),
            2,
        ),
        (
            "cones4",
            Hardware {
                cone_depth: 4,
                ..data
            },
            Material::default(),
            2,
        ),
        (
            "cones4-lat2",
            Hardware {
                cone_depth: 4,
                cone_latency: 2,
                ..data
            },
            Material::default(),
            2,
        ),
        (
            "cones4-prepared",
            Hardware {
                cone_depth: 4,
                ..prepared
            },
            Material::default(),
            2,
        ),
        (
            "cones4+read",
            Hardware {
                cone_depth: 4,
                normalize_reads: 7,
                ..data
            },
            Material::default(),
            2,
        ),
        (
            "cones4+18x18",
            Hardware {
                cone_depth: 4,
                large_multiply: 8,
                ..data
            },
            Material::default(),
            2,
        ),
        (
            "conservative+read",
            Hardware {
                normalize_reads: 7,
                ..Hardware::lighting_architecture_ii2()
            },
            Material::default(),
            2,
        ),
        (
            "conservative+18x18",
            Hardware {
                large_multiply: 8,
                ..Hardware::lighting_architecture_ii2()
            },
            Material::default(),
            2,
        ),
        (
            "conservative-diffuse",
            Hardware::lighting_architecture_ii2(),
            diffuse,
            1,
        ),
        (
            "cones4-diffuse",
            Hardware {
                cone_depth: 4,
                ..data
            },
            diffuse,
            1,
        ),
        (
            "shared-H",
            Hardware {
                kernel: counted::Config {
                    shared_half: true,
                    ..counted::Config::architecture()
                },
                ..base
            },
            Material::default(),
            2,
        ),
        (
            "shared-H-II1",
            Hardware {
                kernel: counted::Config {
                    shared_half: true,
                    ..counted::Config::architecture()
                },
                large_multiply: 8,
                paired_macros: 2,
                ..base
            },
            Material::default(),
            1,
        ),
    ] {
        let plan = plan(
            &[pixel],
            material,
            Light::default(),
            Projection::default(),
            hardware,
            Storage::Registers,
            Strategy::Interleaved,
        )?;
        let (ii, schedule) = (ii..=8)
            .find_map(|candidate| {
                PeriodicSchedule::search(&plan, candidate, 32)
                    .ok()
                    .map(|schedule| (candidate, schedule))
            })
            .ok_or_else(|| format!("profile {name}: no legal II in {ii}..8"))?;
        let phy = schedule.audit_physical(
            &plan,
            audited::physical::GowinMemoryBudget {
                bsram_blocks: 46,
                ssram_cells: 2048,
            },
            &audited::lifecycle::RegisterBudget {
                total_bits: 1000000,
                by_width: Default::default(),
            },
        )?;
        let adders = schedule.adder_inventory(&plan)?;
        fs::write(
            root.join(format!("{name}-adders.txt")),
            format!("{adders:#?}"),
        )?;
        let w = plan.work();
        let count = |k| w.get(&k).copied().unwrap_or(0);
        writeln!(
            out,
            "{name},{ii},{},{},{},{},{},{},{},{},{},{}",
            schedule.latency,
            hardware.multiplier_half_slots(),
            hardware.multiplier_macros(),
            phy.memory_cells.bsram_blocks,
            phy.memory_cells.ssram_cells,
            phy.retained.peak_bits,
            count(LaneKind::Add(18)),
            count(LaneKind::Add(36)),
            count(LaneKind::Increment(18)),
            plan.logic_cones().len()
        )?;
        let mut stages = String::from("stage,ready\n");
        for e in &plan.template.events {
            if let audited::Operation::Publish(ref n) = e.operation {
                writeln!(stages, "{n},{}", schedule.slots[e.id].ready)?;
            }
        }
        fs::write(root.join(format!("{name}-stages.csv")), stages)?;
        fs::write(
            root.join(format!("{name}-work.txt")),
            format!("{w:#?}\nphysical={:?}\n", phy.memory),
        )?;
    }
    let mut capacities = String::from("lanes,status\n");
    for lanes in [1, 2, 3] {
        let hardware = Hardware {
            cone_lanes_per_shape: lanes,
            ..Hardware::lighting_architecture_ii2()
        };
        let p = plan(
            &[pixel],
            Material::default(),
            Light::default(),
            Projection::default(),
            hardware,
            Storage::Registers,
            Strategy::Interleaved,
        )?;
        let status = match PeriodicSchedule::search(&p, 2, 32) {
            Ok(s) => format!("latency{}", s.latency),
            Err(e) => e,
        };
        writeln!(capacities, "{lanes},{status}")?;
    }
    fs::write(root.join("cone-capacities.csv"), capacities)?;
    fs::write(root.join("summary.csv"), &out)?;
    print!("{out}");
    Ok(())
}
