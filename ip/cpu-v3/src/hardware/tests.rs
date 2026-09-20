use super::*;
use crate as cpu_v3;
use crate::rcc_backend::{self, CompilerOptions};
use crate::{AluOp, CpuV3Sim, ImmediateOp, RunOutcome, SpecialRegister, TestCondition};
use digital_design_circuit::{build_circuit, Circuit};
use digital_design_hardware::{HardwareIdentity, Module, ModuleIo, VerilogProject};
use rcc::frontend::compile_program;
use std::collections::HashMap;

struct CoreRun {
    halt_signal: u16,
    retired_words: u32,
    code_segment: u16,
    data_segment: u16,
}

fn drive(
    circuit: &mut Circuit,
    input: &CpuV3CoreInput,
    instruction_response: Option<u16>,
    data_response: Option<u16>,
    device_read_data: u16,
) {
    input.drive(
        circuit,
        &CpuV3CoreInputValue {
            reset: false,
            hold: false,
            instruction_request_ready: true,
            instruction_response_valid: instruction_response.is_some(),
            instruction_data: u64::from(instruction_response.unwrap_or(0)),
            instruction_error: false,
            data_request_ready: true,
            data_response_valid: data_response.is_some(),
            data_read_data: u64::from(data_response.unwrap_or(0)),
            data_error: false,
            device_read_data: u64::from(device_read_data),
        },
    );
}

fn run_core(mut memory: HashMap<u32, u16>, maximum_cycles: usize) -> CoreRun {
    let (mut circuit, (input, output)) = build_circuit(|| {
        let input = CpuV3CoreInput::allocate();
        let output = CpuV3Core::emu(&input);
        (input, output)
    });
    let mut instruction_response = None;
    let mut data_response = None;
    drive(&mut circuit, &input, None, None, 0);
    let mut devices = [0u16; 128];

    for _cycle in 0..maximum_cycles {
        circuit.execute_gates();
        let value = output.sample(&circuit);
        if value.fault {
            panic!(
                "CpuV3 core faulted with code {} at {:#06x}",
                value.fault_code, value.fault_pc
            );
        }
        if value.halted {
            return CoreRun {
                halt_signal: value.halt_signal as u16,
                retired_words: value.retired_words as u32,
                code_segment: value.code_segment as u16,
                data_segment: value.data_segment as u16,
            };
        }

        let next_instruction_response = value.instruction_request_valid.then(|| {
            memory
                .get(&(value.instruction_address as u32))
                .copied()
                .unwrap_or(0)
        });
        let next_data_response = if value.data_request_valid {
            let address = value.data_address as u32;
            if value.data_write {
                memory.insert(address, value.data_write_data as u16);
                Some(0)
            } else {
                Some(memory.get(&address).copied().unwrap_or(0))
            }
        } else {
            None
        };
        let device_address = ((value.device_index as usize) << 4) | value.device_channel as usize;
        if value.device_write_enable {
            devices[device_address] = value.device_write_data as u16;
        }
        drive(
            &mut circuit,
            &input,
            instruction_response.take(),
            data_response.take(),
            devices[device_address],
        );
        circuit.clock_tick();
        instruction_response = next_instruction_response;
        data_response = next_data_response;
    }
    panic!("CpuV3 core exceeded {maximum_cycles} cycles")
}

fn load(memory: &mut HashMap<u32, u16>, base: u32, words: &[u16]) {
    for (offset, word) in words.iter().copied().enumerate() {
        memory.insert(base + offset as u32, word);
    }
}

fn compile(source: &str) -> Vec<u16> {
    let options = CompilerOptions::default();
    let frontend = compile_program(source, &options, &mut |_| {
        Err("test source does not use modules".to_string())
    })
    .unwrap();
    rcc_backend::compile(frontend, &options, "main").words
}

#[test]
fn cycle_model_signal_events_are_single_cycle_and_model_only() {
    let mut state = CpuV3CoreState::default();
    state.registers[5] = 0x2a;

    // A nonzero SIGNAL retires as a NOP and records the event.
    state.instruction = cpu_v3::signal(5, 1);
    state.execute(0);
    assert_eq!(state.phase, Phase::FetchRequest);
    assert_eq!(state.retired_words, 1);
    assert_eq!(
        state.signal_event(),
        Some(crate::SignalEvent {
            signal_type: 1,
            value: 0x2a,
        })
    );
    // Reading does not clear the event; take does.
    assert!(state.signal_event().is_some());
    assert!(state.take_signal_event().is_some());
    assert_eq!(state.signal_event(), None);

    // A fresh nonzero SIGNAL re-arms the event; the next executed
    // instruction ends the single-cycle window.
    state.instruction = cpu_v3::signal(5, 15);
    state.execute(0);
    assert_eq!(
        state.signal_event().map(|event| event.signal_type),
        Some(15)
    );
    state.instruction = 0x0322; // ADD r3, r2, r2 (any ordinary instruction)
    state.execute(0);
    assert_eq!(state.signal_event(), None);

    // Type 0 halts and latches the selected register, exactly like the
    // architectural HALT path.
    let mut state = CpuV3CoreState::default();
    state.registers[7] = 0x1234;
    state.instruction = cpu_v3::signal(7, 0);
    state.execute(0);
    assert_eq!(state.phase, Phase::Halted);
    assert_eq!(state.halt_signal, 0x1234);
    assert_eq!(state.retired_words, 1);
    assert_eq!(state.signal_event(), None);
}

