//! Adapter between the G16 physical data port and its fixed MMIO device page.

use digital_design_circuit::{input_const, mux2, mux2_w, reg, reg_w, CircuitWires, Wire, Wires};
use digital_design_hardware::{HardwareIdentity, Module, ModuleIo, VerilogIdentity};

pub const G16_MMIO_BASE: u32 = 0x0000_ff00;
pub const G16_MMIO_END: u32 = 0x0000_ffff;

#[derive(Clone, ModuleIo)]
pub struct G16MmioBridgeInput {
    pub reset: Wire,
    pub cpu_request_valid: Wire,
    pub cpu_write: Wire,
    pub cpu_address: Wires<32>,
    pub cpu_write_data: Wires<16>,
    pub cpu_response_ready: Wire,
    pub device_read_data: Wires<16>,
}

#[derive(Clone, ModuleIo)]
pub struct G16MmioBridgeOutput {
    pub cpu_request_ready: Wire,
    pub cpu_response_valid: Wire,
    pub cpu_read_data: Wires<16>,
    pub cpu_error: Wire,
    pub device_index: Wires<4>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write_data: Wires<16>,
}

pub struct G16MmioBridge;

impl HardwareIdentity for G16MmioBridge {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("G16MmioBridge").namespace(["components", "cpu", "g16"])
    }
}

#[derive(Default)]
pub struct G16MmioBridgeState {
    response_valid: bool,
    read_data: u16,
    error: bool,
}

impl Module for G16MmioBridge {
    type Input = G16MmioBridgeInput;
    type Output = G16MmioBridgeOutput;
    type EmuState = G16MmioBridgeState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        G16MmioBridgeState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let request_ready = !state.response_valid;
        let selected =
            request_ready && input.cpu_request_valid && is_mmio_address(input.cpu_address as u32);
        output.drive(
            circuit,
            &G16MmioBridgeOutputValue {
                cpu_request_ready: request_ready,
                cpu_response_valid: state.response_valid,
                cpu_read_data: u64::from(state.read_data),
                cpu_error: state.error,
                device_index: (input.cpu_address >> 4) & 15,
                device_channel: input.cpu_address & 15,
                device_read_enable: selected && !input.cpu_write,
                device_write_enable: selected && input.cpu_write,
                device_write_data: input.cpu_write_data,
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
        if input.reset {
            *state = G16MmioBridgeState::default();
            return;
        }
        if state.response_valid {
            if input.cpu_response_ready {
                state.response_valid = false;
            }
            return;
        }
        if input.cpu_request_valid {
            state.response_valid = true;
            state.error = !is_mmio_address(input.cpu_address as u32);
            state.read_data = if input.cpu_write || state.error {
                0
            } else {
                input.device_read_data as u16
            };
        }
    }

    fn nand(input: &Self::Input) -> Self::Output {
        let zero = input_const(0);

        let is_mmio = wires_equal_constant(&input.cpu_address.wires[16..], 0)
            & wires_equal_constant(&input.cpu_address.wires[8..16], 0xff);

        let response_valid = reg();
        let read_data = reg_w::<16>();
        let error = reg();

        let request_ready = !response_valid.out();
        let accepting = input.cpu_request_valid & request_ready;
        let clear = response_valid.out() & input.cpu_response_ready;
        let next_valid = set_clear_next(response_valid.out(), accepting, clear);
        response_valid.set_in(mux2(next_valid, zero, input.reset));

        let load_data = mux2_w(
            input.device_read_data,
            const_wires(0),
            input.cpu_write | !is_mmio,
        );
        read_data.set_in(mux2_w(
            mux2_w(read_data.out, load_data, accepting),
            const_wires(0),
            input.reset,
        ));
        error.set_in(mux2(
            mux2(error.out(), !is_mmio, accepting),
            zero,
            input.reset,
        ));

        G16MmioBridgeOutput {
            cpu_request_ready: request_ready,
            cpu_response_valid: response_valid.out(),
            cpu_read_data: read_data.out,
            cpu_error: error.out(),
            device_index: Wires {
                wires: std::array::from_fn(|bit| input.cpu_address.wires[4 + bit]),
            },
            device_channel: Wires {
                wires: std::array::from_fn(|bit| input.cpu_address.wires[bit]),
            },
            device_read_enable: accepting & is_mmio & !input.cpu_write,
            device_write_enable: accepting & is_mmio & input.cpu_write,
            device_write_data: input.cpu_write_data,
        }
    }
}

fn const_wires<const WIDTH: usize>(value: u64) -> Wires<WIDTH> {
    Wires {
        wires: std::array::from_fn(|bit| input_const(((value >> bit) & 1) as u8)),
    }
}

fn wires_equal_constant(wires: &[Wire], value: u64) -> Wire {
    wires
        .iter()
        .enumerate()
        .fold(input_const(1), |equal, (bit, &wire)| {
            equal & wire.eq_const(((value >> bit) & 1) as u8)
        })
}

/// Registered set/clear behavior: `set` wins over hold, `clear` only applies
/// while the register is set.
fn set_clear_next(current: Wire, set: Wire, clear: Wire) -> Wire {
    set | (current & !clear)
}

const fn is_mmio_address(address: u32) -> bool {
    address >= G16_MMIO_BASE && address <= G16_MMIO_END
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ModuleTest, TestStep, VerilogProject};

