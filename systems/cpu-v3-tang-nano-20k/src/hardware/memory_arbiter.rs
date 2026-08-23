//! Machine-owned arbiter between CpuV3 instruction/data traffic, boot DMA,
//! and the Tang Nano 20K physical SDRAM word port.

use digital_design_circuit::{input_const, mux2, mux2_w, reg_w, CircuitWires, Wire, Wires};
use digital_design_hardware::{HardwareIdentity, Module, ModuleIo, VerilogIdentity};

#[derive(Clone, ModuleIo)]
pub struct CpuV3MemoryArbiterInput {
    pub reset: Wire,

    pub instruction_request_valid: Wire,
    pub instruction_address: Wires<22>,
    pub instruction_response_ready: Wire,

    pub data_request_valid: Wire,
    pub data_write: Wire,
    pub data_address: Wires<22>,
    pub data_write_data: Wires<16>,
    pub data_response_ready: Wire,

    pub dma_request_valid: Wire,
    pub dma_write: Wire,
    pub dma_address: Wires<22>,
    pub dma_write_data: Wires<16>,
    pub dma_response_ready: Wire,

    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<16>,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3MemoryArbiterOutput {
    pub instruction_request_ready: Wire,
    pub instruction_response_valid: Wire,
    pub instruction_read_data: Wires<16>,
    pub instruction_error: Wire,

    pub data_request_ready: Wire,
    pub data_response_valid: Wire,
    pub data_read_data: Wires<16>,
    pub data_error: Wire,

    pub dma_request_ready: Wire,
    pub dma_response_valid: Wire,
    pub dma_read_data: Wires<16>,
    pub dma_error: Wire,

    pub memory_request_valid: Wire,
    pub memory_write: Wire,
    pub memory_address: Wires<22>,
    pub memory_write_data: Wires<16>,
    pub memory_response_ready: Wire,
}

pub struct CpuV3MemoryArbiter;

impl HardwareIdentity for CpuV3MemoryArbiter {
    const TARGET_RESOURCE_LEAF: bool = false;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3MemoryArbiter").namespace(["systems", "cpu_v3_tang_nano_20k"])
    }
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Owner {
    #[default]
    None,
    Instruction,
    Data,
    Dma,
}

#[derive(Default)]
pub struct CpuV3MemoryArbiterState {
    owner: Owner,
}

impl Module for CpuV3MemoryArbiter {
    type Input = CpuV3MemoryArbiterInput;
    type Output = CpuV3MemoryArbiterOutput;
    type EmuState = CpuV3MemoryArbiterState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        CpuV3MemoryArbiterState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let selected = select(&input);
        let request_active = state.owner == Owner::None && selected != Owner::None;
        let response_active = state.owner != Owner::None && input.memory_response_valid;
        output.drive(
            circuit,
            &CpuV3MemoryArbiterOutputValue {
                instruction_request_ready: request_active
                    && selected == Owner::Instruction
                    && input.memory_request_ready,
                instruction_response_valid: response_active && state.owner == Owner::Instruction,
                instruction_read_data: input.memory_read_data,
                instruction_error: response_active
                    && state.owner == Owner::Instruction
                    && input.memory_error,
                data_request_ready: request_active
                    && selected == Owner::Data
                    && input.memory_request_ready,
                data_response_valid: response_active && state.owner == Owner::Data,
                data_read_data: input.memory_read_data,
                data_error: response_active && state.owner == Owner::Data && input.memory_error,
                dma_request_ready: request_active
                    && selected == Owner::Dma
                    && input.memory_request_ready,
                dma_response_valid: response_active && state.owner == Owner::Dma,
                dma_read_data: input.memory_read_data,
                dma_error: response_active && state.owner == Owner::Dma && input.memory_error,
                memory_request_valid: request_active,
                memory_write: match selected {
                    Owner::Instruction | Owner::None => false,
                    Owner::Data => input.data_write,
                    Owner::Dma => input.dma_write,
                },
                memory_address: match selected {
                    Owner::Instruction => input.instruction_address,
                    Owner::Data => input.data_address,
                    Owner::Dma => input.dma_address,
                    Owner::None => 0,
                },
                memory_write_data: match selected {
                    Owner::Data => input.data_write_data,
                    Owner::Dma => input.dma_write_data,
                    Owner::Instruction | Owner::None => 0,
                },
                memory_response_ready: match state.owner {
                    Owner::Instruction => input.instruction_response_ready,
                    Owner::Data => input.data_response_ready,
                    Owner::Dma => input.dma_response_ready,
                    Owner::None => false,
                },
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
            state.owner = Owner::None;
            return;
        }
        if state.owner == Owner::None {
            let selected = select(&input);
            if selected != Owner::None && input.memory_request_ready {
                state.owner = selected;
            }
        } else if input.memory_response_valid && response_ready(state.owner, &input) {
            state.owner = Owner::None;
        }
    }

