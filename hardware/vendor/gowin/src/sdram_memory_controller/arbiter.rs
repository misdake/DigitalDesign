//! Vendor-owned shared arbiter between display scanout, instruction
//! and data caches, boot DMA, the three GPU masters, and the Tang Nano 20K
//! physical SDRAM line/word port.
//!
//! Caches and display transfer one 32-byte line; GPU clients transfer one to
//! four consecutive lines. Each line contains four ordered 64-bit beats
//! (beat n carries words 4*n through 4*n+3). The arbiter
//! forwards one request to the SDRAM adapter, holds ownership while the
//! adapter streams the real burst, and releases the owner on the accepted beat
//! carrying `memory_response_last` (or any error beat). The DMA client keeps
//! single 16-bit word transactions. Read payload is broadcast; each client
//! qualifies it with its own response-valid/error rather than a wide zero mux.
//!
//! Display has strict priority at every transaction boundary. The other six
//! requesters share a circular first-ready schedule. The cursor advances only
//! after an accepted non-display transaction; its accepted owner remains fixed
//! until an accepted last/error response. Fairness is bounded in accepted
//! non-display transactions, excluding display demand and stalled memory.

use digital_design_circuit::{input_const, mux2_w, mux8_w, reg_w, CircuitWires, Wire, Wires};
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
    pub gpu_ro_line_count_minus_1: Wires<2>,
    pub gpu_ro_write_data: Wires<64>,

    pub gpu_fb_r_request_valid: Wire,
    pub gpu_fb_r_write: Wire,
    pub gpu_fb_r_address: Wires<22>,
    pub gpu_fb_r_line_count_minus_1: Wires<2>,
    pub gpu_fb_r_write_data: Wires<64>,

    pub gpu_fb_w_request_valid: Wire,
    pub gpu_fb_w_write: Wire,
    pub gpu_fb_w_address: Wires<22>,
    pub gpu_fb_w_line_count_minus_1: Wires<2>,
    pub gpu_fb_w_write_data: Wires<64>,

    pub memory_request_ready: Wire,
    pub memory_write_data_ready: Wire,
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
    pub data_write_data_ready: Wire,
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
    pub gpu_ro_write_data_ready: Wire,
    pub gpu_ro_response_valid: Wire,
    pub gpu_ro_read_data: Wires<64>,
    pub gpu_ro_response_last: Wire,
    pub gpu_ro_error: Wire,

    pub gpu_fb_r_request_ready: Wire,
    pub gpu_fb_r_write_data_ready: Wire,
    pub gpu_fb_r_response_valid: Wire,
    pub gpu_fb_r_read_data: Wires<64>,
    pub gpu_fb_r_response_last: Wire,
    pub gpu_fb_r_error: Wire,

    pub gpu_fb_w_request_ready: Wire,
    pub gpu_fb_w_write_data_ready: Wire,
    pub gpu_fb_w_response_valid: Wire,
    pub gpu_fb_w_read_data: Wires<64>,
    pub gpu_fb_w_response_last: Wire,
    pub gpu_fb_w_error: Wire,

    pub memory_request_valid: Wire,
    pub memory_write: Wire,
    pub memory_line: Wire,
    pub memory_address: Wires<22>,
    pub memory_line_count_minus_1: Wires<2>,
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
/// in [`OWNER_CODES`].
const REQUESTER_OWNERS: [Owner; 6] = [
    Owner::Instruction,
    Owner::Data,
    Owner::Dma,
    Owner::GpuRo,
    Owner::GpuFbR,
    Owner::GpuFbW,
];
const OWNER_CODES: [u8; 6] = [
    OWNER_INSTRUCTION,
    OWNER_DATA,
    OWNER_DMA,
    OWNER_GPU_RO,
    OWNER_GPU_FB_R,
    OWNER_GPU_FB_W,
];

