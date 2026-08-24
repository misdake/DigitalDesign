use digital_design_code::{add_naive, CircuitWires, Wires};
use digital_design_hardware::{
    Hardware, Module, ModuleIo, ModuleTest, TestStep, VerilogVerification,
};

#[derive(Clone, ModuleIo)]
pub struct Increment6Input {
    pub value: Wires<6>,
}

#[derive(Clone, ModuleIo)]
pub struct Increment6Output {
    pub incremented: Wires<6>,
}

#[derive(Hardware)]
#[hardware(namespace = "examples/basic_adder")]
pub struct Increment6;

impl Module for Increment6 {
    type Input = Increment6Input;
    type Output = Increment6Output;
    type EmuState = ();

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

    fn execute_emu(
        _state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let value = input
            .value
            .wires
            .iter()
            .enumerate()
            .fold(0u8, |value, (bit, wire)| value | (wire.get(circuit) << bit));
        output.drive(
            circuit,
            &Increment6OutputValue {
                incremented: u64::from(value.wrapping_add(1) & 0x3f),
            },
        );
    }

    fn nand(input: &Self::Input) -> Self::Output {
        Self::Output {
            incremented: add_naive(input.value, Wires::<6>::parse_u8(1)).sum,
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("increment.v").to_string())
    }

    fn verilog_verification() -> Option<VerilogVerification> {
        Some(increment_test().verilog_verification(include_str!("increment.verified")))
    }
}

fn increment_test() -> ModuleTest<Increment6> {
    ModuleTest::new([
        TestStep::new(
            Increment6InputValue { value: 0 },
            Increment6OutputValue { incremented: 1 },
        ),
        TestStep::new(
            Increment6InputValue { value: 17 },
            Increment6OutputValue { incremented: 18 },
        ),
        TestStep::new(
            Increment6InputValue { value: 63 },
            Increment6OutputValue { incremented: 0 },
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_vectors_pass_emu_and_nand() {
        increment_test().run_emu_and_nand();
    }

    #[test]
    #[ignore = "explicit external simulator validation; copy the printed record into increment.verified"]
    fn verify_handwritten_verilog_with_iverilog() {
        let record = digital_design_hardware::verify_verilog_with_iverilog::<Increment6>().unwrap();
        println!("{record}");
    }
}