#[test]
fn emulator_async_store_overlaps_alu_and_blocks_next_memory_operation() {
    let mut state = CpuV3CoreState::default();
    state.registers[1] = 0x0100;
    state.registers[2] = 10;

    state.instruction = 0x9210; // STORE r2, [r1]
    state.execute(0);
    assert_eq!(state.phase, Phase::FetchRequest);
    assert!(state.async_store.valid);
    assert!(!state.async_store.issued);
    assert_eq!(state.async_store.address, 0x0100);
    assert_eq!(state.async_store.write_data, 10);

    state.instruction = 0x0322; // ADD r3, r2, r2
    state.execute(0);
    assert_eq!(state.phase, Phase::FetchRequest);
    assert!(state.async_store.valid);
    assert!(state.gpr_write_enable);
    assert_eq!(state.gpr_write_data, 20);

    state.instruction = 0x8010; // LOAD r0, [r1]
    state.execute(0);
    assert_eq!(state.phase, Phase::AsyncStoreWait);
    assert!(!state.pending_data.write);
    assert_eq!(state.pending_data.address, 0x0100);
}

#[test]
fn emulator_matches_oracle_for_compiler_control_memory_and_multiply() {
    let program = compile(
        r#"
                static VALUE: u16 = 7;
                fn twice(value: u16) -> u16 { value + value }
                fn main() {
                    let mut total: u16 = 0;
                    let mut i: u16 = 1;
                    while i < 6 {
                        total = total + twice(i);
                        i = i + 1;
                    }
                    halt(total + VALUE + mul_16x4(3, 4));
                }
            "#,
    );
    let mut oracle = CpuV3Sim::default();
    oracle.load_program(0, &program).unwrap();
    let outcome = oracle.run(10_000).unwrap();
    let RunOutcome::Halted { signal, .. } = outcome else {
        panic!("oracle did not halt")
    };

    let mut memory = HashMap::new();
    load(&mut memory, 0, &program);
    // The compiler's static-data initialization runs from code, so the
    // external memory begins with the same zero-filled state as CpuV3Sim.
    let core = run_core(memory, 20_000);
    assert_eq!(core.halt_signal, signal);
    assert_eq!(core.retired_words as u64, oracle.retired_words());
}

#[test]
fn emulator_matches_segmented_fetch_data_and_special_register_semantics() {
    let mut boot = Vec::new();
    boot.extend(cpu_v3::load_immediate16(1, 1));
    boot.extend(cpu_v3::load_immediate16(2, 0x20));
    boot.extend(cpu_v3::load_immediate16(3, 2));
    boot.extend([cpu_v3::write_data_segment(3), cpu_v3::jump_segment(1, 2)]);
    let mut application = vec![
        cpu_v3::read_special(4, SpecialRegister::CodeSegment),
        cpu_v3::read_special(5, SpecialRegister::DataSegment),
    ];
    application.extend(cpu_v3::load_immediate16(6, 0x1234));
    application.extend([cpu_v3::load(0, 6, 0), cpu_v3::halt()]);

    let mut memory = HashMap::new();
    load(&mut memory, 0, &boot);
    load(&mut memory, 0x0001_0020, &application);
    memory.insert(0x0002_1234, 0xbeef);
    let core = run_core(memory, 1_000);
    assert_eq!(core.halt_signal, 0xbeef);
    assert_eq!((core.code_segment, core.data_segment), (1, 2));
}

#[test]
fn emulator_matches_oracle_for_reserved_prefix_and_comparison_edges() {
    let mut program = Vec::new();
    program.extend(cpu_v3::load_immediate16(1, 0x8000));
    program.extend(cpu_v3::load_immediate16(2, 0x7fff));
    program.extend(cpu_v3::load_immediate16(6, 3));
    program.extend(cpu_v3::load_immediate16(7, 5));
    program.extend([
        cpu_v3::move_register(8, 6),
        cpu_v3::multiply(cpu_v3::MultiplyWindow::Low, 8, 7),
        cpu_v3::move_register(3, 1),
        cpu_v3::set_less_than_signed(3, 2),
        cpu_v3::move_register(4, 1),
        cpu_v3::set_less_than_unsigned(4, 2),
        cpu_v3::population_count(5, 1),
        cpu_v3::shift_immediate(cpu_v3::ShiftOp::RightLogical, 1, 15),
        cpu_v3::alu(AluOp::Add, 0, 3, 4),
        cpu_v3::alu(AluOp::Add, 0, 0, 5),
        cpu_v3::alu(AluOp::Add, 0, 0, 8),
        cpu_v3::immediate_signed(ImmediateOp::CompareSigned, 0, 0),
        cpu_v3::branch(TestCondition::NotEqual, 1),
        cpu_v3::immediate_unsigned(ImmediateOp::LoadUnsigned, 0, 0),
        cpu_v3::halt(),
    ]);
    let mut oracle = CpuV3Sim::default();
    oracle.load_program(0, &program).unwrap();
    let RunOutcome::Halted { signal, .. } = oracle.run(1_000).unwrap() else {
        panic!("oracle did not halt")
    };
    let mut memory = HashMap::new();
    load(&mut memory, 0, &program);
    let core = run_core(memory, 2_000);
    assert_eq!(core.halt_signal, signal);
    assert_eq!(core.retired_words as u64, oracle.retired_words());
}

