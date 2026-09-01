//! Machine-owned arbiter between CpuV3 instruction/data traffic, boot DMA,
//! and the Tang Nano 20K physical SDRAM word port.
//!
//! Cache clients speak line transactions: one aligned line read request is
//! answered by exactly eight ordered 32-bit beats (beat n carries word 2*n in
//! its low half and word 2*n+1 in its high half), while a data-cache store is
//! one write-through word transaction. The arbiter owns the downstream word
//! port for the whole line and releases it once the final beat is accepted,
//! so a waiting client can start while the served cache privately drains its
//! refill buffer. An error beat terminates a line response early. The DMA
//! client keeps single 16-bit word transactions.

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
    pub instruction_read_data: Wires<32>,
    pub instruction_error: Wire,

    pub data_request_ready: Wire,
    pub data_response_valid: Wire,
    pub data_read_data: Wires<32>,
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

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Phase {
    /// Issuing one downstream word request for the pending beat.
    #[default]
    Request,
    /// Awaiting the downstream word response.
    Respond,
    /// Presenting one assembled beat (or the write/error completion) to the
    /// owning client.
    Present,
}

const LINE_BEATS: u8 = 8;

#[derive(Default)]
pub struct CpuV3MemoryArbiterState {
    owner: Owner,
    pending_write: bool,
    pending_address: u32,
    pending_write_data: u16,
    phase: Phase,
    beat: u8,
    half: bool,
    beat_lo: u16,
    beat_hi: u16,
    present_error: bool,
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
        let active = state.owner != Owner::None;
        let present = active && state.phase == Phase::Present;
        let present_data = present && !state.pending_write && !state.present_error;
        let beat_data = (u64::from(state.beat_hi) << 16) | u64::from(state.beat_lo);
        output.drive(
            circuit,
            &CpuV3MemoryArbiterOutputValue {
                instruction_request_ready: state.owner == Owner::None
                    && selected == Owner::Instruction,
                instruction_response_valid: present && state.owner == Owner::Instruction,
                instruction_read_data: if present_data && state.owner == Owner::Instruction {
                    beat_data
                } else {
                    0
                },
                instruction_error: present
                    && state.owner == Owner::Instruction
                    && state.present_error,
                data_request_ready: state.owner == Owner::None && selected == Owner::Data,
                data_response_valid: present && state.owner == Owner::Data,
                data_read_data: if present_data && state.owner == Owner::Data {
                    beat_data
                } else {
                    0
                },
                data_error: present && state.owner == Owner::Data && state.present_error,
                dma_request_ready: state.owner == Owner::None && selected == Owner::Dma,
                dma_response_valid: present && state.owner == Owner::Dma,
                dma_read_data: if present_data && state.owner == Owner::Dma {
                    u64::from(state.beat_lo)
                } else {
                    0
                },
                dma_error: present && state.owner == Owner::Dma && state.present_error,
                memory_request_valid: active && state.phase == Phase::Request,
                memory_write: active
                    && state.phase == Phase::Request
                    && state.pending_write,
                memory_address: u64::from(
                    if !active || state.phase != Phase::Request {
                        0
                    } else if state.pending_write {
                        state.pending_address
                    } else {
                        (state.pending_address & !0xf)
                            | (u32::from(state.beat) << 1)
                            | u32::from(state.half)
                    },
                ),
                memory_write_data: if active && state.phase == Phase::Request {
                    u64::from(state.pending_write_data)
                } else {
                    0
                },
                memory_response_ready: active && state.phase == Phase::Respond,
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
            if selected != Owner::None {
                state.owner = selected;
                state.pending_write = match selected {
                    Owner::Data => input.data_write,
                    Owner::Dma => input.dma_write,
                    _ => false,
                };
                state.pending_address = match selected {
                    Owner::Instruction => input.instruction_address as u32,
                    Owner::Data => input.data_address as u32,
                    Owner::Dma => input.dma_address as u32,
                    Owner::None => 0,
                };
                state.pending_write_data = match selected {
                    Owner::Data => input.data_write_data as u16,
                    Owner::Dma => input.dma_write_data as u16,
                    _ => 0,
                };
                state.phase = Phase::Request;
                state.beat = 0;
                state.half = false;
                state.present_error = false;
            }
            return;
        }
        match state.phase {
            Phase::Request => {
                if input.memory_request_ready {
                    state.phase = Phase::Respond;
                }
            }
            Phase::Respond => {
                if input.memory_response_valid {
                    if input.memory_error {
                        state.present_error = true;
                        state.phase = Phase::Present;
                    } else if state.pending_write {
                        state.phase = Phase::Present;
                    } else if !state.half {
                        state.beat_lo = input.memory_read_data as u16;
                        state.half = true;
                        state.phase = Phase::Request;
                    } else {
                        state.beat_hi = input.memory_read_data as u16;
                        state.phase = Phase::Present;
                    }
                }
            }
            Phase::Present => {
                if response_ready(state.owner, &input) {
                    if state.present_error
                        || state.pending_write
                        || state.beat + 1 == LINE_BEATS
                    {
                        state.owner = Owner::None;
                    } else {
                        state.beat += 1;
                        state.half = false;
                        state.phase = Phase::Request;
                    }
                }
            }
        }
    }