/// A cache line is 32 bytes, sixteen 16-bit words. The native controller
/// owns the physical bank/row mapping after the halfword lane is removed.
pub const LINE_WORDS: u32 = 16;

/// The native controller accepts 32, 64 and 128-byte naturally aligned line
/// requests. Encoding 2 is reserved; bank/row transitions are controller-owned.
pub const fn line_request_is_legal(address: u32, line_count_minus_1: u32) -> bool {
    if line_count_minus_1 > 3 || line_count_minus_1 == 2 {
        return false;
    }
    let words = LINE_WORDS * (line_count_minus_1 + 1);
    address & (words - 1) == 0
}

/// Panics when a host request cannot be sent to the native controller.
pub fn assert_line_request_is_legal(address: u32, line_count_minus_1: u32) {
    assert!(
        line_request_is_legal(address, line_count_minus_1),
        "line request at word {address:#x} has unsupported length/alignment encoding {line_count_minus_1}"
    );
}

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
    /// Round-robin service cursor over the six non-display requesters.
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
        let rotate = reg_w::<3>();

        let requests: [Wire; 6] = [
            input.instruction_request_valid,
            input.data_request_valid,
            input.dma_request_valid,
            input.gpu_ro_request_valid,
            input.gpu_fb_r_request_valid,
            input.gpu_fb_w_request_valid,
        ];
        // Prefer the first request at/after the cursor. If none is present,
        // wrap to the first request before it. No scores or wait counters.
        let cursor_at: [Wire; 6] = std::array::from_fn(|i| eq_const(rotate.out, i as u8));
        let mut cursor_before = zero;
        let masked: [Wire; 6] = std::array::from_fn(|i| {
            cursor_before = cursor_before | cursor_at[i];
            requests[i] & cursor_before
        });
        let masked_any = masked.iter().fold(zero, |acc, &request| acc | request);
        let mut earlier_any = zero;
        let mut earlier_masked = zero;
        let wins: [Wire; 6] = std::array::from_fn(|i| {
            let win = (masked[i] & !earlier_masked) | (!masked_any & requests[i] & !earlier_any);
            earlier_any = earlier_any | requests[i];
            earlier_masked = earlier_masked | masked[i];
            win
        });

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
        // Only the three GPU masters carry a request length this milestone;
        // display, instruction, and D-cache stay fixed at one line.
        let selected_line_count = mux8_w(
            &[
                const_wires::<2>(0),
                const_wires::<2>(0),
                const_wires::<2>(0),
                const_wires::<2>(0),
                const_wires::<2>(0),
                input.gpu_ro_line_count_minus_1,
                input.gpu_fb_r_line_count_minus_1,
                input.gpu_fb_w_line_count_minus_1,
            ],
            selected,
        );

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
        // Choose each payload source once. Address-acceptance and held-owner
        // qualification are shared across the 64-bit lane, rather than two
        // wide mux trees followed by another requesting/owner mux.
        let use_data = (requesting & selected_data) | (owner_data & input.data_line);
        let use_dma = requesting & selected_dma;
        let use_gpu_ro = (requesting & selected_gpu_ro) | (owner_gpu_ro & input.gpu_ro_write);
        let use_gpu_fb_r =
            (requesting & selected_gpu_fb_r) | (owner_gpu_fb_r & input.gpu_fb_r_write);
        let use_gpu_fb_w = (requesting & selected_gpu_fb_w) | owner_gpu_fb_w;
        let memory_write_data = mux2_w(const_wires::<64>(0), input.data_write_data, use_data)
            | mux2_w(
                const_wires::<64>(0),
                input.dma_write_data.expand_unsigned::<64>(),
                use_dma,
            )
            | mux2_w(const_wires::<64>(0), input.gpu_ro_write_data, use_gpu_ro)
            | mux2_w(
                const_wires::<64>(0),
                input.gpu_fb_r_write_data,
                use_gpu_fb_r,
            )
            | mux2_w(
                const_wires::<64>(0),
                input.gpu_fb_w_write_data,
                use_gpu_fb_w,
            );

        let responding = input.memory_response_valid;
        let gpu_ro_write_data_ready = owner_gpu_ro & input.memory_write_data_ready;
        let gpu_fb_r_write_data_ready = owner_gpu_fb_r & input.memory_write_data_ready;
        let gpu_fb_w_write_data_ready = owner_gpu_fb_w & input.memory_write_data_ready;
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
            instruction_read_data: input.memory_read_data,
            instruction_error: instruction_responding & input.memory_error,
            data_request_ready: accepted & selected_data,
            data_write_data_ready: owner_data & input.memory_write_data_ready,
            data_response_valid: data_responding,
            data_read_data: input.memory_read_data,
            data_error: data_responding & input.memory_error,
            dma_request_ready: accepted & selected_dma,
            dma_response_valid: dma_responding,
            dma_read_data,
            dma_error: dma_responding & input.memory_error,
            display_request_ready: accepted & selected_display,
            display_response_valid: display_responding,
            display_read_data: input.memory_read_data,
            display_response_last: display_responding & input.memory_response_last,
            display_error: display_responding & input.memory_error,
            gpu_ro_request_ready: accepted & selected_gpu_ro,
            gpu_ro_write_data_ready,
            gpu_ro_response_valid: gpu_ro_responding,
            gpu_ro_read_data: input.memory_read_data,
            gpu_ro_response_last: gpu_ro_responding & input.memory_response_last,
            gpu_ro_error: gpu_ro_responding & input.memory_error,
            gpu_fb_r_request_ready: accepted & selected_gpu_fb_r,
            gpu_fb_r_write_data_ready,
            gpu_fb_r_response_valid: gpu_fb_r_responding,
            gpu_fb_r_read_data: input.memory_read_data,
            gpu_fb_r_response_last: gpu_fb_r_responding & input.memory_response_last,
            gpu_fb_r_error: gpu_fb_r_responding & input.memory_error,
            gpu_fb_w_request_ready: accepted & selected_gpu_fb_w,
            gpu_fb_w_write_data_ready,
            gpu_fb_w_response_valid: gpu_fb_w_responding,
            gpu_fb_w_read_data: input.memory_read_data,
            gpu_fb_w_response_last: gpu_fb_w_responding & input.memory_response_last,
            gpu_fb_w_error: gpu_fb_w_responding & input.memory_error,
            memory_request_valid: requesting,
            memory_write: requesting & selected_write,
            memory_line: requesting & selected_line,
            memory_address: mux2_w(const_wires::<22>(0), selected_address, requesting),
            memory_line_count_minus_1: mux2_w(const_wires::<2>(0), selected_line_count, requesting),
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

