//! Mixed-mode transition bubbles, ordered results and CE replay.
use gpu_v2::lighting::{ports::*, sim::timed::*};
use std::{fmt::Write, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-lighting/architecture-upgrade".into()),
    );
    fs::create_dir_all(&root)?;
    let contexts = [
        StreamContext {
            material: Material::default(),
            light: Light::default(),
            projection: Projection::default(),
        },
        StreamContext {
            material: Material {
                specular_color: [0; 3],
                ..Default::default()
            },
            light: Light {
                ambient: 71,
                directional: 181,
                ..Light::default()
            },
            projection: Projection::default(),
        },
    ];
    let inputs: Vec<_> = (0..64)
        .map(|i| {
            (
                100 + i,
                PixelInput {
                    normal: [7123, -519, 13567],
                    ndc: [((i as i32 * 3000) % 131073 - 65536) / 4, 0],
                },
                if !(8..56).contains(&i) { 0 } else { 1 },
            )
        })
        .collect();
    let h = Hardware::lighting_architecture_ii2();
    let run = stream(&inputs, &contexts, h, 10000, &Default::default())?;
    let mut out = String::from("id,mode,accept,complete,accept_gap\n");
    for (i, r) in run.results.iter().enumerate() {
        let gap = if i == 0 {
            0
        } else {
            r.accepted - run.results[i - 1].accepted
        };
        writeln!(
            out,
            "{},{},{},{},{gap}",
            r.id,
            if r.context == 0 { "full" } else { "diffuse" },
            r.accepted,
            r.completed
        )?;
    }
    fs::write(root.join("stream.csv"), &out)?;
    println!("full latency={} diffuse latency={} full-to-diffuse first gap={} diffuse-to-full gap={} peak FIFO bits={} CE cycles={}",run.calendars[0].latency,run.calendars[1].latency,run.results[8].accepted-run.results[7].accepted,run.results[56].accepted-run.results[55].accepted,run.trace.final_state.peak_fifo_bits,run.trace.final_state.advancing_cycles);
    print!("{out}");
    Ok(())
}
