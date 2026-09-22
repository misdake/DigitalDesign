//! CpuV3Sim-owned arbiter between the display scanout, the CpuV3 instruction
//! and data caches, boot DMA, the three GPU masters, and the Tang Nano 20K
//! physical SDRAM line/word port.
//!
//! Cache and GPU clients speak fixed one-line transactions: one aligned
//! request transfers four ordered 64-bit beats (beat n carries words 4*n
//! through 4*n+3). Display is also a fixed-line read master. The arbiter
//! forwards one request to the SDRAM adapter, holds ownership while the
//! adapter streams the real burst, and releases the owner on the accepted beat
//! carrying `memory_response_last` (or any error beat). The DMA client keeps
//! single 16-bit word transactions.
//!
//! Display has strict priority at every transaction boundary. The other six
//! requesters score `base + saturating_age[3:0]` with bases DMA 5, I/D 4,
//! `gpu_ro` 3, `gpu_fb_r` 2, `gpu_fb_w` 1; equal scores rotate through a
//! round-robin pointer so no requester can be starved.

use digital_design_circuit::{
    add_naive, input_const, mux2_w, mux8_w, reg_w, CircuitWires, Regs, Wire, Wires,
};
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

    pub display_request_valid: Wire,
    pub display_address: Wires<22>,
    pub display_response_ready: Wire,

    pub gpu_ro_request_valid: Wire,
    pub gpu_ro_write: Wire,
    pub gpu_ro_address: Wires<22>,
    pub gpu_ro_write_data: Wires<64>,

    pub gpu_fb_r_request_valid: Wire,
    pub gpu_fb_r_write: Wire,
    pub gpu_fb_r_address: Wires<22>,
    pub gpu_fb_r_write_data: Wires<64>,

    pub gpu_fb_w_request_valid: Wire,
    pub gpu_fb_w_write: Wire,
    pub gpu_fb_w_address: Wires<22>,
    pub gpu_fb_w_write_data: Wires<64>,

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

    pub display_request_ready: Wire,
    pub display_response_valid: Wire,
    pub display_read_data: Wires<64>,
    pub display_response_last: Wire,
    pub display_error: Wire,

    pub gpu_ro_request_ready: Wire,
    pub gpu_ro_response_valid: Wire,
    pub gpu_ro_read_data: Wires<64>,
    pub gpu_ro_response_last: Wire,
    pub gpu_ro_error: Wire,

    pub gpu_fb_r_request_ready: Wire,
    pub gpu_fb_r_response_valid: Wire,
    pub gpu_fb_r_read_data: Wires<64>,
    pub gpu_fb_r_response_last: Wire,
    pub gpu_fb_r_error: Wire,

    pub gpu_fb_w_request_ready: Wire,
    pub gpu_fb_w_response_valid: Wire,
    pub gpu_fb_w_read_data: Wires<64>,
    pub gpu_fb_w_response_last: Wire,
    pub gpu_fb_w_error: Wire,

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Owner {
    #[default]
    None,
    Display,
    Instruction,
    Data,
    Dma,
    GpuRo,
    GpuFbR,
    GpuFbW,
}

/// Non-display requesters in rotation order. Index `i` maps to the same slot
/// in [`REQUESTER_BASES`] and [`OWNER_CODES`].
const REQUESTER_OWNERS: [Owner; 6] = [
    Owner::Instruction,
    Owner::Data,
    Owner::Dma,
    Owner::GpuRo,
    Owner::GpuFbR,
    Owner::GpuFbW,
];
const REQUESTER_BASES: [u8; 6] = [4, 4, 5, 3, 2, 1];
const OWNER_CODES: [u8; 6] = [
    OWNER_INSTRUCTION,
    OWNER_DATA,
    OWNER_DMA,
    OWNER_GPU_RO,
    OWNER_GPU_FB_R,
    OWNER_GPU_FB_W,
];

