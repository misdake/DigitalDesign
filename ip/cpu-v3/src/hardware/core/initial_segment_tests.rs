//! The fitted core's existing INITIAL_CODE_SEGMENT profile, including reset.
use super::*;
use digital_design_circuit::{build_circuit, external, External};
use digital_design_hardware::{ModuleTest, TestStep, VerilogProject};
use std::{
    collections::VecDeque,
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
struct Core {
    state: CpuV3CoreState,
    input: CpuV3CoreInput,
    output: CpuV3CoreOutput,
}
impl External for Core {
    fn execute(&mut self, c: &mut CircuitWires) {
        CpuV3Core::execute_emu(&mut self.state, c, &self.input, &self.output);
    }
    fn clock(&mut self, c: &mut CircuitWires) {
        CpuV3Core::clock_emu(&mut self.state, c, &self.input, &self.output);
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
fn command(mut c: Command, dir: &Path, label: &str) -> std::process::ExitStatus {
    c.current_dir(dir)
        .stdout(Stdio::from(
            fs::File::create(dir.join(format!("{label}.stdout"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(dir.join(format!("{label}.stderr"))).unwrap(),
        ));
    let mut child = c.spawn().unwrap();
    let start = Instant::now();
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s;
        }
        if start.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{label} timeout: {}", dir.display());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "bounded actual RTL initial code segment and reset differential"]
fn fitted_initial_segment_survives_reset_and_executes_from_cseg7() {
    let (mut circuit, (input, output)) = build_circuit(|| {
        let input = CpuV3CoreInput::allocate();
        let output = CpuV3CoreOutput::allocate();
        external(Core {
            state: CpuV3CoreState::with_initial_code_segment(7),
            input: input.clone(),
            output: output.clone(),
        });
        (input, output)
    });
    let mut words = crate::load_immediate16(0, 0x600d).to_vec();
    words.push(crate::halt());
    let mut pending = VecDeque::new();
    let mut steps = Vec::new();
    let (mut halted_edges, mut was_halted, mut requests) = (0, false, 0);
    for cycle in 0..150 {
        let local = cycle % 80;
        let mut i = input.sample(&circuit);
        i.reset = local < 2;
        i.hold = (25..30).contains(&local);
        if i.reset {
            pending.clear();
        }
        i.instruction_request_ready = pending.is_empty();
        i.instruction_response_valid = !pending.is_empty();
        i.instruction_data = pending.front().copied().unwrap_or(0);
        i.instruction_error = false;
        i.data_request_ready = true;
        i.data_response_valid = false;
        i.data_read_data = 0;
        i.data_error = false;
        i.data_line_copy_ready = true;
        i.data_cache_command_error = false;
        i.device_read_data = 0;
        input.drive(&mut circuit, &i);
        circuit.execute_gates();
        let o = output.sample(&circuit);
        if !i.reset {
            if i.instruction_response_valid && o.instruction_response_ready {
                pending.pop_front();
            }
            if o.instruction_request_valid && i.instruction_request_ready {
                assert_eq!(o.instruction_address >> 16, 7);
                pending.push_back(u64::from(
                    words
                        .get((o.instruction_address & 65535) as usize)
                        .copied()
                        .unwrap_or_else(crate::nop),
                ));
                requests += 1;
            }
        }
        circuit.clock_tick();
        circuit.execute_gates();
        let o = output.sample(&circuit);
        assert_eq!(o.code_segment, 7);
        assert!(!o.fault);
        if o.halted && !was_halted {
            assert_eq!(o.halt_signal, 0x600d);
            halted_edges += 1;
        }
        was_halted = o.halted;
        // Physical GPRs are uninitialized until the real16-cycle reset sweep.
        steps.push(if local < 18 {
            TestStep::drive(i)
        } else {
            TestStep::new(i, o)
        });
    }
    assert_eq!(halted_edges, 2);
    assert!(requests >= words.len() * 2);
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/functional-devices")
        .join(format!(
            "cseg-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    fs::create_dir_all(&dir).unwrap();
    let project = VerilogProject::generate::<CpuV3Core>().unwrap();
    let source = project
        .files
        .iter()
        .filter(|(p, _)| p.extension().is_some_and(|e| e == "v"))
        .map(|(_, s)| s.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.join("production-modules.v"), &source).unwrap();
    let tb = ModuleTest::<CpuV3Core>::new(steps).verilog_testbench();
    let original = format!("{} dut(", CpuV3Core::verilog_identity().module_name());
    assert!(tb.contains(&original));
    for segment in [7, 6] {
        let run = dir.join(format!("rtl-segment-{segment}"));
        fs::create_dir_all(&run).unwrap();
        fs::write(run.join("modules.v"), &source).unwrap();
        let bench=tb.replace(&original,&format!("{} #(.INITIAL_CODE_SEGMENT({segment})) dut(",CpuV3Core::verilog_identity().module_name())).replace("repeat (1) begin #5; clk = 1'b1; #1; clk = 1'b0; #4; end","repeat (1) begin #5; clk = 1'b1; #1; clk = 1'b0; #4; end\n    if (code_segment !==16'd7) begin $display(\"FAIL: initial segment\"); $finish(1); end");
        fs::write(run.join("tb.v"), bench).unwrap();
        let mut compile =
            Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
        compile.args(["-g2005", "-s", "tb", "-o", "run.vvp", "modules.v", "tb.v"]);
        assert!(
            command(compile, &run, "compile").success(),
            "{}",
            run.display()
        );
        let mut execute = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()));
        execute.arg("run.vvp");
        let status = command(execute, &run, "rtl");
        let stdout = fs::read_to_string(run.join("rtl.stdout")).unwrap();
        if segment == 7 {
            assert!(
                status.success() && stdout.contains("DIGITAL_DESIGN_PASS"),
                "{}: {stdout}",
                run.display()
            );
        } else {
            assert!(
                stdout.contains("FAIL:") && !stdout.contains("DIGITAL_DESIGN_PASS"),
                "wrong real RTL parameter must diverge: {stdout}"
            );
        }
    }
}
