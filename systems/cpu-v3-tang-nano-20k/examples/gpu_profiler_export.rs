//! Export the current production GPU for observer-only profiler smoke tests.
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::VerilogProject;
use digital_design_hardware_gowin::sdram_memory_controller::combination;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("Expected output directory")?);
    std::fs::create_dir_all(&out)?;
    let mut project = VerilogProject::generate::<CpuV3Gpu>()?;
    if std::env::args().nth(2).as_deref() == Some("--mc") {
        // Production streaming transport and controller, with an accelerated
        // power-up delay only. The measured submission starts after init.
        for (path, mut source) in combination::rtl_sources_with_options(16, true, false)? {
            if path.to_str() == Some("combination.v") {
                let old = ".PREPARE_NEXT(1'b1), .CHAIN_GROUP_FOUR(1'b0)";
                if !source.contains(old) {
                    return Err("Unexpected production MC connection template".into());
                }
                source = source.replace(
                    old,
                    ".PREPARE_NEXT(1'b1), .CHAIN_GROUP_FOUR(1'b0), .CAS(3), .RCD_CYCLES(3), .RP_CYCLES(3), .CAPTURE_LATENCY(3), .REGISTER_READ_RETURN(1), .DQ_INPUT_DELAY_TAPS(32)",
                );
            }
            if let Some(previous) = project.files.insert(path, source.clone()) {
                if previous != source {
                    return Err("Conflicting dependency in GPU+MC export".into());
                }
            }
        }
    }
    let mut sources = Vec::new();
    for (index, (_, source)) in project.files.into_iter().enumerate() {
        let name = format!("source_{index}.v");
        std::fs::write(out.join(&name), source)?;
        sources.push(name);
    }
    std::fs::write(out.join("rtl.json"), serde_json::to_vec_pretty(&sources)?)?;
    Ok(())
}
