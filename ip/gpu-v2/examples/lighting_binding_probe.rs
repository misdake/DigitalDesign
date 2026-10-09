//! Compare bindings with the same multiplier budget and unchanged counted math.
use gpu_v2::lighting::{ports::*, sim::timed::*};
use std::{fmt::Write, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-lighting/binding".into()),
    );
    fs::create_dir_all(&root)?;
    let pixels: Vec<_> = (0..32)
        .map(|i| PixelInput {
            normal: [7123, -519, 13567],
            ndc: [(i * 7919 % 131073 - 65536) / 4, 3086],
        })
        .collect();
    let mut summary=String::from("binding,batch,multiplier_half_slots,multiplier_macros,narrow_adders,wide_adders,first_result,cycles,cycles_per_pixel,best\n");
    let split = Hardware {
        binding: Binding::LightingDsp,
        narrow_adders: 8,
        incrementers_per_width: 8,
        rounders_per_width: 8,
        ..Hardware::default()
    };
    for (name, h) in [
        ("generic", Hardware::default()),
        ("increment-wiring-small8", split),
        ("pair-mac", Hardware::lighting_dsp()),
        (
            "pair-mac-wide1",
            Hardware {
                adders_per_width: 1,
                ..Hardware::lighting_dsp()
            },
        ),
    ] {
        for n in [1, 4, 8, 16, 32] {
            let baseline = plan(
                &pixels[..n],
                Material::default(),
                Light::default(),
                Projection::default(),
                h,
                Storage::Registers,
                Strategy::Interleaved,
            )?;
            let (best, outcome) = baseline.optimize(32)?;
            best.compare_oracle(
                &pixels[..n],
                Material::default(),
                Light::default(),
                Projection::default(),
            )?;
            writeln!(
                summary,
                "{name},{n},{},{},{},{},{},{},{:.3},{}",
                h.multiplier_half_slots(),
                h.multiplier_macros(),
                if h.binding == Binding::Generic {
                    h.adders_per_width
                } else {
                    h.narrow_adders
                },
                h.adders_per_width,
                best.writes[0].ready,
                best.cycles,
                best.cycles as f64 / n as f64,
                outcome.best_candidate().label
            )?;
            fs::write(
                root.join(format!("{name}-{n}.txt")),
                format!(
                    "work={:#?}\ngroups={:#?}\nwrites={:#?}\n",
                    best.work(),
                    best.fused_groups(),
                    best.writes
                ),
            )?;
        }
    }
    fs::write(root.join("summary.csv"), &summary)?;
    print!("{summary}");
    Ok(())
}
