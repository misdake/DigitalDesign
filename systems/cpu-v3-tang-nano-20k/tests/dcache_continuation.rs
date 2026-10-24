//! Actual D-cache, arbiter and native CL3 SDRAM continuation with ordered retirement.
use cpu_v3::CpuV3DataCache;
use digital_design_hardware::VerilogProject;
use digital_design_hardware_gowin::sdram_memory_controller::combination;
use std::{fs, path::PathBuf, process::Command};
#[test]
#[ignore = "bounded cache clean/invalidate through actual streaming CL3 pins"]
fn clean_and_invalidate_continue_through_native_memory() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::var_os("DCACHE_STREAM_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("../../target/maintainer/dcache-stream-20261024"));
    fs::create_dir_all(&out).unwrap();
    for (path, text) in combination::rtl_sources_with_options(40, true, false).unwrap() {
        fs::write(out.join(path.file_name().unwrap()), text).unwrap();
    }
    for (n, text) in VerilogProject::generate::<CpuV3DataCache>()
        .unwrap()
        .files
        .values()
        .enumerate()
    {
        fs::write(out.join(format!("cache_{n}.v")), text).unwrap();
    }
    let mut command = Command::new("python");
    command
        .arg(root.join("tests/dcache_continuation.py"))
        .arg(&out);
    if let Some(revision) = std::env::var_os("DCACHE_STREAM_BASELINE") {
        command.arg("--baseline-ref").arg(revision);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}
