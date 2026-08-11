use crate::{Module, ModuleIo};
use digital_design_code::build_circuit;

#[derive(Clone, Debug)]
pub struct TestStep<I, O> {
    pub input: I,
    pub expected: O,
}

impl<I, O> TestStep<I, O> {
    pub fn new(input: I, expected: O) -> Self {
        Self { input, expected }
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
        assert_eq!(emu.len(), nand.len());
        for (index, ((emu, nand), step)) in emu.iter().zip(&nand).zip(&self.steps).enumerate() {
            assert_eq!(emu, nand, "emu/NAND mismatch at test step {index}");
            assert_eq!(
                emu, &step.expected,
                "unexpected output at test step {index}"
            );
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
                circuit.clock_tick();
                circuit.execute_gates();
                output.sample(&circuit)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModuleIo, ProjectError, VerilogProject};
    use digital_design_code::{build_circuit, CircuitWires, Wire, Wires};

    #[derive(Clone, ModuleIo)]
    struct ScalarInput {
        value: Wire,
    }

    #[derive(Clone, ModuleIo)]
    struct ScalarOutput {
        result: Wire,
    }

    #[derive(Clone, ModuleIo)]
    struct GenericBus<const WIDTH: usize> {
        value: Wires<WIDTH>,
    }

    #[test]
    fn module_io_supports_const_generic_bus_widths() {
        let (_, io) = build_circuit(GenericBus::<7>::allocate);
        assert_eq!(io.bindings()[0].wires.len(), 7);
    }

    struct MissingNand;

    impl Module for MissingNand {
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
    }

    #[test]
    fn missing_nand_panics_without_poisoning_the_circuit_builder() {
        let panic = std::panic::catch_unwind(|| {
            build_circuit(|| {
                let input = ScalarInput::allocate();
                MissingNand::nand(&input)
            });
        });
        assert!(panic.is_err());
        let (_, wire) = build_circuit(digital_design_code::input);
        assert_ne!(wire.0, 0);
    }

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

        fn verilog_source() -> Option<&'static str> {
            Some("module BadSignature(input wire wrong, output wire result); endmodule")
        }
    }

    #[test]
    fn handwritten_verilog_signature_is_checked() {
        let error = VerilogProject::generate::<BadSignature>().unwrap_err();
        assert!(matches!(error, ProjectError::InvalidHandwrittenVerilog(_)));
    }

    struct ParameterizedVerilog;

    impl Module for ParameterizedVerilog {
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

        fn verilog_source() -> Option<&'static str> {
            Some(
                "module ParameterizedVerilogExtra(input wire value); endmodule\n\
                 module ParameterizedVerilog #(parameter UNUSED = 1) (\n\
                     output wire result,\n\
                     input wire value\n\
                 );\n\
                 assign result = value;\n\
                 endmodule\n",
            )
        }
    }

    #[test]
    fn handwritten_verilog_accepts_parameters_and_port_reordering() {
        VerilogProject::generate::<ParameterizedVerilog>().unwrap();
    }

    struct NamedGeneric<const WIDTH: usize>;

    impl<const WIDTH: usize> Module for NamedGeneric<WIDTH> {
        type Input = ScalarInput;
        type Output = ScalarOutput;
        type EmuState = ();

        fn verilog_name() -> String {
            format!("NamedGeneric{WIDTH}")
        }

        fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

        fn execute_emu(
            _state: &mut Self::EmuState,
            _circuit: &mut CircuitWires,
            _input: &Self::Input,
            _output: &Self::Output,
        ) {
        }

        fn nand(input: &Self::Input) -> Self::Output {
            ScalarOutput {
                result: input.value,
            }
        }
    }

    #[test]
    fn generic_modules_can_supply_stable_hdl_names() {
        let project = VerilogProject::generate::<NamedGeneric<8>>().unwrap();
        assert_eq!(project.top_module, "NamedGeneric8");
        assert!(project
            .files
            .keys()
            .any(|path| path.ends_with("named_generic8.v")));
    }

    struct NandCallsEmu;

    impl Module for NandCallsEmu {
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
            Self::emu(input)
        }
    }

    #[test]
    #[should_panic(expected = "Externals are not supported")]
    fn nand_test_rejects_external_implementations() {
        ModuleTest::<NandCallsEmu>::new([TestStep::new(
            ScalarInputValue { value: false },
            ScalarOutputValue { result: false },
        )])
        .run_emu_and_nand();
    }
}
