//! Read-only timing experiment for serialized cache-clean writeback.
use cpu_v3::CpuV3DataCache;
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[test]
#[ignore = "bounded existing-RTL cache clean timing experiment"]
fn clean_writeback_request_timeline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = root.join("../../target/maintainer/prepare-pipeline-20261024/cache-clean");
    std::fs::create_dir_all(&out).unwrap();
    let project = VerilogProject::generate::<CpuV3DataCache>().unwrap();
    let mut paths = Vec::new();
    for (i, source) in project.files.values().enumerate() {
        let p = out.join(format!("source_{i}.v"));
        std::fs::write(&p, source).unwrap();
        paths.push(p);
    }
    let tb = include_str!("gpu_dcache_clean_trace_tb.v").replace(
        "__CACHE__",
        &CpuV3DataCache::verilog_identity().module_name(),
    );
    std::fs::write(out.join("tb.v"), tb).unwrap();
    let sim = out.join("cache.vvp");
    let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
    command
        .args(["-g2012", "-s", "tb", "-o"])
        .arg(&sim)
        .arg(Path::new(&std::env::var_os("GOWIN_HOME").unwrap()).join("IDE/simlib/gw2a/prim_sim.v"))
        .args(paths)
        .arg(out.join("tb.v"));
    run(&mut command, &out.join("compile.log"));
    for (name, delay, lines, stride) in [
        ("dense-0", 0, 128, 1),
        ("dense-6", 6, 128, 1),
        ("dense-20", 20, 128, 1),
        ("sparse-20", 20, 8, 8),
    ] {
        let log = out.join(format!("{name}.log"));
        let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
        command.arg(&sim).args([
            format!("+DELAY={delay}"),
            format!("+LINES={lines}"),
            format!("+STRIDE={stride}"),
        ]);
        run(&mut command, &log);
        let text = std::fs::read_to_string(log).unwrap();
        println!(
            "{}",
            text.lines()
                .find(|l| l.starts_with("CACHE_TRACE_PASS"))
                .expect("clean trace must complete")
        );
    }
}
fn run(command: &mut Command, log: &Path) {
    let mut child = command
        .stdout(std::fs::File::create(log).unwrap())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            panic!("cache trace host bound");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}: {}",
        log.display(),
        String::from_utf8_lossy(&result.stderr)
    );
}
