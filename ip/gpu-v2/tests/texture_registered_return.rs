//! Native-Q lifetime and II2 of the production command cache/Color handoff.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::{pixel::quad_ingress::rtl::Backend, vliw_command},
};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn production_source() -> String {
    vliw_command::verilog(
        MaterializedSurface {
            color_base_bytes: 0x100000,
            depth_base_bytes: 0x140000,
            width: 400,
            height: 240,
        },
        Backend::Native,
    )
    .unwrap()
}

fn bounded(mut command: Command, directory: &Path, label: &str) {
    command.stdout(Stdio::from(
        std::fs::File::create(directory.join(format!("{label}.out"))).unwrap(),
    ));
    command.stderr(Stdio::from(
        std::fs::File::create(directory.join(format!("{label}.err"))).unwrap(),
    ));
    let mut child = command.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{label}: see {}", directory.display());
            return;
        }
        if start.elapsed() > Duration::from_secs(45) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{label} exceeded 45 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires Icarus and native Gowin SRAM"]
fn registered_cache_return_overlaps_color_and_survives_pauses_and_replacement() {
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/texture-registered-return");
    std::fs::create_dir_all(&directory).unwrap();
    let source = production_source();
    std::fs::write(directory.join("gpu.v"), source).unwrap();
    std::fs::write(
        directory.join("tb.v"),
        include_str!("fixtures/texture_registered_return_tb.v"),
    )
    .unwrap();
    let primitive =
        Path::new(&std::env::var_os("GOWIN_HOME").unwrap()).join("IDE/simlib/gw2a/prim_sim.v");
    for native in [0, 1] {
        let image = format!("probe-{native}.vvp");
        let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        command.current_dir(&directory).args([
            "-g2012",
            "-D__ICARUS__",
            "-s",
            "tb",
            &format!("-Ptb.NATIVE={native}"),
            "-o",
            &image,
            "gpu.v",
            "tb.v",
        ]);
        command.arg(&primitive);
        bounded(command, &directory, &format!("compile-{native}"));
        for mode in [1, 2] {
            for stress in [0, 1] {
                let label = format!("run-{native}-{mode}-{stress}");
                let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
                command.current_dir(&directory).args([
                    &image,
                    &format!("+MODE={mode}"),
                    &format!("+STRESS={stress}"),
                ]);
                bounded(command, &directory, &label);
                let output =
                    std::fs::read_to_string(directory.join(format!("{label}.out"))).unwrap();
                let summary = output.lines().find(|line| line.starts_with("PASS "));
                assert!(summary.is_some(), "missing result in {label}");
                println!("{}", summary.unwrap());
            }
        }
    }
}

#[test]
#[ignore = "requires Icarus and native Gowin RAM/DSP primitives"]
fn packed_coordinate_to_color_preserves_results_and_sustains_ii2() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/texture-packed-pipeline-production");
    std::fs::create_dir_all(&directory).unwrap();
    let source = production_source();
    std::fs::write(directory.join("gpu.v"), source).unwrap();
    std::fs::write(
        directory.join("tb.v"),
        include_str!("fixtures/texture_packed_pipeline_tb.v"),
    )
    .unwrap();
    let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
    command.current_dir(&directory).args([
        "-g2012",
        "-D__ICARUS__",
        "-s",
        "tb",
        "-o",
        "probe.vvp",
        "gpu.v",
        "tb.v",
    ]);
    command.arg(
        Path::new(&std::env::var_os("GOWIN_HOME").unwrap()).join("IDE/simlib/gw2a/prim_sim.v"),
    );
    bounded(command, &directory, "compile");
    for mode in [1, 2, 3, 4, 5] {
        for stress in [0, 1] {
            let label = format!("run-{mode}-{stress}");
            let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
            command.current_dir(&directory).args([
                "probe.vvp",
                &format!("+MODE={mode}"),
                &format!("+STRESS={stress}"),
                match mode {
                    2 => "+II=4",
                    4 => "+II=8",
                    5 => "+II=10",
                    _ => "+II=2",
                },
            ]);
            bounded(command, &directory, &label);
            let output = std::fs::read_to_string(directory.join(format!("{label}.out"))).unwrap();
            let summary = output.lines().find(|line| line.starts_with("PASS PACKED "));
            assert!(
                summary.is_some(),
                "missing packed pipeline completion in {label}"
            );
            println!("{}", summary.unwrap());
        }
    }
}

#[test]
#[ignore = "requires Icarus and native Gowin RAM"]
fn group_native_q_preserves_words_under_full_credit_and_stalls() {
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/texture-group-native-q");
    std::fs::create_dir_all(&directory).unwrap();
    let source = production_source();
    let begin = source.find("module gpu_v2_packet_store_unique ").unwrap();
    let end = begin + source[begin..].find("endmodule").unwrap() + "endmodule".len();
    std::fs::write(directory.join("group.v"), &source[begin..end]).unwrap();
    std::fs::write(
        directory.join("tb.v"),
        include_str!("fixtures/texture_group_q_tb.v"),
    )
    .unwrap();
    for native in [0, 1] {
        let image = format!("probe-{native}.vvp");
        let mut command = Command::new(std::env::var_os("IVERILOG_EXE").unwrap());
        command.current_dir(&directory).args([
            "-g2012",
            "-s",
            "tb",
            &format!("-Ptb.NATIVE={native}"),
            "-o",
            &image,
            "group.v",
            "tb.v",
        ]);
        command.arg(
            Path::new(&std::env::var_os("GOWIN_HOME").unwrap()).join("IDE/simlib/gw2a/prim_sim.v"),
        );
        bounded(command, &directory, &format!("compile-{native}"));
        let label = format!("run-{native}");
        let mut command = Command::new(std::env::var_os("VVP_EXE").unwrap());
        command.current_dir(&directory).arg(image);
        bounded(command, &directory, &label);
        let output = std::fs::read_to_string(directory.join(format!("{label}.out"))).unwrap();
        let summary = output
            .lines()
            .find(|line| line.starts_with("PASS GROUP_Q "));
        assert!(summary.is_some(), "missing Group-Q completion");
        println!("{}", summary.unwrap());
    }
}
