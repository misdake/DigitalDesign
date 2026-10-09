//! Bounded protocol-derived average oracle profiles, without board access.
use digital_design_hardware_gowin::sdram_memory_controller::{
    emu::service,
    ports::*,
    sim::{calibration, cycle_calibration, traffic::Load},
};
use std::{fs, io::Write, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("target/gpu-v2-sdram/early-model".into()),
    );
    fs::create_dir_all(&root)?;
    let mut csv = fs::File::create(root.join("calibration.csv"))?;
    writeln!(csv,"load,early_grant,chained_groups,access,bytes,samples,first_mean,complete_mean,p95,grant_wait_mean,self_queue_mean")?;
    let mut profiles = fs::File::create(root.join("profiles.txt"))?;
    let trace = calibration::representative_trace(128)?;
    for (early_grant, chained_groups) in [(false, false), (true, false), (true, true)] {
        for (name, load) in [
            ("solo", Load::solo()),
            ("cpu", Load::cpu()),
            ("display", Load::display(1)),
            ("both", Load::display_and_cpu(1)),
            ("batch50", Load::display_and_cpu(50)),
        ] {
            let report = cycle_calibration::analyze(
                &trace,
                cycle_calibration::Config {
                    service: service::Config {
                        early_grant,
                        chained_groups,
                        max_requests: 65536,
                        max_queued: 512,
                        ..Default::default()
                    },
                    load,
                    ..Default::default()
                },
            )?;
            for (i, s) in report.classes.iter().enumerate() {
                writeln!(
                    csv,
                    "{name},{early_grant},{chained_groups},{},{},{},{:.6},{:.6},{},{:.6},{:.6}",
                    if i < 4 { "read" } else { "write" },
                    BURST_BYTES[i % 4],
                    s.samples,
                    s.mean_first(),
                    s.mean_complete(),
                    s.p95(),
                    s.sum_grant_wait as f64 / s.samples as f64,
                    s.sum_self_queue as f64 / s.samples as f64
                )?;
            }
            writeln!(
                profiles,
                "{name} early={early_grant} group={chained_groups}: {:?}",
                report.profile()?
            )?;
            println!(
                "{name} early={early_grant} group={chained_groups} read512={:.2} write512={:.2} background={}",
                report.classes[3].mean_complete(),
                report.classes[7].mean_complete(),
                report.background_submitted
            );
        }
    }
    Ok(())
}
