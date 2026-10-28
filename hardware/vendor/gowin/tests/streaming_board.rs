//! CL3 real-pin compatibility and physical continuation, bound to the fixed stream profile.
use digital_design_hardware_gowin::sdram_memory_controller::combination;
use std::{fs, path::PathBuf, process::Command};

#[test]
#[ignore = "bounded CL3 pin replay: held CPU scalar/LAST plus mixed-length GPU continuation"]
fn cl3_streaming_transport_with_cpu_holds() {
    run_replay("cpu-holds", &["--display", "--bursts", "8"]);
}

#[test]
#[ignore = "bounded CL3 pin replay with injected related-clock skew and producer clock-to-Q"]
fn cl3_related_clock_payload_skew_stress() {
    for skew in ["0", "-0.35", "0.35"] {
        let result = run_replay(
            &format!("payload-skew-{skew}"),
            &[
                "--display",
                "--bursts",
                "32",
                "--late-bank",
                "--logic-skew-ns",
                skew,
                "--pair-cq-ns",
                "0.201",
            ],
        );
        // Exact latency belongs to the nominal clock relationship. Injected
        // skew may move request sampling across a native edge; data/ownership
        // must still pass the independent pin scoreboard in every case.
        if skew == "0" {
            assert!(
                result.contains("NATIVE_CALENDAR cycles=1234"),
                "changed nominal native calendar: {result}"
            );
            assert!(result.contains("work=560 commit_span=532"));
            assert!(result.contains("work=461 commit_span=444"));
        }
    }
}

fn run_replay(name: &str, args: &[&str]) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::var_os("STREAM_SDRAM_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/streaming-board"))
        .join(name);
    let peer = out.join("peer");
    fs::create_dir_all(&peer).unwrap();
    for (path, text) in combination::rtl_sources(40).unwrap() {
        fs::write(peer.join(path.file_name().unwrap()), text).unwrap();
    }
    let run = out.join(format!("run-{}", std::process::id()));
    let output = Command::new("python")
        .arg(root.join("tests/stream_compatibility.py"))
        .arg("--peer")
        .arg(peer)
        .arg("--output")
        .arg(run)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    print!("{stdout}");
    stdout.into_owned()
}
