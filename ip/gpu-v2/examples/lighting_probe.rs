//! Reproducible bounded probe. Detailed reservations stay in the supplied directory.
use gpu_v2::lighting::{
    ports::*,
    sim::{counted, timed::*},
};
use std::{fmt::Write, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/gpu-v2-lighting".into()),
    );
    fs::create_dir_all(&root)?;
    let pixels: Vec<_> = (0..16)
        .map(|i| PixelInput {
            normal: [7123, -519, 13567],
            ndc: [i * 7919 % 131073 - 65536, 12345],
        })
        .collect();
    let (m, l, p) = (Material::default(), Light::default(), Projection::default());
    let counted = counted::evaluate(pixels[0], m, l, p, 2048).map_err(|e| format!("{e:?}"))?;
    fs::write(
        root.join("counted.txt"),
        format!(
            "counts={:#?}\noutputs={:#?}\n",
            counted.frame.counts, counted.frame.outputs
        ),
    )?;
    let mut summary =
        String::from("hardware,storage,batch,serial,interleaved,search,best_candidate\n");
    for (label, h) in [
        ("default", Hardware::default()),
        (
            "restricted",
            Hardware {
                small_multiply: 1,
                large_multiply: 1,
                normalize_reads: 1,
                ..Hardware::default()
            },
        ),
    ] {
        for (store, s) in [
            ("registers", Storage::Registers),
            (
                "rows1",
                Storage::Rows {
                    read_lanes: 1,
                    latency: 2,
                },
            ),
            (
                "rows2",
                Storage::Rows {
                    read_lanes: 2,
                    latency: 2,
                },
            ),
        ] {
            for n in [1, 2, 4, 8, 16] {
                let serial = plan(&pixels[..n], m, l, p, h, s, Strategy::Serial)?;
                let interleaved = plan(&pixels[..n], m, l, p, h, s, Strategy::Interleaved)?;
                let baseline = interleaved.cycles;
                let (best, search) = interleaved.optimize(32)?;
                best.compare_oracle(&pixels[..n], m, l, p)?;
                writeln!(
                    summary,
                    "{label},{store},{n},{},{baseline},{},{}",
                    serial.cycles,
                    best.cycles,
                    search.best_candidate().label
                )?;
                let prefix = format!("{label}-{store}-{n}");
                let mut events = String::from("pixel,event,resource,lane,issue,ready\n");
                for r in &best.events {
                    writeln!(
                        events,
                        "{},{},{:?},{:?},{},{}",
                        r.pixel, r.event, r.kind, r.lane, r.issue, r.ready
                    )?;
                }
                fs::write(root.join(format!("{prefix}-events.csv")), events)?;
                fs::write(
                    root.join(format!("{prefix}-io.txt")),
                    format!(
                        "reads={:#?}\nwrites={:#?}\noutputs={:?}\n",
                        best.reads, best.writes, best.outputs
                    ),
                )?;
                let mut candidates =
                    String::from("candidate,seed,cycles,live_node_pressure,feasible\n");
                for c in &search.candidates {
                    writeln!(
                        candidates,
                        "{},{:?},{},{},{}",
                        c.label, c.seed, c.makespan, c.live_pressure, c.within_deadline
                    )?;
                }
                fs::write(root.join(format!("{prefix}-candidates.csv")), candidates)?;
            }
        }
    }
    fs::write(root.join("summary.csv"), &summary)?;
    print!("{summary}");
    println!("reports: {}", root.display());
    Ok(())
}