#[test]
fn emulator_matches_oracle_for_dedicated_device_instructions() {
    let mut program = Vec::new();
    program.extend(cpu_v3::load_immediate16(1, 0x1234));
    program.extend([
        cpu_v3::device_send(1, 2, 3),
        cpu_v3::device_receive(0, 2, 3),
        cpu_v3::halt(),
    ]);
    let mut oracle = CpuV3Sim::default();
    oracle.load_program(0, &program).unwrap();
    struct EchoDevice([u16; 16]);
    impl cpu_v3::Device for EchoDevice {
        fn read(&mut self, _memory: &mut [u16], channel: u8) -> u16 {
            self.0[usize::from(channel)]
        }
        fn write(&mut self, _memory: &mut [u16], channel: u8, value: u16) {
            self.0[usize::from(channel)] = value;
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    oracle.attach_device(2, Box::new(EchoDevice([0; 16])));
    let RunOutcome::Halted { signal, .. } = oracle.run(1_000).unwrap() else {
        panic!("oracle did not halt")
    };
    assert_eq!(signal, 0x1234);
    let mut memory = HashMap::new();
    load(&mut memory, 0, &program);
    let core = run_core(memory, 1_000);
    assert_eq!(core.halt_signal, signal);
    assert_eq!(core.retired_words as u64, oracle.retired_words());
}

#[test]
#[ignore = "explicit external simulation of the reusable CpuV3 core"]
fn verify_verilog_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Core>().unwrap();
}

// ---- emulator vs RTL co-simulation of the CpuV3 core ----
//
// The fetch queue and the two-way cache already have cycle-accurate
// emulator-vs-Icarus co-simulations. The core itself only had separate
// oracle (emulator) and `cpu_v3_core_tb.v` (RTL) regression suites, which
// never drove the same stimulus through both. The Stage 12 overlap changed
// the core's fetch/execute timing and added a GPR forwarding mux, so this
// test runs the same program through the Rust emulator and the RTL and
// compares a curated set of deterministic architectural outputs every
// cycle, including the fetch/execute overlap, the forwarded store value,
// and the barrier paths.

#[derive(Clone, Copy, Debug, Default)]
struct CoreCosimIn {
    reset: bool,
    instruction_request_ready: bool,
    instruction_response_valid: bool,
    instruction_data: u16,
    data_request_ready: bool,
    data_response_valid: bool,
    data_read_data: u16,
    device_read_data: u16,
}

impl CoreCosimIn {
    fn into_value(self) -> CpuV3CoreInputValue {
        CpuV3CoreInputValue {
            reset: self.reset,
            hold: false,
            instruction_request_ready: self.instruction_request_ready,
            instruction_response_valid: self.instruction_response_valid,
            instruction_data: u64::from(self.instruction_data),
            instruction_error: false,
            data_request_ready: self.data_request_ready,
            data_response_valid: self.data_response_valid,
            data_read_data: u64::from(self.data_read_data),
            data_error: false,
            device_read_data: u64::from(self.device_read_data),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CoreCosimOut {
    pc: u16,
    code_segment: u16,
    data_segment: u16,
    retired_words: u32,
    halted: bool,
    halt_signal: u16,
    fault: bool,
    fault_code: u8,
    fault_pc: u16,
    instruction_request_valid: bool,
    instruction_address: u32,
    instruction_response_ready: bool,
    data_request_valid: bool,
    data_write: bool,
    data_address: u32,
    data_write_data: u16,
    data_response_ready: bool,
}

impl CoreCosimOut {
    /// `halt_signal` is now a registered value latched at the HALT retire
    /// edge (mirrored by the emulator), so it is a stable architectural
    /// property of every cycle after reset and is compared directly.
    fn equal_core(&self, other: &Self) -> bool {
        self.pc == other.pc
            && self.code_segment == other.code_segment
            && self.data_segment == other.data_segment
            && self.retired_words == other.retired_words
            && self.halted == other.halted
            && self.halt_signal == other.halt_signal
            && self.fault == other.fault
            && self.fault_code == other.fault_code
            && self.fault_pc == other.fault_pc
            && self.instruction_request_valid == other.instruction_request_valid
            && self.instruction_address == other.instruction_address
            && self.instruction_response_ready == other.instruction_response_ready
            && self.data_request_valid == other.data_request_valid
            && self.data_write == other.data_write
            && self.data_address == other.data_address
            && self.data_write_data == other.data_write_data
            && self.data_response_ready == other.data_response_ready
    }
}

impl From<&CpuV3CoreOutputValue> for CoreCosimOut {
    fn from(value: &CpuV3CoreOutputValue) -> Self {
        // Device-bus fields are intentionally omitted: they are
        // instruction-decode-derived and are not part of the Stage 12
        // pipeline/forwarding behavior under test.
        Self {
            pc: value.pc as u16,
            code_segment: value.code_segment as u16,
            data_segment: value.data_segment as u16,
            retired_words: value.retired_words as u32,
            halted: value.halted,
            halt_signal: value.halt_signal as u16,
            fault: value.fault,
            fault_code: value.fault_code as u8,
            fault_pc: value.fault_pc as u16,
            instruction_request_valid: value.instruction_request_valid,
            instruction_address: value.instruction_address as u32,
            instruction_response_ready: value.instruction_response_ready,
            data_request_valid: value.data_request_valid,
            data_write: value.data_write,
            data_address: value.data_address as u32,
            data_write_data: value.data_write_data as u16,
            data_response_ready: value.data_response_ready,
        }
    }
}

/// Pipeline-focused program: a wide (SETP) load, a dependent immediate
/// chain that the forwarding bypass permits to run one instruction per
/// cycle, control-path ALU ops, a comparison, and a store whose data value
/// must equal the forwarded `r0`, then halt, followed by ISA 0.8 coverage
/// for the destructive shift/multiply family, the Boolean comparisons and
/// conditional moves, non-halting SIGNAL, the special registers, and the
/// relative/register call-return pair. Keeping both phases here means the
/// Rust cycle model and the RTL stay compared on every new decoder and
/// pipeline classification.
fn core_cosim_program() -> Vec<u16> {
    let mut p = Vec::new();
    p.extend(cpu_v3::load_immediate16(0, 0)); // r0 = 0 (SETP + ADDIU, two physical words)
    p.extend(cpu_v3::load_immediate16(1, 0x4000)); // r1 = 0x4000 (data base)
    for _ in 0..5 {
        // Dependent r0 <- r0 + 1; the forwarding mux lets each read see the
        // previous cycle's pending write so they run at one per cycle.
        p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Add, 0, 1));
    }
    p.push(cpu_v3::immediate_unsigned(
        crate::ImmediateOp::CompareUnsigned,
        0,
        5,
    )); // pending test = r0 == 5
    p.push(cpu_v3::branch(crate::TestCondition::Equal, 1)); // taken if r0 == 5
    p.push(cpu_v3::nop()); // not taken
    p.push(cpu_v3::alu(crate::AluOp::Add, 2, 0, 1)); // r2 = r0 + r1 (5 + 0x4000)
    p.push(cpu_v3::move_register(3, 2)); // r3 = r2
    p.push(cpu_v3::not(4, 3)); // r4 = ~r3
    p.push(cpu_v3::negate(5, 4)); // r5 = -r4
    p.push(cpu_v3::store(0, 1, 4)); // mem[r1+4] = r0 (async store, observes forwarded r0 = 5)

    // Destructive shift/multiply family (major 2). `MUL8 rd == rs`
    // exercises the read-before-write window, the immediate shifts cover
    // the 4-bit amount field, and the register form masks `rs & 15`.
    p.extend(cpu_v3::load_immediate16(2, 0x00ff)); // r2 = 0x00ff
    p.extend(cpu_v3::load_immediate16(3, 0x00ff)); // r3 = 0x00ff
    p.push(cpu_v3::multiply(cpu_v3::MultiplyWindow::Low, 2, 3)); // MUL0, rd != rs
    p.push(cpu_v3::multiply(cpu_v3::MultiplyWindow::Shift8, 3, 3)); // MUL8, rd == rs
    p.push(cpu_v3::multiply(cpu_v3::MultiplyWindow::Shift16, 3, 2)); // MUL16
    p.extend(cpu_v3::load_immediate16(4, 0x8001)); // r4 = 0x8001
    p.push(cpu_v3::shift_immediate(
        cpu_v3::ShiftOp::RightLogical,
        4,
        15,
    ));
    p.push(cpu_v3::shift_immediate(cpu_v3::ShiftOp::Left, 4, 15));
    p.push(cpu_v3::shift_immediate(
        cpu_v3::ShiftOp::RightArithmetic,
        4,
        15,
    ));
    p.extend(cpu_v3::load_immediate16(5, 0x0011)); // r5 = 0x11
    p.push(cpu_v3::shift_register(cpu_v3::ShiftOp::RightLogical, 4, 5)); // amount = 0x11 & 15 = 1
    p.push(cpu_v3::multiply_immediate(2, 3)); // MULI, u4
    p.extend(cpu_v3::prefixed(cpu_v3::multiply_immediate(3, 0), 4)); // MULI with PFX12

    // Boolean comparisons and conditional moves consume the pending test
    // whether or not the move writes.
    p.push(cpu_v3::compare_signed(2, 3));
    p.push(cpu_v3::conditional_move(
        crate::TestCondition::GreaterOrEqual,
        6,
        2,
    ));
    p.push(cpu_v3::compare_unsigned(2, 3));
    p.push(cpu_v3::conditional_move(
        crate::TestCondition::LessThan,
        7,
        3,
    ));
    p.push(cpu_v3::set_equal(8, 3)); // SEQ rd, rs
    p.push(cpu_v3::set_less_than_signed(9, 2)); // SLT rd, rs
    p.push(cpu_v3::set_less_than_unsigned(10, 4)); // SLTU rd, rs

    // LDC/ADDC (major A functions 7/B) index the shared constant table
    // and never consume PFX12; the prefix before the final LDC expires
    // unused and retires separately. Both table signs are covered.
    p.push(cpu_v3::load_constant(12, 0)); // r12 = 8
    p.push(cpu_v3::load_constant(11, 15)); // r11 = -512
    p.push(cpu_v3::add_constant(12, 7)); // r12 = 8 + 512 = 520
    p.push(cpu_v3::add_constant(11, 9)); // r11 = -512 + -16 = -528
    p.push(cpu_v3::prefix12(0xabc));
    p.push(cpu_v3::load_constant(12, 0)); // r12 = 8 (prefix expires)

    // ADDI/SUBI read the unprefixed immediate as an unsigned u4; 0 and 15
    // are the range boundaries (15 was -1 under the signed reading).
    p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Add, 12, 15)); // r12 = 23
    p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Sub, 12, 0)); // r12 = 23
    p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Sub, 12, 15)); // r12 = 8
    p.push(cpu_v3::immediate_unsigned(crate::ImmediateOp::Add, 12, 0)); // r12 = 8

    // Non-halting SIGNAL types retire as a NOP in the RTL; both special
    // registers are read and DSEG is rewritten with its current value.
    p.push(cpu_v3::signal(6, 1));
    p.push(cpu_v3::signal(7, 15));
    p.push(cpu_v3::read_special(
        11,
        crate::SpecialRegister::DataSegment,
    ));
    p.push(cpu_v3::write_data_segment(11));
    p.push(cpu_v3::read_special(
        12,
        crate::SpecialRegister::CodeSegment,
    ));

    // JALREL links the fixed r14 and JREG returns through it; the trailing
    // JREL skips the subroutine body once control comes back.
    p.push(cpu_v3::jump_and_link_relative(2)); // r14 = next word, pc = +3
    p.push(cpu_v3::nop()); // return point
    p.push(cpu_v3::jump_relative(1)); // skip the JREG subroutine body
    p.push(cpu_v3::jump_register(cpu_v3::LINK_REGISTER)); // subroutine body: return

    // JALR loads an absolute target, links r14, jumps, and returns through
    // JREG. The subroutine sits after the final halt so fall-through never
    // reaches it.
    let subroutine = (p.len() + 5) as u16;
    p.extend(cpu_v3::load_immediate16(13, subroutine));
    p.push(cpu_v3::jump_and_link_register(13));
    p.push(cpu_v3::alu(crate::AluOp::Add, 0, 2, 3));
    // Fold r12 (= 8 from the LDC/ADDI chain above) into the halt signal so
    // the constant-table and unsigned-immediate results are observed.
    p.push(cpu_v3::alu(crate::AluOp::Add, 0, 0, 12));
    p.push(cpu_v3::halt());
    p.push(cpu_v3::jump_register(cpu_v3::LINK_REGISTER));
    p
}

