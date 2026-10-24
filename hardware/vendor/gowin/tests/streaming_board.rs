//! CL3 real-pin compatibility and physical continuation, separate from the legacy oracle.
use digital_design_hardware_gowin::sdram_memory_controller::combination;
use std::{fs, path::PathBuf, process::Command};

#[test]
#[ignore = "bounded CL3 pin replay: held CPU scalar/LAST plus mixed-length GPU continuation"]
fn cl3_streaming_transport_with_cpu_holds() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::var_os("STREAM_SDRAM_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/streaming-board"));
    let peer = out.join("peer");
    fs::create_dir_all(&peer).unwrap();
    for (path, text) in combination::rtl_sources_with_options(40, true, false).unwrap() {
        fs::write(peer.join(path.file_name().unwrap()), text).unwrap();
    }
    let run = out.join(format!("run-{}", std::process::id()));
    let output = Command::new("python")
        .arg(root.join("tests/stream_compatibility.py"))
        .arg("--peer")
        .arg(peer)
        .arg("--output")
        .arg(run)
        .args(["--cl3", "--display", "--bursts", "8"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}