    fn nand(input: &Self::Input) -> Self::Output {
        let zero = input_const(0);
        let owner = reg_w::<2>();
        let pending_write = reg_w::<1>();
        let pending_address = reg_w::<22>();
        let pending_write_data = reg_w::<16>();
        let phase = reg_w::<2>();
        let beat = reg_w::<3>();
        let half = reg_w::<1>();
        let beat_lo = reg_w::<16>();
        let beat_hi = reg_w::<16>();
        let present_error = reg_w::<1>();

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
        let active = !owner_none;
        let selected_any = selected.wires[0] | selected.wires[1];
        let accept = owner_none & selected_any;
        let selected_instruction = selected.eq_const(OWNER_INSTRUCTION);
        let selected_data = selected.eq_const(OWNER_DATA);
        let selected_dma = selected.eq_const(OWNER_DMA);
        let owner_instruction = owner.out.eq_const(OWNER_INSTRUCTION);
        let owner_data = owner.out.eq_const(OWNER_DATA);
        let owner_dma = owner.out.eq_const(OWNER_DMA);

        let phase_request = phase.out.eq_const(PHASE_REQUEST);
        let phase_respond = phase.out.eq_const(PHASE_RESPOND);
        let phase_present = phase.out.eq_const(PHASE_PRESENT);
        let write = pending_write.out.wires[0];
        let half_bit = half.out.wires[0];
        let beat_last = beat.out.wires[0] & beat.out.wires[1] & beat.out.wires[2];

        let client_ready = (owner_instruction & input.instruction_response_ready)
            | (owner_data & input.data_response_ready)
            | (owner_dma & input.dma_response_ready);
        let request_done = active & phase_request & input.memory_request_ready;
        let respond_done = active & phase_respond & input.memory_response_valid;
        let respond_present = input.memory_error | write | half_bit;
        let present_done = active & phase_present & client_ready;
        let transaction_done = present_done & (present_error.out.wires[0] | write | beat_last);

        // Owner: capture on accept, release once the final beat (or the
        // write/error completion) is consumed.
        let next_owner = mux2_w(owner.out, selected, accept);
        let next_owner = mux2_w(next_owner, const_wires(OWNER_NONE), transaction_done);
        owner.set_in(mux2_w(next_owner, const_wires(OWNER_NONE), input.reset));

        let capture_low = respond_done & !input.memory_error & !write & !half_bit;
        let capture_high = respond_done & !input.memory_error & !write & half_bit;

        phase.set_in({
            let next = mux2_w(phase.out, const_wires(PHASE_RESPOND), request_done);
            let next = mux2_w(
                next,
                mux2_w(const_wires(PHASE_REQUEST), const_wires(PHASE_PRESENT), respond_present),
                respond_done,
            );
            let next = mux2_w(next, const_wires(PHASE_REQUEST), present_done);
            mux2_w(next, const_wires(PHASE_REQUEST), accept)
        });

        beat.set_in({
            let b = beat.out.wires;
            let carry = b[0] & b[1];
            let incremented = Wires {
                wires: [
                    !b[0],
                    (b[1] | b[0]) & !(b[1] & b[0]),
                    (b[2] | carry) & !(b[2] & carry),
                ],
            };
            let next = mux2_w(beat.out, incremented, present_done & !transaction_done);
            mux2_w(next, const_wires(0), accept)
        });

        half.set_in({
            let next = mux2_w(half.out, const_wires(1), capture_low);
            mux2_w(next, const_wires(0), present_done | accept)
        });

        beat_lo.set_in(mux2_w(beat_lo.out, input.memory_read_data, capture_low));
        beat_hi.set_in(mux2_w(beat_hi.out, input.memory_read_data, capture_high));
        present_error.set_in({
            let next = mux2_w(present_error.out, const_wires(1), respond_done & input.memory_error);
            mux2_w(next, const_wires(0), accept)
        });

        pending_write.set_in(mux2_w(
            pending_write.out,
            Wires {
                wires: [mux2(
                    mux2(zero, input.data_write, selected_data),
                    input.dma_write,
                    selected_dma,
                )],
            },
            accept,
        ));
        pending_address.set_in(mux2_w(
            pending_address.out,
            mux4(
                [
                    const_wires(0),
                    input.instruction_address,
                    input.data_address,
                    input.dma_address,
                ],
                selected,
            ),
            accept,
        ));
        pending_write_data.set_in(mux2_w(
            pending_write_data.out,
            mux4(
                [
                    const_wires(0),
                    const_wires(0),
                    input.data_write_data,
                    input.dma_write_data,
                ],
                selected,
            ),
            accept,
        ));

        let line_address = Wires {
            wires: std::array::from_fn(|bit| match bit {
                0 => half_bit,
                1..=3 => beat.out.wires[bit - 1],
                _ => pending_address.out.wires[bit],
            }),
        };
        let read_data32 = Wires {
            wires: std::array::from_fn(|bit| {
                if bit < 16 {
                    beat_lo.out.wires[bit]
                } else {
                    beat_hi.out.wires[bit - 16]
                }
            }),
        };

        let instruction_present = active & phase_present & owner_instruction;
        let data_present = active & phase_present & owner_data;
        let dma_present = active & phase_present & owner_dma;
        let present_data = phase_present & !write & !present_error.out.wires[0];
        let request_phase = active & phase_request;

        CpuV3MemoryArbiterOutput {
            instruction_request_ready: accept & selected_instruction,
            instruction_response_valid: instruction_present,
            instruction_read_data: mux2_w(
                const_wires(0),
                read_data32,
                instruction_present & present_data,
            ),
            instruction_error: instruction_present & present_error.out.wires[0],
            data_request_ready: accept & selected_data,
            data_response_valid: data_present,
            data_read_data: mux2_w(const_wires(0), read_data32, data_present & present_data),
            data_error: data_present & present_error.out.wires[0],
            dma_request_ready: accept & selected_dma,
            dma_response_valid: dma_present,
            dma_read_data: mux2_w(const_wires(0), beat_lo.out, dma_present & present_data),
            dma_error: dma_present & present_error.out.wires[0],
            memory_request_valid: request_phase,
            memory_write: request_phase & write,
            memory_address: mux2_w(
                const_wires(0),
                mux2_w(line_address, pending_address.out, write),
                request_phase,
            ),
            memory_write_data: mux2_w(const_wires(0), pending_write_data.out, request_phase),
            memory_response_ready: active & phase_respond,
        }
    }
}