fn run_core_emu_trace(program: &[u16], max_cycles: usize) -> Vec<CoreCosimOut> {
    let mut memory = vec![0u16; 65536];
    for (index, word) in program.iter().copied().enumerate() {
        memory[index] = word;
    }
    let (mut circuit, (input, output)) = build_circuit(|| {
        let input = CpuV3CoreInput::allocate();
        let output = CpuV3Core::emu(&input);
        (input, output)
    });
    let mut prev_instr_req = false;
    let mut prev_instr_word: u16 = 0;
    let mut prev_data_req = false;
    let mut prev_data_read: u16 = 0;
    let mut started = false;
    let mut trace = Vec::new();
    for _ in 0..max_cycles {
        let cin = CoreCosimIn {
            reset: false,
            instruction_request_ready: true,
            instruction_response_valid: prev_instr_req,
            instruction_data: prev_instr_word,
            data_request_ready: true,
            data_response_valid: prev_data_req,
            data_read_data: prev_data_read,
            device_read_data: 0,
        };
        input.drive(&mut circuit, &cin.into_value());
        circuit.execute_gates();
        let value = output.sample(&circuit);
        if started || value.instruction_request_valid {
            started = true;
            trace.push(CoreCosimOut::from(&value));
            if value.halted || value.fault {
                break;
            }
        }
        prev_instr_req = value.instruction_request_valid;
        if value.instruction_request_valid {
            prev_instr_word = memory[(value.instruction_address as usize) & 0xffff];
        }
        prev_data_req = value.data_request_valid;
        if value.data_request_valid {
            let address = (value.data_address as usize) & 0xffff;
            if value.data_write {
                memory[address] = value.data_write_data as u16;
                prev_data_read = 0;
            } else {
                prev_data_read = memory[address];
            }
        }
        circuit.clock_tick();
    }
    trace
}

