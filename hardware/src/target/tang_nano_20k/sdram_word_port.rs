use crate::{HardwareIdentity, Module, ModuleIo, VerilogIdentity};
use digital_design_code::{CircuitWires, Wire, Wires};

const PHYSICAL_WORDS: usize = 1 << 22;

/// One physical 16-bit memory transaction above the fitted Controller HS.
#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramWordPortInput {
    pub reset: Wire,
    pub request_valid: Wire,
    pub write: Wire,
    pub address: Wires<22>,
    pub write_data: Wires<16>,
    pub response_ready: Wire,
    pub controller_read_data: Wires<32>,
    pub controller_read_valid: Wire,
    pub controller_init_done: Wire,
    pub controller_command_ack: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct TangNano20KSdramWordPortOutput {
    pub request_ready: Wire,
    pub response_valid: Wire,
    pub read_data: Wires<16>,
    pub error: Wire,
    pub controller_command_valid: Wire,
    pub controller_command: Wires<3>,
    pub controller_precharge: Wire,
    pub controller_address: Wires<21>,
    pub controller_write_mask: Wires<4>,
    pub controller_write_data: Wires<32>,
    pub controller_burst_length: Wires<8>,
}

pub struct TangNano20KSdramWordPort;

impl HardwareIdentity for TangNano20KSdramWordPort {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("TangNano20KSdramWordPort").namespace([
            "target",
            "tang_nano_20k",
            "sdram",
        ])
    }
}

pub struct TangNano20KSdramWordPortState {
    memory: Box<[u16]>,
    pending: Option<Pending>,
    delay: u8,
    response_valid: bool,
    read_data: u16,
}

#[derive(Clone, Copy)]
struct Pending {
    write: bool,
    address: usize,
    write_data: u16,
}

impl Module for TangNano20KSdramWordPort {
    type Input = TangNano20KSdramWordPortInput;
    type Output = TangNano20KSdramWordPortOutput;
    type EmuState = TangNano20KSdramWordPortState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        TangNano20KSdramWordPortState {
            memory: vec![0; PHYSICAL_WORDS].into_boxed_slice(),
            pending: None,
            delay: 0,
            response_valid: false,
            read_data: 0,
        }
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        output.drive(
            circuit,
            &TangNano20KSdramWordPortOutputValue {
                request_ready: input.controller_init_done
                    && state.pending.is_none()
                    && !state.response_valid,
                response_valid: state.response_valid,
                read_data: u64::from(state.read_data),
                error: false,
                controller_command_valid: false,
                controller_command: 0b111,
                controller_precharge: false,
                controller_address: 0,
                controller_write_mask: 0,
                controller_write_data: 0,
                controller_burst_length: 0,
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
            state.pending = None;
            state.response_valid = false;
            state.delay = 0;
            return;
        }
        if !input.controller_init_done {
            state.pending = None;
            state.response_valid = false;
            state.delay = 0;
            return;
        }
        if state.response_valid {
            if input.response_ready {
                state.response_valid = false;
            }
            return;
        }
        if let Some(pending) = state.pending {
            if state.delay != 0 {
                state.delay -= 1;
                return;
            }
            if pending.write {
                state.memory[pending.address] = pending.write_data;
                state.read_data = 0;
            } else {
                state.read_data = state.memory[pending.address];
            }
            state.pending = None;
            state.response_valid = true;
            return;
        }
        if input.request_valid {
            state.pending = Some(Pending {
                write: input.write,
                address: input.address as usize,
                write_data: input.write_data as u16,
            });
            state.delay = 2;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("sdram_word_port.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("sdram_word_port_tb.v").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_code::{build_circuit, Circuit};

    fn drive(
        circuit: &mut Circuit,
        input: &TangNano20KSdramWordPortInput,
        request_valid: bool,
        write: bool,
        address: u64,
        write_data: u64,
        response_ready: bool,
    ) {
        input.drive(
            circuit,
            &TangNano20KSdramWordPortInputValue {
                reset: false,
                request_valid,
                write,
                address,
                write_data,
                response_ready,
                controller_read_data: 0,
                controller_read_valid: false,
                controller_init_done: true,
                controller_command_ack: false,
            },
        );
    }

    fn transact(
        circuit: &mut Circuit,
        input: &TangNano20KSdramWordPortInput,
        output: &TangNano20KSdramWordPortOutput,
        write: bool,
        address: u64,
        write_data: u64,
    ) -> u64 {
        drive(circuit, input, true, write, address, write_data, false);
        circuit.execute_gates();
        assert!(output.sample(circuit).request_ready);
        circuit.clock_tick();
        drive(circuit, input, false, false, 0, 0, false);
        for _ in 0..8 {
            circuit.execute_gates();
            let value = output.sample(circuit);
            if value.response_valid {
                let result = value.read_data;
                drive(circuit, input, false, false, 0, 0, true);
                circuit.clock_tick();
                return result;
            }
            circuit.clock_tick();
        }
        panic!("SDRAM emulator transaction did not complete")
    }

    #[test]
    fn emulator_preserves_full_physical_addresses() {
        let (mut circuit, (input, output)) = build_circuit(|| {
            let input = TangNano20KSdramWordPortInput::allocate();
            let output = TangNano20KSdramWordPort::hardware(&input);
            (input, output)
        });
        drive(&mut circuit, &input, false, false, 0, 0, false);
        assert_eq!(transact(&mut circuit, &input, &output, true, 7, 0x1234), 0);
        assert_eq!(
            transact(&mut circuit, &input, &output, true, 0x10_0007, 0xabcd),
            0
        );
        assert_eq!(transact(&mut circuit, &input, &output, false, 7, 0), 0x1234);
        assert_eq!(
            transact(&mut circuit, &input, &output, false, 0x10_0007, 0),
            0xabcd
        );
    }

    #[test]
    #[ignore = "explicit external simulation of the Controller HS transaction adapter"]
    fn verify_verilog_with_iverilog() {
        crate::verify_verilog_with_iverilog::<TangNano20KSdramWordPort>().unwrap();
    }
}
