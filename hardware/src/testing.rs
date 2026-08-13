use crate::{Module, ModuleIo, ProjectError};
use digital_design_code::build_circuit;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct TestStep<I, O> {
    pub input: I,
    pub expected: Option<O>,
    pub cycles: u64,
}

impl<I, O> TestStep<I, O> {
    pub fn new(input: I, expected: O) -> Self {
        Self {
            input,
            expected: Some(expected),
            cycles: 1,
        }
    }

    /// Drive a cycle without checking its output.
    ///
    /// This is useful while priming uninitialized physical memories. Later
    /// checked steps must establish every value on which they rely.
    pub fn drive(input: I) -> Self {
        Self {
            input,
            expected: None,
            cycles: 1,
        }
    }

    pub fn after_cycles(mut self, cycles: u64) -> Self {
        assert!(cycles > 0, "module test step must run at least one cycle");
        self.cycles = cycles;
        self
    }
}

pub struct ModuleTest<M: Module> {
    steps: Vec<TestStep<<M::Input as ModuleIo>::Value, <M::Output as ModuleIo>::Value>>,
}

impl<M: Module> ModuleTest<M> {
    pub fn new(
        steps: impl IntoIterator<
            Item = TestStep<<M::Input as ModuleIo>::Value, <M::Output as ModuleIo>::Value>,
        >,
    ) -> Self {
        Self {
            steps: steps.into_iter().collect(),
        }
    }

    pub fn run_emu_and_nand(&self) {
        let emu = self.run_backend(M::emu, false);
        let nand = self.run_backend(M::nand, true);
        self.compare_backends(&emu, &nand, "emu/NAND");
    }

    /// Compare whole-module emulation with a local NAND implementation that
    /// may deliberately contain emulated external leaves such as FPGA BSRAM.
    ///
    /// Use `run_emu_and_nand` when the NAND side must be fully gate-exportable.
    pub fn run_emu_and_mixed_nand(&self) {
        let emu = self.run_backend(M::emu, false);
        let mixed = self.run_backend(M::nand, false);
        self.compare_backends(&emu, &mixed, "emu/mixed-NAND");
    }

    fn compare_backends(
        &self,
        left: &[<M::Output as ModuleIo>::Value],
        right: &[<M::Output as ModuleIo>::Value],
        label: &str,
    ) {
        assert_eq!(left.len(), right.len());
        for (index, ((left, right), step)) in left.iter().zip(right).zip(&self.steps).enumerate() {
            assert_eq!(left, right, "{label} mismatch at test step {index}");
            if let Some(expected) = &step.expected {
                assert_eq!(left, expected, "unexpected output at test step {index}");
            }
        }
    }

    pub fn run_emu(&self) {
        for (index, (actual, step)) in self
            .run_backend(M::emu, false)
            .iter()
            .zip(&self.steps)
            .enumerate()
        {
            if let Some(expected) = &step.expected {
                assert_eq!(actual, expected, "unexpected output at test step {index}");
            }
        }
    }

    fn run_backend(
        &self,
        build: fn(&M::Input) -> M::Output,
        require_nand_only: bool,
    ) -> Vec<<M::Output as ModuleIo>::Value> {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = M::Input::allocate();
            let output = build(&input);
            (input, output)
        });
        if require_nand_only {
            let _ = circuit.export_gate_reg();
        }

        self.steps
            .iter()
            .map(|step| {
                input.drive(&mut circuit, &step.input);
                circuit.execute_gates();
                for _ in 0..step.cycles {
                    circuit.clock_tick();
                    circuit.execute_gates();
                }
                output.sample(&circuit)
            })
            .collect()
    }

    pub fn verilog_testbench(&self) -> String {
        let module_name = M::verilog_identity().module_name();
        let (_, (input, output)) = build_circuit(|| (M::Input::allocate(), M::Output::allocate()));
        let inputs = input.bindings();
        let outputs = output.bindings();
        let mut text = String::from("module tb;\n");
        if M::USES_MAIN_CLOCK {
            text.push_str("reg clk = 1'b0;\n");
        }
        for binding in &inputs {
            text.push_str(&format!(
                "reg {}{};\n",
                verilog_width(binding.wires.len()),
                binding.name
            ));
        }
        for binding in &outputs {
            text.push_str(&format!(
                "wire {}{};\n",
                verilog_width(binding.wires.len()),
                binding.name
            ));
        }
        text.push_str(&format!("\n{module_name} dut("));
        let mut connections = Vec::new();
        if M::USES_MAIN_CLOCK {
            connections.push(".clk(clk)".to_string());
        }
        connections.extend(
            inputs
                .iter()
                .chain(&outputs)
                .map(|binding| format!(".{}({})", binding.name, binding.name)),
        );
        text.push_str(&connections.join(", "));
        text.push_str(");\n\ninitial begin\n");
        for (index, step) in self.steps.iter().enumerate() {
            for value in M::Input::verilog_values(&step.input) {
                validate_verilog_io_value(&value);
                text.push_str(&format!(
                    "    {} = {}'d{};\n",
                    value.name, value.width, value.value
                ));
            }
            if M::USES_MAIN_CLOCK {
                text.push_str(&format!(
                    "    repeat ({}) begin #5; clk = 1'b1; #1; clk = 1'b0; #4; end\n",
                    step.cycles
                ));
            } else {
                text.push_str("    #1;\n");
            }
            if let Some(expected) = &step.expected {
                for value in M::Output::verilog_values(expected) {
                    validate_verilog_io_value(&value);
                    text.push_str(&format!(
                        "    if ({} !== {}'d{}) begin $display(\"FAIL: step {} output {}\"); $finish(1); end\n",
                        value.name, value.width, value.value, index, value.name
                    ));
                }
            }
        }
        text.push_str("    $display(\"DIGITAL_DESIGN_PASS\");\n    $finish;\nend\nendmodule\n");
        text
    }
}

