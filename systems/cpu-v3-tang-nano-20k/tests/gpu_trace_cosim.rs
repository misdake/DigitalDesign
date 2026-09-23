//! GPU transaction-level trace differential: the Rust model
//! (`GpuCore` + `HostGpuMemory`, driven through the `gpu_tb.v` scenario
//! sequence) and the Icarus RTL run of `gpu.v` + `gpu_tb.v` must emit the same
//! ordered per-port transaction trace and the same global completion points.
//!
//! The testbench responder and the host memory model intentionally differ in
//! cycle timing (recovery cycles vs. write-data backpressure), so only the
//! transaction content sequence is compared, never the cycle-by-cycle timing.
//! The RTL side enforces its own 4M-cycle global bound; the Rust scenario
//! driver bounds every submission wait at 500k cycles.

use cpu_v3_tang_nano_20k::hardware::trace::{compare_traces, rust_cosim_trace};
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::Module;

/// Compiles `gpu.v` + `gpu_tb.v` with Icarus and returns the simulator stdout.
fn run_gpu_rtl() -> String {
    let directory = std::env::temp_dir().join(format!("gpu-trace-cosim-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let module_path = directory.join("module.v");
    let testbench_path = directory.join("testbench.v");
    std::fs::write(&module_path, CpuV3Gpu::verilog_source().unwrap()).unwrap();
    std::fs::write(&testbench_path, CpuV3Gpu::verilog_testbench().unwrap()).unwrap();

    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let output_path = directory.join("sim.vvp");
    let compile = std::process::Command::new(&iverilog)
        .current_dir(&directory)
        .args(["-g2005", "-s", "tb", "-o"])
        .arg(&output_path)
        .arg(&module_path)
        .arg(&testbench_path)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "iverilog compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let simulation = std::process::Command::new(&vvp)
        .current_dir(&directory)
        .arg(&output_path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&simulation.stdout).into_owned();
    std::fs::remove_dir_all(&directory).ok();
    stdout
}

#[test]
#[ignore = "explicit GPU transaction trace co-simulation against Icarus RTL"]
fn gpu_trace_matches_rtl() {
    let rust_trace = rust_cosim_trace();
    let stdout = run_gpu_rtl();
    assert!(
        stdout
            .lines()
            .any(|line| line.trim() == "DIGITAL_DESIGN_PASS"),
        "RTL run did not pass:\n{stdout}"
    );
    let rtl_trace: Vec<String> = stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("GPU "))
        .map(|line| line.trim().to_string())
        .collect();
    if let Err(message) = compare_traces(&rust_trace, &rtl_trace) {
        panic!("GPU transaction trace mismatch:\n{message}");
    }
    println!(
        "PASS gpu trace: {} events, {} rtl lines",
        rust_trace.len(),
        rtl_trace.len()
    );
}
