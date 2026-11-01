//! Bounded offline public API -> real production GPU RTL pixels. No board access.
use cpu_v3::{rcc_backend, CpuV3Sim, StepOutcome};
use cpu_v3_tang_nano_20k::CpuV3Gpu;
use digital_design_hardware::{Module, VerilogProject};
use std::{
    cell::RefCell,
    fmt::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

fn bounded(command: &mut Command, directory: &Path, label: &str) {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("GPU fixture tool");
    let start = Instant::now();
    while child.try_wait().expect("GPU tool poll").is_none() {
        if start.elapsed() > Duration::from_secs(90) {
            child.kill().unwrap();
            panic!("bounded GPU tool timeout: {label}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = child.wait_with_output().unwrap();
    let text = [output.stdout, output.stderr].concat();
    std::fs::write(directory.join(format!("{label}.log")), &text).unwrap();
    assert!(
        output.status.success(),
        "{label}: {}",
        String::from_utf8_lossy(&text)
    );
}

pub(super) fn execute(directory: &Path, memory: &mut [u16], base: u16, count: u16) {
    let mut text = String::new();
    for sector in [base, 0x9a00] {
        writeln!(text, "@{:x}", usize::from(sector) * 16).unwrap();
        for words in memory[usize::from(sector) * 64..usize::from(sector) * 64 + 64]
            .as_chunks::<4>()
            .0
            .iter()
        {
            let value = words.iter().enumerate().fold(0u64, |value, (index, word)| {
                value | (u64::from(*word) << (index * 16))
            });
            writeln!(text, "{value:016x}").unwrap();
        }
    }
    std::fs::write(directory.join("input.hex"), text).unwrap();
    let mut command = Command::new(std::env::var_os("VVP_EXE").expect("VVP_EXE"));
    command
        .current_dir(directory)
        .arg(directory.join("sim.vvp"))
        .arg(format!("+COUNT={count}"))
        .arg(format!("+BASE={base}"));
    bounded(&mut command, directory, "run");
    let log = std::fs::read_to_string(directory.join("run.log")).unwrap();
    assert!(log.contains("PUBLIC_GPU_CLEAR_PASS"), "{log}");
    // Import actual RTL pixels before permitting CPU retirement.
    let image = std::fs::read_to_string(directory.join("color.hex")).unwrap();
    let pixels: Vec<_> = image
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(str::split_whitespace)
        .filter(|word| !word.starts_with('@'))
        .map(|word| u64::from_str_radix(word, 16).expect("known RTL pixel"))
        .collect();
    assert_eq!(pixels.len(), 25600);
    for (index, pixel) in pixels.into_iter().enumerate() {
        assert_eq!(pixel, 0x1357135713571357);
        for lane in 0..4 {
            memory[0x8000 * 64 + index * 4 + lane] = (pixel >> (lane * 16)) as u16;
        }
    }
}

pub(super) fn compile_clear_fixture(directory: &Path) {
    std::fs::create_dir_all(directory).unwrap();
    let directory = directory.canonicalize().unwrap();
    let project = VerilogProject::generate::<CpuV3Gpu>().unwrap();
    project.write_to(&directory).unwrap();
    let ports = CpuV3Gpu::verilog_testbench().unwrap();
    let ports = ports.split("    integer cycle=").next().unwrap();
    std::fs::write(
        directory.join("tb.v"),
        format!(
            "{ports}{}",
            include_str!("../fixtures/graphics_api_a_clear.v")
        ),
    )
    .unwrap();
    let models =
        PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap()).join("IDE/simlib/gw2a/prim_sim.v");
    let mut command = Command::new(std::env::var_os("IVERILOG_EXE").expect("IVERILOG_EXE"));
    command
        .args(["-g2012", "-s", "tb", "-o"])
        .arg(directory.join("sim.vvp"))
        .arg(models)
        .args(project.files.keys().map(|path| directory.join(path)))
        .arg(directory.join("tb.v"));
    bounded(&mut command, &directory, "compile");
}

#[test]
#[ignore = "bounded production GPU pixel test requires Icarus and GOWIN_HOME"]
fn public_clear_retires_only_after_production_rtl_pixels() {
    use super::gpu_api_integration::{Peer, Trace, APPLICATION};
    let directory = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(PathBuf::from)
        .expect("RCC_RUSTC_TEST_DIR")
        .join("public-g1-pixels");
    compile_clear_fixture(&directory);
    let prefix = APPLICATION.split(" gpu.submit(first);").next().unwrap();
    let application=format!("{prefix}gpu.submit(first);first=gpu.wait();drop(first);drop(second);drop(color);drop(depth);drop(gpu);halt(0x600d);}}");
    let parsed = super::gpu_api_resources::parse(&application, "public-g1", false)
        .unwrap_or_else(|error| panic!("{error}"));
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let program = rcc_backend::try_compile(parsed, &options, "main").unwrap();
    let mut trace = Trace::default();
    trace.rtl = Some(directory.clone());
    let state = Rc::new(RefCell::new(trace));
    let mut cpu = CpuV3Sim::default();
    cpu.set_boot_window(&program.words);
    for base in [0x8000usize, 0x9000] {
        cpu.physical_memory_mut()[base * 64..(base + 2048) * 64].fill(0xa55a);
    }
    for kind in [0, 1, 4] {
        cpu.attach_device(kind, Box::new(Peer(kind, state.clone())));
    }
    for _ in 0..3_000_000 {
        if let StepOutcome::Halted { signal } = cpu.step().unwrap() {
            assert_eq!(signal, 0x600d);
            return;
        }
    }
    panic!("bounded public GPU pixel application timeout");
}
