//! Machine-owned arbiter between CpuV3 instruction/data traffic, boot DMA,
//! and the Tang Nano 20K physical SDRAM line/word port.
//!
//! Cache clients speak line transactions: one aligned request transfers four
//! ordered 64-bit beats (beat n carries words 4*n through 4*n+3). The arbiter forwards one request to
//! the SDRAM adapter, holds ownership while the adapter streams the real
//! burst, and releases the owner on the accepted beat carrying
//! `memory_response_last` (or any error beat), so a waiting client can start
//! while the served cache privately drains its refill buffer. The DMA client
//! keeps single 16-bit word transactions.

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
    pub data_line: Wire,
    pub data_address: Wires<22>,
    pub data_write_data: Wires<64>,
    pub data_response_ready: Wire,

    pub dma_request_valid: Wire,
    pub dma_write: Wire,
    pub dma_address: Wires<22>,
    pub dma_write_data: Wires<16>,
    pub dma_response_ready: Wire,

    pub memory_request_ready: Wire,
    pub memory_response_valid: Wire,
    pub memory_read_data: Wires<64>,
    pub memory_response_last: Wire,
    pub memory_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3MemoryArbiterOutput {
    pub instruction_request_ready: Wire,
    pub instruction_response_valid: Wire,
    pub instruction_read_data: Wires<64>,
    pub instruction_error: Wire,

    pub data_request_ready: Wire,
    pub data_response_valid: Wire,
    pub data_read_data: Wires<64>,
    pub data_error: Wire,

    pub dma_request_ready: Wire,
    pub dma_response_valid: Wire,
    pub dma_read_data: Wires<16>,
    pub dma_error: Wire,

    pub memory_request_valid: Wire,
    pub memory_write: Wire,
    pub memory_line: Wire,
    pub memory_address: Wires<22>,
    pub memory_write_data: Wires<64>,
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
        let requesting = state.owner == Owner::None && selected != Owner::None;
        let accepted = requesting && input.memory_request_ready;
        let selected_line =
            selected == Owner::Instruction || (selected == Owner::Data && input.data_line);
        let instruction_responding =
            state.owner == Owner::Instruction && input.memory_response_valid;
        let data_responding = state.owner == Owner::Data && input.memory_response_valid;
        let dma_responding = state.owner == Owner::Dma && input.memory_response_valid;
        output.drive(
            circuit,
            &CpuV3MemoryArbiterOutputValue {
                instruction_request_ready: accepted && selected == Owner::Instruction,
                instruction_response_valid: instruction_responding,
                instruction_read_data: if instruction_responding {
                    input.memory_read_data
                } else {
                    0
                },
                instruction_error: instruction_responding && input.memory_error,
                data_request_ready: accepted && selected == Owner::Data,
                data_response_valid: data_responding,
                data_read_data: if data_responding {
                    input.memory_read_data
                } else {
                    0
                },
                data_error: data_responding && input.memory_error,
                dma_request_ready: accepted && selected == Owner::Dma,
                dma_response_valid: dma_responding,
                dma_read_data: if dma_responding {
                    input.memory_read_data & 0xffff
                } else {
                    0
                },
                dma_error: dma_responding && input.memory_error,
                memory_request_valid: requesting,
                memory_write: requesting
                    && match selected {
                        Owner::Instruction | Owner::None => false,
                        Owner::Data => input.data_write,
                        Owner::Dma => input.dma_write,
                    },
                memory_line: requesting && selected_line,
                memory_address: if requesting {
                    match selected {
                        Owner::Instruction => input.instruction_address,
                        Owner::Data => input.data_address,
                        Owner::Dma => input.dma_address,
                        Owner::None => 0,
                    }
                } else {
                    0
                },
                memory_write_data: if requesting {
                    match selected {
                        Owner::Data => input.data_write_data,
                        Owner::Dma => input.dma_write_data,
                        Owner::Instruction | Owner::None => 0,
                    }
                } else if state.owner == Owner::Data && input.data_line {
                    input.data_write_data
                } else {
                    0
                },
                memory_response_ready: response_ready(state.owner, &input),
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
        } else if input.memory_response_valid
            && response_ready(state.owner, &input)
            && (input.memory_response_last || input.memory_error)
        {
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

        let memory_response_ready = (owner_instruction & input.instruction_response_ready)
            | (owner_data & input.data_response_ready)
            | (owner_dma & input.dma_response_ready);

        // Synchronous reset; otherwise capture the accepted owner and release
        // it once the last (or an error) beat is consumed.
        let release = !owner_none
            & input.memory_response_valid
            & memory_response_ready
            & (input.memory_response_last | input.memory_error);
        let next_owner = mux2_w(owner.out, selected, accepted);
        let next_owner = mux2_w(next_owner, const_wires(OWNER_NONE), release);
        owner.set_in(mux2_w(next_owner, const_wires(OWNER_NONE), input.reset));

        let instruction_responding = owner_instruction & input.memory_response_valid;
        let data_responding = owner_data & input.memory_response_valid;
        let dma_responding = owner_dma & input.memory_response_valid;
        let selected_line = selected_instruction | (selected_data & input.data_line);
        let read_data_lo = Wires {
            wires: std::array::from_fn(|bit| input.memory_read_data.wires[bit]),
        };
        let dma_write_data = Wires {
            wires: std::array::from_fn(|bit| {
                if bit < 16 {
                    input.dma_write_data.wires[bit]
                } else {
                    zero
                }
            }),
        };

        CpuV3MemoryArbiterOutput {
            instruction_request_ready: accepted & selected_instruction,
            instruction_response_valid: instruction_responding,
            instruction_read_data: mux2_w(
                const_wires(0),
                input.memory_read_data,
                instruction_responding,
            ),
            instruction_error: instruction_responding & input.memory_error,
            data_request_ready: accepted & selected_data,
            data_response_valid: data_responding,
            data_read_data: mux2_w(const_wires(0), input.memory_read_data, data_responding),
            data_error: data_responding & input.memory_error,
            dma_request_ready: accepted & selected_dma,
            dma_response_valid: dma_responding,
            dma_read_data: mux2_w(const_wires(0), read_data_lo, dma_responding),
            dma_error: dma_responding & input.memory_error,
            memory_request_valid: requesting,
            memory_write: requesting
                & mux2(
                    mux2(zero, input.data_write, selected_data),
                    input.dma_write,
                    selected_dma,
                ),
            memory_line: requesting & selected_line,
            memory_address: mux2_w(
                const_wires(0),
                mux4(
                    [
                        const_wires(0),
                        input.instruction_address,
                        input.data_address,
                        input.dma_address,
                    ],
                    selected,
                ),
                requesting,
            ),
            memory_write_data: mux2_w(
                const_wires(0),
                input.data_write_data,
                owner_data & input.data_line,
            ) | mux2_w(
                const_wires(0),
                mux2_w(
                    mux2_w(const_wires(0), input.data_write_data, selected_data),
                    dma_write_data,
                    selected_dma,
                ),
                requesting,
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

    type Step = TestStep<CpuV3MemoryArbiterInputValue, CpuV3MemoryArbiterOutputValue>;

    fn idle() -> CpuV3MemoryArbiterInputValue {
        CpuV3MemoryArbiterInputValue {
            reset: false,
            instruction_request_valid: false,
            instruction_address: 0,
            instruction_response_ready: false,
            data_request_valid: false,
            data_write: false,
            data_line: false,
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
            memory_response_last: false,
            memory_error: false,
        }
    }

    fn z() -> CpuV3MemoryArbiterOutputValue {
        CpuV3MemoryArbiterOutputValue {
            instruction_request_ready: false,
            instruction_response_valid: false,
            instruction_read_data: 0,
            instruction_error: false,
            data_request_ready: false,
            data_response_valid: false,
            data_read_data: 0,
            data_error: false,
            dma_request_ready: false,
            dma_response_valid: false,
            dma_read_data: 0,
            dma_error: false,
            memory_request_valid: false,
            memory_write: false,
            memory_line: false,
            memory_address: 0,
            memory_write_data: 0,
            memory_response_ready: false,
        }
    }

    fn reset_step() -> Step {
        TestStep::new(
            CpuV3MemoryArbiterInputValue {
                reset: true,
                ..idle()
            },
            z(),
        )
    }

    fn beat_data(n: u64) -> u64 {
        ((0x2003 + 4 * n) << 48)
            | ((0x2002 + 4 * n) << 32)
            | ((0x2001 + 4 * n) << 16)
            | (0x2000 + 4 * n)
    }

    /// Forward one instruction beat through to the client.
    fn instruction_beat(steps: &mut Vec<Step>, n: u64, last: bool) {
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: beat_data(n),
                memory_response_last: last,
                instruction_response_ready: true,
                ..idle()
            },
            if last {
                // The release edge returns the arbiter to idle.
                z()
            } else {
                CpuV3MemoryArbiterOutputValue {
                    instruction_response_valid: true,
                    instruction_read_data: beat_data(n),
                    memory_response_ready: true,
                    ..z()
                }
            },
        ));
    }

    /// Present a request, accept it, and stream a whole instruction line.
    fn instruction_line_steps(steps: &mut Vec<Step>, base: u64) {
        // The request is forwarded combinationally while the port is busy.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: base,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_line: true,
                memory_address: base,
                ..z()
            },
        ));
        // The port accepts it; the arbiter captures the owner.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: base,
                memory_request_ready: true,
                ..idle()
            },
            z(),
        ));
        for n in 0..3 {
            instruction_beat(steps, n, false);
        }
        // The last beat is first presented without the client ready, proving
        // the response holds, then consumed, releasing the owner.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: beat_data(3),
                memory_response_last: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                instruction_response_valid: true,
                instruction_read_data: beat_data(3),
                ..z()
            },
        ));
        instruction_beat(steps, 3, true);
    }

    #[test]
    fn emu_and_nand_stream_one_line_per_instruction_request() {
        let mut steps = vec![reset_step()];
        instruction_line_steps(&mut steps, 0x120);
        // A follow-up request is forwarded as soon as the owner is released.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x2a0,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_line: true,
                memory_address: 0x2a0,
                ..z()
            },
        ));
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_forward_data_and_dma_word_writes() {
        let mut steps = vec![reset_step()];
        // Data single-word transaction.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                data_request_valid: true,
                data_write: true,
                data_address: 0x222,
                data_write_data: 0xdddd,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_write: true,
                memory_address: 0x222,
                memory_write_data: 0xdddd,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                data_request_valid: true,
                data_write: true,
                data_address: 0x222,
                data_write_data: 0xdddd,
                memory_request_ready: true,
                ..idle()
            },
            z(),
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_response_last: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                data_response_valid: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_response_last: true,
                data_response_ready: true,
                ..idle()
            },
            z(),
        ));
        // DMA word transaction.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                dma_request_valid: true,
                dma_write: true,
                dma_address: 0x333,
                dma_write_data: 0xaaaa,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_write: true,
                memory_address: 0x333,
                memory_write_data: 0xaaaa,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                dma_request_valid: true,
                dma_write: true,
                dma_address: 0x333,
                dma_write_data: 0xaaaa,
                memory_request_ready: true,
                ..idle()
            },
            z(),
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_response_last: true,
                memory_read_data: 0xbeef,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                dma_response_valid: true,
                dma_read_data: 0xbeef,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_response_last: true,
                memory_read_data: 0xbeef,
                dma_response_ready: true,
                ..idle()
            },
            z(),
        ));
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_grant_dma_then_data_then_instruction() {
        let mut steps = vec![reset_step()];
        let contending = || CpuV3MemoryArbiterInputValue {
            instruction_request_valid: true,
            instruction_address: 0x110,
            data_request_valid: true,
            data_write: true,
            data_address: 0x222,
            data_write_data: 0xdddd,
            dma_request_valid: true,
            dma_write: true,
            dma_address: 0x333,
            dma_write_data: 0xaaaa,
            ..idle()
        };
        // All three clients request together; DMA wins the forwarded request.
        steps.push(TestStep::new(
            contending(),
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_write: true,
                memory_address: 0x333,
                memory_write_data: 0xaaaa,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_request_ready: true,
                ..contending()
            },
            z(),
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_response_last: true,
                ..contending()
            },
            CpuV3MemoryArbiterOutputValue {
                dma_response_valid: true,
                ..z()
            },
        ));
        // Consuming the DMA response releases the port; the waiting data
        // client is forwarded combinationally.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_response_last: true,
                dma_response_ready: true,
                dma_request_valid: false,
                ..contending()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_write: true,
                memory_address: 0x222,
                memory_write_data: 0xdddd,
                ..z()
            },
        ));
        // The data request is accepted at this edge.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                dma_request_valid: false,
                memory_request_ready: true,
                ..contending()
            },
            z(),
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                dma_request_valid: false,
                memory_response_valid: true,
                memory_response_last: true,
                ..contending()
            },
            CpuV3MemoryArbiterOutputValue {
                data_response_valid: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                dma_request_valid: false,
                data_request_valid: false,
                memory_response_valid: true,
                memory_response_last: true,
                data_response_ready: true,
                ..contending()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_line: true,
                memory_address: 0x110,
                ..z()
            },
        ));
        // The instruction line request is accepted and two beats stream.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x110,
                memory_request_ready: true,
                ..idle()
            },
            z(),
        ));
        instruction_beat(&mut steps, 0, false);
        instruction_beat(&mut steps, 1, false);
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_release_on_an_error_beat_without_last() {
        let mut steps = vec![reset_step()];
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x120,
                memory_request_ready: true,
                ..idle()
            },
            z(),
        ));
        instruction_beat(&mut steps, 0, false);
        // An error beat is presented with its error flag.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: beat_data(1),
                memory_error: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                instruction_response_valid: true,
                instruction_read_data: beat_data(1),
                instruction_error: true,
                ..z()
            },
        ));
        // Consuming the error beat releases the owner even without last.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: beat_data(1),
                memory_error: true,
                instruction_response_ready: true,
                ..idle()
            },
            z(),
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x2a0,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_line: true,
                memory_address: 0x2a0,
                ..z()
            },
        ));
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_reset_releases_the_owner_mid_transaction() {
        let mut steps = vec![reset_step()];
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x120,
                memory_request_ready: true,
                ..idle()
            },
            z(),
        ));
        instruction_beat(&mut steps, 0, false);
        steps.push(reset_step());
        steps.push(TestStep::new(idle(), z()));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x2a0,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_line: true,
                memory_address: 0x2a0,
                ..z()
            },
        ));
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn export_has_no_target_resource_claims() {
        assert!(VerilogProject::generate::<CpuV3MemoryArbiter>()
            .unwrap()
            .resource_claims
            .is_empty());
    }
}
