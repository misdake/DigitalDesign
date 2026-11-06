//! Production instruction frontend and its state.
use super::*;

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuFrontendInput {
    pub word_valid: Wire,
    pub word: Wires<16>,
    pub abort: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3FpuFrontendOutput {
    pub read_valid: Wire,
    pub rf_read_a_address: Wires<9>,
    pub rf_read_b_address: Wires<9>,
    pub word0_raw: Wires<16>,
    pub word1_raw: Wires<16>,
    pub instr_opcode: Wires<4>,
    pub instr_complete: Wire,
}

/// FPU v2 instruction front-end: assembles the 32-bit instruction from two
/// 16-bit words and forms the register-file read addresses combinationally
/// in the word0 cycle so the synchronous RF presents operands one cycle
/// later, before word1 is decoded. Word1 field splitting (len/subop/mode)
/// is left to the downstream controller; this leaf only latches raw halves.
#[derive(Default)]
pub struct CpuV3FpuFrontend;

impl HardwareIdentity for CpuV3FpuFrontend {
    const TARGET_RESOURCE_LEAF: bool = true;

    fn verilog_identity() -> VerilogIdentity {
        VerilogIdentity::new("CpuV3FpuFrontend").namespace(["components", "cpu", "cpu_v3"])
    }
}

#[derive(Default, Clone)]
pub struct CpuV3FpuFrontendState {
    pub(super) waiting_word1: bool,
    pub(super) word0_raw: u16,
    pub(super) word1_raw: u16,
    pub(super) instr_opcode: u8,
    pub(super) instr_complete: bool,
}

impl Module for CpuV3FpuFrontend {
    type Input = CpuV3FpuFrontendInput;
    type Output = CpuV3FpuFrontendOutput;
    type EmuState = CpuV3FpuFrontendState;

    const USES_MAIN_CLOCK: bool = true;

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        Self::EmuState::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let input = input.sample(circuit);
        let word = input.word as u16;
        let read_valid = input.word_valid && !state.waiting_word1;
        // AUX word0 carries Fa at bits [7:2]; the other opcodes at [11:6].
        let read_a = if encoding::opcode(word) == encoding::OPCODE_AUX {
            encoding::aux_fa(word)
        } else {
            encoding::word0_fa(word)
        };
        output.drive(
            circuit,
            &CpuV3FpuFrontendOutputValue {
                read_valid,
                rf_read_a_address: u64::from(read_a),
                rf_read_b_address: u64::from(encoding::word0_fb(word)),
                word0_raw: u64::from(state.word0_raw),
                word1_raw: u64::from(state.word1_raw),
                instr_opcode: u64::from(state.instr_opcode),
                instr_complete: state.instr_complete,
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
        let word = input.word as u16;
        let accept_word0 = input.word_valid && !state.waiting_word1;
        let accept_word1 = input.word_valid && state.waiting_word1 && !input.abort;
        let discard_word0 = input.abort && state.waiting_word1;
        state.instr_complete = accept_word1;
        if accept_word0 {
            state.word0_raw = word;
            state.instr_opcode = encoding::opcode(word);
            state.waiting_word1 = true;
        } else if accept_word1 || discard_word0 {
            state.waiting_word1 = false;
        }
        if accept_word1 {
            state.word1_raw = word;
        }
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_frontend.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("cpu_v3_fpu_frontend_tb.v").to_string())
    }
}
