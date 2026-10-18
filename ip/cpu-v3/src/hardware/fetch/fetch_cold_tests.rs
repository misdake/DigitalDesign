use super::*;
use digital_design_hardware::HardwareIdentity;

#[test]
#[ignore = "no-reset cold CSEG7 first fetch, spare/victim installation and BTC replay"]
fn cold_frontend_initial_ownership_installs_and_replays() {
    use std::{path::PathBuf, process::Command};
    let evidence = std::env::var_os("CPU_V3_BTC_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("cpu-v3-cold-fetch-{}", std::process::id()))
        })
        .join("cold-no-reset");
    std::fs::create_dir_all(&evidence).unwrap();
    let project =
        digital_design_hardware::VerilogProject::generate::<CpuV3InstructionFetchQueue>().unwrap();
    let sources = project.files.into_values().collect::<Vec<_>>().join("\n");
    let tb = include_str!("fetch_cold_tb.v").replace(
        "__FETCH_QUEUE__",
        &CpuV3InstructionFetchQueue::verilog_identity().module_name(),
    );
    std::fs::write(evidence.join("modules.v"), sources).unwrap();
    std::fs::write(evidence.join("tb.v"), tb).unwrap();
    let compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()))
            .current_dir(&evidence)
            .args(["-g2012", "-s", "tb", "-o", "sim.vvp", "modules.v", "tb.v"])
            .output()
            .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
        .current_dir(&evidence)
        .arg("sim.vvp")
        .output()
        .unwrap();
    std::fs::write(evidence.join("run.log"), &run.stdout).unwrap();
    assert!(
        run.status.success() && String::from_utf8_lossy(&run.stdout).contains("PASS cold frontend"),
        "{} {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
