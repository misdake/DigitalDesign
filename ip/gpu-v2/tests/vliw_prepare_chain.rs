//! Small regression for R-owned origin recovery, using the current factory.
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires Python, Icarus, and Gowin simulation primitives"]
fn prepare_to_raster_origin_portable_native_smoke() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tag = std::env::var("VLIW_PREPARE_CHAIN_TEST_TAG").unwrap_or_else(|_| {
        format!(
            "smoke-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    });
    let exports = repo
        .join("target/gpu-vliw-prepare-chain")
        .join(format!("{tag}-exports"));
    std::fs::create_dir_all(&exports).unwrap();
    for (name, portable) in [("portable", true), ("native", false)] {
        std::fs::write(
            exports.join(format!("frontend-{name}.v")),
            gpu_v2::frontend::vliw::verilog(portable),
        )
        .unwrap();
    }
    let mut child = Command::new("python")
        .arg(repo.join("ip/gpu-v2/tests/vliw_prepare_chain.py"))
        .args(["--tag", &tag, "--export-root"])
        .arg(exports)
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(240) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("prepare-chain replay exceeded wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