const OWNER_NONE: u8 = 0;
const OWNER_INSTRUCTION: u8 = 1;
const OWNER_DATA: u8 = 2;
const OWNER_DMA: u8 = 3;

const PHASE_REQUEST: u8 = 0;
const PHASE_RESPOND: u8 = 1;
const PHASE_PRESENT: u8 = 2;

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

    fn beat_words(n: u64) -> (u64, u64) {
        (0x2000 + 2 * n, 0x2001 + 2 * n)
    }

    fn beat_data(n: u64) -> u64 {
        let (lo, hi) = beat_words(n);
        hi << 16 | lo
    }

    /// Present and complete beat `n` of the in-flight instruction line read
    /// against an always-ready memory. The final beat releases the arbiter.
    fn instruction_beat_steps(steps: &mut Vec<Step>, base: u64, n: u64) {
        let (lo, hi) = beat_words(n);
        // Low word accepted; the arbiter waits for its response.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        // Low word captured; the high word request is presented.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: lo,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: base + 2 * n + 1,
                ..z()
            },
        ));
        // High word accepted.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        // High word captured; the assembled 32-bit beat is presented.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: hi,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                instruction_response_valid: true,
                instruction_read_data: beat_data(n),
                ..z()
            },
        ));
        // The client consumes the beat; the next word request (or release)
        // follows.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_response_ready: true,
                ..idle()
            },
            if n == 7 {
                z()
            } else {
                CpuV3MemoryArbiterOutputValue {
                    memory_request_valid: true,
                    memory_address: base + 2 * (n + 1),
                    ..z()
                }
            },
        ));
    }

    /// Accept one instruction line request and stream the whole line.
    fn instruction_line_steps(steps: &mut Vec<Step>, base: u64) {
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: base,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: base,
                ..z()
            },
        ));
        for n in 0..8 {
            instruction_beat_steps(steps, base, n);
        }
    }

    #[test]
    fn emu_and_nand_stream_one_line_per_instruction_request() {
        let mut steps = vec![reset_step()];
        instruction_line_steps(&mut steps, 0x120);
        // A follow-up request is arbitrated normally after the release.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x2a0,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: 0x2a0,
                ..z()
            },
        ));
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_forward_data_and_dma_word_writes() {
        let mut steps = vec![reset_step()];
        // Data write-through word transaction.
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
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                data_response_valid: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
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
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                dma_response_valid: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
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
        // All three clients request together; DMA wins.
        let contending = || CpuV3MemoryArbiterInputValue {
            instruction_request_valid: true,
            instruction_address: 0x111,
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
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                ..contending()
            },
            CpuV3MemoryArbiterOutputValue {
                dma_response_valid: true,
                ..z()
            },
        ));
        // Consuming the DMA response releases the port; the completed DMA
        // request drops and the waiting data client is granted
        // combinationally.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                dma_request_valid: false,
                dma_response_ready: true,
                ..contending()
            },
            CpuV3MemoryArbiterOutputValue {
                data_request_ready: true,
                ..z()
            },
        ));
        // The data request is captured at this edge.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
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
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                dma_request_valid: false,
                memory_request_ready: true,
                ..contending()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                dma_request_valid: false,
                memory_response_valid: true,
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
                data_response_ready: true,
                ..contending()
            },
            CpuV3MemoryArbiterOutputValue {
                instruction_request_ready: true,
                ..z()
            },
        ));
        // The instruction line request is captured and streamed.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x110,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: 0x110,
                ..z()
            },
        ));
        for n in 0..8 {
            instruction_beat_steps(&mut steps, 0x110, n);
        }
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_hold_requests_and_beats_under_backpressure() {
        let mut steps = vec![reset_step()];
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x120,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: 0x120,
                ..z()
            },
        ));
        // The downstream port stalls the first word request; the request
        // stays asserted with a stable address.
        steps.push(
            TestStep::new(
                idle(),
                CpuV3MemoryArbiterOutputValue {
                    memory_request_valid: true,
                    memory_address: 0x120,
                    ..z()
                },
            )
            .after_cycles(2),
        );
        // Beat zero completes.
        let (lo, hi) = beat_words(0);
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: lo,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: 0x121,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: hi,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                instruction_response_valid: true,
                instruction_read_data: beat_data(0),
                ..z()
            },
        ));
        // The client stalls the presented beat; the data stays stable.
        steps.push(
            TestStep::new(
                idle(),
                CpuV3MemoryArbiterOutputValue {
                    instruction_response_valid: true,
                    instruction_read_data: beat_data(0),
                    ..z()
                },
            )
            .after_cycles(2),
        );
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_response_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: 0x122,
                ..z()
            },
        ));
        for n in 1..8 {
            instruction_beat_steps(&mut steps, 0x120, n);
        }
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_terminate_the_line_on_a_memory_error() {
        let mut steps = vec![reset_step()];
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x120,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: 0x120,
                ..z()
            },
        ));
        instruction_beat_steps(&mut steps, 0x120, 0);
        // Beat one's high word response arrives with an error: the arbiter
        // presents one error beat and terminates the line early.
        let (lo, _) = beat_words(1);
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: lo,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: 0x123,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_error: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                instruction_response_valid: true,
                instruction_error: true,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_response_ready: true,
                ..idle()
            },
            z(),
        ));
        // The arbiter recovers and serves the next request.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x2a0,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
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
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_address: 0x120,
                ..z()
            },
        ));
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_response_ready: true,
                ..z()
            },
        ));
        // Reset mid-transaction drops the owner and all downstream activity.
        steps.push(reset_step());
        steps.push(TestStep::new(idle(), z()));
        // A fresh request is served normally afterwards.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x2a0,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
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
