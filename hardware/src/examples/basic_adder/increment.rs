use crate::{Module, ModuleIo};
use digital_design_code::{add_naive, CircuitWires, Wires};

#[derive(Clone, ModuleIo)]
pub struct Increment6Input {
    pub value: Wires<6>,
}

#[derive(Clone, ModuleIo)]
pub struct Increment6Output {
    pub incremented: Wires<6>,
}

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

    fn verilog_source() -> Option<&'static str> {
        Some(include_str!("increment.v"))
    }
}