fn validate_verilog_io_value(value: &crate::VerilogIoValue) {
    assert!(
        (1..=64).contains(&value.width),
        "Verilog test value `{}` has invalid width {}",
        value.name,
        value.width
    );
    if value.width < 64 {
        assert!(
            value.value < (1u64 << value.width),
            "Verilog test value `{}`={} does not fit in {} bits",
            value.name,
            value.value,
            value.width
        );
    }
}

fn verilog_width(width: usize) -> String {
    if width == 1 {
        String::new()
    } else {
        format!("[{}:0] ", width - 1)
    }
}

#[derive(Debug)]
pub enum VerilogSimulationError {
    Project(ProjectError),
    Io(std::io::Error),
    ToolFailed {
        tool: String,
        status: ExitStatus,
        stdout: String,
        stderr: String,
        working_directory: PathBuf,
    },
    MissingPassMarker {
        stdout: String,
        stderr: String,
        working_directory: PathBuf,
    },
}

impl Display for VerilogSimulationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Project(error) => Display::fmt(error, formatter),
            Self::Io(error) if error.kind() == std::io::ErrorKind::NotFound => write!(
                formatter,
                "Verilog simulator tool was not found ({error}); install Icarus Verilog or set IVERILOG and VVP"
            ),
            Self::Io(error) => Display::fmt(error, formatter),
            Self::ToolFailed {
                tool,
                status,
                stdout,
                stderr,
                working_directory,
            } => write!(
                formatter,
                "`{tool}` failed with {status} in {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                working_directory.display()
            ),
            Self::MissingPassMarker {
                stdout,
                stderr,
                working_directory,
            } => write!(
                formatter,
                "Verilog simulation finished without `DIGITAL_DESIGN_PASS` in {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                working_directory.display()
            ),
        }
    }
}

impl std::error::Error for VerilogSimulationError {}

impl From<ProjectError> for VerilogSimulationError {
    fn from(value: ProjectError) -> Self {
        Self::Project(value)
    }
}

impl From<std::io::Error> for VerilogSimulationError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Explicitly compile and run a module's Verilog source testbench.
///
/// This is intentionally not called by ordinary test helpers.
pub fn verify_verilog_with_iverilog<M: Module>() -> Result<(), VerilogSimulationError> {
    let test = crate::project::explicit_verilog_source_test::<M>()?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "digital-design-verilog-{}-{}-{nonce}",
        std::process::id(),
        test.module_name
    ));
    fs::create_dir(&directory)?;
    let module_path = directory.join("module.v");
    let testbench_path = directory.join("testbench.v");
    let output_path = directory.join("simulation.vvp");
    fs::write(&module_path, &test.source)?;
    fs::write(&testbench_path, &test.testbench)?;

    let iverilog = std::env::var_os("IVERILOG").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP").unwrap_or_else(|| "vvp".into());
    let tool_path = simulator_path(&iverilog, &vvp)?;
    let mut compile_command = Command::new(&iverilog);
    compile_command
        .current_dir(&directory)
        .env("PATH", &tool_path)
        .env("TMP", &directory)
        .env("TEMP", &directory)
        .args(["-g2005", "-s", "tb", "-o"])
        .arg(&output_path)
        .arg(&module_path)
        .arg(&testbench_path);
    let compile = compile_command.output()?;
    if !compile.status.success() {
        return Err(VerilogSimulationError::ToolFailed {
            tool: iverilog.to_string_lossy().into_owned(),
            status: compile.status,
            stdout: String::from_utf8_lossy(&compile.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&compile.stderr).into_owned(),
            working_directory: directory,
        });
    }

    let mut simulation_command = Command::new(&vvp);
    simulation_command
        .current_dir(&directory)
        .env("PATH", &tool_path)
        .env("TMP", &directory)
        .env("TEMP", &directory)
        .arg(&output_path);
    let simulation = simulation_command.output()?;
    if !simulation.status.success() {
        return Err(VerilogSimulationError::ToolFailed {
            tool: vvp.to_string_lossy().into_owned(),
            status: simulation.status,
            stdout: String::from_utf8_lossy(&simulation.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&simulation.stderr).into_owned(),
            working_directory: directory,
        });
    }
    let stdout = String::from_utf8_lossy(&simulation.stdout);
    if !stdout
        .lines()
        .any(|line| line.trim() == "DIGITAL_DESIGN_PASS")
    {
        return Err(VerilogSimulationError::MissingPassMarker {
            stdout: stdout.into_owned(),
            stderr: String::from_utf8_lossy(&simulation.stderr).into_owned(),
            working_directory: directory,
        });
    }

    fs::remove_dir_all(&directory)?;
    Ok(())
}

