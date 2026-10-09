//! Deterministic bounded calibration of the shared SDRAM combination.
use digital_design_hardware_gowin::sdram_memory_controller::{
    ports::*,
    sim::{average::Profile, calibration, traffic::*},
};
use std::{fs, io::Write, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("target/gpu-v2-sdram".into()),
    );
    fs::create_dir_all(&directory)?;
    let trace = calibration::representative_trace(128)?;
    let mut csv = fs::File::create(directory.join("calibration.csv"))?;
    writeln!(csv,"load,chain,access,bytes,samples,first_mean,complete_mean,p95,arbiter_mean,bank_changes,hidden_prepare_core,refresh_wait_core,chained_sectors")?;
    let mut profiles = fs::File::create(directory.join("profiles.txt"))?;
    for chain in [
        ChainPolicy::ExistingUnchained,
        ChainPolicy::ChainedCandidate,
    ] {
        for (name, load) in [
            ("solo", Load::solo()),
            ("display", Load::display(1)),
            ("cpu", Load::cpu()),
            ("both", Load::display_and_cpu(1)),
            ("display-batch50", Load::display_and_cpu(50)),
        ] {
            let report = calibration::analyze(
                &trace,
                calibration::Config {
                    load,
                    chain,
                    ..Default::default()
                },
            )?;
            for (i, s) in report.classes.iter().enumerate() {
                writeln!(
                    csv,
                    "{name},{chain:?},{},{},{},{:.6},{:.6},{},{:.6},{},{},{},{}",
                    if i < 4 { "read" } else { "write" },
                    BURST_BYTES[i % 4],
                    s.samples,
                    s.mean_first(),
                    s.mean_complete(),
                    s.p95(),
                    s.mean_arbiter(),
                    s.bank_changes,
                    s.hidden_prepare_core,
                    s.refresh_core,
                    s.chained_sectors
                )?;
            }
            writeln!(
                profiles,
                "{name}/{chain:?}: {:?}",
                Profile::from_calibration(&report)?
            )?;
            println!(
                "PASS {name}/{chain:?}: {} GPU samples, {} background requests",
                trace.len(),
                report.background_requests
            );
        }
    }
    println!("reports: {}", directory.display());
    Ok(())
}
