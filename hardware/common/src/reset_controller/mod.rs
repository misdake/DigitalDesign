//! Clock-domain reset conditioning for FPGA top-level designs.

use crate::{Hardware, HardwareIdentity, Module, ModuleIo};
use askama::Template;
use digital_design_circuit::{CircuitWires, Wire};

#[derive(Clone, ModuleIo)]
pub struct ResetControllerInput {
    /// Asynchronous, active-high board or supervisor reset request.
    pub external_reset: Wire,
    /// Asynchronous indication that the clock source is usable.
    pub clock_ready: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct ResetControllerOutput {
    /// Active-high reset, deasserted synchronously after the hold interval.
    pub reset: Wire,
    /// Two-flop synchronized copy of `clock_ready`.
    pub clock_ready_synchronized: Wire,
    /// Sticky diagnostic indicating that an external reset was observed.
    pub external_reset_seen: Wire,
}

#[derive(Clone, Copy, Debug)]
pub struct ResetControllerState<const HOLD_CYCLES: u32> {
    external_meta: bool,
    external_sync: bool,
    ready_meta: bool,
    ready_sync: bool,
    remaining: u32,
    external_reset_seen: bool,
}

impl<const HOLD_CYCLES: u32> Default for ResetControllerState<HOLD_CYCLES> {
    fn default() -> Self {
        validate::<HOLD_CYCLES>();
        Self {
            external_meta: false,
            external_sync: false,
            ready_meta: false,
            ready_sync: false,
            remaining: HOLD_CYCLES,
            external_reset_seen: false,
        }
    }
}

impl<const HOLD_CYCLES: u32> ResetControllerState<HOLD_CYCLES> {
    pub fn reset(&self) -> bool {
        self.external_sync || !self.ready_sync || self.remaining != 0
    }

    pub fn clock_ready(&self) -> bool {
        self.ready_sync
    }

    pub fn external_reset_seen(&self) -> bool {
        self.external_reset_seen
    }

    pub fn advance(&mut self, external_reset: bool, clock_ready: bool) {
        let previous_external_meta = self.external_meta;
        let previous_ready_meta = self.ready_meta;
        self.external_meta = external_reset;
        self.external_sync = previous_external_meta;
        self.ready_meta = clock_ready;
        self.ready_sync = previous_ready_meta;

        if self.external_sync {
            self.external_reset_seen = true;
        }
        if self.external_sync || !self.ready_sync {
            self.remaining = HOLD_CYCLES;
        } else if self.remaining != 0 {
            self.remaining -= 1;
        }
    }
}

/// Synchronizes reset inputs and guarantees a deterministic reset hold time.
///
/// Reset assertion may be delayed by the two-flop synchronizer. Deassertion is
/// always synchronous and occurs only after `clock_ready` has crossed the
/// synchronizer and `HOLD_CYCLES` full clock edges have elapsed.
#[derive(Hardware)]
#[hardware(namespace = "components/control")]
pub struct ResetController<const HOLD_CYCLES: u32>;

#[derive(Template)]
#[template(path = "reset_controller/reset_controller.v", escape = "none")]
struct ResetControllerTemplate<'a> {
    module_name: &'a str,
    counter_width: usize,
    counter_high_bit: usize,
    hold_cycles: u32,
}

#[derive(Template)]
#[template(path = "reset_controller/reset_controller_tb.v", escape = "none")]
struct ResetControllerTestbenchTemplate<'a> {
    module_name: &'a str,
    hold_cycles: u32,
}

impl<const HOLD_CYCLES: u32> Module for ResetController<HOLD_CYCLES> {
    type Input = ResetControllerInput;
    type Output = ResetControllerOutput;
    type EmuState = ResetControllerState<HOLD_CYCLES>;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        ResetControllerState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        _input: &Self::Input,
        output: &Self::Output,
    ) {
        output.drive(
            circuit,
            &ResetControllerOutputValue {
                reset: state.reset(),
                clock_ready_synchronized: state.clock_ready(),
                external_reset_seen: state.external_reset_seen(),
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        state.advance(input.external_reset, input.clock_ready);
    }

    fn generated_verilog_source() -> Option<String> {
        validate::<HOLD_CYCLES>();
        let module_name = Self::verilog_identity().module_name();
        let counter_width = counter_width(HOLD_CYCLES);
        Some(
            ResetControllerTemplate {
                module_name: &module_name,
                counter_width,
                counter_high_bit: counter_width - 1,
                hold_cycles: HOLD_CYCLES,
            }
            .render()
            .expect("reset controller Verilog template must render"),
        )
    }

    fn verilog_testbench() -> Option<String> {
        validate::<HOLD_CYCLES>();
        let module_name = Self::verilog_identity().module_name();
        Some(
            ResetControllerTestbenchTemplate {
                module_name: &module_name,
                hold_cycles: HOLD_CYCLES,
            }
            .render()
            .expect("reset controller testbench template must render"),
        )
    }
}

fn validate<const HOLD_CYCLES: u32>() {
    assert!(HOLD_CYCLES > 0, "reset hold interval must be non-zero");
}

fn counter_width(maximum: u32) -> usize {
    usize::try_from(u32::BITS - maximum.leading_zeros())
        .unwrap()
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_waits_for_ready_and_restarts_after_external_reset() {
        let mut state = ResetControllerState::<3>::default();
        assert!(state.reset());

        state.advance(false, true);
        state.advance(false, true);
        assert!(state.clock_ready());
        assert!(state.reset());
        for _ in 0..3 {
            state.advance(false, true);
        }
        assert!(!state.reset());

        state.advance(true, true);
        state.advance(true, true);
        assert!(state.reset());
        assert!(state.external_reset_seen());
        state.advance(false, true);
        state.advance(false, true);
        for _ in 0..3 {
            state.advance(false, true);
        }
        assert!(!state.reset());
        assert!(state.external_reset_seen());
    }

    #[test]
    #[ignore = "explicit external simulator validation"]
    fn verify_verilog_with_iverilog() {
        crate::verify_verilog_with_iverilog::<ResetController<3>>().unwrap();
    }
}
