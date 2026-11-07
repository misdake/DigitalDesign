//! Run the registered CpuV3Gpu fixture with the production Gowin DSP/BSRAM models.
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::{Module, VerilogProject};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn bounded(command: &mut Command) -> Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start native GPU fixture tool");
    let start = Instant::now();
    while child.try_wait().expect("poll native GPU fixture").is_none() {
        if start.elapsed() > Duration::from_secs(90) {
            child.kill().expect("stop timed-out fixture");
            let output = child.wait_with_output().expect("collect timed-out fixture");
            panic!(
                "native GPU fixture timed out: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child
        .wait_with_output()
        .expect("collect native GPU fixture")
}

#[test]
#[ignore = "bounded native GPU fixture requires Icarus and GOWIN_HOME"]
fn production_device_fixture_runs_with_native_primitives() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-device-export-fixture");
    std::fs::create_dir_all(&out).unwrap();
    let project = VerilogProject::generate::<CpuV3Gpu>().unwrap();
    project.write_to(&out).unwrap();
    let tb = out.join("tb.v");
    std::fs::write(&tb, CpuV3Gpu::verilog_testbench().unwrap()).unwrap();
    let models = PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
        .join("IDE/simlib/gw2a/prim_sim.v");
    let exe = out.join("sim.vvp");
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .args(["-g2012", "-s", "tb", "-o"])
        .arg(&exe)
        .arg(models)
        .args(project.files.keys().map(|p| out.join(p)))
        .arg(tb);
    let result = bounded(&mut compile);
    std::fs::write(
        out.join("compile.log"),
        [&result.stdout[..], &result.stderr[..]].concat(),
    )
    .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut simulate = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    simulate.arg(exe);
    let result = bounded(&mut simulate);
    std::fs::write(
        out.join("run.log"),
        [&result.stdout[..], &result.stderr[..]].concat(),
    )
    .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success() && stdout.contains("DIGITAL_DESIGN_PASS"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{stdout}");
}