/// `value == constant` for a narrow bus.
fn eq_const<const WIDTH: usize>(value: Wires<WIDTH>, constant: u8) -> Wire {
    let mut equal = input_const(1);
    for bit in 0..WIDTH {
        equal = equal & value.wires[bit].eq_const((constant >> bit) & 1);
    }
    equal
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

/// Circular first-ready selection, independently described from the RTL mask.
fn select(input: &CpuV3MemoryArbiterInputValue, rotate: u8) -> (Owner, Option<usize>) {
    if input.display_request_valid {
        return (Owner::Display, None);
    }
    let requests = request_bits(input);
    for offset in 0..6 {
        let index = (usize::from(rotate) + offset) % 6;
        if requests[index] {
            return (REQUESTER_OWNERS[index], Some(index));
        }
    }
    (Owner::None, None)
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
    let (selected, winner) = select(input, state.rotate);
    let at_boundary = state.owner == Owner::None;
    if at_boundary {
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
    let (selected, _) = select(input, state.rotate);
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
    let selected_line_count = match selected {
        Owner::GpuRo => input.gpu_ro_line_count_minus_1,
        Owner::GpuFbR => input.gpu_fb_r_line_count_minus_1,
        Owner::GpuFbW => input.gpu_fb_w_line_count_minus_1,
        _ => 0,
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
    let respond_last = |is_responding: bool| is_responding && input.memory_response_last;

    CpuV3MemoryArbiterOutputValue {
        instruction_request_ready: accepted && selected_is(Owner::Instruction),
        instruction_response_valid: owner_responding(Owner::Instruction),
        instruction_read_data: input.memory_read_data,
        instruction_error: owner_responding(Owner::Instruction) && input.memory_error,
        data_request_ready: accepted && selected_is(Owner::Data),
        data_write_data_ready: owner == Owner::Data && input.memory_write_data_ready,
        data_response_valid: owner_responding(Owner::Data),
        data_read_data: input.memory_read_data,
        data_error: owner_responding(Owner::Data) && input.memory_error,
        dma_request_ready: accepted && selected_is(Owner::Dma),
        dma_response_valid: owner_responding(Owner::Dma),
        dma_read_data: input.memory_read_data & 0xffff,
        dma_error: owner_responding(Owner::Dma) && input.memory_error,
        display_request_ready: accepted && selected_is(Owner::Display),
        display_response_valid: owner_responding(Owner::Display),
        display_read_data: input.memory_read_data,
        display_response_last: respond_last(owner_responding(Owner::Display)),
        display_error: owner_responding(Owner::Display) && input.memory_error,
        gpu_ro_request_ready: accepted && selected_is(Owner::GpuRo),
        gpu_ro_write_data_ready: owner == Owner::GpuRo && input.memory_write_data_ready,
        gpu_ro_response_valid: owner_responding(Owner::GpuRo),
        gpu_ro_read_data: input.memory_read_data,
        gpu_ro_response_last: respond_last(owner_responding(Owner::GpuRo)),
        gpu_ro_error: owner_responding(Owner::GpuRo) && input.memory_error,
        gpu_fb_r_request_ready: accepted && selected_is(Owner::GpuFbR),
        gpu_fb_r_write_data_ready: owner == Owner::GpuFbR && input.memory_write_data_ready,
        gpu_fb_r_response_valid: owner_responding(Owner::GpuFbR),
        gpu_fb_r_read_data: input.memory_read_data,
        gpu_fb_r_response_last: respond_last(owner_responding(Owner::GpuFbR)),
        gpu_fb_r_error: owner_responding(Owner::GpuFbR) && input.memory_error,
        gpu_fb_w_request_ready: accepted && selected_is(Owner::GpuFbW),
        gpu_fb_w_write_data_ready: owner == Owner::GpuFbW && input.memory_write_data_ready,
        gpu_fb_w_response_valid: owner_responding(Owner::GpuFbW),
        gpu_fb_w_read_data: input.memory_read_data,
        gpu_fb_w_response_last: respond_last(owner_responding(Owner::GpuFbW)),
        gpu_fb_w_error: owner_responding(Owner::GpuFbW) && input.memory_error,
        memory_request_valid: requesting,
        memory_write: requesting && selected_write,
        memory_line: requesting && selected_line,
        memory_address: if requesting { selected_address } else { 0 },
        memory_line_count_minus_1: if requesting { selected_line_count } else { 0 },
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

    // Payload is a shared wire; ownership/valid/error assertions remain exact.
    fn step(
        input: CpuV3MemoryArbiterInputValue,
        mut expected: CpuV3MemoryArbiterOutputValue,
    ) -> Step {
        expected.instruction_read_data = input.memory_read_data;
        expected.data_read_data = input.memory_read_data;
        expected.dma_read_data = input.memory_read_data & 0xffff;
        expected.display_read_data = input.memory_read_data;
        expected.gpu_ro_read_data = input.memory_read_data;
        expected.gpu_fb_r_read_data = input.memory_read_data;
        expected.gpu_fb_w_read_data = input.memory_read_data;
        TestStep::new(input, expected)
    }

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
            gpu_ro_line_count_minus_1: 0,
            gpu_ro_write_data: 0,
            gpu_fb_r_request_valid: false,
            gpu_fb_r_write: false,
            gpu_fb_r_address: 0,
            gpu_fb_r_line_count_minus_1: 0,
            gpu_fb_r_write_data: 0,
            gpu_fb_w_request_valid: false,
            gpu_fb_w_write: false,
            gpu_fb_w_address: 0,
            gpu_fb_w_line_count_minus_1: 0,
            gpu_fb_w_write_data: 0,
            memory_request_ready: false,
            memory_write_data_ready: false,
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
            data_write_data_ready: false,
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
            gpu_ro_write_data_ready: false,
            gpu_ro_response_valid: false,
            gpu_ro_read_data: 0,
            gpu_ro_response_last: false,
            gpu_ro_error: false,
            gpu_fb_r_request_ready: false,
            gpu_fb_r_write_data_ready: false,
            gpu_fb_r_response_valid: false,
            gpu_fb_r_read_data: 0,
            gpu_fb_r_response_last: false,
            gpu_fb_r_error: false,
            gpu_fb_w_request_ready: false,
            gpu_fb_w_write_data_ready: false,
            gpu_fb_w_response_valid: false,
            gpu_fb_w_read_data: 0,
            gpu_fb_w_response_last: false,
            gpu_fb_w_error: false,
            memory_request_valid: false,
            memory_write: false,
            memory_line: false,
            memory_address: 0,
            memory_line_count_minus_1: 0,
            memory_write_data: 0,
            memory_response_ready: false,
        }
    }

    fn reset_step() -> Step {
        step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_response_last: true,
                data_response_ready: true,
                ..idle()
            },
            z(),
        ));
        // DMA is the only requesting client and is forwarded immediately.
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
            steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
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
    fn emu_and_nand_forward_each_gpu_line_count_and_default_to_one_line() {
        // gpu_ro carries four lines.
        let mut steps = vec![reset_step()];
        steps.push(step(
            CpuV3MemoryArbiterInputValue {
                gpu_ro_request_valid: true,
                gpu_ro_address: 0x1000,
                gpu_ro_line_count_minus_1: 3,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_line: true,
                memory_address: 0x1000,
                memory_line_count_minus_1: 3,
                ..z()
            },
        ));
        steps.push(reset_step());
        // gpu_fb_r carries two lines.
        steps.push(step(
            CpuV3MemoryArbiterInputValue {
                gpu_fb_r_request_valid: true,
                gpu_fb_r_address: 0x1080,
                gpu_fb_r_line_count_minus_1: 1,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_line: true,
                memory_address: 0x1080,
                memory_line_count_minus_1: 1,
                ..z()
            },
        ));
        steps.push(reset_step());
        // gpu_fb_w carries four lines and a write beat.
        steps.push(step(
            CpuV3MemoryArbiterInputValue {
                gpu_fb_w_request_valid: true,
                gpu_fb_w_write: true,
                gpu_fb_w_address: 0x1100,
                gpu_fb_w_line_count_minus_1: 3,
                gpu_fb_w_write_data: 0xdead,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_write: true,
                memory_line: true,
                memory_address: 0x1100,
                memory_line_count_minus_1: 3,
                memory_write_data: 0xdead,
                ..z()
            },
        ));
        steps.push(reset_step());
        // A fixed one-line instruction request keeps the default length.
        steps.push(step(
            CpuV3MemoryArbiterInputValue {
                instruction_request_valid: true,
                instruction_address: 0x0140,
                ..idle()
            },
            CpuV3MemoryArbiterOutputValue {
                memory_request_valid: true,
                memory_line: true,
                memory_address: 0x0140,
                ..z()
            },
        ));
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_route_long_write_beat_ready_only_to_the_owner() {
        let steps = vec![
            reset_step(),
            step(
                CpuV3MemoryArbiterInputValue {
                    gpu_fb_w_request_valid: true,
                    gpu_fb_w_write: true,
                    gpu_fb_w_line_count_minus_1: 3,
                    gpu_fb_w_write_data: 0x1111,
                    ..idle()
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_request_valid: true,
                    memory_write: true,
                    memory_line: true,
                    memory_line_count_minus_1: 3,
                    memory_write_data: 0x1111,
                    ..z()
                },
            ),
            step(
                CpuV3MemoryArbiterInputValue {
                    gpu_fb_w_request_valid: true,
                    gpu_fb_w_write: true,
                    gpu_fb_w_line_count_minus_1: 3,
                    gpu_fb_w_write_data: 0x1111,
                    memory_request_ready: true,
                    ..idle()
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_write_data: 0x1111,
                    memory_response_ready: true,
                    ..z()
                },
            ),
            step(
                CpuV3MemoryArbiterInputValue {
                    gpu_fb_w_write: true,
                    gpu_fb_w_write_data: 0x2222,
                    memory_write_data_ready: true,
                    ..idle()
                },
                CpuV3MemoryArbiterOutputValue {
                    gpu_fb_w_write_data_ready: true,
                    memory_write_data: 0x2222,
                    memory_response_ready: true,
                    ..z()
                },
            ),
        ];
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_route_data_write_beat_ready_only_after_ownership() {
        let steps = vec![
            reset_step(),
            step(
                CpuV3MemoryArbiterInputValue {
                    data_request_valid: true,
                    data_write: true,
                    data_line: true,
                    data_write_data: 0x1111,
                    ..idle()
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_request_valid: true,
                    memory_write: true,
                    memory_line: true,
                    memory_write_data: 0x1111,
                    ..z()
                },
            ),
            step(
                CpuV3MemoryArbiterInputValue {
                    data_request_valid: true,
                    data_write: true,
                    data_line: true,
                    data_write_data: 0x1111,
                    memory_request_ready: true,
                    ..idle()
                },
                CpuV3MemoryArbiterOutputValue {
                    memory_write_data: 0x1111,
                    ..z()
                },
            ),
            step(
                CpuV3MemoryArbiterInputValue {
                    data_write: true,
                    data_line: true,
                    instruction_request_valid: true,
                    data_write_data: 0x2222,
                    memory_write_data_ready: true,
                    ..idle()
                },
                CpuV3MemoryArbiterOutputValue {
                    data_write_data_ready: true,
                    memory_write_data: 0x2222,
                    ..z()
                },
            ),
        ];
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn emu_and_nand_release_on_an_error_beat_without_last() {
        let mut steps = vec![reset_step()];
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(
            CpuV3MemoryArbiterInputValue {
                memory_response_valid: true,
                memory_read_data: beat_data(1),
                memory_error: true,
                instruction_response_ready: true,
                ..idle()
            },
            z(),
        ));
        steps.push(step(
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
        steps.push(step(
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
        steps.push(step(idle(), z()));
        steps.push(step(
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
            let (selected, _) = select(&input, state.rotate);
            assert_eq!(selected, Owner::Display);
            advance_state(&mut state, &input);
        }
    }

    #[test]
    fn persistent_clients_are_served_within_six_non_display_acceptances() {
        let mut input = idle();
        input.memory_request_ready = true;
        for i in 0..6 {
            set_request(&mut input, i, true);
        }
        let mut state = CpuV3MemoryArbiterState::default();
        for transaction in 0..120 {
            let expected = transaction % 6;
            assert_eq!(select(&input, state.rotate).0, REQUESTER_OWNERS[expected]);
            advance_state(&mut state, &input);
            assert_eq!(state.owner, REQUESTER_OWNERS[expected]);
            let mut response = idle();
            response.memory_response_valid = true;
            response.memory_response_last = true;
            response.instruction_response_ready = true;
            response.data_response_ready = true;
            response.dma_response_ready = true;
            advance_state(&mut state, &response);
            assert_eq!(state.owner, Owner::None);
        }
    }

    #[test]
    fn sparse_requests_wrap_without_advancing_on_stall_or_display() {
        let mut input = idle();
        input.data_request_valid = true;
        input.gpu_fb_w_request_valid = true;
        let mut state = CpuV3MemoryArbiterState {
            rotate: 3,
            ..CpuV3MemoryArbiterState::default()
        };
        for _ in 0..20 {
            advance_state(&mut state, &input);
        }
        assert_eq!(state.rotate, 3);
        assert_eq!(select(&input, state.rotate).0, Owner::GpuFbW);
        input.memory_request_ready = true;
        input.display_request_valid = true;
        advance_state(&mut state, &input);
        assert_eq!(state.owner, Owner::Display);
        assert_eq!(state.rotate, 3);
        input.display_response_ready = true;
        input.memory_response_valid = true;
        input.memory_response_last = true;
        advance_state(&mut state, &input);
        input.display_request_valid = false;
        advance_state(&mut state, &input);
        assert_eq!(state.owner, Owner::GpuFbW);
        assert_eq!(state.rotate, 0);
    }

    #[test]
    fn mixed_lengths_persistent_and_temporary_requests_match_independent_schedule() {
        let mut state = CpuV3MemoryArbiterState::default();
        let mut owner = Owner::None;
        let mut cursor = 0usize;
        let mut beats_left = 0usize;
        let mut rng = 0x8af931d2u32;
        let mut steps = vec![reset_step()];
        let mut served = [0usize; 6];
        let mut display_served = 0usize;
        let mut error_releases = 0usize;
        let mut held = 0usize;
        for cycle in 0..6000 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let mut input = idle();
            for i in 0..6 {
                set_request(&mut input, i, cycle < 3000 || (rng >> (i + 4)) & 1 != 0);
            }
            input.instruction_address = 0x100;
            input.data_address = 0x200;
            input.dma_address = 0x300;
            input.gpu_ro_address = 0x400;
            input.gpu_fb_r_address = 0x500;
            input.gpu_fb_w_address = 0x600;
            input.display_address = 0x700;
            input.data_line = true;
            input.data_write = true;
            input.dma_write = true;
            input.data_write_data = 0x2222;
            input.dma_write_data = 0x3333;
            input.gpu_ro_write_data = 0x4444;
            input.gpu_fb_r_write_data = 0x5555;
            input.gpu_fb_w_write_data = 0x6666;
            input.gpu_fb_w_write = true;
            input.gpu_ro_line_count_minus_1 = 1;
            input.gpu_fb_r_line_count_minus_1 = 0;
            input.gpu_fb_w_line_count_minus_1 = 3;
            input.display_request_valid = cycle % 101 < 3;
            input.memory_request_ready = rng & 3 != 0;
            input.memory_write_data_ready = rng & 8 != 0;
            input.instruction_response_ready = rng & 16 != 0;
            input.data_response_ready = rng & 32 != 0;
            input.dma_response_ready = rng & 64 != 0;
            input.display_response_ready = rng & 128 != 0;
            input.memory_response_valid = owner != Owner::None && rng & 256 != 0;
            input.memory_response_last = beats_left == 1;
            input.memory_error = input.memory_response_valid && cycle % 97 == 0;
            input.memory_read_data = u64::from(rng) | (u64::from(rng.rotate_left(9)) << 32);
            let output = compute_output(&state, &input);
            assert_eq!(state.owner, owner);
            if owner == Owner::None {
                let expected = if input.display_request_valid {
                    Owner::Display
                } else {
                    let requests = request_bits(&input);
                    (0..6)
                        .map(|n| (cursor + n) % 6)
                        .find(|&i| requests[i])
                        .map_or(Owner::None, |i| REQUESTER_OWNERS[i])
                };
                assert_eq!(output.memory_request_valid, expected != Owner::None);
                if expected != Owner::None {
                    let address = match expected {
                        Owner::Instruction => 0x100,
                        Owner::Data => 0x200,
                        Owner::Dma => 0x300,
                        Owner::GpuRo => 0x400,
                        Owner::GpuFbR => 0x500,
                        Owner::GpuFbW => 0x600,
                        Owner::Display => 0x700,
                        Owner::None => unreachable!(),
                    };
                    assert_eq!(output.memory_address, address);
                    if input.memory_request_ready {
                        owner = expected;
                        if let Some(index) = REQUESTER_OWNERS.iter().position(|&o| o == owner) {
                            cursor = (index + 1) % 6;
                            served[index] += 1;
                        } else {
                            display_served += 1;
                        }
                        beats_left = match owner {
                            Owner::GpuRo => 8,
                            Owner::GpuFbR => 4,
                            Owner::GpuFbW => 16,
                            Owner::Dma => 1,
                            _ => 4,
                        };
                    }
                }
            } else {
                assert!(!output.memory_request_valid);
                let valids = [
                    output.instruction_response_valid,
                    output.data_response_valid,
                    output.dma_response_valid,
                    output.gpu_ro_response_valid,
                    output.gpu_fb_r_response_valid,
                    output.gpu_fb_w_response_valid,
                ];
                for i in 0..6 {
                    assert_eq!(
                        valids[i],
                        owner == REQUESTER_OWNERS[i] && input.memory_response_valid
                    );
                }
                assert_eq!(
                    output.display_response_valid,
                    owner == Owner::Display && input.memory_response_valid
                );
                let write_ready = [
                    output.data_write_data_ready,
                    output.gpu_ro_write_data_ready,
                    output.gpu_fb_r_write_data_ready,
                    output.gpu_fb_w_write_data_ready,
                ];
                for (ready, candidate) in write_ready.into_iter().zip([
                    Owner::Data,
                    Owner::GpuRo,
                    Owner::GpuFbR,
                    Owner::GpuFbW,
                ]) {
                    assert_eq!(ready, owner == candidate && input.memory_write_data_ready);
                }
                if input.memory_response_valid && output.memory_response_ready {
                    beats_left -= 1;
                    if input.memory_error || input.memory_response_last {
                        error_releases += usize::from(input.memory_error);
                        owner = Owner::None;
                    }
                } else {
                    held += 1;
                }
            }
            advance_state(&mut state, &input);
            steps.push(TestStep::new(input.clone(), compute_output(&state, &input)));
            assert_eq!(state.owner, owner);
            assert_eq!(usize::from(state.rotate), cursor);
        }
        assert!(served.iter().all(|&n| n > 10));
        assert!(display_served > 0 && error_releases > 0 && held > 100);
        println!("mixed arbitration served={served:?} display={display_served} error_releases={error_releases} held={held}");
        ModuleTest::<CpuV3MemoryArbiter>::new(steps).run_emu_and_nand();
    }

    #[test]
    fn native_line_request_requires_supported_size_and_alignment() {
        assert!(line_request_is_legal(0x000, 0));
        assert!(line_request_is_legal(0x1f0, 0));
        assert!(line_request_is_legal(0x1e0, 1));
        assert!(line_request_is_legal(0x1c0, 3));
        assert!(line_request_is_legal(0x800, 3));
        assert!(!line_request_is_legal(0x1f0, 1));
        assert!(!line_request_is_legal(0x1e0, 3));
        assert!(!line_request_is_legal(0x1d0, 2));
        assert!(!line_request_is_legal(0x800, 4));
    }

    #[test]
    #[should_panic(expected = "unsupported length/alignment")]
    fn native_line_assertion_reports_invalid_alignment() {
        assert_line_request_is_legal(0x1f0, 3);
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