fn simulator_path(
    iverilog: &std::ffi::OsStr,
    vvp: &std::ffi::OsStr,
) -> Result<std::ffi::OsString, std::io::Error> {
    let mut paths = Vec::new();
    for executable in [iverilog, vvp] {
        let path = std::path::Path::new(executable);
        if let Some(parent) = path.parent().filter(|_| path.is_absolute()) {
            if !paths.iter().any(|existing| existing == parent) {
                paths.push(parent.to_path_buf());
            }
        }
    }
    if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current));
    }
    std::env::join_paths(paths).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("cannot construct Verilog simulator PATH: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Hardware, ModuleIo, ProjectError, VerilogProject};
    use digital_design_code::{build_circuit, CircuitWires, Wire};

    #[derive(Clone, ModuleIo)]
    struct ScalarInput {
        value: Wire,
    }

    #[derive(Clone, ModuleIo)]
    struct ScalarOutput {
        result: Wire,
    }

    #[derive(Hardware)]
    #[hardware(namespace = "tests")]
    struct BadSignature;

    impl Module for BadSignature {
        type Input = ScalarInput;
        type Output = ScalarOutput;
        type EmuState = ();

        fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

        fn execute_emu(
            _state: &mut Self::EmuState,
            _circuit: &mut CircuitWires,
            _input: &Self::Input,
            _output: &Self::Output,
        ) {
        }

        fn verilog_source() -> Option<String> {
            Some("module BadSignature(input wire wrong, output wire result); endmodule".to_string())
        }
    }

    #[test]
    fn handwritten_verilog_signature_is_checked() {
        let error = VerilogProject::generate::<BadSignature>().unwrap_err();
        assert!(matches!(error, ProjectError::InvalidHandwrittenVerilog(_)));
    }

    #[derive(Hardware)]
    #[hardware(namespace = "tests")]
    struct UntestedVerilog;

    impl Module for UntestedVerilog {
        type Input = ScalarInput;
        type Output = ScalarOutput;
        type EmuState = ();

        fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

        fn execute_emu(
            _state: &mut Self::EmuState,
            _circuit: &mut CircuitWires,
            _input: &Self::Input,
            _output: &Self::Output,
        ) {
        }

        fn verilog_source() -> Option<String> {
            Some(
                "module UntestedVerilog(input wire value, output wire result); assign result = value; endmodule"
                    .to_string(),
            )
        }
    }

    #[test]
    fn explicit_verilog_requires_a_testbench() {
        let error = VerilogProject::generate::<UntestedVerilog>().unwrap_err();
        assert!(matches!(error, ProjectError::MissingVerilogTestbench(_)));
    }

    #[derive(Hardware)]
    #[hardware(namespace = "tests")]
    struct MixedLeaf;

    impl Module for MixedLeaf {
        type Input = ScalarInput;
        type Output = ScalarOutput;
        type EmuState = ();

        fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

        fn execute_emu(
            _state: &mut Self::EmuState,
            circuit: &mut CircuitWires,
            input: &Self::Input,
            output: &Self::Output,
        ) {
            output.result.set(circuit, input.value.get(circuit));
        }

        fn nand(input: &Self::Input) -> Self::Output {
            ScalarOutput {
                result: !(!input.value),
            }
        }
    }

    #[test]
    fn host_circuit_can_mix_explicit_emu_and_nand_children() {
        let (mut circuit, (input, emu, nand)) = build_circuit(|| {
            let input = ScalarInput::allocate();
            let emu = MixedLeaf::emu(&input);
            let nand = MixedLeaf::nand(&input);
            (input, emu, nand)
        });
        input.drive(&mut circuit, &ScalarInputValue { value: true });
        circuit.execute_gates();
        assert_eq!(emu.sample(&circuit), ScalarOutputValue { result: true });
        assert_eq!(nand.sample(&circuit), ScalarOutputValue { result: true });
    }
}
