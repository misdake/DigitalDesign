//! Registered clock-enable divider component.

use crate::{Hardware, Module, ModuleIo, ModuleTest, TestStep};
use askama::Template;
use digital_design_circuit::{
    add_naive, input_const, input_w_const, mux2_w, reg, reg_w, CircuitWires, Wire, Wires,
};

#[derive(Clone, ModuleIo)]
pub struct ClockDividerInput {}

#[derive(Clone, ModuleIo)]
pub struct ClockDividerOutput {
    /// One main-clock-cycle pulse after every `DIVISOR` input cycles.
    pub tick: Wire,
}

#[derive(Clone, Copy, Debug)]
pub struct ClockDividerState<const DIVISOR: u64, const WIDTH: usize> {
    counter: u64,
    tick: bool,
}

impl<const DIVISOR: u64, const WIDTH: usize> Default for ClockDividerState<DIVISOR, WIDTH> {
    fn default() -> Self {
        validate::<DIVISOR, WIDTH>();
        Self {
            counter: 0,
            tick: false,
        }
    }
}

impl<const DIVISOR: u64, const WIDTH: usize> ClockDividerState<DIVISOR, WIDTH> {
    pub fn tick(&self) -> bool {
        self.tick
    }

    pub fn advance(&mut self) {
        let terminal = self.counter == DIVISOR - 1;
        self.counter = if terminal { 0 } else { self.counter + 1 };
        self.tick = terminal;
    }
}

/// Produces a registered, single-cycle clock-enable pulse.
///
/// `WIDTH` describes the hardware counter and must be large enough to hold
/// `DIVISOR - 1`. This module intentionally generates an enable rather than a
/// derived clock, keeping downstream logic in the main clock domain.
#[derive(Hardware)]
#[hardware(namespace = "components/timing")]
pub struct ClockDivider<const DIVISOR: u64, const WIDTH: usize>;

#[derive(Template)]
#[template(path = "clock_divider/clock_divider.v", escape = "none")]
struct ClockDividerTemplate<'a> {
    module_name: &'a str,
    width: usize,
    high_bit: usize,
    terminal: u64,
}

