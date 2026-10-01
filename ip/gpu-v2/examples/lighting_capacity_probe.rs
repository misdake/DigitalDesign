//! Bound the effect of extra adders, then isolate the next logic bottlenecks.
use gpu_v2::lighting::{ports::*, sim::timed::*};
use std::{fmt::Write, fs, path::PathBuf};

fn capacity(h: Hardware, k: &LaneKind) -> usize {
    match k {
        LaneKind::LogicCone { .. } => h.cone_lanes_per_shape,
        LaneKind::SmallMultiply => h.small_multiply,
        LaneKind::LargeMultiply => h.large_multiply,
        LaneKind::PairMultiplyAdd => h.paired_macros,
        LaneKind::Negate(_) => h.negators_per_width,
        LaneKind::Increment(_) => h.incrementers_per_width,
        LaneKind::Add(18) => h.narrow_adders,
        LaneKind::Add(_) => h.adders_per_width,
        LaneKind::Compare(_) => h.compares_per_width,
        LaneKind::Select(_) => h.selects_per_width,
        LaneKind::Shift(_) => h.shifts_per_width,
        LaneKind::Round(_) => h.rounders_per_width,
        LaneKind::LeadingZeros(_) => h.leading_zeros_per_width,
        LaneKind::PowerRead | LaneKind::ContextRead => 1,
        LaneKind::NormalizeRead => h.normalize_reads,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-lighting/capacity".into()),
    );
    fs::create_dir_all(&root)?;
    let pixels: Vec<_> = (0..64)
        .map(|i| PixelInput {
            normal: [7123, -519, 13567],
            ndc: [i * 7919 % 131073 - 65536, 12345],
        })
        .collect();
    let base = Hardware::lighting_dsp();
    let add8 = Hardware {
        adders_per_width: 8,
        ..base
    };
    let mut profiles = vec![("pair-mac", base)];
    for lanes in [8, 16, 32] {
        profiles.push((
            match lanes {
                8 => "add-round8",
                16 => "add-round16",
                _ => "add-round32",
            },
            Hardware {
                narrow_adders: lanes,
                adders_per_width: lanes,
                incrementers_per_width: lanes,
                rounders_per_width: lanes,
                ..base
            },
        ));
    }
    profiles.extend([
        (
            "add8-compare4",
            Hardware {
                compares_per_width: 4,
                ..add8
            },
        ),
        (
            "add8-compare8",
            Hardware {
                compares_per_width: 8,
                ..add8
            },
        ),
        (
            "add8-compare8-select8",
            Hardware {
                compares_per_width: 8,
                selects_per_width: 8,
                ..add8
            },
        ),
        (
            "logic8",
            Hardware {
                compares_per_width: 8,
                selects_per_width: 8,
                shifts_per_width: 8,
                ..add8
            },
        ),
    ]);
    for lanes in [16, 32] {
        profiles.push((
            if lanes == 16 { "logic16" } else { "logic32" },
            Hardware {
                narrow_adders: lanes,
                adders_per_width: lanes,
                incrementers_per_width: lanes,
                rounders_per_width: lanes,
                compares_per_width: lanes,
                selects_per_width: lanes,
                shifts_per_width: lanes,
                ..base
            },
        ));
    }
    let mut summary = String::from(
        "profile,batch,first_result,cycles,cycles_per_pixel,resource_bound_per_pixel,bound_kind,best\n",
    );
    for (name, h) in profiles {
        assert_eq!(h.multiplier_half_slots(), base.multiplier_half_slots());
        assert_eq!(h.multiplier_macros(), base.multiplier_macros());
        for n in [1, 16, 32, 64] {
            let p = plan(
                &pixels[..n],
                Material::default(),
                Light::default(),
                Projection::default(),
                h,
                Storage::Registers,
                Strategy::Interleaved,
            )?;
            let (best, outcome) = p.optimize(32)?;
            best.compare_oracle(
                &pixels[..n],
                Material::default(),
                Light::default(),
                Projection::default(),
            )?;
            let mut bounds: Vec<_> = best
                .work()
                .into_iter()
                .map(|(k, work)| {
                    let lanes = capacity(h, &k);
                    let per_pixel = work / n;
                    (k, per_pixel, lanes, work as f64 / n as f64 / lanes as f64)
                })
                .collect();
            bounds.sort_by(|a, b| b.3.total_cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
            writeln!(
                summary,
                "{name},{n},{},{},{:.4},{:.4},{:?},{}",
                best.writes[0].ready,
                best.cycles,
                best.cycles as f64 / n as f64,
                bounds[0].3,
                bounds[0].0,
                outcome.best_candidate().label
            )?;
            if n == 64 {
                fs::write(
                    root.join(format!("{name}-64.txt")),
                    format!(
                        "hardware={h:#?}\nbounds (kind, work/pixel, lanes, cycles/pixel)={bounds:#?}\nwrites={:#?}\n",
                        best.writes
                    ),
                )?;
            }
        }
        // Emit one compact checkpoint per completed profile.
        println!("completed profile={name}");
    }
    fs::write(root.join("summary.csv"), &summary)?;
    print!("{summary}");
    Ok(())
}
