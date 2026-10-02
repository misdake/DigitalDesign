//! Prove an II=2 static calendar, then expand it through the finite-plan audit.
use gpu_v2::lighting::{ports::*, sim::timed::*};
use std::{fmt::Write, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-lighting/ii2".into()),
    );
    fs::create_dir_all(&root)?;
    let profile = std::env::args().nth(2).unwrap_or_else(|| "baseline".into());
    let hardware = match profile.as_str() {
        "baseline" => Hardware::lighting_ii2(),
        "optimized" => Hardware::lighting_optimized_ii2(),
        "optimized-extra1" => Hardware {
            large_multiply: 8,
            ..Hardware::lighting_optimized_ii2()
        },
        "optimized-extra2" => Hardware {
            large_multiply: 9,
            ..Hardware::lighting_optimized_ii2()
        },
        _ => return Err("unknown II=2 probe profile".into()),
    };
    if matches!(profile.as_str(), "baseline" | "optimized") {
        assert_eq!(
            hardware.multiplier_half_slots(),
            Hardware::default().multiplier_half_slots()
        );
        assert_eq!(
            hardware.multiplier_macros(),
            Hardware::default().multiplier_macros()
        );
    }
    let pixels: Vec<_> = (0..64)
        .map(|i| PixelInput {
            normal: [7123, -519, 13567],
            ndc: [i * 7919 % 131073 - 65536, 12345],
        })
        .collect();
    let reference = plan(
        &pixels[..1],
        Material::default(),
        Light::default(),
        Projection::default(),
        hardware,
        Storage::Registers,
        Strategy::Interleaved,
    )?;
    let periodic = PeriodicSchedule::search(&reference, 2, 32)?;
    let initial_physical = periodic.audit_physical(
        &reference,
        audited::physical::GowinMemoryBudget {
            bsram_blocks: 46,
            ssram_cells: 2048,
        },
        &audited::lifecycle::RegisterBudget {
            total_bits: 1_000_000,
            by_width: Default::default(),
        },
    )?;
    let compact = periodic.clone().compact_lifetimes(&reference)?;
    let compact_physical = compact.audit_physical(
        &reference,
        audited::physical::GowinMemoryBudget {
            bsram_blocks: 46,
            ssram_cells: 2048,
        },
        &audited::lifecycle::RegisterBudget {
            total_bits: 1_000_000,
            by_width: Default::default(),
        },
    )?;
    println!(
        "retained bits initial={} compact={}",
        initial_physical.retained.peak_bits, compact_physical.retained.peak_bits
    );
    let (periodic, physical) =
        if compact_physical.retained.peak_bits <= initial_physical.retained.peak_bits {
            (compact, compact_physical)
        } else {
            (periodic, initial_physical)
        };
    fs::write(root.join("physical.txt"),format!("DSP={:?}\nMemory={:?}\nCells={:?}\nPeak retained bits={}\nBy width={:?}\nIntervals={:#?}\n",physical.dsp,physical.memory,physical.memory_cells,physical.retained.peak_bits,physical.retained.peak_bits_by_width,physical.retained.intervals))?;
    let mut calendar = String::from("event,kind,lane,phase,issue,ready\n");
    for s in &periodic.slots {
        if let Some(k) = &s.kind {
            writeln!(
                calendar,
                "{},{k:?},{},{},{},{}",
                s.event,
                s.lane.unwrap(),
                s.issue % 2,
                s.issue,
                s.ready
            )?;
        }
    }
    fs::write(root.join("calendar.csv"), &calendar)?;
    let mut summary =
        String::from("batch,first_result,last_result,cycles_per_pixel,output_spacing\n");
    for n in [1, 4, 16, 32, 64] {
        let p = plan(
            &pixels[..n],
            Material::default(),
            Light::default(),
            Projection::default(),
            hardware,
            Storage::Registers,
            Strategy::Interleaved,
        )?;
        let expanded = periodic.expand(p)?;
        expanded.compare_oracle(
            &pixels[..n],
            Material::default(),
            Light::default(),
            Projection::default(),
        )?;
        assert!(expanded
            .writes
            .windows(2)
            .all(|w| w[1].ready == w[0].ready + 2));
        assert_eq!(expanded.cycles, periodic.latency + 2 * (n as u64 - 1));
        writeln!(
            summary,
            "{n},{},{},{:.4},2",
            expanded.writes[0].ready,
            expanded.cycles,
            expanded.cycles as f64 / n as f64
        )?;
    }
    fs::write(root.join("summary.csv"), &summary)?;
    fs::write(
        root.join("hardware.txt"),
        format!(
            "{hardware:#?}\nII={} latency={}\nwork={:#?}\n",
            periodic.initiation_interval,
            periodic.latency,
            reference.work()
        ),
    )?;
    println!(
        "II={} latency={} DSP-half-slots={} macros={}",
        periodic.initiation_interval,
        periodic.latency,
        hardware.multiplier_half_slots(),
        hardware.multiplier_macros()
    );
    print!("{summary}");
    Ok(())
}