const OWNER_NONE: u8 = 0;
const OWNER_DISPLAY: u8 = 1;
const OWNER_INSTRUCTION: u8 = 2;
const OWNER_DATA: u8 = 3;
const OWNER_DMA: u8 = 4;
const OWNER_GPU_RO: u8 = 5;
const OWNER_GPU_FB_R: u8 = 6;
const OWNER_GPU_FB_W: u8 = 7;

#[derive(Clone, Default)]
pub struct CpuV3MemoryArbiterState {
    owner: Owner,
    /// Saturating wait age, one counter per non-display requester.
    age: [u8; 6],
    /// Round-robin tie-breaker cursor over the six non-display requesters.
    rotate: u8,
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
        output.drive(circuit, &compute_output(state, &input));
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        advance_state(state, &input);
    }

    fn nand(input: &Self::Input) -> Self::Output {
        let zero = input_const(0);
        let owner = reg_w::<3>();
        let age_regs: [Regs<4>; 6] = std::array::from_fn(|_| reg_w::<4>());
        let rotate = reg_w::<3>();

        let requests: [Wire; 6] = [
            input.instruction_request_valid,
            input.data_request_valid,
            input.dma_request_valid,
            input.gpu_ro_request_valid,
            input.gpu_fb_r_request_valid,
            input.gpu_fb_w_request_valid,
        ];
        let ages: [Wires<4>; 6] = std::array::from_fn(|index| age_regs[index].out);
        let score: [Wires<5>; 6] = std::array::from_fn(|index| {
            add_naive(
                ages[index].expand_unsigned::<5>(),
                const_wires::<5>(REQUESTER_BASES[index]),
            )
            .sum
        });

        // A requester wins when it beats every other requester that is also
        // asking. Non-requesting masters are ignored.
        let mut wins = [zero; 6];
        for index in 0..6 {
            let mut win = requests[index];
            for other in 0..6 {
                if index == other {
                    continue;
                }
                let (greater, equal) = cmp(score[index], score[other]);
                let better = greater | (equal & earlier(index, other, rotate.out));
                win = win & (!requests[other] | better);
            }
            wins[index] = win;
        }

        let mut selected_bits = [zero; 3];
        for index in 0..6 {
            let code = OWNER_CODES[index];
            for (bit, selected_bit) in selected_bits.iter_mut().enumerate() {
                if (code >> bit) & 1 == 1 {
                    *selected_bit = *selected_bit | wins[index];
                }
            }
        }
        let selected = mux2_w(
            Wires {
                wires: selected_bits,
            },
            const_wires::<3>(OWNER_DISPLAY),
            input.display_request_valid,
        );
        let non_display_any = wins.iter().fold(zero, |acc, &win| acc | win);
        let selected_any = input.display_request_valid | non_display_any;
        let owner_none = eq_const(owner.out, OWNER_NONE);
        let requesting = owner_none & selected_any;
        let accepted = requesting & input.memory_request_ready;

        let owner_display = eq_const(owner.out, OWNER_DISPLAY);
        let owner_instruction = eq_const(owner.out, OWNER_INSTRUCTION);
        let owner_data = eq_const(owner.out, OWNER_DATA);
        let owner_dma = eq_const(owner.out, OWNER_DMA);
        let owner_gpu_ro = eq_const(owner.out, OWNER_GPU_RO);
        let owner_gpu_fb_r = eq_const(owner.out, OWNER_GPU_FB_R);
        let owner_gpu_fb_w = eq_const(owner.out, OWNER_GPU_FB_W);
        let selected_display = eq_const(selected, OWNER_DISPLAY);
        let selected_instruction = eq_const(selected, OWNER_INSTRUCTION);
        let selected_data = eq_const(selected, OWNER_DATA);
        let selected_dma = eq_const(selected, OWNER_DMA);
        let selected_gpu_ro = eq_const(selected, OWNER_GPU_RO);
        let selected_gpu_fb_r = eq_const(selected, OWNER_GPU_FB_R);
        let selected_gpu_fb_w = eq_const(selected, OWNER_GPU_FB_W);

        let memory_response_ready = (owner_display & input.display_response_ready)
            | (owner_instruction & input.instruction_response_ready)
            | (owner_data & input.data_response_ready)
            | (owner_dma & input.dma_response_ready)
            | owner_gpu_ro
            | owner_gpu_fb_r
            | owner_gpu_fb_w;

        let release = !owner_none
            & input.memory_response_valid
            & memory_response_ready
            & (input.memory_response_last | input.memory_error);
        let next_owner = mux2_w(owner.out, selected, accepted);
        let next_owner = mux2_w(next_owner, const_wires::<3>(OWNER_NONE), release);
        owner.set_in(mux2_w(
            next_owner,
            const_wires::<3>(OWNER_NONE),
            input.reset,
        ));

        for index in 0..6 {
            let request = requests[index];
            let selected_this = eq_const(selected, OWNER_CODES[index]);
            let incremented = {
                let sum = add_naive(ages[index], const_wires::<4>(1)).sum;
                mux2_w(sum, const_wires::<4>(15), eq_const(ages[index], 15))
            };
            let after = mux2_w(ages[index], const_wires::<4>(0), !request);
            let after = mux2_w(after, incremented, request & !selected_this);
            let after = mux2_w(after, const_wires::<4>(0), selected_this);
            let next = mux2_w(after, ages[index], !owner_none);
            age_regs[index].set_in(mux2_w(next, const_wires::<4>(0), input.reset));
        }

        let non_display_accepted = accepted & !selected_display;
        let mut rotate_bits = [zero; 3];
        for (index, owner_code) in OWNER_CODES.iter().copied().enumerate() {
            let selected_this = eq_const(selected, owner_code);
            let next_rotate = ((index + 1) % 6) as u8;
            for (bit, rotate_bit) in rotate_bits.iter_mut().enumerate() {
                if (next_rotate >> bit) & 1 == 1 {
                    *rotate_bit = *rotate_bit | selected_this;
                }
            }
        }
        let next_rotate = mux2_w(
            rotate.out,
            Wires { wires: rotate_bits },
            non_display_accepted,
        );
        rotate.set_in(mux2_w(next_rotate, const_wires::<3>(0), input.reset));

        let selected_write = (selected_data & input.data_write)
            | (selected_dma & input.dma_write)
            | (selected_gpu_ro & input.gpu_ro_write)
            | (selected_gpu_fb_r & input.gpu_fb_r_write)
            | (selected_gpu_fb_w & input.gpu_fb_w_write);
        let selected_line = selected_instruction
            | selected_display
            | (selected_data & input.data_line)
            | selected_gpu_ro
            | selected_gpu_fb_r
            | selected_gpu_fb_w;

        let selected_address = mux8_w(
            &[
                const_wires::<22>(0),
                input.display_address,
                input.instruction_address,
                input.data_address,
                input.dma_address,
                input.gpu_ro_address,
                input.gpu_fb_r_address,
                input.gpu_fb_w_address,
            ],
            selected,
        );
        let dma_write_data = input.dma_write_data.expand_unsigned::<64>();
        let selected_write_data = mux8_w(
            &[
                const_wires::<64>(0),
                const_wires::<64>(0),
                const_wires::<64>(0),
                input.data_write_data,
                dma_write_data,
                input.gpu_ro_write_data,
                input.gpu_fb_r_write_data,
                input.gpu_fb_w_write_data,
            ],
            selected,
        );
        let held_write_data = mux2_w(
            const_wires::<64>(0),
            input.data_write_data,
            owner_data & input.data_line,
        ) | mux2_w(
            const_wires::<64>(0),
            input.gpu_ro_write_data,
            owner_gpu_ro & input.gpu_ro_write,
        ) | mux2_w(
            const_wires::<64>(0),
            input.gpu_fb_r_write_data,
            owner_gpu_fb_r & input.gpu_fb_r_write,
        ) | mux2_w(
            const_wires::<64>(0),
            input.gpu_fb_w_write_data,
            owner_gpu_fb_w,
        );
        let memory_write_data = mux2_w(held_write_data, selected_write_data, requesting);

        let responding = input.memory_response_valid;
        let instruction_responding = owner_instruction & responding;
        let data_responding = owner_data & responding;
        let dma_responding = owner_dma & responding;
        let display_responding = owner_display & responding;
        let gpu_ro_responding = owner_gpu_ro & responding;
        let gpu_fb_r_responding = owner_gpu_fb_r & responding;
        let gpu_fb_w_responding = owner_gpu_fb_w & responding;
        let dma_read_data: Wires<16> = Wires {
            wires: std::array::from_fn(|bit| input.memory_read_data.wires[bit]),
        };

        CpuV3MemoryArbiterOutput {
            instruction_request_ready: accepted & selected_instruction,
            instruction_response_valid: instruction_responding,
            instruction_read_data: mux2_w(
                const_wires::<64>(0),
                input.memory_read_data,
                instruction_responding,
            ),
            instruction_error: instruction_responding & input.memory_error,
            data_request_ready: accepted & selected_data,
            data_response_valid: data_responding,
            data_read_data: mux2_w(
                const_wires::<64>(0),
                input.memory_read_data,
                data_responding,
            ),
            data_error: data_responding & input.memory_error,
            dma_request_ready: accepted & selected_dma,
            dma_response_valid: dma_responding,
            dma_read_data: mux2_w(const_wires::<16>(0), dma_read_data, dma_responding),
            dma_error: dma_responding & input.memory_error,
            display_request_ready: accepted & selected_display,
            display_response_valid: display_responding,
            display_read_data: mux2_w(
                const_wires::<64>(0),
                input.memory_read_data,
                display_responding,
            ),
            display_response_last: display_responding & input.memory_response_last,
            display_error: display_responding & input.memory_error,
            gpu_ro_request_ready: accepted & selected_gpu_ro,
            gpu_ro_response_valid: gpu_ro_responding,
            gpu_ro_read_data: mux2_w(
                const_wires::<64>(0),
                input.memory_read_data,
                gpu_ro_responding,
            ),
            gpu_ro_response_last: gpu_ro_responding & input.memory_response_last,
            gpu_ro_error: gpu_ro_responding & input.memory_error,
            gpu_fb_r_request_ready: accepted & selected_gpu_fb_r,
            gpu_fb_r_response_valid: gpu_fb_r_responding,
            gpu_fb_r_read_data: mux2_w(
                const_wires::<64>(0),
                input.memory_read_data,
                gpu_fb_r_responding,
            ),
            gpu_fb_r_response_last: gpu_fb_r_responding & input.memory_response_last,
            gpu_fb_r_error: gpu_fb_r_responding & input.memory_error,
            gpu_fb_w_request_ready: accepted & selected_gpu_fb_w,
            gpu_fb_w_response_valid: gpu_fb_w_responding,
            gpu_fb_w_read_data: mux2_w(
                const_wires::<64>(0),
                input.memory_read_data,
                gpu_fb_w_responding,
            ),
            gpu_fb_w_response_last: gpu_fb_w_responding & input.memory_response_last,
            gpu_fb_w_error: gpu_fb_w_responding & input.memory_error,
            memory_request_valid: requesting,
            memory_write: requesting & selected_write,
            memory_line: requesting & selected_line,
            memory_address: mux2_w(const_wires::<22>(0), selected_address, requesting),
            memory_write_data,
            memory_response_ready,
        }
    }
}