    fn nand(input: &Self::Input) -> Self::Output {
        let zero = input_const(0);
        let owner = reg_w::<2>();

        // Combinational fixed-priority grant: DMA > data > instruction.
        let selected: Wires<2> = mux2_w(
            mux2_w(
                mux2_w(
                    const_wires(OWNER_NONE),
                    const_wires(OWNER_INSTRUCTION),
                    input.instruction_request_valid,
                ),
                const_wires(OWNER_DATA),
                input.data_request_valid,
            ),
            const_wires(OWNER_DMA),
            input.dma_request_valid,
        );

        let owner_none = owner.out.all_0();
        let selected_any = selected.wires[0] | selected.wires[1];
        let requesting = owner_none & selected_any;
        let accepted = requesting & input.memory_request_ready;

        let selected_instruction = selected.eq_const(OWNER_INSTRUCTION);
        let selected_data = selected.eq_const(OWNER_DATA);
        let selected_dma = selected.eq_const(OWNER_DMA);
        let owner_instruction = owner.out.eq_const(OWNER_INSTRUCTION);
        let owner_data = owner.out.eq_const(OWNER_DATA);
        let owner_dma = owner.out.eq_const(OWNER_DMA);

        let instruction_response_valid = owner_instruction & input.memory_response_valid;
        let data_response_valid = owner_data & input.memory_response_valid;
        let dma_response_valid = owner_dma & input.memory_response_valid;

        let memory_response_ready = (owner_instruction & input.instruction_response_ready)
            | (owner_data & input.data_response_ready)
            | (owner_dma & input.dma_response_ready);

        // Synchronous reset; otherwise capture the accepted owner and release
        // it once the routed response completes.
        let release = !owner_none & input.memory_response_valid & memory_response_ready;
        let next_owner = mux2_w(owner.out, selected, accepted);
        let next_owner = mux2_w(next_owner, const_wires(OWNER_NONE), release);
        owner.set_in(mux2_w(next_owner, const_wires(OWNER_NONE), input.reset));

        CpuV3MemoryArbiterOutput {
            instruction_request_ready: accepted & selected_instruction,
            instruction_response_valid,
            instruction_read_data: input.memory_read_data,
            instruction_error: instruction_response_valid & input.memory_error,
            data_request_ready: accepted & selected_data,
            data_response_valid,
            data_read_data: input.memory_read_data,
            data_error: data_response_valid & input.memory_error,
            dma_request_ready: accepted & selected_dma,
            dma_response_valid,
            dma_read_data: input.memory_read_data,
            dma_error: dma_response_valid & input.memory_error,
            memory_request_valid: requesting,
            memory_write: mux2(
                mux2(zero, input.data_write, selected_data),
                input.dma_write,
                selected_dma,
            ),
            memory_address: mux4(
                [
                    const_wires(0),
                    input.instruction_address,
                    input.data_address,
                    input.dma_address,
                ],
                selected,
            ),
            memory_write_data: mux4(
                [
                    const_wires(0),
                    const_wires(0),
                    input.data_write_data,
                    input.dma_write_data,
                ],
                selected,
            ),
            memory_response_ready,
        }
    }
}

const OWNER_NONE: u8 = 0;
const OWNER_INSTRUCTION: u8 = 1;
const OWNER_DATA: u8 = 2;
const OWNER_DMA: u8 = 3;

fn const_wires<const WIDTH: usize>(value: u8) -> Wires<WIDTH> {
    Wires {
        wires: std::array::from_fn(|bit| input_const(((u64::from(value) >> bit) & 1) as u8)),
    }
}

/// Four-way bus mux. `digital_design_circuit::mux4_w` misroutes select value 2,
/// so the owner mux is built from nested two-way muxes here.
fn mux4<const WIDTH: usize>(values: [Wires<WIDTH>; 4], select: Wires<2>) -> Wires<WIDTH> {
    let [first, second, third, fourth] = values;
    let low = mux2_w(first, second, select.wires[0]);
    let high = mux2_w(third, fourth, select.wires[0]);
    mux2_w(low, high, select.wires[1])
}

fn select(input: &CpuV3MemoryArbiterInputValue) -> Owner {
    if input.dma_request_valid {
        Owner::Dma
    } else if input.data_request_valid {
        Owner::Data
    } else if input.instruction_request_valid {
        Owner::Instruction
    } else {
        Owner::None
    }
}

fn response_ready(owner: Owner, input: &CpuV3MemoryArbiterInputValue) -> bool {
    match owner {
        Owner::Instruction => input.instruction_response_ready,
        Owner::Data => input.data_response_ready,
        Owner::Dma => input.dma_response_ready,
        Owner::None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_hardware::{ModuleTest, TestStep, VerilogProject};

    fn idle() -> CpuV3MemoryArbiterInputValue {
        CpuV3MemoryArbiterInputValue {
            reset: false,
            instruction_request_valid: false,
            instruction_address: 0,
            instruction_response_ready: false,
            data_request_valid: false,
            data_write: false,
            data_address: 0,
            data_write_data: 0,
            data_response_ready: false,
            dma_request_valid: false,
            dma_write: false,
            dma_address: 0,
            dma_write_data: 0,
            dma_response_ready: false,
            memory_request_ready: false,
            memory_response_valid: false,
            memory_read_data: 0,
            memory_error: false,
        }
    }

    /// Raise client requests with recognizable per-client payloads.
    fn requests(instruction: bool, data: bool, dma: bool) -> CpuV3MemoryArbiterInputValue {
        CpuV3MemoryArbiterInputValue {
            instruction_request_valid: instruction,
            instruction_address: 0x111,
            data_request_valid: data,
            data_write: true,
            data_address: 0x222,
            data_write_data: 0xdddd,
            dma_request_valid: dma,
            dma_write: true,
            dma_address: 0x333,
            dma_write_data: 0xaaaa,
            memory_read_data: 0xbeef,
            ..idle()
        }
    }

    /// Output baseline: no handshake activity, read data broadcast through.
    fn quiescent(read_data: u16) -> CpuV3MemoryArbiterOutputValue {
        let read_data = u64::from(read_data);
        CpuV3MemoryArbiterOutputValue {
            instruction_request_ready: false,
            instruction_response_valid: false,
            instruction_read_data: read_data,
            instruction_error: false,
            data_request_ready: false,
            data_response_valid: false,
            data_read_data: read_data,
            data_error: false,
            dma_request_ready: false,
            dma_response_valid: false,
            dma_read_data: read_data,
            dma_error: false,
            memory_request_valid: false,
            memory_write: false,
            memory_address: 0,
            memory_write_data: 0,
            memory_response_ready: false,
        }
    }

    #[test]
    fn emu_and_nand_grant_dma_then_data_then_instruction_atomically() {
        ModuleTest::<CpuV3MemoryArbiter>::new(vec![
            // Synchronous reset clears the owner.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    reset: true,
                    ..idle()
                },
                quiescent(0),
            ),
            // Priority tie under backpressure: DMA wins the forwarded request,
            // but no owner is captured while the memory is not ready.
            TestStep::new(
                requests(true, true, true),
                CpuV3MemoryArbiterOutputValue {
                    memory_request_valid: true,
                    memory_write: true,
                    memory_address: 0x333,
                    memory_write_data: 0xaaaa,
                    ..quiescent(0xbeef)
                },
            ),
            // Memory ready: the DMA request is accepted at this clock edge.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_request_ready: true,
                    ..requests(true, true, true)
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_write: true,
                    memory_address: 0x333,
                    memory_write_data: 0xaaaa,
                    ..quiescent(0xbeef)
                },
            ),
            // DMA owns the port until its response completes. The request
            // signals still combinationally forward the highest-priority
            // requester while `memory_request_valid` is low.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_request_ready: true,
                    memory_response_valid: true,
                    ..requests(true, true, false)
                },
                CpuV3MemoryArbiterOutputValue {
                    dma_response_valid: true,
                    memory_write: true,
                    memory_address: 0x222,
                    memory_write_data: 0xdddd,
                    ..quiescent(0xbeef)
                },
            ),
            // Completing the DMA response releases the port; the data client
            // is granted immediately afterwards.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_request_ready: true,
                    memory_response_valid: true,
                    dma_response_ready: true,
                    ..requests(true, true, false)
                },
                CpuV3MemoryArbiterOutputValue {
                    data_request_ready: true,
                    memory_request_valid: true,
                    memory_write: true,
                    memory_address: 0x222,
                    memory_write_data: 0xdddd,
                    ..quiescent(0xbeef)
                },
            ),
            // The data request is accepted at this clock edge.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_request_ready: true,
                    ..requests(true, true, false)
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_write: true,
                    memory_address: 0x222,
                    memory_write_data: 0xdddd,
                    ..quiescent(0xbeef)
                },
            ),
            // The data response is routed only to the data client, including
            // the error flag, until it is consumed.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_request_ready: true,
                    memory_response_valid: true,
                    memory_error: true,
                    ..requests(true, true, false)
                },
                CpuV3MemoryArbiterOutputValue {
                    data_response_valid: true,
                    data_error: true,
                    memory_write: true,
                    memory_address: 0x222,
                    memory_write_data: 0xdddd,
                    ..quiescent(0xbeef)
                },
            ),
            // Consuming the data response releases the port to the
            // instruction client (the data client drops its request).
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_request_ready: true,
                    memory_response_valid: true,
                    memory_error: true,
                    data_response_ready: true,
                    ..requests(true, false, false)
                },
                CpuV3MemoryArbiterOutputValue {
                    instruction_request_ready: true,
                    memory_request_valid: true,
                    memory_address: 0x111,
                    ..quiescent(0xbeef)
                },
            ),
            // The instruction request is accepted; its response is routed
            // back with `memory_response_ready` forwarded to the memory.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_request_ready: true,
                    memory_response_valid: true,
                    instruction_response_ready: true,
                    ..requests(true, false, false)
                },
                CpuV3MemoryArbiterOutputValue {
                    instruction_response_valid: true,
                    memory_address: 0x111,
                    memory_response_ready: true,
                    ..quiescent(0xbeef)
                },
            ),
            // Consuming the instruction response returns the port to idle.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_response_valid: true,
                    instruction_response_ready: true,
                    memory_read_data: 0xbeef,
                    ..idle()
                },
                quiescent(0xbeef),
            ),
        ])
        .run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_reset_releases_the_owner_mid_transaction() {
        ModuleTest::<CpuV3MemoryArbiter>::new(vec![
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    reset: true,
                    ..idle()
                },
                quiescent(0),
            ),
            // The instruction request is accepted at this clock edge.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_request_ready: true,
                    ..requests(true, false, false)
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_address: 0x111,
                    ..quiescent(0xbeef)
                },
            ),
            // Reset mid-transaction drops the owner; the still-raised request
            // is forwarded again without being accepted.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    reset: true,
                    ..requests(true, false, false)
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_request_valid: true,
                    memory_address: 0x111,
                    ..quiescent(0xbeef)
                },
            ),
            // After reset the retried request is accepted normally.
            TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    memory_request_ready: true,
                    ..requests(true, false, false)
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_address: 0x111,
                    ..quiescent(0xbeef)
                },
            ),
        ])
        .run_emu_and_nand();
    }

    #[test]
    fn export_has_no_target_resource_claims() {
        assert!(VerilogProject::generate::<CpuV3MemoryArbiter>()
            .unwrap()
            .resource_claims
            .is_empty());
    }
}