    fn idle() -> G16MmioBridgeInputValue {
        G16MmioBridgeInputValue {
            reset: false,
            cpu_request_valid: false,
            cpu_write: false,
            cpu_address: 0,
            cpu_write_data: 0,
            cpu_response_ready: false,
            device_read_data: 0,
        }
    }

    fn request(
        address: u32,
        write: bool,
        response_ready: bool,
        device_read_data: u16,
    ) -> G16MmioBridgeInputValue {
        G16MmioBridgeInputValue {
            cpu_request_valid: true,
            cpu_write: write,
            cpu_address: u64::from(address),
            cpu_write_data: 0x1234,
            cpu_response_ready: response_ready,
            device_read_data: u64::from(device_read_data),
            ..idle()
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn expected(
        request_ready: bool,
        response_valid: bool,
        read_data: u16,
        error: bool,
        device_index: u64,
        device_channel: u64,
        read_enable: bool,
        write_enable: bool,
        write_data: u16,
    ) -> G16MmioBridgeOutputValue {
        G16MmioBridgeOutputValue {
            cpu_request_ready: request_ready,
            cpu_response_valid: response_valid,
            cpu_read_data: u64::from(read_data),
            cpu_error: error,
            device_index,
            device_channel,
            device_read_enable: read_enable,
            device_write_enable: write_enable,
            device_write_data: u64::from(write_data),
        }
    }

    #[test]
    fn emu_and_nand_decode_hold_and_reset_the_response_channel() {
        ModuleTest::<G16MmioBridge>::new(vec![
            // Synchronous reset clears the response registers.
            TestStep::new(
                G16MmioBridgeInputValue {
                    reset: true,
                    ..idle()
                },
                expected(true, false, 0, false, 0, 0, false, false, 0),
            ),
            // Read request: the response is registered one cycle later.
            TestStep::new(
                request(0x0000_ff2e, false, false, 0xabcd),
                expected(false, true, 0xabcd, false, 2, 14, false, false, 0x1234),
            ),
            // Consuming the response frees the channel; the still-raised
            // request decodes combinationally but is not yet accepted.
            TestStep::new(
                request(0x0000_ff2e, false, true, 0xabcd),
                expected(true, false, 0xabcd, false, 2, 14, true, false, 0x1234),
            ),
            // The held request is accepted back-to-back.
            TestStep::new(
                request(0x0000_ff2e, false, true, 0xabcd),
                expected(false, true, 0xabcd, false, 2, 14, false, false, 0x1234),
            ),
            // Write request pulses the device write enable while the previous
            // response is consumed; write data passes through.
            TestStep::new(
                request(0x0000_ff20, true, true, 0xabcd),
                expected(true, false, 0xabcd, false, 2, 0, false, true, 0x1234),
            ),
            // Writes register a zeroed, error-free response.
            TestStep::new(
                request(0x0000_ff20, true, true, 0xabcd),
                expected(false, true, 0, false, 2, 0, false, false, 0x1234),
            ),
            // Addresses outside the fixed page do not touch device enables.
            TestStep::new(
                request(0x0001_ff20, false, true, 0xabcd),
                expected(true, false, 0, false, 2, 0, false, false, 0x1234),
            ),
            // Out-of-page requests register an error response with zero data.
            TestStep::new(
                request(0x0001_ff20, false, true, 0xabcd),
                expected(false, true, 0, true, 2, 0, false, false, 0x1234),
            ),
            // Reset mid-transaction clears the registers; the combinational
            // device enables do not depend on reset.
            TestStep::new(
                G16MmioBridgeInputValue {
                    reset: true,
                    ..request(0x0000_ff2e, false, false, 0x5555)
                },
                expected(true, false, 0, false, 2, 14, true, false, 0x1234),
            ),
            // A request after reset captures fresh device read data.
            TestStep::new(
                request(0x0000_ff2e, false, false, 0x5555),
                expected(false, true, 0x5555, false, 2, 14, false, false, 0x1234),
            ),
        ])
        .run_emu_and_nand();
    }

    #[test]
    fn export_has_no_target_resource_claims() {
        assert!(VerilogProject::generate::<G16MmioBridge>()
            .unwrap()
            .resource_claims
            .is_empty());
    }
}
