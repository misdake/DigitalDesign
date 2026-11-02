//! Production gear + MC + existing routed-corner pin oracle, one frozen input.
mod native_support;
#[path = "native_support/traffic.rs"]
mod traffic;
use digital_design_hardware_gowin::sdram_memory_controller::{
    cycle::gear::{Input, LogicOutput},
    RtlSources,
};
use native_support::run_tool;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
fn b(v: bool) -> u64 {
    u64::from(v)
}
fn input(i: Input) -> Vec<u64> {
    vec![
        b(i.reset),
        b(i.request_valid),
        b(i.continuation),
        b(i.writing),
        i.address.into(),
        i.words.into(),
        i.write_mask.into(),
        i.write_data,
        b(i.write_data_valid),
        b(i.next_valid),
        i.next_address.into(),
    ]
}
fn output(o: LogicOutput) -> Vec<u64> {
    vec![
        b(o.request_ready),
        b(o.stream_active),
        b(o.write_data_ready),
        o.read_data,
        b(o.read_valid),
        b(o.write_done),
        b(o.initialized),
    ]
}
fn line(v: &[u64]) -> String {
    v.iter()
        .map(|x| format!("{x:x}"))
        .collect::<Vec<_>>()
        .join(" ")
        + "\n"
}
fn replay(dir: &Path, run: &traffic::Run, corrupt: bool) -> String {
    fs::write(
        dir.join("input.txt"),
        run.trace
            .iter()
            .map(|(i, _)| line(&input(*i)))
            .collect::<String>(),
    )
    .unwrap();
    fs::write(
        dir.join("rust.txt"),
        run.trace
            .iter()
            .map(|(_, o)| line(&output(*o)))
            .collect::<String>(),
    )
    .unwrap();
    fs::write(dir.join("production-controller.v"), RtlSources::CONTROLLER).unwrap();
    let controller = if corrupt {
        RtlSources::CONTROLLER.replace("dq_out <= write_data;", "dq_out <= write_data ^ 32'h1;")
    } else {
        RtlSources::CONTROLLER.to_owned()
    };
    if corrupt {
        assert_ne!(controller, RtlSources::CONTROLLER);
    }
    for (name, source) in [
        ("bridge.v", RtlSources::STREAM_GEARBOX),
        ("controller.v", controller.as_str()),
        ("pins.v", RtlSources::PIN_MODEL),
        ("input-delay.v", RtlSources::INPUT_DELAY_MODEL),
        ("cl3_pins.py", include_str!("cl3_pins.py")),
    ] {
        fs::write(dir.join(name), source).unwrap();
    }
    fs::write(
        dir.join("tb-base.v"),
        include_str!("native_closure_tb.v").replace("__COUNT__", &run.trace.len().to_string()),
    )
    .unwrap();
    let mut prepare = Command::new("python");
    prepare.args(["-B","-c","from pathlib import Path;from cl3_pins import with_board_pins;Path('tb.v').write_text(with_board_pins(Path('tb-base.v').read_text(),'dut.u_controller'))"]);
    run_tool(prepare, dir, "prepare");
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile.args([
        "-g2012",
        "-DSTREAM_STATIC_INPUT_DELAY",
        "-s",
        "tb",
        "-o",
        "run.vvp",
        "bridge.v",
        "controller.v",
        "pins.v",
        "input-delay.v",
        "tb.v",
    ]);
    run_tool(compile, dir, "compile");
    let mut execute = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
    execute.arg("run.vvp");
    run_tool(execute, dir, "rtl");
    fs::read_to_string(dir.join("rtl.stdout")).unwrap()
}
#[test]
#[ignore = "bounded full native memory closure; Icarus and Python required"]
fn production_native_closure_matches_rust_logic_events_and_pin_commits() {
    let run = traffic::run();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(format!("target/native-memory/closure-{nonce}"));
    fs::create_dir_all(&dir).unwrap();
    let raw = replay(&dir, &run, false);
    assert!(raw.contains("COMPLETE"), "{}", dir.display());
    let applied: Vec<Vec<u64>> = raw
        .lines()
        .filter_map(|l| l.strip_prefix("INPUT "))
        .map(|l| {
            l.split_whitespace()
                .map(|v| u64::from_str_radix(v, 16).unwrap())
                .collect()
        })
        .collect();
    let expected_inputs: Vec<_> = run.trace.iter().map(|(i, _)| input(*i)).collect();
    assert_eq!(
        applied,
        expected_inputs,
        "actual RTL stimulus differs: {}",
        dir.display()
    );
    let actual: Vec<Vec<u64>> = raw
        .lines()
        .filter_map(|l| l.strip_prefix("TRACE "))
        .map(|l| {
            l.split_whitespace()
                .map(|v| {
                    u64::from_str_radix(v, 16).unwrap_or_else(|_| {
                        panic!("unknown owned bridge value {v}: {}", dir.display())
                    })
                })
                .collect()
        })
        .collect();
    assert_eq!(actual.len(), run.trace.len());
    let names = [
        "request_ready",
        "stream_active",
        "write_data_ready",
        "read_data",
        "read_valid",
        "write_done",
        "initialized",
    ];
    for (cycle, (a, (_, o))) in actual.iter().zip(&run.trace).enumerate() {
        let e = output(*o);
        assert_eq!(
            a.len(),
            names.len(),
            "logic cycle {cycle}: output field count"
        );
        for (field, (a, e)) in names.iter().zip(a.iter().zip(e)) {
            assert_eq!(*a, e, "logic cycle {cycle}, {field}: {}", dir.display());
        }
    }
    let writes: Vec<_> = fs::read_to_string(dir.join("writes.txt"))
        .unwrap()
        .lines()
        .map(|l| {
            let v: Vec<_> = l
                .split_whitespace()
                .map(|v| u32::from_str_radix(v, 16).unwrap())
                .collect();
            (v[0], v[1] as u8, v[2])
        })
        .collect();
    assert_eq!(
        writes,
        run.writes,
        "independent physical write address/mask/result sequence: {}",
        dir.display()
    );
    let final_words: Vec<_> = fs::read_to_string(dir.join("final.txt"))
        .unwrap()
        .lines()
        .map(|l| u32::from_str_radix(l, 16).unwrap())
        .collect();
    assert_eq!(final_words, &run.final_words[..4096]);
    let negative = dir.join("mutated-write");
    fs::create_dir_all(&negative).unwrap();
    replay(&negative, &run, true);
    let bad = fs::read_to_string(negative.join("writes.txt")).unwrap();
    let good = fs::read_to_string(dir.join("writes.txt")).unwrap();
    let mismatch = bad
        .lines()
        .zip(good.lines())
        .position(|(a, b)| a != b)
        .expect("DUT write-data mutation must change physical commits");
    fs::write(
        negative.join("detected.txt"),
        format!(
            "physical write {mismatch}: expected {}, mutated {}\n",
            good.lines().nth(mismatch).unwrap(),
            bad.lines().nth(mismatch).unwrap()
        ),
    )
    .unwrap();
}