/// Generates a testbench that runs the RTL core with the same 1-cycle
/// instruction/data memory semantics used by `run_core_emu_trace`, resets,
/// then records the same curated output set each cycle from the first
/// emitted instruction request until halt/fault.
fn build_core_cosim_tb(program: &[u16], module_name: &str, max_cycles: usize) -> String {
    let mut t = format!(
        "module tb;\n\
             reg clk = 0;\n\
             reg reset = 1;\n\
             reg hold = 0;\n\
             reg instruction_request_ready = 1;\n\
             reg instruction_response_valid = 0;\n\
             reg [15:0] instruction_data = 0;\n\
             reg instruction_error = 0;\n\
             reg data_request_ready = 1;\n\
             reg data_response_valid = 0;\n\
             reg [15:0] data_read_data = 0;\n\
             reg data_error = 0;\n\
             wire [15:0] device_read_data;\n\
             wire instruction_request_valid;\n\
             wire [31:0] instruction_address;\n\
             wire instruction_response_ready;\n\
             wire data_request_valid;\n\
             wire data_write;\n\
             wire [31:0] data_address;\n\
             wire [15:0] data_write_data;\n\
             wire data_response_ready;\n\
             wire [2:0] device_index;\n\
             wire [3:0] device_channel;\n\
             wire device_read_enable;\n\
             wire device_write_enable;\n\
             wire [15:0] device_write_data;\n\
             wire halted;\n\
             wire [15:0] halt_signal;\n\
             wire fault;\n\
             wire [7:0] fault_code;\n\
             wire [15:0] fault_pc;\n\
             wire [15:0] pc;\n\
             wire [15:0] code_segment;\n\
             wire [15:0] data_segment;\n\
             wire [31:0] retired_words;\n\n\
             {module_name} dut(.*);\n\
             always #5 clk = ~clk;\n\n\
             reg [15:0] memory [0:65535];\n\
             reg [15:0] devices [0:127];\n\
             assign device_read_data = devices[{{device_index, device_channel}}];\n\
             integer index;\n\
             integer cycles;\n\
             reg started;\n\
             reg end_flag;\n\n\
             always @(posedge clk) begin\n\
                 instruction_response_valid <= instruction_request_valid;\n\
                 if (instruction_request_valid)\n\
                     instruction_data <= memory[instruction_address[15:0]];\n\
                 data_response_valid <= data_request_valid;\n\
                 if (data_request_valid && data_write)\n\
                     memory[data_address[15:0]] <= data_write_data;\n\
                 else if (data_request_valid)\n\
                     data_read_data <= memory[data_address[15:0]];\n\
             end\n\n\
             initial begin\n",
    );
    for (index, word) in program.iter().copied().enumerate() {
        t.push_str(&format!("    memory[{index}] = 16'h{word:04x};\n"));
    }
    t.push_str(&format!(
            "    for (index = 0; index < 128; index = index + 1) devices[index] = 16'h0000;\n\
             repeat (3) @(posedge clk);\n\
             #1 reset = 0;\n\
             cycles = 0;\n\
             started = 0;\n\
             end_flag = 0;\n\
             while (cycles < {max_cycles} && !end_flag) begin\n\
                 #1;\n\
                 if (started || instruction_request_valid)\n\
                     started = 1;\n\
                 if (started) begin\n\
                     $display(\"CORE %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d\", cycles, pc, code_segment, data_segment, retired_words, halted, halt_signal, fault, fault_code, fault_pc, instruction_request_valid, instruction_address, instruction_response_ready, data_request_valid, data_write, data_address, data_write_data, data_response_ready);\n\
                     $display(\"STATE %0d %0d %0d\", cycles, dut.state, dut.fpu2_busy);\n\
                     if (halted || fault) end_flag = 1;\n\
                 end\n\
                 @(posedge clk);\n\
                 cycles = cycles + 1;\n\
             end\n\
             $display(\"TRACE_END\");\n\
             $finish;\n\
             end\n\
              initial begin\n\
                  repeat ({timeout_cycles}) @(posedge clk);\n\
                  $display(\"TIMEOUT\");\n\
                  $finish(1);\n\
              end\n\
              endmodule\n",
            timeout_cycles = max_cycles * 5 + 500
        ));
    t
}

