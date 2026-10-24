//! Diagnostic only: a failed full clean currently discards dirty CPU stack data.
//! Memory errors are unrecoverable: require a halt/fault and no GPU launch.
use cpu_v3::{rcc_backend, CpuV3Core, CpuV3DataCache};
use cpu_v3_tang_nano_20k::{hardware::SystemControlDevice, CpuV3Gpu};
use digital_design_hardware::{HardwareIdentity, VerilogProject};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
#[path = "../build_support/rcc_modules.rs"]
mod resolver;

#[test]
#[ignore = "requires completed ui-inplace-v1 timing artifacts; compiler only"]
fn direct_source_matches_timed_binary() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let artifacts = root.join("../../target/maintainer/demo-toggle-20261023");
    let constants = std::fs::read_to_string(artifacts.join("boot/boot_resources.rs")).unwrap();
    let out = artifacts.join("cpu-prep-ui-inplace-v1");
    let source = std::fs::read_to_string(out.join("program.rs")).unwrap();
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = cpu_v3_tang_nano_20k::boot::SINGLE_BOOT_STACK;
    let ir =
        rcc::frontend::compile_program_named("Classic profile", &source, &options, &mut |name| {
            resolver::load_module(
                &root.join("examples/gpu_demo/rcc"),
                &root.join("rcc"),
                &constants,
                name,
                &mut |_| {},
            )
        })
        .unwrap();
    let program = rcc_backend::try_compile(ir, &options, "demo_one_frame_main").unwrap();
    let expected: Vec<u16> = std::fs::read_to_string(out.join("program.hex"))
        .unwrap()
        .lines()
        .map(|x| u16::from_str_radix(x, 16).unwrap())
        .collect();
    assert_eq!(
        program.words, expected,
        "Final candidate sources must reproduce timed instructions"
    );
}
fn bounded(command: &mut Command, log: &Path) -> Output {
    let mut child = command
        .stdout(std::fs::File::create(log).unwrap())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(600) {
            child.kill().unwrap();
            let result = child.wait_with_output().unwrap();
            panic!(
                "bounded RCC GPU tool timed out: {}\n{}",
                std::fs::read_to_string(log).unwrap_or_default(),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut result = child.wait_with_output().unwrap();
    result.stdout = std::fs::read(log).unwrap();
    result
}
#[test]
#[ignore = "bounded real CPU/cache/SystemControl/Device4/GPU; requires Icarus and GOWIN_HOME"]
fn direct_recording_fail_stops_on_memory_errors() {
    for mode in [1u8, 2] {
        run_case(mode);
    }
}
fn run_case(mode: u8) {
    let out = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/maintainer/cb-build-distribution-20261024/fault");
    std::fs::create_dir_all(&out).unwrap();
    let mut sources = BTreeMap::new();
    for p in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<SystemControlDevice<4>>().unwrap(),
        VerilogProject::generate::<CpuV3Gpu>().unwrap(),
    ] {
        for (path, source) in p.files {
            if !source.contains("module tb") {
                if let Some(old) = sources.insert(path, source.clone()) {
                    assert_eq!(old, source);
                }
            }
        }
    }
    let mut paths = Vec::new();
    for (i, source) in sources.values().enumerate() {
        let path = out.join(format!("source_{i}.v"));
        std::fs::write(&path, source).unwrap();
        paths.push(path);
    }
    let ending = r#"
 check(gpu_pass_clear(cb,0x1357,0x2468));
 check(gpu_submit_idle(cb)==GPU_CACHE_ERROR);
 check(read_dseg_page(3)==19);check(read_dseg_page(0)==0 && read_dseg_page(1)==1 && read_dseg_page(2)==2);
 check(gpu_poll(cb)==GPU_FAULT);check(gpu_cb_begin()==GPU_CB_NONE);
 check(gpu_submit_idle(cb)==GPU_FAULT);
"#;
    let source = format!(
        r#"
mod gpu;
static FST:Buf<u16,8>=Buf::new([0;8]);
fn check(ok:bool){{if !ok{{halt(0xbad);}}}}
fn main(){{
 unsafe {{write_dseg_page(3,19);}}
 check(gpu_init()==GPU_IDLE);let cb=gpu_cb_begin();check(cb==GPU_CB_HANDLE);
 {ending}
 halt(0x600d);
}}
"#
    );
    // A device marker survives a cache-error scrub, unlike a RAM counter.
    let source = source
        .split("check(")
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part.to_string()
            } else if i == 1 {
                format!("check({part}")
            } else {
                format!("dev_send(7,3,{});check({part}", i - 1)
            }
        })
        .collect::<String>();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let parsed =
        rcc::frontend::compile_program_named("real GPU driver", &source, &options, &mut |name| {
            std::fs::read_to_string(root.join(format!("{name}.rs"))).map_err(|e| e.to_string())
        })
        .unwrap();
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let name = match mode {
        0 => "repeat-clear",
        1 => "cache-error",
        _ => "eviction-error",
    };
    std::fs::write(out.join(format!("{name}.rs")), source).unwrap();
    let hex = out.join(format!("{name}.hex"));
    std::fs::write(
        &hex,
        program
            .words
            .iter()
            .map(|w| format!("{w:04x}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let tb = include_str!("gpu_demo_direct_fault_tb.v")
        .replace("__CORE__", &CpuV3Core::verilog_identity().module_name())
        .replace(
            "__CACHE__",
            &CpuV3DataCache::verilog_identity().module_name(),
        )
        .replace(
            "__SYSCTL__",
            &SystemControlDevice::<4>::verilog_identity().module_name(),
        )
        .replace("__FAULT__", &mode.to_string())
        .replace("__PROGRAM__", &hex.to_string_lossy().replace('\\', "/"));
    let tb_path = out.join(format!("{name}-tb.v"));
    std::fs::write(&tb_path, tb).unwrap();
    let exe = out.join(format!("{name}.vvp"));
    let models = PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"))
        .join("IDE/simlib/gw2a/prim_sim.v");
    let mut compiler =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compiler
        .args(["-g2012", "-s", "tb", "-o"])
        .arg(&exe)
        .arg(models)
        .args(&paths)
        .arg(tb_path);
    let result = bounded(&mut compiler, &out.join(format!("{name}-compile.log")));
    std::fs::write(
        out.join(format!("{name}-compile.log")),
        [&result.stdout[..], &result.stderr[..]].concat(),
    )
    .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut sim = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    sim.arg(exe);
    let result = bounded(&mut sim, &out.join(format!("{name}.log")));
    std::fs::write(
        out.join(format!("{name}.log")),
        [&result.stdout[..], &result.stderr[..]].concat(),
    )
    .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success() && stdout.contains("RCC_GPU_PASS"),
        "{name}: {stdout}\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    println!("{stdout}");
}
