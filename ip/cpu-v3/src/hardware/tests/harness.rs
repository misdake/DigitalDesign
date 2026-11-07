use super::*;

pub(super) struct CoreRun {
    pub(super) halt_signal: u16,
    pub(super) retired_words: u32,
    pub(super) code_segment: u16,
    pub(super) data_segment: u16,
}

pub(super) fn drive(
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
            data_line_copy_ready: true,
            data_cache_command_error: false,
            device_read_data: u64::from(device_read_data),
        },
    );
}

pub(super) fn run_core(mut memory: HashMap<u32, u16>, maximum_cycles: usize) -> CoreRun {
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

pub(super) fn load(memory: &mut HashMap<u32, u16>, base: u32, words: &[u16]) {
    for (offset, word) in words.iter().copied().enumerate() {
        memory.insert(base + offset as u32, word);
    }
}

pub(super) fn compile(source: &str) -> Vec<u16> {
    let options = CompilerOptions::default();
    let frontend = compile_program(source, &options, &mut |_| {
        Err("test source does not use modules".to_string())
    })
    .unwrap();
    rcc_backend::compile(frontend, &options, "main").words
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CoreCosimIn {
    pub(super) reset: bool,
    pub(super) instruction_request_ready: bool,
    pub(super) instruction_response_valid: bool,
    pub(super) instruction_data: u16,
    pub(super) data_request_ready: bool,
    pub(super) data_response_valid: bool,
    pub(super) data_read_data: u16,
    pub(super) device_read_data: u16,
}

impl CoreCosimIn {
    pub(super) fn into_value(self) -> CpuV3CoreInputValue {
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
            data_line_copy_ready: true,
            data_cache_command_error: false,
            device_read_data: u64::from(self.device_read_data),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CoreCosimOut {
    pub(super) pc: u16,
    pub(super) code_segment: u16,
    pub(super) data_segment: u16,
    pub(super) retired_words: u32,
    pub(super) halted: bool,
    pub(super) halt_signal: u16,
    pub(super) fault: bool,
    pub(super) fault_code: u8,
    pub(super) fault_pc: u16,
    pub(super) instruction_request_valid: bool,
    pub(super) instruction_address: u32,
    pub(super) instruction_response_ready: bool,
    pub(super) data_request_valid: bool,
    pub(super) data_write: bool,
    pub(super) data_address: u32,
    pub(super) data_write_data: u16,
    pub(super) data_response_ready: bool,
}

impl CoreCosimOut {
    /// `halt_signal` is now a registered value latched at the HALT retire
    /// edge (mirrored by the emulator), so it is a stable architectural
    /// property of every cycle after reset and is compared directly.
    pub(super) fn equal_core(&self, other: &Self) -> bool {
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
pub(super) fn core_cosim_program() -> Vec<u16> {
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

pub(super) fn run_core_emu_trace(program: &[u16], max_cycles: usize) -> Vec<CoreCosimOut> {
    run_core_emu_trace_with_hold(program, max_cycles, |_| false)
}

pub(super) fn run_core_emu_trace_with_hold(
    program: &[u16],
    max_cycles: usize,
    hold: impl Fn(usize) -> bool,
) -> Vec<CoreCosimOut> {
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
    for cycle in 0..max_cycles {
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
        let mut cin = cin.into_value();
        cin.hold = hold(cycle);
        input.drive(&mut circuit, &cin);
        circuit.execute_gates();
        let value = output.sample(&circuit);
        if started || value.instruction_request_valid {
            started = true;
            trace.push(CoreCosimOut::from(&value));
            if value.halted || value.fault {
                break;
            }
        }
        if !cin.hold {
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
        }
        circuit.clock_tick();
    }
    trace
}

/// Generates a testbench that runs the RTL core with the same 1-cycle
/// instruction/data memory semantics used by `run_core_emu_trace`, resets,
/// then records the same curated output set each cycle from the first
/// emitted instruction request until halt/fault.
pub(super) fn build_core_cosim_tb(program: &[u16], module_name: &str, max_cycles: usize) -> String {
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
             reg data_line_copy_ready = 1;\n\
             reg data_cache_command_error = 0;\n\
             wire [15:0] device_read_data;\n\
             wire instruction_request_valid;\n\
             wire [31:0] instruction_address;\n\
             wire instruction_response_ready;\n\
             wire data_request_valid;\n\
             wire data_write;\n\
             wire [31:0] data_address;\n\
             wire [15:0] data_write_data;\n\
             wire data_response_ready;\n\
             wire data_line_copy_valid;\n\
             wire [21:0] data_line_copy_source;\n\
             wire data_line_clean_valid;\n\
             wire [21:0] data_line_clean_address;\n\
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

pub(super) fn collect_verilog_files(
    directory: &std::path::Path,
    into: &mut Vec<std::path::PathBuf>,
) {
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

pub(super) fn create_core_cosim_directory(
    next: &AtomicUsize,
) -> std::io::Result<std::path::PathBuf> {
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&target)?;
    loop {
        let directory = target.join(format!(
            "core-cosim-{}-{}",
            std::process::id(),
            next.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn run_core_rtl_trace(tb: &str) -> Vec<CoreCosimOut> {
    // Retain the generated project until task cleanup, including failures in
    // the caller's later emulator comparison. Atomically reserve a fresh name
    // so concurrent tests and a reused process ID cannot overwrite a replay.
    static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);
    let directory = create_core_cosim_directory(&NEXT_DIR).unwrap();
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
    if let Some(path) = std::env::var_os("SHARED_WIDE_TRACE_FILE") {
        std::fs::write(path, simulation.stdout.as_slice()).unwrap();
    }
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
    trace
}

/// Source for the v3 geometry tests: one call to each release helper, folded
/// into a single halt signal through the raw-half bridge. The lanes are integer
/// literals so each case compiles a fresh, bounded program.
pub(super) fn geometry_helper_source(ax: i16, ay: i16, bx: i16, by: i16, threshold: i16) -> String {
    format!(
        r#"
    fn main() {{
        let a = vec3::new(fix32::from_int({ax}), fix32::from_int({ay}), fix32::zero());
        let b = vec3::new(fix32::from_int({bx}), fix32::from_int({by}), fix32::zero());
        let s = v3_length2(a);
        let n = v3_normalize(a);
        let gt = v3_distance_gt(a, b, fix32::from_int({threshold}));
        halt(s.lo_bits() ^ (s.hi_bits() << 1)
             ^ (n.x().lo_bits() << 2) ^ (n.y().hi_bits() << 3)
             ^ ((gt as u16) << 4));
    }}
"#
    )
}

/// The same calls through the `_checked` debug helpers.
pub(super) fn geometry_checked_source(
    ax: i16,
    ay: i16,
    bx: i16,
    by: i16,
    threshold: i16,
) -> String {
    format!(
        r#"
    fn main() {{
        let a = vec3::new(fix32::from_int({ax}), fix32::from_int({ay}), fix32::zero());
        let b = vec3::new(fix32::from_int({bx}), fix32::from_int({by}), fix32::zero());
        let s = v3_length2_checked(a);
        let n = v3_normalize_checked(a);
        let gt = v3_distance_gt_checked(a, b, fix32::from_int({threshold}));
        halt(s.lo_bits() ^ (s.hi_bits() << 1)
             ^ (n.x().lo_bits() << 2) ^ (n.y().hi_bits() << 3)
             ^ ((gt as u16) << 4));
    }}
"#
    )
}

/// The CPU V3 reference model result for [`geometry_helper_source`]. The target
/// sequence and this model share `DOTSTORE`, `RSQRT`, `VMULS`, `VSUB` and the
/// scalar `CMP`, so they must agree bit for bit.
pub(super) fn geometry_expected_signal(ax: i32, ay: i32, bx: i32, by: i32, threshold: i32) -> u16 {
    let a = [ax << 16, ay << 16, 0];
    let b = [bx << 16, by << 16, 0];
    let s = cpu_v3::v3_length2(a);
    let n = cpu_v3::v3_normalize(a);
    let gt = cpu_v3::v3_distance_gt(a, b, threshold << 16);
    (s as u16)
        ^ (((s as u32 >> 16) as u16) << 1)
        ^ ((n[0] as u16) << 2)
        ^ (((n[1] as u32 >> 16) as u16) << 3)
        ^ (u16::from(gt) << 4)
}

/// Cases inside the small-range contract: every `a` component is within
/// `[-104, +104]`, every input component is within `[-16384, +16383]`, and
/// every difference component is within `[-104, +104]`.
pub(super) const GEOMETRY_CASES: &[(i16, i16, i16, i16, i16)] = &[
    (3, 4, 1, 1, 2),
    (100, -100, 90, -90, 100),
    (-104, 104, 0, 0, 50),
    (0, 0, 0, 0, 0),
    (1, 2, 1, 2, -5),
];

/// Runs a compiled program on the architectural simulator and returns its halt
/// signal, failing if it faults or exceeds the bounded step count.
pub(super) fn run_program_signal(program: &[u16], maximum_steps: usize) -> u16 {
    let mut machine = CpuV3Sim::default();
    machine.load_program(0, program).unwrap();
    let outcome = machine.run(maximum_steps).unwrap();
    let RunOutcome::Halted { signal, .. } = outcome else {
        panic!("program did not halt in {maximum_steps} steps")
    };
    signal
}