fn const_wires<const WIDTH: usize>(value: u8) -> Wires<WIDTH> {
    Wires {
        wires: std::array::from_fn(|bit| input_const(((u64::from(value) >> bit) & 1) as u8)),
    }
}

fn bool_wire(value: bool) -> Wire {
    input_const(u8::from(value))
}

/// `value == constant` for a narrow bus.
fn eq_const<const WIDTH: usize>(value: Wires<WIDTH>, constant: u8) -> Wire {
    let mut equal = input_const(1);
    for bit in 0..WIDTH {
        equal = equal & value.wires[bit].eq_const((constant >> bit) & 1);
    }
    equal
}

/// `(a > b, a == b)` for an unsigned bus.
fn cmp<const WIDTH: usize>(a: Wires<WIDTH>, b: Wires<WIDTH>) -> (Wire, Wire) {
    let mut greater = input_const(0);
    let mut equal = input_const(1);
    for bit in (0..WIDTH).rev() {
        let a_bit = a.wires[bit];
        let b_bit = b.wires[bit];
        greater = greater | (equal & a_bit & !b_bit);
        equal = equal & !(a_bit ^ b_bit);
    }
    (greater, equal)
}

/// True when requester `index` sits before `other` in the rotation that starts
/// at `rotate`. Used only to break equal-score ties.
fn earlier(index: usize, other: usize, rotate: Wires<3>) -> Wire {
    let table: [Wires<1>; 8] = std::array::from_fn(|value| {
        let value = value % 6;
        Wires {
            wires: [bool_wire((index + 6 - value) % 6 < (other + 6 - value) % 6)],
        }
    });
    mux8_w(&table, rotate).wires[0]
}

