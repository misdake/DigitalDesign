//! Exact system-side descriptor register: ordering, stalls and no fall-through.
use std::{fs, path::PathBuf, process::Command};
#[test]
#[ignore = "bounded RTL admission slot with changing source and consumer backpressure"]
fn gpu_descriptor_slot_preserves_order_and_holds() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::var_os("GPU_REQUEST_SLOT_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("../../target/gpu-request-slot"));
    fs::create_dir_all(&out).unwrap();
    let top = include_str!("../examples/cpu_v3_system/self_test.v");
    let (_, body) = top.split_once("module CpuV3GpuMemoryRequestSlot(").unwrap();
    fs::write(
        out.join("slot.v"),
        format!("module CpuV3GpuMemoryRequestSlot({body}"),
    )
    .unwrap();
    fs::write(out.join("tb.v"), include_str!("gpu_request_slot_tb.v")).unwrap();
    let compile = Command::new(std::env::var_os("IVERILOG_EXE").unwrap())
        .current_dir(&out)
        .args(["-g2012", "-s", "tb", "-o", "sim.vvp", "slot.v", "tb.v"])
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(std::env::var_os("VVP_EXE").unwrap())
        .current_dir(&out)
        .arg("sim.vvp")
        .output()
        .unwrap();
    fs::write(out.join("run.log"), &run.stdout).unwrap();
    assert!(
        run.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(String::from_utf8_lossy(&run.stdout).contains("PASS slot"));
}