fn collect_verilog_files(directory: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_verilog_files(&path, into);
        } else if path.extension().and_then(|value| value.to_str()) == Some("v")
            && path.file_name().and_then(|value| value.to_str()) != Some("tb.v")
        {
            into.push(path);
        }
    }
}

fn run_core_rtl_trace(tb: &str) -> Vec<CoreCosimOut> {
    let directory = std::env::temp_dir().join(format!("core-cosim-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    // Write the core with every dependency (DSP multiplier, GPR RAM)
    // via the same flattening used by `verify_verilog_with_iverilog`, then
    // add our replay testbench.
    let project = VerilogProject::generate::<CpuV3Core>().unwrap();
    project.write_to(&directory).unwrap();
    std::fs::write(directory.join("tb.v"), tb).unwrap();
    let iverilog = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let vvp = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let output_path = directory.join("sim.vvp");
    // The generated project writes dependency files under nested paths; walk
    // the tree to gather every module source.
    let mut module_paths = Vec::new();
    collect_verilog_files(&directory, &mut module_paths);
    module_paths.push(directory.join("tb.v"));
    let mut compile = std::process::Command::new(&iverilog);
    compile
        .current_dir(&directory)
        .args(["-g2005", "-s", "tb", "-o"])
        .arg(&output_path);
    for path in module_paths {
        compile.arg(&path);
    }
    let compile_output = compile.output().unwrap();
    assert!(
        compile_output.status.success(),
        "iverilog compile failed:\n{}",
        String::from_utf8_lossy(&compile_output.stderr)
    );
    let simulation = std::process::Command::new(&vvp)
        .current_dir(&directory)
        .arg(&output_path)
        .output()
        .unwrap();
    assert!(
        simulation.status.success(),
        "vvp failed:\n{}",
        String::from_utf8_lossy(&simulation.stderr)
    );
    let stdout = String::from_utf8_lossy(&simulation.stdout);
    let mut trace = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("CORE ") {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            assert_eq!(fields.len(), 18, "unexpected CORE line: {line}");
            let num = |i: usize| fields[i].parse().unwrap_or(0);
            trace.push(CoreCosimOut {
                pc: num(1) as u16,
                code_segment: num(2) as u16,
                data_segment: num(3) as u16,
                retired_words: num(4) as u32,
                halted: num(5) == 1,
                halt_signal: num(6) as u16,
                fault: num(7) == 1,
                fault_code: num(8) as u8,
                fault_pc: num(9) as u16,
                instruction_request_valid: num(10) == 1,
                instruction_address: num(11) as u32,
                instruction_response_ready: num(12) == 1,
                data_request_valid: num(13) == 1,
                data_write: num(14) == 1,
                data_address: num(15) as u32,
                data_write_data: num(16) as u16,
                data_response_ready: num(17) == 1,
            });
        } else if line == "TRACE_END" {
            break;
        }
    }
    if std::env::var_os("FPU2_KEEP_COSIM_DIR").is_none() {
        std::fs::remove_dir_all(&directory).ok();
    }
    trace
}

#[test]
fn core_cosim_program_halts_in_the_simulators() {
    // The ignored emu/RTL co-simulation only compares well-defined runs, so
    // keep its program valid, in-range, and terminating. This checks the
    // program against the architectural oracle and the Rust cycle model
    // (the emu half of the co-sim) without needing Icarus, and pins the
    // cycle count below the emu trace cap used by the co-sim.
    let program = core_cosim_program();
    let mut machine = CpuV3Sim::with_physical_memory_words(1 << 16);
    machine.load_program(0, &program).unwrap();
    assert!(matches!(machine.run(10_000), Ok(RunOutcome::Halted { .. })));

    let emu = run_core_emu_trace(&program, 10_000);
    let last = emu.last().copied().expect("emu trace empty");
    assert!(!last.fault, "co-sim program faulted in the cycle model");
    assert!(
        last.halted,
        "co-sim program did not halt in the cycle model"
    );
    assert!(
        emu.len() < 2000,
        "co-sim program needs {} cycles, above the co-sim emu trace cap",
        emu.len()
    );
}

/// FPU v2 back-to-back FLD after an async store: the reproducer for the
/// one-cycle request slip seen at system level.
#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of FPU v2 memory beats"]
fn core_emu_matches_rtl_fpu_ldst() {
    // r1=0x0100, r2=0x0102; store r0=7 to [r1]; FLD f1,[r1]; FLD f2,[r2]; HALT
    let program = vec![
        0xf010, 0xa310, // r1 = 0x0100
        0xf010, 0xa322, // r2 = 0x0102
        0xa007, // ADDI r0, 7
        0x9010, // STORE r0, [r1] (async store buffer)
        0xe100, 0x0400, // FLD f1, [r1]
        0xe200, 0x0800, // FLD f2, [r2]
        0xd041, 0x0c20, // MUL f3 = f1 * f2
        0xc042, 0x1018, // VMULS.2 f4..f5 = f1..f2 * f2
        0xc042, 0x0068, // DOT.2 ACC = f1*f1 + f2*f2 = 1 + 4 = 5.0
        0xc042, 0x0070, // DOTADD.2: ACC = 10.0
        0xc042, 0x1878, // DOTSTORE.2 f6 = 5.0, ACC cleared
        0x6c00, // HALT
    ];
    let module_name = CpuV3Core::verilog_identity().module_name();
    let emu = run_core_emu_trace(&program, 2000);
    let max_cycles = emu.len() + 400;
    let tb = build_core_cosim_tb(&program, &module_name, max_cycles);
    let rtl = run_core_rtl_trace(&tb);
    if emu.len() != rtl.len() {
        let dump = |trace: &[CoreCosimOut]| {
            trace
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    format!(
                        "{i}: pc={} retired={} ireq={} iaddr={:#06x} dreq={} dw={} daddr={:#06x}",
                        v.pc,
                        v.retired_words,
                        v.instruction_request_valid,
                        v.instruction_address,
                        v.data_request_valid,
                        v.data_write,
                        v.data_address
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        panic!(
            "length mismatch emu={} rtl={}\n--- emu ---\n{}\n--- rtl ---\n{}",
            emu.len(),
            rtl.len(),
            dump(&emu),
            dump(&rtl)
        );
    }
    for (index, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
        assert!(
            actual.equal_core(expected),
            "mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}"
        );
    }
    assert!(emu.last().copied().expect("emu trace empty").halted);
}

#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of a compiled scalar FPU program"]
fn core_emu_matches_rtl_compiled_scalar_fpu_program() {
    // The rcc scalar `fix16` path end to end on the hardware-supported subset:
    // integer construction (`I16TOF`), calls through the F-register ABI, an
    // FPU `ADD`/`MUL`/`trunc`, the raw-half bridge (`ILO2F`/`IHI2F`), a scalar
    // `CMP` feeding a GPR branch, the signed numeric conversion back
    // (`FTOI16`, truncating toward zero), and values kept live across a call so
    // they spill through FLD/FST:
    // a=7, b=-2, addfix(a,b)=5, scaled=-14, kept=1.5, down=1, pick=1,
    // live=-13, (5 + -13 + 0.5).to_int() = -7 -> 0xfff9.
    let source = r#"
        fn addfix(a: fix16, b: fix16) -> fix16 { a + b }
        fn main() {
            let a = fix16::from_int(7);
            let b = fix16::from_int(-2);
            let s = addfix(a, b);
            let scaled = a * b;
            let kept = fix16::from_words(0x8000, 0x0001);
            let down = kept.trunc();
            let pick = if s < down { s } else { down };
            let live = addfix(scaled, pick);
            halt((s + live + (kept - down)).to_int() as u16);
        }
    "#;
    let program = compile(source);
    let module_name = CpuV3Core::verilog_identity().module_name();
    let emu = run_core_emu_trace(&program, 4000);
    assert!(!emu.is_empty(), "emu trace empty");
    let last_emu = emu.last().copied().expect("emu trace non-empty");
    assert!(
        !last_emu.fault,
        "compiled FPU program faulted in the cycle model: code={} pc={:#06x}",
        last_emu.fault_code, last_emu.fault_pc
    );
    assert!(last_emu.halted, "compiled FPU program did not halt");
    assert_eq!(last_emu.halt_signal, 0xfff9, "unexpected Q16.16 result");

    let max_cycles = emu.len() + 600;
    let tb = build_core_cosim_tb(&program, &module_name, max_cycles);
    let rtl = run_core_rtl_trace(&tb);
    // Report the first divergence (if any) before the length check, so a timing
    // slip points at the exact cycle.
    for (index, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
        if !actual.equal_core(expected) {
            panic!("mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}");
        }
    }
    assert_eq!(
        emu.len(),
        rtl.len(),
        "emu/RTL trace length mismatch: emu={} rtl={}",
        emu.len(),
        rtl.len()
    );
    assert_eq!(
        rtl.last().copied().expect("rtl trace empty").halt_signal,
        0xfff9
    );
}

#[test]
#[ignore = "explicit emulator-vs-Icarus co-simulation of the CpuV3 core pipeline"]
fn core_emu_matches_rtl_pipeline_overlap() {
    let program = core_cosim_program();
    let module_name = CpuV3Core::verilog_identity().module_name();
    let emu = run_core_emu_trace(&program, 2000);
    // A dependent-immediate chain plus a device handshake need a bounded
    // but generous window.
    let max_cycles = emu.len() + 400;
    let tb = build_core_cosim_tb(&program, &module_name, max_cycles);
    let rtl = run_core_rtl_trace(&tb);
    assert!(
        emu.len() == rtl.len(),
        "emu/RTL trace length mismatch: emu={} rtl={}\nemu={:?}\nrtl={:?}",
        emu.len(),
        rtl.len(),
        emu.iter().map(|v| v.pc).collect::<Vec<_>>(),
        rtl.iter().map(|v| v.pc).collect::<Vec<_>>()
    );
    for (index, (expected, actual)) in emu.iter().zip(&rtl).enumerate() {
        assert!(
            actual.equal_core(expected),
            "emu/RTL core mismatch at cycle {index}\nemu={expected:?}\nrtl={actual:?}"
        );
    }
    let last_emu = emu.last().copied().expect("emu trace empty");
    assert!(last_emu.halted, "core co-sim must end in halt");
}

#[test]
#[ignore = "explicit external simulation of the scalar register file"]
fn verify_gpr_ram_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3GprRam>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 register file"]
fn verify_fpu_register_ram_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuRegisterRam>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 instruction front-end"]
fn verify_fpu_frontend_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuFrontend>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 scalar ALU"]
fn verify_fpu_scalar_alu_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuScalarAlu>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 scalar path"]
fn verify_fpu_scalar_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuScalarPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 special-function path"]
fn verify_fpu_special_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuSpecialPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 unit top"]
fn verify_fpu_unit_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3Fpu>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 vector path"]
fn verify_fpu_vector_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuVectorPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 multiply path"]
fn verify_fpu_multiply_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuMultiplyPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the FPU v2 dot path"]
fn verify_fpu_dot_path_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuDotPath>().unwrap();
}

#[test]
#[ignore = "explicit external simulation of the shared FPU v2 multiply pipe"]
fn verify_fpu_mul_pipe_with_iverilog() {
    digital_design_hardware::verify_verilog_with_iverilog::<CpuV3FpuMulPipe>().unwrap();
}