impl<const DIVISOR: u64, const WIDTH: usize> Module for ClockDivider<DIVISOR, WIDTH> {
    type Input = ClockDividerInput;
    type Output = ClockDividerOutput;
    type EmuState = ClockDividerState<DIVISOR, WIDTH>;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        ClockDividerState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.tick.set(circuit, u8::from(state.tick()));
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        _circuit: &mut CircuitWires,
        _input: &Self::Input,
        _output: &Self::Output,
    ) {
        state.advance();
    }

    fn nand(_input: &Self::Input) -> Self::Output {
        validate::<DIVISOR, WIDTH>();
        let counter = reg_w::<WIDTH>();
        let terminal = wires_equal_constant(counter.out, DIVISOR - 1);
        let incremented = add_naive(counter.out, constant_wires::<WIDTH>(1)).sum;
        counter.set_in(mux2_w(incremented, input_w_const(0), terminal));

        let tick = reg();
        tick.set_in(terminal);
        ClockDividerOutput { tick: tick.out() }
    }

    fn generated_verilog_source() -> Option<String> {
        validate::<DIVISOR, WIDTH>();
        let module_name = <Self as crate::HardwareIdentity>::verilog_identity().module_name();
        Some(
            ClockDividerTemplate {
                module_name: &module_name,
                width: WIDTH,
                high_bit: WIDTH - 1,
                terminal: DIVISOR - 1,
            }
            .render()
            .expect("clock divider Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        Some(divider_test::<DIVISOR, WIDTH>().verilog_testbench())
    }
}

fn divider_test<const DIVISOR: u64, const WIDTH: usize>() -> ModuleTest<ClockDivider<DIVISOR, WIDTH>>
{
    validate::<DIVISOR, WIDTH>();
    let mut steps = vec![TestStep::new(
        ClockDividerInputValue {},
        ClockDividerOutputValue { tick: true },
    )
    .after_cycles(DIVISOR)];
    if DIVISOR == 1 {
        steps.push(TestStep::new(
            ClockDividerInputValue {},
            ClockDividerOutputValue { tick: true },
        ));
    } else {
        steps.push(TestStep::new(
            ClockDividerInputValue {},
            ClockDividerOutputValue { tick: false },
        ));
        steps.push(
            TestStep::new(
                ClockDividerInputValue {},
                ClockDividerOutputValue { tick: true },
            )
            .after_cycles(DIVISOR - 1),
        );
    }
    ModuleTest::new(steps)
}

fn validate<const DIVISOR: u64, const WIDTH: usize>() {
    assert!(DIVISOR > 0, "clock divisor must be non-zero");
    assert!(
        (1..=64).contains(&WIDTH),
        "clock divider width {WIDTH} is outside 1..=64"
    );
    let capacity = if WIDTH == 64 {
        u128::from(u64::MAX) + 1
    } else {
        1u128 << WIDTH
    };
    assert!(
        u128::from(DIVISOR) <= capacity,
        "clock divisor {DIVISOR} exceeds the {WIDTH}-bit counter"
    );
}

fn constant_wires<const WIDTH: usize>(value: u64) -> Wires<WIDTH> {
    Wires {
        wires: std::array::from_fn(|bit| input_const(((value >> bit) & 1) as u8)),
    }
}

fn wires_equal_constant<const WIDTH: usize>(wires: Wires<WIDTH>, value: u64) -> Wire {
    wires
        .wires
        .iter()
        .enumerate()
        .fold(input_const(1), |equal, (bit, &wire)| {
            equal & wire.eq_const(((value >> bit) & 1) as u8)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VerilogProject;
    use digital_design_circuit::Wires;
    use std::path::Path;

    #[test]
    fn emu_and_nand_produce_the_same_registered_tick() {
        divider_test::<3, 2>().run_emu_and_nand();
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn verify_verilog_with_iverilog() {
        crate::verify_verilog_with_iverilog::<ClockDivider<3, 2>>().unwrap();
        crate::verify_verilog_with_iverilog::<ClockDivider<5, 3>>().unwrap();
    }

    #[test]
    fn large_specialization_exports_without_long_simulation() {
        let project = VerilogProject::generate::<ClockDivider<6_750_000, 23>>().unwrap();
        let source =
            &project.files[Path::new("components/timing/clock_divider/divisor6750000_width23.v")];
        assert!(source.contains("counter == 23'd6749999"));
    }

    #[derive(Clone, ModuleIo)]
    struct DividerBankInput {}

    #[derive(Clone, ModuleIo)]
    struct DividerBankOutput {
        ticks: Wires<3>,
    }

    #[derive(Hardware)]
    #[hardware(namespace = "tests")]
    struct DividerBank;

    impl Module for DividerBank {
        type Input = DividerBankInput;
        type Output = DividerBankOutput;
        type EmuState = ();

        fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {}

        fn execute_emu(
            _state: &mut Self::EmuState,
            _circuit: &mut CircuitWires,
            _input: &Self::Input,
            _output: &Self::Output,
        ) {
        }

        fn nand(input: &Self::Input) -> Self::Output {
            build_bank(
                input,
                ClockDivider::<3, 2>::nand,
                ClockDivider::<5, 3>::nand,
            )
        }

        fn build_verilog(input: &Self::Input) -> Self::Output {
            build_bank(
                input,
                ClockDivider::<3, 2>::verilog,
                ClockDivider::<5, 3>::verilog,
            )
        }
    }

    fn build_bank(
        _input: &DividerBankInput,
        divider3: fn(&ClockDividerInput) -> ClockDividerOutput,
        divider5: fn(&ClockDividerInput) -> ClockDividerOutput,
    ) -> DividerBankOutput {
        let first = divider3(&ClockDividerInput {}).tick;
        let second = divider3(&ClockDividerInput {}).tick;
        let third = divider5(&ClockDividerInput {}).tick;
        DividerBankOutput {
            ticks: Wires {
                wires: [first, second, third],
            },
        }
    }

    #[test]
    fn repeated_and_distinct_specializations_emit_the_right_definitions() {
        let project = VerilogProject::generate::<DividerBank>().unwrap();
        assert_eq!(project.files.len(), 3);
        assert!(project.files.contains_key(Path::new(
            "components/timing/clock_divider/divisor3_width2.v"
        )));
        assert!(project.files.contains_key(Path::new(
            "components/timing/clock_divider/divisor5_width3.v"
        )));
        let top = &project.files[Path::new("tests/divider_bank.v")];
        assert_eq!(top.matches("ClockDivider_DIVISOR3_WIDTH2 ").count(), 2);
        assert_eq!(top.matches("ClockDivider_DIVISOR5_WIDTH3 ").count(), 1);
        assert!(top.contains("u_clock_divider_0"));
        assert!(top.contains("u_clock_divider_1"));
        assert!(top.contains("u_clock_divider_2"));
    }
}