fn request_bits(input: &CpuV3MemoryArbiterInputValue) -> [bool; 6] {
    [
        input.instruction_request_valid,
        input.data_request_valid,
        input.dma_request_valid,
        input.gpu_ro_request_valid,
        input.gpu_fb_r_request_valid,
        input.gpu_fb_w_request_valid,
    ]
}

fn position(index: usize, rotate: u8) -> u8 {
    (index as u8 + 6 - rotate) % 6
}

/// The winning non-display requester, or [`Owner::Display`] when the display
/// is asking, or [`Owner::None`] when nothing is.
fn select(
    input: &CpuV3MemoryArbiterInputValue,
    age: &[u8; 6],
    rotate: u8,
) -> (Owner, Option<usize>) {
    if input.display_request_valid {
        return (Owner::Display, None);
    }
    let requests = request_bits(input);
    let mut winner = None;
    'requesters: for index in 0..6 {
        if !requests[index] {
            continue;
        }
        let score = u16::from(REQUESTER_BASES[index]) + u16::from(age[index]);
        for other in 0..6 {
            if index == other || !requests[other] {
                continue;
            }
            let other_score = u16::from(REQUESTER_BASES[other]) + u16::from(age[other]);
            let better = if score != other_score {
                score > other_score
            } else {
                position(index, rotate) < position(other, rotate)
            };
            if !better {
                continue 'requesters;
            }
        }
        winner = Some(index);
        break;
    }
    match winner {
        Some(index) => (REQUESTER_OWNERS[index], Some(index)),
        None => (Owner::None, None),
    }
}

