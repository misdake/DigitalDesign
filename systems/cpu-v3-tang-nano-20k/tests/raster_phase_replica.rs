//! Materialize native frontend and final production RTL for phase replay.
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::VerilogProject;
use std::{fs, path::PathBuf};

#[test]
#[ignore = "exports frontend and final production RTL for bounded phase replay"]
fn export_raster_phase_inputs() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/raster-phase-native/native/input");
    fs::create_dir_all(&out).unwrap();
    let gpu = VerilogProject::generate::<CpuV3Gpu>().unwrap();
    let source = gpu
        .files
        .values()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(out.join("gpu.v"), &source).unwrap();
    // Existing command benches instantiate the inner top, not its Device4 leaf.
    let command_start = source.find("`define GPU_V2_COMMAND_PORTS").unwrap();
    fs::write(out.join("command.v"), &source[command_start..]).unwrap();
    for (name, portable) in [("frontend-native.v", false), ("frontend-portable.v", true)] {
        fs::write(out.join(name), gpu_v2::frontend::vliw::verilog(portable)).unwrap();
    }
    println!("Raster phase replay input: {}", out.display());
}
