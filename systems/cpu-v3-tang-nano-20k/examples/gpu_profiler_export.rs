//! Export the current production GPU for observer-only profiler smoke tests.
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::VerilogProject;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = PathBuf::from(std::env::args().nth(1).ok_or("Expected output directory")?);
    std::fs::create_dir_all(&out)?;
    let project = VerilogProject::generate::<CpuV3Gpu>()?;
    let mut sources = Vec::new();
    for (index, (_, source)) in project.files.into_iter().enumerate() {
        let name = format!("source_{index}.v");
        std::fs::write(out.join(&name), source)?;
        sources.push(name);
    }
    std::fs::write(out.join("rtl.json"), serde_json::to_vec_pretty(&sources)?)?;
    Ok(())
}