fn owner_response_ready(owner: Owner, input: &CpuV3MemoryArbiterInputValue) -> bool {
    match owner {
        Owner::None => false,
        Owner::Display => input.display_response_ready,
        Owner::Instruction => input.instruction_response_ready,
        Owner::Data => input.data_response_ready,
        Owner::Dma => input.dma_response_ready,
        Owner::GpuRo | Owner::GpuFbR | Owner::GpuFbW => true,
    }
}

fn advance_state(state: &mut CpuV3MemoryArbiterState, input: &CpuV3MemoryArbiterInputValue) {
    if input.reset {
        *state = CpuV3MemoryArbiterState::default();
        return;
    }
    let requests = request_bits(input);
    let (selected, winner) = select(input, &state.age, state.rotate);
    let at_boundary = state.owner == Owner::None;
    if at_boundary {
        // One arbitration boundary: the winner and any silent client reset
        // their age, every other waiting client ages one step (saturating).
        for index in 0..6 {
            if !requests[index] || selected == REQUESTER_OWNERS[index] {
                state.age[index] = 0;
            } else {
                state.age[index] = (state.age[index] + 1).min(15);
            }
        }
        if selected != Owner::None && input.memory_request_ready {
            if let Some(index) = winner {
                state.rotate = ((index + 1) % 6) as u8;
            }
            state.owner = selected;
        }
    } else if input.memory_response_valid
        && owner_response_ready(state.owner, input)
        && (input.memory_response_last || input.memory_error)
    {
        state.owner = Owner::None;
    }
}

fn compute_output(
    state: &CpuV3MemoryArbiterState,
    input: &CpuV3MemoryArbiterInputValue,
) -> CpuV3MemoryArbiterOutputValue {
    let (selected, _) = select(input, &state.age, state.rotate);
    let requesting = state.owner == Owner::None && selected != Owner::None;
    let accepted = requesting && input.memory_request_ready;
    let responding = input.memory_response_valid;
    let owner = state.owner;
    let owner_responding = |candidate| owner == candidate && responding;
    let selected_is = |candidate| selected == candidate;

    let selected_write = match selected {
        Owner::Data => input.data_write,
        Owner::Dma => input.dma_write,
        Owner::GpuRo => input.gpu_ro_write,
        Owner::GpuFbR => input.gpu_fb_r_write,
        Owner::GpuFbW => input.gpu_fb_w_write,
        Owner::None | Owner::Display | Owner::Instruction => false,
    };
    let selected_line = match selected {
        Owner::Instruction | Owner::Display | Owner::GpuRo | Owner::GpuFbR | Owner::GpuFbW => true,
        Owner::Data => input.data_line,
        Owner::None | Owner::Dma => false,
    };
    let selected_address = match selected {
        Owner::None => 0,
        Owner::Display => input.display_address,
        Owner::Instruction => input.instruction_address,
        Owner::Data => input.data_address,
        Owner::Dma => input.dma_address,
        Owner::GpuRo => input.gpu_ro_address,
        Owner::GpuFbR => input.gpu_fb_r_address,
        Owner::GpuFbW => input.gpu_fb_w_address,
    };
    let selected_write_data = match selected {
        Owner::Data => input.data_write_data,
        Owner::Dma => input.dma_write_data,
        Owner::GpuRo => input.gpu_ro_write_data,
        Owner::GpuFbR => input.gpu_fb_r_write_data,
        Owner::GpuFbW => input.gpu_fb_w_write_data,
        Owner::None | Owner::Display | Owner::Instruction => 0,
    };
    let held_write_data = match owner {
        Owner::Data if input.data_line => input.data_write_data,
        Owner::GpuRo if input.gpu_ro_write => input.gpu_ro_write_data,
        Owner::GpuFbR if input.gpu_fb_r_write => input.gpu_fb_r_write_data,
        Owner::GpuFbW => input.gpu_fb_w_write_data,
        _ => 0,
    };
    let memory_response_ready = owner_response_ready(owner, input);
    let read = |is_responding| {
        if is_responding {
            input.memory_read_data
        } else {
            0
        }
    };
    let respond_last = |is_responding: bool| is_responding && input.memory_response_last;

    CpuV3MemoryArbiterOutputValue {
        instruction_request_ready: accepted && selected_is(Owner::Instruction),
        instruction_response_valid: owner_responding(Owner::Instruction),
        instruction_read_data: read(owner_responding(Owner::Instruction)),
        instruction_error: owner_responding(Owner::Instruction) && input.memory_error,
        data_request_ready: accepted && selected_is(Owner::Data),
        data_response_valid: owner_responding(Owner::Data),
        data_read_data: read(owner_responding(Owner::Data)),
        data_error: owner_responding(Owner::Data) && input.memory_error,
        dma_request_ready: accepted && selected_is(Owner::Dma),
        dma_response_valid: owner_responding(Owner::Dma),
        dma_read_data: if owner_responding(Owner::Dma) {
            input.memory_read_data & 0xffff
        } else {
            0
        },
        dma_error: owner_responding(Owner::Dma) && input.memory_error,
        display_request_ready: accepted && selected_is(Owner::Display),
        display_response_valid: owner_responding(Owner::Display),
        display_read_data: read(owner_responding(Owner::Display)),
        display_response_last: respond_last(owner_responding(Owner::Display)),
        display_error: owner_responding(Owner::Display) && input.memory_error,
        gpu_ro_request_ready: accepted && selected_is(Owner::GpuRo),
        gpu_ro_response_valid: owner_responding(Owner::GpuRo),
        gpu_ro_read_data: read(owner_responding(Owner::GpuRo)),
        gpu_ro_response_last: respond_last(owner_responding(Owner::GpuRo)),
        gpu_ro_error: owner_responding(Owner::GpuRo) && input.memory_error,
        gpu_fb_r_request_ready: accepted && selected_is(Owner::GpuFbR),
        gpu_fb_r_response_valid: owner_responding(Owner::GpuFbR),
        gpu_fb_r_read_data: read(owner_responding(Owner::GpuFbR)),
        gpu_fb_r_response_last: respond_last(owner_responding(Owner::GpuFbR)),
        gpu_fb_r_error: owner_responding(Owner::GpuFbR) && input.memory_error,
        gpu_fb_w_request_ready: accepted && selected_is(Owner::GpuFbW),
        gpu_fb_w_response_valid: owner_responding(Owner::GpuFbW),
        gpu_fb_w_read_data: read(owner_responding(Owner::GpuFbW)),
        gpu_fb_w_response_last: respond_last(owner_responding(Owner::GpuFbW)),
        gpu_fb_w_error: owner_responding(Owner::GpuFbW) && input.memory_error,
        memory_request_valid: requesting,
        memory_write: requesting && selected_write,
        memory_line: requesting && selected_line,
        memory_address: if requesting { selected_address } else { 0 },
        memory_write_data: if requesting {
            selected_write_data
        } else {
            held_write_data
        },
        memory_response_ready,
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
            display_request_valid: false,
            display_address: 0,
            display_response_ready: false,
            gpu_ro_request_valid: false,
            gpu_ro_write: false,
            gpu_ro_address: 0,
            gpu_ro_write_data: 0,
            gpu_fb_r_request_valid: false,
            gpu_fb_r_write: false,
            gpu_fb_r_address: 0,
            gpu_fb_r_write_data: 0,
            gpu_fb_w_request_valid: false,
            gpu_fb_w_write: false,
            gpu_fb_w_address: 0,
            gpu_fb_w_write_data: 0,
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
            display_request_ready: false,
            display_response_valid: false,
            display_read_data: 0,
            display_response_last: false,
            display_error: false,
            gpu_ro_request_ready: false,
            gpu_ro_response_valid: false,
            gpu_ro_read_data: 0,
            gpu_ro_response_last: false,
            gpu_ro_error: false,
            gpu_fb_r_request_ready: false,
            gpu_fb_r_response_valid: false,
            gpu_fb_r_read_data: 0,
            gpu_fb_r_response_last: false,
            gpu_fb_r_error: false,
            gpu_fb_w_request_ready: false,
            gpu_fb_w_response_valid: false,
            gpu_fb_w_read_data: 0,
            gpu_fb_w_response_last: false,
            gpu_fb_w_error: false,
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
        // DMA word transaction; its base score outranks the idle data client
        // and the age-free rotator, so it is forwarded immediately.
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
    fn emu_and_nand_route_gpu_masters_with_hold_until_last() {
        let mut steps = vec![reset_step()];
        // A framebuffer write issues and holds its stream while the owner
        // stays latched until `last`.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                gpu_fb_w_request_valid: true,
                gpu_fb_w_write: true,
                gpu_fb_w_address: 0x4000,
                gpu_fb_w_write_data: 0x0102_0304_0506_0708,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_write: true,
                memory_line: true,
                memory_address: 0x4000,
                memory_write_data: 0x0102_0304_0506_0708,
                ..z()
            },
        ));
        // The port accepts it; the owner latches and the write stream is held.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                gpu_fb_w_request_valid: true,
                gpu_fb_w_write: true,
                gpu_fb_w_address: 0x4000,
                gpu_fb_w_write_data: 0x0102_0304_0506_0708,
                memory_request_ready: true,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_write_data: 0x0102_0304_0506_0708,
                memory_response_ready: true,
                ..z()
            },
        ));
        // The write stream advances while the arbitration port stays busy.
        for beat in 0..2u64 {
            let data = 0x1111_2222_3333_0000 + beat;
            steps.push(TestStep::new(
                CpuV3MemoryArbiterInputValue {
                    gpu_fb_w_request_valid: true,
                    gpu_fb_w_write: true,
                    gpu_fb_w_address: 0x4000,
                    gpu_fb_w_write_data: data,
                    ..idle()
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_write_data: data,
                    memory_response_ready: true,
                    ..z()
                },
            ));
        }
        // A non-final response beat is visible to the client while the owner
        // holds; the GPU masters keep response-ready asserted.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: 0xdead_beef_0000_0001,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                gpu_fb_w_response_valid: true,
                gpu_fb_w_read_data: 0xdead_beef_0000_0001,
                memory_response_ready: true,
                ..z()
            },
        ));
        // The last beat releases the owner.
        steps.push(TestStep::new(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_response_last: true,
                ..idle()
            },
            z(),
        ));
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
    fn display_beats_every_other_requester_at_the_boundary() {
        let mut state = CpuV3MemoryArbiterState::default();
        let mut input = idle();
        input.display_request_valid = true;
        input.display_address = 0x200000;
        input.instruction_request_valid = true;
        input.data_request_valid = true;
        input.gpu_fb_w_request_valid = true;
        // The display wins the selection immediately and keeps winning.
        for _ in 0..40 {
            let (selected, _) = select(&input, &state.age, state.rotate);
            assert_eq!(selected, Owner::Display);
            advance_state(&mut state, &input);
        }
    }

    #[test]
    fn every_non_display_requester_ages_up_and_is_eventually_served() {
        let mut served = [false; 6];
        for requester in 0..6 {
            let mut state = CpuV3MemoryArbiterState::default();
            let mut input = idle();
            // The highest-base master always asks as well.
            input.dma_request_valid = true;
            set_request(&mut input, requester, true);
            let mut selected_winner = false;
            for _ in 0..40 {
                let (selected, _) = select(&input, &state.age, state.rotate);
                if selected == REQUESTER_OWNERS[requester] {
                    selected_winner = true;
                    break;
                }
                advance_state(&mut state, &input);
            }
            served[requester] = selected_winner;
        }
        // Every requester, including the lowest-base framebuffer writer,
        // eventually overtakes the highest-base DMA after waiting long enough.
        assert!(served.iter().all(|s| *s));
    }

    #[test]
    fn equal_score_requesters_share_the_arbiter() {
        let mut state = CpuV3MemoryArbiterState::default();
        let mut input = idle();
        input.instruction_request_valid = true;
        input.data_request_valid = true;
        let mut instruction_wins = 0;
        let mut data_wins = 0;
        for _ in 0..12 {
            match select(&input, &state.age, state.rotate).0 {
                Owner::Instruction => instruction_wins += 1,
                Owner::Data => data_wins += 1,
                other => panic!("unexpected winner {other:?}"),
            }
            advance_state(&mut state, &input);
        }
        assert!(instruction_wins > 0, "instruction never won");
        assert!(data_wins > 0, "data never won");
    }

    #[test]
    fn export_has_no_target_resource_claims() {
        assert!(VerilogProject::generate::<CpuV3MemoryArbiter>()
            .unwrap()
            .resource_claims
            .is_empty());
    }

    fn set_request(input: &mut CpuV3MemoryArbiterInputValue, requester: usize, value: bool) {
        match requester {
            0 => input.instruction_request_valid = value,
            1 => input.data_request_valid = value,
            2 => input.dma_request_valid = value,
            3 => input.gpu_ro_request_valid = value,
            4 => input.gpu_fb_r_request_valid = value,
            5 => input.gpu_fb_w_request_valid = value,
            _ => unreachable!(),
        }
    }
}
