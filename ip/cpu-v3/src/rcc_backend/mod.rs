//! CpuV3 lowering and symbolic linking with iterative branch relaxation.
//!
//! Control-flow references start in the two-word wide form (PFX12 + relative)
//! and the linker shrinks any branch, jump, or call whose final offset fits
//! the signed 8-bit relative encoding to one word, iterating until the layout
//! is stable.

mod options;

pub use options::CompilerOptions;

use crate as cpu_v3;
use crate::{
    AluOp, FpuAuxKind, FpuAuxSubop, FpuScalarSubop, ImmediateOp, SpecialRegister, TestCondition,
    Word,
};
use crate::{CACHE_MAINTENANCE_DEVICE, D_INVALIDATE_ALL, ICACHE_INVALIDATE_ALL_DELAYED};
use rcc::*;
use std::collections::{HashMap, HashSet};

const REG_TMP: u8 = 12;
const REG_SP: u8 = 13;
const REG_LINK: u8 = 14;
/// Reserved FPU register for breaking parallel-move cycles (never allocated).
/// The FPU v2 convention reserves `F63` as the parallel-move scratch.
const FPU_SCRATCH: u8 = 63;

/// FPU v2 argument registers: `F4..F27`, i.e. six `vec4` values placed
/// compactly after the `F0..F3` return area (design `fpu-design-v2` todo C0).
const FPU_ARGUMENT_REGISTERS: &[u8] = &[
    4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
];

/// FPU v2 ordinary allocation area: `F28..F62`. `F0..F3` are reserved for
/// returns, `F4..F27` for arguments, and `F63` for parallel-move cycles.
const FPU_ALLOCATABLE_REGISTERS: &[u8] = &[
    28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51,
    52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62,
];

const CPU_V3_REGISTER_CONVENTION: rcc::RegisterConvention = rcc::RegisterConvention {
    return_registers: &[0, 1],
    argument_registers: &[2, 3, 4, 5, 6, 7],
    allocatable_registers: &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 15],
    caller_saved: &[0, 1, 2, 3, 4, 5, 6, 7, 15],
    callee_saved: &[8, 9, 10, 11],
    link_register: REG_LINK,
    stack_register: REG_SP,
    temporary_register: REG_TMP,
    maximum_frame_words: 255,
    // FPU v2 ABI (Q16.16, contiguous scalar ranges): F0..F3 returns, F4..F27
    // arguments placed compactly, F28..F62 ordinary allocation, F63 the
    // parallel-move scratch. All FPU registers are caller-saved and ACC is
    // caller-clobbered. C0 freezes this layout; range-aware allocation for
    // vec2/vec3/vec4 values lands with C2.
    fpu: Some(rcc::FpuRegisterConvention {
        return_registers: &[0, 1, 2, 3],
        argument_registers: FPU_ARGUMENT_REGISTERS,
        allocatable_registers: FPU_ALLOCATABLE_REGISTERS,
        scratch_register: FPU_SCRATCH,
    }),
};

#[derive(Clone, Debug)]
pub struct CpuV3Program {
    pub code_base: Word,
    pub words: Vec<Word>,
    pub listing: String,
    pub debug: rcc::DebugInfo,
}

#[derive(Clone)]
enum Line {
    Word {
        word: Word,
        line: Option<u32>,
    },
    Label(usize),
    Branch {
        condition: TestCondition,
        target: usize,
        line: Option<u32>,
        /// one-word i8 relative form selected by relaxation
        short: bool,
    },
    Jump {
        target: usize,
        line: Option<u32>,
        short: bool,
    },
    Call {
        function: FuncName,
        line: Option<u32>,
        short: bool,
    },
    LoadFunctionAddress {
        function: FuncName,
        dst: u8,
        line: Option<u32>,
    },
}

impl Line {
    fn size(&self) -> usize {
        match self {
            Self::Word { .. } => 1,
            Self::Label(_) => 0,
            Self::Branch { short, .. } | Self::Jump { short, .. } | Self::Call { short, .. } => {
                if *short {
                    1
                } else {
                    2
                }
            }
            Self::LoadFunctionAddress { .. } => 2,
        }
    }
}

/// Lowers instructions into `Line`s while stamping each with the source line
/// of the instruction that produced it (for the debug line map). `None` marks
/// compiler-generated code with no source ownership.
struct Lines {
    items: Vec<Line>,
    cur_line: Option<u32>,
}

impl Lines {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            cur_line: None,
        }
    }

    fn set_line(&mut self, line: Option<u32>) {
        self.cur_line = line;
    }

    fn len(&self) -> usize {
        self.items.len()
    }

    fn label(&mut self, block: usize) {
        self.items.push(Line::Label(block));
    }

    fn word(&mut self, word: Word) {
        self.items.push(Line::Word {
            word,
            line: self.cur_line,
        });
    }

    fn branch(&mut self, condition: TestCondition, target: usize) {
        self.items.push(Line::Branch {
            condition,
            target,
            line: self.cur_line,
            short: false,
        });
    }

    fn jump(&mut self, target: usize) {
        self.items.push(Line::Jump {
            target,
            line: self.cur_line,
            short: false,
        });
    }

    fn call(&mut self, function: FuncName) {
        self.items.push(Line::Call {
            function,
            line: self.cur_line,
            short: false,
        });
    }

    fn load_function_address(&mut self, function: FuncName, dst: u8) {
        self.items.push(Line::LoadFunctionAddress {
            function,
            dst,
            line: self.cur_line,
        });
    }
}

struct LoweredFunction {
    name: FuncName,
    lines: Vec<Line>,
    static_addresses: Vec<u16>,
    frame_size: usize,
    callee_saved: usize,
}

/// An error the CpuV3 backend reports after the frontend has accepted the
/// source program: a missing function, or a linked image that violates an
/// encoding requirement. Callers that want to display a compiler error should
/// use [`try_compile`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendError {
    /// A referenced function was not provided by the frontend.
    UnknownFunction(String),
    /// The linked code image violated an encoding requirement.
    Validation(ProgramValidationError),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownFunction(name) => write!(f, "unknown function `{name}`"),
            Self::Validation(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BackendError {}

impl From<ProgramValidationError> for BackendError {
    fn from(error: ProgramValidationError) -> Self {
        Self::Validation(error)
    }
}

/// Compile target-independent RCC IR for the CpuV3 ABI and ISA, panicking on a
/// backend error. Compiler front ends should call [`try_compile`] and report
/// the error instead.
pub fn compile(
    program: rcc::frontend::Program,
    options: &CompilerOptions,
    main: FuncName,
) -> CpuV3Program {
    try_compile(program, options, main).unwrap_or_else(|error| panic!("{error}"))
}

/// Fallible form of [`compile`]: returns a [`BackendError`] for a missing
/// function or an invalid code image so the caller can display it.
pub fn try_compile(
    program: rcc::frontend::Program,
    options: &CompilerOptions,
    main: FuncName,
) -> Result<CpuV3Program, BackendError> {
    let debug = program.debug;
    let functions = program
        .funcs
        .into_iter()
        .map(|function| (function.name, function))
        .collect();
    compile_ir(functions, options, main, debug)
}

fn compile_ir(
    functions: HashMap<FuncName, IrFunc>,
    options: &CompilerOptions,
    main: FuncName,
    debug: rcc::frontend::FrontendDebug,
) -> Result<CpuV3Program, BackendError> {
    let reachable = reachable_functions(&functions, main)?;
    let mut order = vec![main];
    let mut rest = reachable
        .iter()
        .copied()
        .filter(|name| *name != main)
        .collect::<Vec<_>>();
    rest.sort_unstable();
    order.extend(rest);

    let mut lowered = Vec::with_capacity(order.len());
    for name in order {
        let mut function = functions
            .get(name)
            .ok_or_else(|| BackendError::UnknownFunction(name.to_string()))?
            .clone();
        // Safe if-conversion (CpuV3): simple one-instruction diamonds become
        // Boolean comparisons or conditional moves. It changes the CFG and
        // debugger stepping shape, so it only runs with optimizations enabled;
        // it runs before the optimizer's constant-arm hoisting, which would
        // otherwise dissolve the diamond shape.
        if !options.opt.is_disabled() {
            convert_diamonds(&mut function);
        }
        optimize(&mut function, &options.opt);
        let (function, mut allocation) =
            allocate_with_convention(&function, options.opt.coalesce, CPU_V3_REGISTER_CONVENTION);
        if name == main {
            allocation.callee_saved.clear();
        }
        lowered.push(lower_function(
            &function,
            &allocation,
            name == main,
            options.stack_init,
        ));
    }

    link(lowered, options, debug).map_err(BackendError::from)
}

fn reachable_functions(
    functions: &HashMap<FuncName, IrFunc>,
    main: FuncName,
) -> Result<HashSet<FuncName>, BackendError> {
    let mut reachable = HashSet::from([main]);
    let mut work = vec![main];
    while let Some(name) = work.pop() {
        let function = functions
            .get(name)
            .ok_or_else(|| BackendError::UnknownFunction(name.to_string()))?;
        for block in function.rpo() {
            for instruction in &function.blocks[block].insts {
                if let Instr::Call { func, .. } | Instr::LoadFuncAddr { func, .. } = instruction {
                    if !functions.contains_key(func) {
                        return Err(BackendError::UnknownFunction(func.to_string()));
                    }
                    if reachable.insert(*func) {
                        work.push(*func);
                    }
                }
            }
        }
    }
    Ok(reachable)
}

fn lower_function(
    function: &IrFunc,
    allocation: &Allocation,
    is_main: bool,
    stack_init: u16,
) -> LoweredFunction {
    let mut lines = Lines::new();
    let mut static_addresses = vec![];
    let register = |vreg: VReg| allocation.reg[&vreg];

    // prologue is compiler-generated (no source line)
    lines.set_line(None);
    if is_main && stack_init != 0 {
        emit_load_immediate(&mut lines, REG_SP, stack_init);
    }
    if allocation.frame_size() != 0 {
        emit_immediate(
            &mut lines,
            ImmediateOp::Sub,
            REG_SP,
            allocation.frame_size() as u16,
            true,
        );
        for (slot, &register) in allocation.callee_saved.iter().enumerate() {
            emit_store(&mut lines, register, REG_SP, slot as i16);
        }
    }

    let layout = function.rpo();
    for (layout_index, &block_id) in layout.iter().enumerate() {
        lines.label(block_id);
        let block = &function.blocks[block_id];
        if block.preds.len() == 1 && !block.phis.is_empty() {
            let predecessor = block.preds[0];
            let moves = block
                .phis
                .iter()
                .map(|phi| {
                    let value = phi
                        .args
                        .iter()
                        .find(|(pred, _)| *pred == predecessor)
                        .expect("missing phi predecessor")
                        .1;
                    (
                        register(value),
                        register(phi.dst),
                        function.class_of(phi.dst),
                    )
                })
                .collect::<Vec<_>>();
            lines.set_line(None);
            emit_parallel_moves(&mut lines, &moves);
        }

        for (index, instruction) in block.insts.iter().enumerate() {
            lines.set_line(block.lines.get(index).copied().flatten());
            lower_instruction(
                function,
                instruction,
                &register,
                allocation,
                &mut lines,
                &mut static_addresses,
            );
        }

        lines.set_line(block.term_line);
        match block
            .term
            .as_ref()
            .expect("reachable block is unterminated")
        {
            Terminator::Jmp { target } => {
                emit_edge_moves(function, block_id, *target, &register, &mut lines);
                if layout.get(layout_index + 1) != Some(target) {
                    lines.jump(*target);
                }
            }
            Terminator::Br {
                cmp,
                if_true,
                if_false,
            } => {
                let next = layout.get(layout_index + 1).copied();
                if if_true == if_false {
                    emit_edge_moves(function, block_id, *if_true, &register, &mut lines);
                    if next != Some(*if_true) {
                        lines.jump(*if_true);
                    }
                } else {
                    // Critical edges were split by allocation, so neither
                    // conditional successor needs edge-local phi moves here.
                    let condition = lower_comparison(function, cmp, &register, &mut lines);
                    if next == Some(*if_false) {
                        lines.branch(condition, *if_true);
                    } else if next == Some(*if_true) {
                        lines.branch(condition.invert(), *if_false);
                    } else {
                        lines.branch(condition, *if_true);
                        lines.jump(*if_false);
                    }
                }
            }
            Terminator::Ret { .. } => {
                for (slot, &register) in allocation.callee_saved.iter().enumerate() {
                    emit_load(&mut lines, register, REG_SP, slot as i16);
                }
                if allocation.frame_size() != 0 {
                    emit_immediate(
                        &mut lines,
                        ImmediateOp::Add,
                        REG_SP,
                        allocation.frame_size() as u16,
                        true,
                    );
                }
                lines.word(cpu_v3::jump_register(REG_LINK));
            }
            Terminator::Halt { signal } => {
                let signal = register(*signal);
                if signal != 0 {
                    lines.word(cpu_v3::move_register(0, signal));
                }
                lines.word(cpu_v3::halt());
            }
            Terminator::IcacheInvalidateDelayedAndJump { cseg, target } => {
                let cseg = register(*cseg);
                let target = register(*target);
                lines.word(cpu_v3::device_send(
                    cseg,
                    CACHE_MAINTENANCE_DEVICE,
                    ICACHE_INVALIDATE_ALL_DELAYED,
                ));
                lines.word(cpu_v3::jump_segment(cseg, target));
            }
        }
    }

    LoweredFunction {
        name: function.name,
        lines: lines.items,
        static_addresses,
        frame_size: allocation.frame_size(),
        callee_saved: allocation.callee_saved.len(),
    }
}

fn lower_instruction(
    function: &IrFunc,
    instruction: &Instr,
    register: &dyn Fn(VReg) -> u8,
    allocation: &Allocation,
    lines: &mut Lines,
    static_addresses: &mut Vec<u16>,
) {
    match instruction {
        Instr::Bin { dst, op, lhs, rhs } => {
            let dst = register(*dst);
            let lhs = register(*lhs);
            match rhs {
                IntOperand::Reg(rhs) => {
                    let rhs = register(*rhs);
                    let operation = match op {
                        BinOp::Add => AluOp::Add,
                        BinOp::Sub => AluOp::Sub,
                        BinOp::And => AluOp::And,
                        BinOp::Or => AluOp::Or,
                        BinOp::Xor => AluOp::Xor,
                    };
                    lines.word(cpu_v3::alu(operation, dst, lhs, rhs));
                }
                IntOperand::Imm(value) => {
                    // destructive immediate op on rd: lhs moves into dst first.
                    // ADDI/SUBI read an unsigned u4 (`emit_immediate` flips a
                    // negative adjustment to the opposite operation); SEQI/
                    // SLTI keep a signed i4 short form, and ANDI/ORI/XORI read
                    // a zero-extended u4 mask.
                    let (operation, signed_short) = match op {
                        BinOp::Add => (ImmediateOp::Add, true),
                        BinOp::Sub => (ImmediateOp::Sub, true),
                        BinOp::And => (ImmediateOp::And, false),
                        BinOp::Or => (ImmediateOp::Or, false),
                        BinOp::Xor => (ImmediateOp::Xor, false),
                    };
                    if dst != lhs {
                        lines.word(cpu_v3::move_register(dst, lhs));
                    }
                    emit_immediate(lines, operation, dst, *value, signed_short);
                }
            }
        }
        Instr::Mul {
            dst,
            window,
            lhs,
            rhs,
        } => {
            let dst = register(*dst);
            let lhs = register(*lhs);
            let window = match window {
                MulWindow::Low => cpu_v3::MultiplyWindow::Low,
                MulWindow::Shift8 => cpu_v3::MultiplyWindow::Shift8,
                MulWindow::Shift16 => cpu_v3::MultiplyWindow::Shift16,
            };
            match rhs {
                IntOperand::Reg(rhs) => {
                    let rhs = register(*rhs);
                    // destructive multiply is commutative, so a dst == rhs
                    // collision swaps the operands
                    if dst == lhs {
                        lines.word(cpu_v3::multiply(window, dst, rhs));
                    } else if dst == rhs {
                        lines.word(cpu_v3::multiply(window, dst, lhs));
                    } else {
                        lines.word(cpu_v3::move_register(dst, lhs));
                        lines.word(cpu_v3::multiply(window, dst, rhs));
                    }
                }
                IntOperand::Imm(value) => {
                    if matches!(window, cpu_v3::MultiplyWindow::Low) {
                        // MULI: destructive multiply by the unsigned immediate
                        if dst != lhs {
                            lines.word(cpu_v3::move_register(dst, lhs));
                        }
                        emit_muli(lines, dst, *value);
                    } else {
                        // MUL8/MUL16 have no immediate form: materialize first
                        emit_load_immediate(lines, REG_TMP, *value);
                        if dst != lhs {
                            lines.word(cpu_v3::move_register(dst, lhs));
                        }
                        lines.word(cpu_v3::multiply(window, dst, REG_TMP));
                    }
                }
            }
        }
        Instr::Un { dst, op, src } => lower_unary(register(*dst), *op, register(*src), lines),
        Instr::Shift {
            dst,
            op,
            src,
            amount,
        } => {
            let dst = register(*dst);
            let src = register(*src);
            let operation = match op {
                ShiftOp::Lsl => cpu_v3::ShiftOp::Left,
                ShiftOp::Lsr => cpu_v3::ShiftOp::RightLogical,
                ShiftOp::Asr => cpu_v3::ShiftOp::RightArithmetic,
            };
            match amount {
                IntOperand::Imm(amount) => {
                    if dst != src {
                        lines.word(cpu_v3::move_register(dst, src));
                    }
                    lines.word(cpu_v3::shift_immediate(operation, dst, *amount as u8))
                }
                IntOperand::Reg(amount) => {
                    let amount = register(*amount);
                    // when dst aliases the amount register, the MOV into dst
                    // would clobber the amount before the shift reads it
                    if dst == amount {
                        lines.word(cpu_v3::move_register(REG_TMP, amount));
                        if dst != src {
                            lines.word(cpu_v3::move_register(dst, src));
                        }
                        lines.word(cpu_v3::shift_register(operation, dst, REG_TMP));
                    } else {
                        if dst != src {
                            lines.word(cpu_v3::move_register(dst, src));
                        }
                        lines.word(cpu_v3::shift_register(operation, dst, amount));
                    }
                }
            }
        }
        Instr::Signal { signal_type, value } => {
            lines.word(cpu_v3::signal(register(*value), *signal_type));
        }
        Instr::Mfsr { dst, sr } => {
            let sr = match sr {
                SpecialReg::Cseg => SpecialRegister::CodeSegment,
                SpecialReg::Dseg => SpecialRegister::DataSegment,
            };
            lines.word(cpu_v3::read_special(register(*dst), sr));
        }
        Instr::Bool { dst, cmp } => lower_bool(register(*dst), cmp, function, register, lines),
        Instr::CMov { dst, cmp, src } => {
            lower_compare(lines, cmp, register);
            let condition = test_condition(cmp.cond);
            lines.word(cpu_v3::conditional_move(
                condition,
                register(*dst),
                register(*src),
            ));
        }
        Instr::Mov { dst, src } => {
            let dst = register(*dst);
            let src = register(*src);
            if dst != src {
                lines.word(cpu_v3::move_register(dst, src));
            }
        }
        Instr::LoadImm { dst, value } => emit_load_immediate(lines, register(*dst), *value),
        Instr::LoadMem { dst, base, offset } => {
            emit_load(lines, register(*dst), register(*base), *offset)
        }
        Instr::StoreMem { base, offset, src } => {
            emit_store(lines, register(*src), register(*base), *offset)
        }
        Instr::StoreStatic { addr, value } => {
            static_addresses.push(*addr);
            // __data_init runs at main entry, where the link register is
            // still dead; borrow it as the second scratch next to REG_TMP.
            emit_load_immediate(lines, REG_TMP, *addr);
            emit_load_immediate(lines, REG_LINK, *value);
            emit_store(lines, REG_LINK, REG_TMP, 0);
        }
        Instr::Call { func, .. } => lines.call(func),
        Instr::LoadFuncAddr { dst, func } => lines.load_function_address(func, register(*dst)),
        Instr::CallPtr { .. } => lines.word(cpu_v3::jump_and_link_register(REG_TMP)),
        Instr::DevRecv {
            dst,
            device,
            channel,
        } => {
            check_device(*device);
            lines.word(cpu_v3::device_receive(register(*dst), *device, *channel));
        }
        Instr::DevSend {
            device,
            channel,
            src,
        } => {
            check_device(*device);
            lines.word(cpu_v3::device_send(register(*src), *device, *channel));
        }
        Instr::DcacheInvalidateAll => lines.word(cpu_v3::device_send(
            REG_TMP,
            CACHE_MAINTENANCE_DEVICE,
            D_INVALIDATE_ALL,
        )),
        Instr::MtsrDseg { src } => lines.word(cpu_v3::write_data_segment(register(*src))),
        Instr::Jseg { cseg, target } => {
            lines.word(cpu_v3::jump_segment(register(*cseg), register(*target)))
        }
        Instr::LoadSp { dst, slot } => emit_load(
            lines,
            register(*dst),
            REG_SP,
            (allocation.callee_saved.len() as u8 + allocation.local_slots + *slot) as i16,
        ),
        Instr::StoreSp { slot, src } => emit_store(
            lines,
            register(*src),
            REG_SP,
            (allocation.callee_saved.len() as u8 + allocation.local_slots + *slot) as i16,
        ),
        Instr::LoadLocal { dst, slot } => emit_load(
            lines,
            register(*dst),
            REG_SP,
            (allocation.callee_saved.len() as u8 + *slot) as i16,
        ),
        Instr::StoreLocal { slot, src } => emit_store(
            lines,
            register(*src),
            REG_SP,
            (allocation.callee_saved.len() as u8 + *slot) as i16,
        ),
        Instr::AddrOfLocal { dst, slot } => {
            let dst = register(*dst);
            lines.word(cpu_v3::move_register(dst, REG_SP));
            emit_immediate(
                lines,
                ImmediateOp::Add,
                dst,
                u16::from(allocation.callee_saved.len() as u8 + *slot),
                true,
            );
        }
        Instr::FpuBin { dst, op, lhs, rhs } => {
            let subop = match op {
                FpuBinOp::Add => FpuScalarSubop::Add,
                FpuBinOp::Sub => FpuScalarSubop::Sub,
                FpuBinOp::Mul => FpuScalarSubop::Mul,
            };
            emit_fpu_scalar(lines, register(*lhs), register(*rhs), register(*dst), subop);
        }
        Instr::FpuUn { dst, op, src } => {
            let subop = match op {
                FpuUnOp::Abs => FpuScalarSubop::Abs,
                FpuUnOp::Neg => FpuScalarSubop::Neg,
                FpuUnOp::Floor => FpuScalarSubop::Floor,
                FpuUnOp::Ceil => FpuScalarSubop::Ceil,
                FpuUnOp::Round => FpuScalarSubop::Round,
                FpuUnOp::Trunc => FpuScalarSubop::Trunc,
            };
            emit_fpu_scalar(lines, register(*src), 0, register(*dst), subop);
        }
        Instr::FpuMov { dst, src } => {
            let dst = register(*dst);
            let src = register(*src);
            if dst != src {
                emit_fpu_scalar(lines, src, 0, dst, FpuScalarSubop::Mov);
            }
        }
        Instr::FpuFromInt { dst, src_gpr } => {
            emit_fpu_aux(
                lines,
                register(*src_gpr),
                0,
                register(*dst),
                FpuAuxSubop::I16tof,
            );
        }
        Instr::FpuToInt { dst_gpr, src } => {
            emit_fpu_aux(
                lines,
                register(*dst_gpr),
                register(*src),
                0,
                FpuAuxSubop::Ftoi16,
            );
        }
        Instr::FpuFromLo { dst, src_gpr } => {
            emit_fpu_aux(
                lines,
                register(*src_gpr),
                0,
                register(*dst),
                FpuAuxSubop::Ilo2f,
            );
        }
        Instr::FpuFromHi { dst, src, src_gpr } => {
            let dst = register(*dst);
            let src = register(*src);
            // IHI2F reads and writes the same F register, so materialize the
            // low half into dst first.
            if dst != src {
                emit_fpu_scalar(lines, src, 0, dst, FpuScalarSubop::Mov);
            }
            emit_fpu_aux(lines, register(*src_gpr), 0, dst, FpuAuxSubop::Ihi2f);
        }
        Instr::FpuToLo { dst_gpr, src } => {
            emit_fpu_aux(
                lines,
                register(*dst_gpr),
                register(*src),
                0,
                FpuAuxSubop::Flo2i,
            );
        }
        Instr::FpuToHi { dst_gpr, src } => {
            emit_fpu_aux(
                lines,
                register(*dst_gpr),
                register(*src),
                0,
                FpuAuxSubop::Fhi2i,
            );
        }
        Instr::FpuLoad {
            dst,
            base_gpr,
            offset,
        } => {
            let addr = emit_fpu_address(lines, register(*base_gpr), *offset);
            emit_fpu_aux(lines, addr, 0, register(*dst), FpuAuxSubop::Fld);
        }
        Instr::FpuStore {
            base_gpr,
            offset,
            src,
        } => {
            let addr = emit_fpu_address(lines, register(*base_gpr), *offset);
            emit_fpu_aux(lines, addr, register(*src), 0, FpuAuxSubop::Fst);
        }
        Instr::AddrOfFpuSpill { dst, slot } => {
            // dst = align4(sp + fpu_area_offset) + 4 * slot; alignment is
            // computed at run time because nothing guarantees sp mod 4 == 0.
            let dst = register(*dst);
            lines.word(cpu_v3::move_register(dst, REG_SP));
            emit_immediate(
                lines,
                ImmediateOp::Add,
                dst,
                u16::from(allocation.fpu_area_offset()) + 3,
                true,
            );
            emit_load_immediate(lines, REG_TMP, 0xfffc);
            lines.word(cpu_v3::alu(AluOp::And, dst, dst, REG_TMP));
            if *slot != 0 {
                emit_immediate(lines, ImmediateOp::Add, dst, 4 * u16::from(*slot), true);
            }
        }
    }
}

/// Emits one two-word FPU SCALAR instruction.
fn emit_fpu_scalar(lines: &mut Lines, fa: u8, fb: u8, fd: u8, subop: FpuScalarSubop) {
    for word in cpu_v3::fpu_scalar(fa, fb, fd, subop, 0) {
        lines.word(word);
    }
}

/// Emits one two-word FPU AUX instruction (kind `00`, integer register X).
fn emit_fpu_aux(lines: &mut Lines, x: u8, fa: u8, fd: u8, subop: FpuAuxSubop) {
    for word in cpu_v3::fpu_aux(FpuAuxKind::IntegerRegister, x, fa, fd, subop, 0) {
        lines.word(word);
    }
}

/// Materializes `base + offset` into REG_TMP when an FPU load/store needs an
/// address register; `base` itself is returned when the offset is zero.
fn emit_fpu_address(lines: &mut Lines, base: u8, offset: i16) -> u8 {
    if offset == 0 {
        return base;
    }
    lines.word(cpu_v3::move_register(REG_TMP, base));
    emit_immediate(lines, ImmediateOp::Add, REG_TMP, offset as u16, true);
    REG_TMP
}

fn lower_unary(dst: u8, operation: UnOp, src: u8, lines: &mut Lines) {
    match operation {
        UnOp::Inv => lines.word(cpu_v3::not(dst, src)),
        UnOp::Neg => lines.word(cpu_v3::negate(dst, src)),
        UnOp::Cnt1 => lines.word(cpu_v3::population_count(dst, src)),
        UnOp::Sextb => lines.word(cpu_v3::sign_extend_byte(dst, src)),
        UnOp::Clz => lines.word(cpu_v3::leading_zeros(dst, src)),
        UnOp::Log2 => {
            // log2(0) is defined by rcc as zero.
            lines.word(cpu_v3::leading_zeros(dst, src));
            emit_load_immediate(lines, REG_TMP, 15);
            lines.word(cpu_v3::alu(AluOp::Sub, dst, REG_TMP, dst));
            let done = usize::MAX - lines.len();
            emit_test_nonzero(lines, src);
            lines.branch(TestCondition::NotEqual, done);
            emit_load_immediate(lines, dst, 0);
            lines.label(done);
        }
        UnOp::Not0 => {
            if dst != src {
                lines.word(cpu_v3::move_register(dst, src));
            }
            lines.word(cpu_v3::immediate_signed(ImmediateOp::SetEqual, dst, 0));
            lines.word(cpu_v3::immediate_unsigned(ImmediateOp::Xor, dst, 1));
        }
    }
}

/// the hardware condition code for one of the six real predicates
fn test_condition(cond: CompareOp) -> TestCondition {
    match cond {
        CompareOp::Equal => TestCondition::Equal,
        CompareOp::NotEqual => TestCondition::NotEqual,
        CompareOp::Less => TestCondition::LessThan,
        CompareOp::GreaterEqual => TestCondition::GreaterOrEqual,
        CompareOp::Greater => TestCondition::GreaterThan,
        CompareOp::LessEqual => TestCondition::LessOrEqual,
        CompareOp::Never | CompareOp::Always => {
            unreachable!("diamond conversion never emits degenerate conditions")
        }
    }
}

/// Emits the CMP-class instruction feeding a conditional move (registers or
/// immediate, signed or unsigned).
fn lower_compare(lines: &mut Lines, cmp: &Cmp, register: &dyn Fn(VReg) -> u8) {
    let lhs = register(cmp.lhs);
    match &cmp.rhs {
        CmpRhs::Reg(r) => {
            let r = register(*r);
            lines.word(if cmp.signed {
                cpu_v3::compare_signed(lhs, r)
            } else {
                cpu_v3::compare_unsigned(lhs, r)
            });
        }
        CmpRhs::Imm(value) => emit_immediate(
            lines,
            if cmp.signed {
                ImmediateOp::CompareSigned
            } else {
                ImmediateOp::CompareUnsigned
            },
            lhs,
            *value,
            cmp.signed,
        ),
    }
}

/// Lowers a Boolean-producing comparison (`dst = lhs cond rhs` as 0/1) to the
/// destructive S* instructions; inverted conditions use an operand swap or a
/// trailing XORI 1.
/// kind of a destructive S* operation
#[derive(Clone, Copy)]
enum SetKind {
    Equal,
    Less { signed: bool },
}

impl SetKind {
    fn reg_op(self, dst: u8, rhs: u8) -> Word {
        match self {
            SetKind::Equal => cpu_v3::set_equal(dst, rhs),
            SetKind::Less { signed: true } => cpu_v3::set_less_than_signed(dst, rhs),
            SetKind::Less { signed: false } => cpu_v3::set_less_than_unsigned(dst, rhs),
        }
    }
    fn imm_op(self) -> ImmediateOp {
        match self {
            SetKind::Equal => ImmediateOp::SetEqual,
            SetKind::Less { signed: true } => ImmediateOp::SetLessThanSigned,
            SetKind::Less { signed: false } => ImmediateOp::SetLessThanUnsigned,
        }
    }
    fn signed(self) -> bool {
        match self {
            SetKind::Equal => true,
            SetKind::Less { signed } => signed,
        }
    }
}

/// Emits `dst = left SET_OP right` through the destructive S* forms. The
/// scratch register preserves the right operand when `dst` aliases it.
fn set_op(
    lines: &mut Lines,
    dst: u8,
    left: u8,
    kind: SetKind,
    right: &CmpRhs,
    register: &dyn Fn(VReg) -> u8,
) {
    match right {
        CmpRhs::Reg(r) => {
            let r = register(*r);
            if dst == r {
                // preserve the right operand before dst is overwritten
                lines.word(cpu_v3::move_register(REG_TMP, r));
                if dst != left {
                    lines.word(cpu_v3::move_register(dst, left));
                }
                lines.word(kind.reg_op(dst, REG_TMP));
            } else {
                if dst != left {
                    lines.word(cpu_v3::move_register(dst, left));
                }
                lines.word(kind.reg_op(dst, r));
            }
        }
        CmpRhs::Imm(value) => {
            if dst != left {
                lines.word(cpu_v3::move_register(dst, left));
            }
            emit_immediate(lines, kind.imm_op(), dst, *value, kind.signed());
        }
    }
}

/// Lowers a Boolean-producing comparison (`dst = lhs cond rhs` as 0/1). Integer
/// comparisons use the destructive S* instructions (inverted conditions swap
/// operands or append XORI 1); an FPU comparison sets the pending test with the
/// scalar `CMP` and materializes 0/1 through one conditional move.
fn lower_bool(
    dst: u8,
    cmp: &Cmp,
    function: &IrFunc,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Lines,
) {
    let lhs = register(cmp.lhs);
    if function.class_of(cmp.lhs) == RegClass::Fpu {
        // `CMP` sets the pending test and the following conditional move
        // consumes it immediately, so the two constants are prepared first.
        let CmpRhs::Reg(rhs) = cmp.rhs else {
            unreachable!("FPU comparisons always have a register operand");
        };
        let condition = test_condition(cmp.cond);
        emit_load_immediate(lines, REG_TMP, 1);
        emit_load_immediate(lines, dst, 0);
        emit_fpu_scalar(lines, lhs, register(rhs), 0, FpuScalarSubop::Cmp);
        lines.word(cpu_v3::conditional_move(condition, dst, REG_TMP));
        return;
    }
    let xor1 = |lines: &mut Lines| {
        lines.word(cpu_v3::immediate_unsigned(ImmediateOp::Xor, dst, 1));
    };
    let less = SetKind::Less { signed: cmp.signed };
    match cmp.cond {
        CompareOp::Equal => set_op(lines, dst, lhs, SetKind::Equal, &cmp.rhs, register),
        CompareOp::NotEqual => {
            set_op(lines, dst, lhs, SetKind::Equal, &cmp.rhs, register);
            xor1(lines);
        }
        CompareOp::Less => set_op(lines, dst, lhs, less, &cmp.rhs, register),
        CompareOp::GreaterEqual => {
            set_op(lines, dst, lhs, less, &cmp.rhs, register);
            xor1(lines);
        }
        // swapped: lhs > rhs is rhs < lhs; lhs <= rhs is !(rhs < lhs)
        CompareOp::Greater | CompareOp::LessEqual => {
            match &cmp.rhs {
                CmpRhs::Reg(r) => {
                    let r = register(*r);
                    set_op(lines, dst, r, less, &CmpRhs::Reg(cmp.lhs), register);
                }
                CmpRhs::Imm(value) => {
                    // dst must start as the immediate; when it aliases lhs the
                    // sequence routes through the scratch register
                    if dst == lhs {
                        emit_load_immediate(lines, REG_TMP, *value);
                        lines.word(less.reg_op(REG_TMP, dst));
                        lines.word(cpu_v3::move_register(dst, REG_TMP));
                    } else {
                        emit_load_immediate(lines, dst, *value);
                        lines.word(less.reg_op(dst, lhs));
                    }
                }
            }
            if matches!(cmp.cond, CompareOp::LessEqual) {
                xor1(lines);
            }
        }
        CompareOp::Never | CompareOp::Always => {
            unreachable!("diamond conversion never emits degenerate conditions")
        }
    }
}

fn check_device(device: u8) {
    assert!(
        device < 8,
        "CpuV3 device index {device} exceeds the ISA v0.6 limit of 8 devices"
    );
}

/// Emits the generic "test a value against zero" comparison used when a
/// branch tests a plain value rather than a comparison outcome.
fn emit_test_nonzero(lines: &mut Lines, test: u8) {
    lines.word(cpu_v3::immediate_signed(
        ImmediateOp::CompareSigned,
        test,
        0,
    ));
}

/// Lowers a branch comparison directly to a CMP-class instruction feeding a
/// conditional branch; no 0/1 value is materialized. Returns the condition
/// that branches when `cmp` holds.
fn lower_comparison(
    function: &IrFunc,
    comparison: &Cmp,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Lines,
) -> TestCondition {
    let lhs = register(comparison.lhs);
    let condition = match comparison.cond {
        CompareOp::Equal => TestCondition::Equal,
        CompareOp::NotEqual => TestCondition::NotEqual,
        CompareOp::Less => TestCondition::LessThan,
        CompareOp::GreaterEqual => TestCondition::GreaterOrEqual,
        CompareOp::Greater => TestCondition::GreaterThan,
        CompareOp::LessEqual => TestCondition::LessOrEqual,
        // A value always compares Equal to itself, so these degenerate
        // conditions do not depend on the compared values at all.
        CompareOp::Always => {
            emit_self_compare(function, comparison, register, lines);
            return TestCondition::Equal;
        }
        CompareOp::Never => {
            emit_self_compare(function, comparison, register, lines);
            return TestCondition::NotEqual;
        }
    };
    if function.class_of(comparison.lhs) == RegClass::Fpu {
        // The FPU scalar `CMP` sets the pending test from the signed Q16.16
        // ordering; the following conditional branch consumes it exactly like
        // CMPS/CMPU, and nothing may be emitted between the compare and branch.
        let CmpRhs::Reg(rhs) = comparison.rhs else {
            unreachable!("FPU comparisons always have a register operand");
        };
        emit_fpu_scalar(lines, lhs, register(rhs), 0, FpuScalarSubop::Cmp);
        return condition;
    }
    match comparison.rhs {
        CmpRhs::Reg(rhs) => {
            let rhs = register(rhs);
            lines.word(if comparison.signed {
                cpu_v3::compare_signed(lhs, rhs)
            } else {
                cpu_v3::compare_unsigned(lhs, rhs)
            });
        }
        CmpRhs::Imm(rhs) => emit_immediate(
            lines,
            if comparison.signed {
                ImmediateOp::CompareSigned
            } else {
                ImmediateOp::CompareUnsigned
            },
            lhs,
            rhs,
            comparison.signed,
        ),
    }
    condition
}

/// the degenerate Always/Never conditions compare a value with itself
fn emit_self_compare(
    function: &IrFunc,
    comparison: &Cmp,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Lines,
) {
    let lhs = register(comparison.lhs);
    if function.class_of(comparison.lhs) == RegClass::Fpu {
        emit_fpu_scalar(lines, lhs, lhs, 0, FpuScalarSubop::Cmp);
        return;
    }
    lines.word(cpu_v3::compare_signed(lhs, lhs));
}

fn emit_edge_moves(
    function: &IrFunc,
    predecessor: BlockId,
    target: BlockId,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Lines,
) {
    let block = &function.blocks[target];
    if block.phis.is_empty() || block.preds.len() == 1 {
        return;
    }
    let moves = block
        .phis
        .iter()
        .map(|phi| {
            let value = phi
                .args
                .iter()
                .find(|(pred, _)| *pred == predecessor)
                .expect("missing phi edge")
                .1;
            (
                register(value),
                register(phi.dst),
                function.class_of(phi.dst),
            )
        })
        .collect::<Vec<_>>();
    emit_parallel_moves(lines, &moves);
}

/// parallel phi moves, split by register class: GPR moves use MOV with
/// REG_TMP as the cycle-breaking scratch, FPU moves use FMOV with the
/// reserved FPU_SCRATCH register
fn emit_parallel_moves(lines: &mut Lines, moves: &[(u8, u8, RegClass)]) {
    for class in [RegClass::Gpr, RegClass::Fpu] {
        let class_moves = moves
            .iter()
            .filter(|&&(_, _, c)| c == class)
            .map(|&(from, to, _)| (from, to))
            .collect::<Vec<_>>();
        emit_parallel_moves_in_file(lines, &class_moves, class);
    }
}

fn emit_parallel_moves_in_file(lines: &mut Lines, moves: &[(u8, u8)], class: RegClass) {
    let emit_move = |lines: &mut Lines, to: u8, from: u8| match class {
        RegClass::Gpr => lines.word(cpu_v3::move_register(to, from)),
        RegClass::Fpu => emit_fpu_scalar(lines, from, 0, to, FpuScalarSubop::Mov),
    };
    let scratch = match class {
        RegClass::Gpr => REG_TMP,
        RegClass::Fpu => FPU_SCRATCH,
    };
    let mut pending = moves
        .iter()
        .copied()
        .filter(|(from, to)| from != to)
        .collect::<Vec<_>>();
    while !pending.is_empty() {
        if let Some(index) = pending.iter().enumerate().find_map(|(index, &(_, to))| {
            (!pending
                .iter()
                .enumerate()
                .any(|(other, &(from, _))| other != index && from == to))
            .then_some(index)
        }) {
            let (from, to) = pending.remove(index);
            emit_move(lines, to, from);
        } else {
            let target = pending[0].1;
            emit_move(lines, scratch, target);
            for (from, _) in &mut pending {
                if *from == target {
                    *from = scratch;
                }
            }
        }
    }
}

fn emit_load_immediate(lines: &mut Lines, dst: u8, value: u16) {
    if value <= 15 {
        lines.word(cpu_v3::immediate_unsigned(
            ImmediateOp::LoadUnsigned,
            dst,
            value as u8,
        ));
    } else if value >= 0xfff8 {
        lines.word(cpu_v3::immediate_signed(
            ImmediateOp::LoadSigned,
            dst,
            value as i16,
        ));
    } else {
        for word in cpu_v3::load_immediate16(dst, value) {
            lines.word(word);
        }
    }
}

/// Emits a MULI (short u4, or PFX12 + full unsigned u16 bit pattern).
fn emit_muli(lines: &mut Lines, dst: u8, value: u16) {
    if value <= 15 {
        lines.word(cpu_v3::multiply_immediate(dst, value as u8));
    } else {
        for word in cpu_v3::prefixed(cpu_v3::multiply_immediate(dst, 0), value) {
            lines.word(word);
        }
    }
}

fn emit_immediate(
    lines: &mut Lines,
    operation: ImmediateOp,
    dst: u8,
    value: u16,
    signed_short: bool,
) {
    // ADDI/SUBI take an unsigned u4 unprefixed, so a negative adjustment is
    // expressed with the opposite operation of its magnitude (equivalent
    // under wrapping arithmetic, also for the prefixed 16-bit form).
    if matches!(operation, ImmediateOp::Add | ImmediateOp::Sub) {
        let (operation, value) = if (value as i16) < 0 {
            let flipped = match operation {
                ImmediateOp::Add => ImmediateOp::Sub,
                ImmediateOp::Sub => ImmediateOp::Add,
                _ => unreachable!(),
            };
            (flipped, (value as i16).unsigned_abs())
        } else {
            (operation, value)
        };
        if value <= 15 {
            lines.word(cpu_v3::immediate_unsigned(operation, dst, value as u8));
        } else {
            let consumer = 0xa000 | ((operation as u16) << 8) | (u16::from(dst) << 4);
            for word in cpu_v3::prefixed(consumer, value) {
                lines.word(word);
            }
        }
        return;
    }
    if signed_short && (-8..=7).contains(&(value as i16)) {
        lines.word(cpu_v3::immediate_signed(operation, dst, value as i16));
    } else if !signed_short && value <= 15 {
        lines.word(cpu_v3::immediate_unsigned(operation, dst, value as u8));
    } else {
        let consumer = 0xa000 | ((operation as u16) << 8) | (u16::from(dst) << 4);
        for word in cpu_v3::prefixed(consumer, value) {
            lines.word(word);
        }
    }
}

fn emit_load(lines: &mut Lines, dst: u8, base: u8, offset: i16) {
    if (-8..=7).contains(&offset) {
        lines.word(cpu_v3::load(dst, base, offset));
    } else {
        for word in cpu_v3::prefixed(cpu_v3::load(dst, base, 0), offset as u16) {
            lines.word(word);
        }
    }
}

fn emit_store(lines: &mut Lines, src: u8, base: u8, offset: i16) {
    if (-8..=7).contains(&offset) {
        lines.word(cpu_v3::store(src, base, offset));
    } else {
        for word in cpu_v3::prefixed(cpu_v3::store(src, base, 0), offset as u16) {
            lines.word(word);
        }
    }
}

/// One relaxation decision: shrink to the one-word form when the target is
/// reachable with a signed 8-bit offset from the following word.
fn relax_one(from: usize, target: usize, short: &mut bool, changed: &mut bool) {
    let offset = target as i64 - (from + 1) as i64;
    let fits = (-128..=127).contains(&offset);
    if *short != fits {
        *short = fits;
        *changed = true;
    }
}

/// Iterative branch relaxation: any branch, jump, or call whose final signed
/// 8-bit relative offset fits shrinks from the two-word wide form (PFX12 +
/// relative) to one word. Widths only ever shrink, so the layout converges.
fn relax_branches(functions: &mut [LoweredFunction], code_base: usize) {
    loop {
        let mut function_addresses = HashMap::new();
        let mut cursor = code_base;
        for function in functions.iter() {
            function_addresses.insert(function.name, cursor);
            cursor += function.lines.iter().map(Line::size).sum::<usize>() + 1;
        }
        let mut changed = false;
        for function in functions.iter_mut() {
            let start = function_addresses[&function.name];
            let mut labels = HashMap::new();
            let mut address = start;
            for line in &function.lines {
                if let Line::Label(label) = line {
                    labels.insert(*label, address);
                } else {
                    address += line.size();
                }
            }
            let mut address = start;
            for line in &mut function.lines {
                match line {
                    Line::Branch { target, short, .. } => {
                        relax_one(address, labels[target], short, &mut changed);
                    }
                    Line::Jump { target, short, .. } => {
                        relax_one(address, labels[target], short, &mut changed);
                    }
                    Line::Call {
                        function: target,
                        short,
                        ..
                    } => {
                        relax_one(address, function_addresses[target], short, &mut changed);
                    }
                    _ => {}
                }
                address += line.size();
            }
        }
        if !changed {
            return;
        }
    }
}

/// A requirement violated by the generated CpuV3 code image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramValidationError {
    /// The word is not a legal instruction: a reserved opcode/function or a
    /// non-canonical unused field.
    InvalidInstruction { address: Word, word: Word },
    /// A `PFX12` word is not immediately followed by a prefix consumer.
    DanglingPrefix { address: Word, next: Option<Word> },
    /// A `PFX12` feeding a major-B relative consumer (function 0..=7) must
    /// carry only the low eight payload bits; `payload12[11:8]` is ignored by
    /// the ISA, and the backend promises to zero it.
    RelativePrefixHighBits { address: Word, payload: Word },
}

impl std::fmt::Display for ProgramValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInstruction { address, word } => {
                write!(f, "illegal instruction {word:#06x} at {address:#06x}")
            }
            Self::DanglingPrefix { address, next } => match next {
                Some(next) => write!(
                    f,
                    "PFX12 at {address:#06x} is not followed by a consumer ({next:#06x})"
                ),
                None => write!(f, "PFX12 at {address:#06x} is the last word"),
            },
            Self::RelativePrefixHighBits { address, payload } => write!(
                f,
                "relative PFX12 at {address:#06x} has non-zero payload12[11:8] ({payload:#05x})"
            ),
        }
    }
}

/// The single final check of the CpuV3 backend's output. It asserts the
/// encoding requirements the frontend, optimizer, and linker are jointly
/// responsible for, after linking and branch relaxation:
///
/// - every word is a legal instruction with canonical unused fields, so no
///   reserved major/function, `JREG`/`JALR` non-canonical nibble, or invalid
///   `MFSR`/`MTSR` selector can reach the image;
/// - every `PFX12` is immediately consumed by the next word;
/// - a relative (`B 0..=7`) prefixed consumer is given `payload12[11:8] == 0`.
///
/// Constant shift amounts, immediate widths, and `PFX12` payload widths are
/// bounded before encoding (`shift_operand` rejects constant shift amounts
/// above 15, and the encoding helpers assert their field widths), so they need
/// no separate image re-check once the words exist.
pub fn validate_program(words: &[Word], code_base: Word) -> Result<(), ProgramValidationError> {
    let address_of = |index: usize| code_base.wrapping_add(index as Word);
    let mut index = 0;
    while index < words.len() {
        let word = words[index];
        if let cpu_v3::Instruction::Prefix { payload } = cpu_v3::decode(word) {
            let Some(&next) = words.get(index + 1) else {
                return Err(ProgramValidationError::DanglingPrefix {
                    address: address_of(index),
                    next: None,
                });
            };
            if !cpu_v3::is_prefix_consumer(next) {
                return Err(ProgramValidationError::DanglingPrefix {
                    address: address_of(index),
                    next: Some(next),
                });
            }
            if next >> 12 == 0xb && (next >> 8) & 0xf <= 7 && payload >> 8 != 0 {
                return Err(ProgramValidationError::RelativePrefixHighBits {
                    address: address_of(index),
                    payload,
                });
            }
            index += 2;
            continue;
        }
        // An FPU v2 instruction is two physical words; a reserved pair is an
        // illegal image and a lone first word is truncated.
        if cpu_v3::FpuOpcode::from_word0(word).is_some() {
            let Some(&next) = words.get(index + 1) else {
                return Err(ProgramValidationError::InvalidInstruction {
                    address: address_of(index),
                    word,
                });
            };
            if matches!(
                cpu_v3::decode_fpu_pair(word, next),
                cpu_v3::Instruction::FpuReserved { .. }
            ) {
                return Err(ProgramValidationError::InvalidInstruction {
                    address: address_of(index),
                    word,
                });
            }
            index += 2;
            continue;
        }
        if matches!(cpu_v3::decode(word), cpu_v3::Instruction::Invalid { .. }) {
            return Err(ProgramValidationError::InvalidInstruction {
                address: address_of(index),
                word,
            });
        }
        index += 1;
    }
    Ok(())
}

fn link(
    mut functions: Vec<LoweredFunction>,
    options: &CompilerOptions,
    frontend_debug: rcc::frontend::FrontendDebug,
) -> Result<CpuV3Program, ProgramValidationError> {
    let code_base = usize::from(options.code_base);
    relax_branches(&mut functions, code_base);
    let mut function_addresses = HashMap::new();
    let mut cursor = code_base;
    for function in &functions {
        function_addresses.insert(function.name, cursor);
        cursor += function.lines.iter().map(Line::size).sum::<usize>() + 1;
    }
    assert!(
        cursor <= 1 << 16,
        "CpuV3 program at code_base {:#06x} exceeds the 64K code-segment window",
        options.code_base
    );
    assert!(
        cursor <= usize::from(options.data_base),
        "CpuV3 unified-memory image uses {cursor} code words and crosses data_base {:#06x}",
        options.data_base
    );
    let stack_limit = if options.stack_init == 0 {
        1 << 16
    } else {
        usize::from(options.stack_init)
    };
    assert!(
        cursor <= stack_limit,
        "CpuV3 unified-memory image uses {cursor} code words and crosses stack top {stack_limit:#07x}"
    );
    let heap_end = options
        .heap_begin
        .checked_add(options.heap_size)
        .expect("CpuV3 heap range wraps the address space");
    assert!(
        usize::from(heap_end) <= stack_limit,
        "CpuV3 heap {:#06x}..{heap_end:#06x} overlaps the stack top {stack_limit:#07x}",
        options.heap_begin
    );

    let static_addresses = functions
        .iter()
        .flat_map(|function| function.static_addresses.iter().copied())
        .collect::<Vec<_>>();
    if let Some(address) = static_addresses
        .iter()
        .copied()
        .find(|address| usize::from(*address) < cursor)
    {
        panic!(
            "CpuV3 unified-memory image uses {cursor} code words but static data starts at {address:#06x}; select a non-overlapping data_base"
        );
    }
    if let Some(address) = static_addresses
        .iter()
        .copied()
        .find(|address| *address >= options.heap_begin)
    {
        panic!(
            "CpuV3 static data reaches {address:#06x}, overlapping heap_begin {:#06x}",
            options.heap_begin
        );
    }

    let mut words = Vec::with_capacity(cursor - code_base);
    let mut listing = String::new();
    let mut debug_lines = Vec::new();
    let mut debug_functions = Vec::new();
    for function in &functions {
        let local_start = words.len();
        let start = code_base + local_start;
        let file = frontend_debug
            .funcs
            .iter()
            .find(|d| d.name == function.name)
            .map(|d| d.file)
            .unwrap_or(0);
        let mut labels = HashMap::new();
        let mut address = start;
        for line in &function.lines {
            if let Line::Label(label) = line {
                assert!(
                    labels.insert(*label, address).is_none(),
                    "duplicate CpuV3 label"
                );
            } else {
                address += line.size();
            }
        }
        listing.push_str(&format!("{} @ {start:#06x}\n", function.name));
        for line in &function.lines {
            if let Line::Label(_) = line {
                continue;
            }
            let emitted = words.len();
            let (count, src_line) = match line {
                Line::Word { word, line } => {
                    words.push(*word);
                    (1, *line)
                }
                Line::Label(_) => continue,
                Line::Branch {
                    condition,
                    target,
                    line,
                    short,
                } => {
                    let span = if *short { 1 } else { 2 };
                    let offset = relative_offset(code_base + words.len() + span, labels[target]);
                    if *short {
                        words.push(cpu_v3::branch(*condition, offset));
                        (1, *line)
                    } else {
                        words.extend(wide_branch(*condition, offset));
                        (2, *line)
                    }
                }
                Line::Jump {
                    target,
                    line,
                    short,
                } => {
                    let span = if *short { 1 } else { 2 };
                    let offset = relative_offset(code_base + words.len() + span, labels[target]);
                    if *short {
                        words.push(cpu_v3::jump_relative(offset));
                        (1, *line)
                    } else {
                        words.extend(wide_jump(offset));
                        (2, *line)
                    }
                }
                Line::Call {
                    function: target,
                    line,
                    short,
                } => {
                    let span = if *short { 1 } else { 2 };
                    let offset =
                        relative_offset(code_base + words.len() + span, function_addresses[target]);
                    if *short {
                        words.push(cpu_v3::jump_and_link_relative(offset));
                        (1, *line)
                    } else {
                        words.extend(wide_call(offset));
                        (2, *line)
                    }
                }
                Line::LoadFunctionAddress {
                    function: target,
                    dst,
                    line,
                } => {
                    words.extend(cpu_v3::load_immediate16(
                        *dst,
                        function_addresses[target] as u16,
                    ));
                    (2, *line)
                }
            };
            let emitted_count = words.len() - emitted;
            debug_assert_eq!(emitted_count, count);
            if let Some(source_line) = src_line {
                for word in emitted..words.len() {
                    debug_lines.push((code_base + word, file, source_line));
                }
            }
        }
        let end = code_base + words.len();
        words.push(cpu_v3::halt());
        debug_functions.push(rcc::DebugFunc {
            name: function.name.to_string(),
            file,
            addr: (start, end),
            frame_size: function.frame_size,
            locals: frontend_debug
                .funcs
                .iter()
                .find(|d| d.name == function.name)
                .map(|d| {
                    d.locals
                        .iter()
                        .map(|v| {
                            let mut v = v.clone();
                            match v.loc {
                                rcc::VarLoc::Frame(slot) => {
                                    v.loc = rcc::VarLoc::Frame(function.callee_saved as u8 + slot);
                                }
                                rcc::VarLoc::ParamIndex(index) => {
                                    v.loc = rcc::VarLoc::Param(
                                        CPU_V3_REGISTER_CONVENTION.argument_registers
                                            [index as usize],
                                    );
                                }
                                _ => {}
                            }
                            v
                        })
                        .collect()
                })
                .unwrap_or_default(),
        });
        // mnemonic listing: wide (prefixed) operations occupy one line
        for line in cpu_v3::disassemble_words(&words[local_start..], start as u16) {
            let span = if line.wide { 2 } else { 1 };
            let raw: Vec<String> = (0..span)
                .map(|i| {
                    format!(
                        "{:04x}",
                        words[usize::from(line.address) - (start - local_start) + i]
                    )
                })
                .collect();
            listing.push_str(&format!(
                "  {:04x}: {:<11} {}\n",
                line.address,
                raw.join(" "),
                line.text
            ));
        }
    }
    validate_program(&words, options.code_base)?;
    debug_lines.sort();
    Ok(CpuV3Program {
        code_base: options.code_base,
        words,
        listing,
        debug: rcc::DebugInfo {
            files: frontend_debug.files,
            function_table: vec![],
            init_sections: vec![],
            functions: debug_functions,
            globals: frontend_debug.globals,
            types: frontend_debug.types,
            consts: frontend_debug.consts,
            lines: debug_lines,
        },
    })
}

fn relative_offset(from: usize, to: usize) -> i16 {
    (to as u16).wrapping_sub(from as u16) as i16
}

fn wide_branch(condition: TestCondition, offset: i16) -> [Word; 2] {
    cpu_v3::prefixed_branch(cpu_v3::branch(condition, 0), offset as u16)
}

fn wide_jump(offset: i16) -> [Word; 2] {
    cpu_v3::prefixed_branch(cpu_v3::jump_relative(0), offset as u16)
}

fn wide_call(offset: i16) -> [Word; 2] {
    cpu_v3::prefixed_branch(cpu_v3::jump_and_link_relative(0), offset as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcc::frontend::parse_source_with;

    /// C1 lowers scalar `fix16` end to end through the two-word v2 ISA:
    /// construction, arithmetic, unary operations and the numeric conversion.
    #[test]
    fn scalar_fix16_arithmetic_unary_and_conversions_run_on_the_machine() {
        let source = r#"
            fn main() {
                let a = fix16::from_int(7);
                let b = fix16::from_int(-2);
                let sum = a + b;             // 5
                let diff = a - b;            // 9
                let prod = a * b;            // -14
                let neg = -b;                // 2
                let fl = fix16::from_words(0x8000, 0x0001); // 1.5
                let down = fl.trunc();       // 1.0
                let up = fl.ceil();          // 2.0
                halt((sum + diff + prod + neg + down + up).to_int() as u16);
            }
        "#;
        // 5 + 9 - 14 + 2 + 1 + 2 = 5
        assert_eq!(run(source), 5);
    }

    /// The raw-half bridge (`ILO2F`/`IHI2F`/`FLO2I`/`FHI2I`) moves both words
    /// without numeric conversion.
    #[test]
    fn scalar_fix16_raw_half_bridge_round_trips_words() {
        let source = r#"
            fn main() {
                let v = fix16::from_words(0x1234u16, 0xabcdu16);
                halt(v.lo_bits() ^ v.hi_bits());
            }
        "#;
        assert_eq!(run(source), 0x1234u16 ^ 0xabcdu16);
    }

    /// A fix16 value live across a call has no callee-saved F register to live
    /// in, so it spills through a frame slot (`AddrOfFpuSpill` + FST/FLD).
    #[test]
    fn scalar_fix16_values_spill_across_calls() {
        let source = r#"
            fn addfix(a: fix16, b: fix16) -> fix16 { a + b }
            fn main() {
                let a = fix16::from_int(5);
                let b = addfix(a, fix16::from_int(1)); // `a` lives across the call
                halt((a + b).to_int() as u16);
            }
        "#;
        assert_eq!(run(source), 11);
    }

    /// fix16 comparisons feed the shared pending test; a materialized bool uses
    /// a scalar `CMP` plus one conditional move.
    #[test]
    fn scalar_fix16_comparisons_branch_and_materialize_bools() {
        let source = r#"
            fn main() {
                let a = fix16::from_int(2);
                let b = fix16::from_int(3);
                let lt = a < b;
                let ge = a >= b;
                let eq = a == a;
                let le = a <= b;
                if lt && !ge && eq && le && b != a {
                    halt(1);
                } else {
                    halt(0);
                }
            }
        "#;
        assert_eq!(run(source), 1);
    }

    /// fix16 variables, loops and if-expressions exercise phis, compound
    /// assignment and range-free scalar allocation.
    #[test]
    fn scalar_fix16_variables_loops_and_if_expressions_run() {
        let source = r#"
            fn main() {
                let mut acc = fix16::zero();
                let mut i = fix16::zero();
                let one = fix16::from_int(1);
                let limit = fix16::from_int(5);
                while i < limit {
                    acc += i;
                    i += one;
                }
                let pick = if acc > limit { acc } else { limit };
                halt((acc + pick).to_int() as u16);
            }
        "#;
        // acc = 0+1+2+3+4 = 10, pick = acc (10), total = 20
        assert_eq!(run(source), 20);
    }

    /// The scalar FPU lowering emits the exact two-word v2 encodings; this
    /// hand-built IR function pins the whole image (allocation is deterministic
    /// for a straight-line function).
    #[test]
    fn scalar_fix16_add_emits_exact_two_word_words() {
        use rcc::{FpuBinOp, FuncBuilder, RegClass};
        let (mut b, params) = FuncBuilder::new_typed("main", &[RegClass::Fpu, RegClass::Fpu], 1);
        let a = b.get(params[0]);
        let c = b.get(params[1]);
        let sum = b.fpu_bin(FpuBinOp::Add, a, c);
        b.ret(&[sum]);
        let function = b.finish();
        let functions = std::collections::HashMap::from([("main", function)]);
        let program = compile_ir(
            functions,
            &CompilerOptions::default(),
            "main",
            rcc::frontend::FrontendDebug::default(),
        )
        .unwrap();
        // Pin the allocator/codegen contract, not merely decoder round-tripping:
        // the two ABI inputs are moved to f29/f28, the destructive ADD reuses
        // f28 as its destination, and the resulting pair is exactly these two
        // architectural words.
        let mut found = Vec::new();
        let mut index = 0;
        while index + 1 < program.words.len() {
            if let cpu_v3::Instruction::FpuScalar {
                fa,
                fb,
                fd,
                subop: cpu_v3::FpuScalarSubop::Add,
                ..
            } = cpu_v3::decode_fpu_pair(program.words[index], program.words[index + 1])
            {
                found.push((index, fa, fb, fd));
            }
            index += 1;
        }
        assert_eq!(found, vec![(4, 29, 28, 28)], "{}", program.listing);
        assert_eq!(&program.words[4..6], &[0xd75c, 0x7000]);
    }

    /// The backend no longer refuses FPU programs: an FPU-typed function
    /// pointer plus a scalar body compiles.
    #[test]
    fn an_fpu_class_program_is_now_lowered_by_the_backend() {
        let source = r#"
            fn identity(v: fix16) -> fix16 { v }
            fn main() {
                let keep: fn(fix16) -> fix16 = identity;
                halt(keep(fix16::from_int(9)).to_int() as u16);
            }
        "#;
        assert_eq!(run(source), 9);
    }

    /// C0 freezes the Q16.16 contiguous-range ABI even though range-aware
    /// allocation lands with C2.
    #[test]
    fn cpu_v3_fpu_convention_uses_the_frozen_q16_layout() {
        let fpu = CPU_V3_REGISTER_CONVENTION.fpu.unwrap();
        assert_eq!(fpu.return_registers, &[0, 1, 2, 3]);
        assert_eq!(fpu.argument_registers.first(), Some(&4));
        assert_eq!(fpu.argument_registers.last(), Some(&27));
        assert_eq!(fpu.allocatable_registers.first(), Some(&28));
        assert_eq!(fpu.allocatable_registers.last(), Some(&62));
        assert_eq!(fpu.scratch_register, 63);
    }

    #[test]
    fn debug_info_covers_modules_lines_and_locals() {
        let source = r#"
            mod helper;
            fn main() {
                let x: u16 = double(21);
                halt(x);
            }
        "#;
        let module = "fn double(v: u16) -> u16 { v + v }";
        let program = rcc::frontend::compile_program_named(
            "main.rs",
            source,
            &CompilerOptions::default(),
            &mut |name| {
                assert_eq!(name, "helper");
                Ok(module.to_string())
            },
        )
        .unwrap();
        let program = super::compile(program, &CompilerOptions::default(), "main");
        let debug = &program.debug;
        // both files recorded
        assert!(debug.files.len() >= 2, "files: {:?}", debug.files);
        // the line map is non-empty, sorted, and in range
        assert!(!debug.lines.is_empty());
        assert!(debug.lines.windows(2).all(|w| w[0].0 <= w[1].0));
        let main_fn = debug
            .functions
            .iter()
            .find(|f| f.name == "main")
            .expect("main in debug info");
        assert!(main_fn.addr.1 > main_fn.addr.0);
        // `x` is a local in main with a concrete lowered location
        let x = main_fn
            .locals
            .iter()
            .find(|v| v.name == "x")
            .expect("local x in debug info");
        match x.loc {
            rcc::VarLoc::Frame(_) | rcc::VarLoc::Param(_) | rcc::VarLoc::Ssa => {}
            ref other => panic!("unexpected location for x: {other:?}"),
        }
        // render/parse round-trip
        let parsed = rcc::parse_debug(&debug.render()).expect("debug info round-trips");
        assert_eq!(parsed.files.len(), debug.files.len());
        assert_eq!(parsed.lines.len(), debug.lines.len());
    }

    #[test]
    fn integer_multiply_uses_the_hardware_mul() {
        let source = r#"
            fn main() {
                let a: u16 = 37;
                let b: u16 = 11;
                let mut acc: u16 = 0;
                let mut i: u16 = 0;
                while i < 8 {
                    acc = acc + a * b + i * 3;
                    i = i + 1;
                }
                halt(acc);
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(
            !program.listing.contains("mul_16x"),
            "CpuV3 must not call the software multiply library:
{}",
            program.listing
        );
        // sum of (407 + 3i) for i in 0..8 = 3256 + 84
        assert_eq!(run(source), 3340);
    }

    #[test]
    fn div_and_rem_run_on_the_machine() {
        let source = r#"
            fn main() {
                let a: u16 = 1000;
                let b: u16 = 7;
                let q = a / b;   // 142
                let r = a % b;   // 6
                let s: i16 = -1000;
                let t: i16 = 7;
                let u = s / t;   // -142: the quotient truncates toward zero
                let v = s % t;   // -6: the remainder follows the dividend
                let x: u16 = 0xabcd;
                let shifted = x / 16u16;  // literal power of two: a shift
                let masked = x % 16u16;   // literal power of two: a mask
                halt(q + r + (u + v + 300i16) as u16 + shifted + masked);
            }
        "#;
        // 142 + 6 + (-142 - 6 + 300) + 0x0abc + 0x000d = 3061
        assert_eq!(run_with_std(source), 3061);
    }

    #[test]
    fn div_and_rem_define_a_zero_divisor() {
        let source = r#"
            fn main() {
                let z: u16 = 0;
                let zi: i16 = 0;
                let a: u16 = 1000;
                let s: i16 = -1000;
                if div_u16(a, z) == 0 && rem_u16(a, z) == a && div_i16(s, zi) == 0i16 && rem_i16(s, zi) == s {
                    halt(1);
                } else {
                    halt(0);
                }
            }
        "#;
        assert_eq!(run_with_std(source), 1);
    }

    #[test]
    fn constant_divisor_magic_divide_runs_on_the_machine() {
        // A constant divisor lowers to the MUL16 magic path (the single-multiply
        // High form and the 17-bit Add form), so this compares the emitted
        // sequence against the host quotient across a range and at the edges.
        let source = r#"
            fn main() {
                let mut acc: u16 = 0;
                let mut x: u16 = 0;
                while x < 512u16 {
                    acc = acc + (x / 3u16) + (x % 3u16) + (x / 7u16) + (x % 7u16)
                        + (x / 10u16) + (x % 10u16) + (x / 1000u16) + (x % 1000u16);
                    x = x + 1;
                }
                acc = acc + (65535u16 / 3u16) + (65535u16 % 7u16) + (65534u16 / 65534u16)
                    + (65535u16 / 65534u16) + (40000u16 % 257u16) + (32768u16 / 9u16)
                    + (1u16 / 65535u16) + (65535u16 / 1u16);
                halt(acc);
            }
        "#;
        let mut expected: u16 = 0;
        for x in 0u16..512 {
            expected = expected
                .wrapping_add(x / 3)
                .wrapping_add(x % 3)
                .wrapping_add(x / 7)
                .wrapping_add(x % 7)
                .wrapping_add(x / 10)
                .wrapping_add(x % 10)
                .wrapping_add(x / 1000)
                .wrapping_add(x % 1000);
        }
        for term in [
            65535u16 / 3,
            65535u16 % 7,
            65534u16 / 65534,
            65535u16 / 65534,
            40000u16 % 257,
            32768u16 / 9,
            1u16 / 65535,
            // the source divides by 1u16; on the host that is the identity
            65535u16,
        ] {
            expected = expected.wrapping_add(term);
        }
        assert_eq!(run_with_std_capped(source, 500_000), expected);
    }

    #[test]
    fn stored_bool_materializes_negates_casts_and_branches() {
        // A comparison bound to a variable becomes a 0/1 value (the ISA's
        // Boolean-producing form); it can be negated, cast, passed, returned and
        // used as a condition again, and `&&` builds a compound condition.
        let source = r#"
            fn is_small(x: u16) -> bool {
                x < 100u16
            }
            fn main() {
                let mut acc: u16 = 0;
                let mut x: u16 = 0;
                while x < 200u16 {
                    let small = is_small(x);
                    let odd = (x & 1u16) != 0u16;
                    let flag = small && odd;
                    if flag {
                        acc = acc + 1u16;
                    }
                    if !small {
                        acc = acc + 2u16;
                    }
                    acc = acc + (small as u16);
                    x = x + 1;
                }
                halt(acc);
            }
        "#;
        let mut expected: u16 = 0;
        for x in 0u16..200 {
            let small = x < 100;
            let odd = (x & 1) != 0;
            if small && odd {
                expected = expected.wrapping_add(1);
            }
            if !small {
                expected = expected.wrapping_add(2);
            }
            expected = expected.wrapping_add(u16::from(small));
        }
        assert_eq!(run_with_std_capped(source, 200_000), expected);
    }

    #[test]
    fn bool_parameter_return_and_loop_condition() {
        let source = r#"
            fn keep_going(done: bool) -> bool {
                !done
            }
            fn main() {
                let mut i: u16 = 0;
                let mut go: bool = true;
                while go {
                    i = i + 1;
                    go = keep_going(i >= 5u16);
                }
                let pick = if 1u16 < 2u16 { 7u16 } else { 9u16 };
                halt(i + pick);
            }
        "#;
        // the loop stops the iteration i reaches 5, and the if-expression picks 7
        assert_eq!(run_with_std(source), 12);
    }

    #[test]
    fn struct_fields_nest_copy_and_run_on_the_machine() {
        let source = r#"
            #[repr(C)]
            struct Inner {
                a: u16,
                b: i16,
            }

            struct Point {
                x: u16,
                y: u16,
                inner: Inner,
                flags: Buf<u16, 2>,
                valid: bool,
            }

            fn main() {
                let mut p: Point = Point {
                    x: 3,
                    y: 4,
                    inner: Inner { a: 5, b: -6 },
                    flags: Buf::new([7, 8]),
                    valid: true,
                };
                p.x = p.x + 1u16;
                p.inner.a += 10u16;
                p.flags[1u16] = 9u16;
                p.y = p.flags[0u16];
                let copy: Point = p;
                let total = copy.x + copy.y + copy.inner.a + (copy.inner.b as u16)
                    + copy.flags[0u16] + copy.flags[1u16] + (copy.valid as u16);
                halt(total);
            }
        "#;
        // x = 3 + 1 = 4, y = flags[0] = 7, inner.a = 5 + 10 = 15, inner.b = 0xfffa,
        // flags = {7, 9}, valid = 1
        let expected = 4u16
            .wrapping_add(7)
            .wrapping_add(15)
            .wrapping_add((-6i16) as u16)
            .wrapping_add(7)
            .wrapping_add(9)
            .wrapping_add(1);
        assert_eq!(run_with_std_capped(source, 20_000), expected);
    }

    #[test]
    fn struct_copy_is_an_independent_value() {
        let source = r#"
            struct Pair {
                a: u16,
                b: u16,
            }
            fn main() {
                let mut first: Pair = Pair { a: 1, b: 2 };
                let mut second: Pair = first;
                second.a = 40;
                first.b = 3;
                halt(first.a + first.b + second.a + second.b);
            }
        "#;
        // the copy took its own slot: 1 + 3 + 40 + 2 = 46
        assert_eq!(run_with_std_capped(source, 20_000), 46);
    }

    #[test]
    fn struct_views_pass_by_pointer_and_index_fields() {
        let source = r#"
            struct Point { x: u16, y: u16 }

            fn sum_x(points: Array<Point>, count: u16) -> u16 {
                let mut total: u16 = 0;
                let mut i: u16 = 0;
                while i < count {
                    total = total + points[i].x;
                    i = i + 1u16;
                }
                total
            }

            fn bump(mut view: Array<Point>, dx: u16) {
                view[0u16].x = view[0u16].x + dx;
            }

            fn main() {
                let arr: Buf<Point, 3> = Buf::new([
                    Point { x: 10, y: 1 },
                    Point { x: 20, y: 2 },
                    Point { x: 30, y: 3 },
                ]);
                let view = arr.as_array();
                bump(view, 5u16);
                let total = sum_x(view, 3u16);
                let mut single: Point = Point { x: 7, y: 0 };
                bump(view_of(&single), 100u16);
                halt(total + single.x);
            }
        "#;
        // bump(view) makes x = 15, so sum_x = 65; bump(view_of(&single)) makes 107
        assert_eq!(run_with_std_capped(source, 40_000), 172);
    }

    #[test]
    fn struct_view_index_scales_by_the_element_size() {
        // five words per element: a runtime index needs a real multiply, not a shift
        let source = r#"
            struct Wide { a: u16, b: u16, c: u16, d: u16, e: u16 }

            fn middle(view: Array<Wide>, i: u16) -> u16 {
                view[i].c
            }

            fn main() {
                let arr: Buf<Wide, 3> = Buf::new([
                    Wide { a: 1, b: 2, c: 3, d: 4, e: 5 },
                    Wide { a: 10, b: 20, c: 30, d: 40, e: 50 },
                    Wide { a: 100, b: 200, c: 300, d: 400, e: 500 },
                ]);
                let view = arr.as_array();
                let mut acc: u16 = 0;
                let mut i: u16 = 0;
                while i < 3u16 {
                    acc = acc + middle(view, i) + view[i].e;
                    i = i + 1u16;
                }
                halt(acc);
            }
        "#;
        // (3 + 5) + (30 + 50) + (300 + 500) = 888
        assert_eq!(run_with_std_capped(source, 40_000), 888);
    }

    #[test]
    fn tuples_return_destructure_and_copy() {
        let source = r#"
            fn divmod_pair(a: u16, b: u16) -> (u16, u16) {
                (a / b, a % b)
            }

            fn stats(x: u16, y: u16) -> (u16, u16, u16) {
                let lo = if x < y { x } else { y };
                let hi = if x < y { y } else { x };
                (lo, hi, hi - lo)
            }

            fn main() {
                let t = divmod_pair(47u16, 5u16);
                let pair: (u16, u16) = t;
                let (q, r) = pair;
                let (lo, hi, span) = stats(9u16, 4u16);
                halt(q + r + lo + hi + span + t.0);
            }
        "#;
        // divmod_pair(47, 5) = (9, 2), stats(9, 4) = (4, 9, 5)
        assert_eq!(run_with_std_capped(source, 60_000), 9 + 2 + 4 + 9 + 5 + 9);
    }

    #[test]
    fn struct_returns_write_through_the_hidden_pointer() {
        let source = r#"
            struct Point { x: u16, y: u16 }

            fn make(x: u16, y: u16) -> Point {
                Point { x: x, y: y }
            }

            fn shifted(p: Array<Point>, dx: u16) -> Point {
                Point { x: p[0u16].x + dx, y: p[0u16].y }
            }

            fn pass_through(p: Array<Point>) -> Point {
                shifted(p, 1u16)
            }

            fn main() {
                let p: Point = make(3u16, 4u16);
                let q: Point = pass_through(view_of(&p));
                let mut r: Point = q;
                r = make(10u16, 20u16);
                halt(p.x + p.y + q.x + q.y + r.x + r.y);
            }
        "#;
        // p = (3, 4), q = shifted(p, 1) = (4, 4), r = make(10, 20)
        assert_eq!(run_with_std_capped(source, 60_000), 3 + 4 + 4 + 4 + 10 + 20);
    }

    #[test]
    fn buf_returns_and_struct_field_shorthand() {
        let source = r#"
            struct Pair { a: u16, b: u16 }

            fn table() -> Buf<u16, 3> {
                Buf::new([5, 6, 7])
            }

            fn pack(a: u16) -> Pair {
                Pair { a: a, b: a + 1u16 }
            }

            fn main() {
                let t: Buf<u16, 3> = table();
                let view = t.as_array();
                let p: Pair = pack(view[2u16]);
                halt(view[0u16] + p.a + p.b);
            }
        "#;
        // t = {5,6,7}; p = pack(7) = (7,8) => 5 + 7 + 8 = 20
        assert_eq!(run_with_std_capped(source, 40_000), 20);
    }

    #[test]
    fn labeled_break_and_continue_leave_the_right_loop() {
        let source = r#"
            fn main() {
                let mut hits: u16 = 0;
                let mut rows: u16 = 0;
                'outer: for i in 0u16..4 {
                    let mut j: u16 = 0;
                    while j < 4u16 {
                        j = j + 1u16;
                        if i == 2u16 && j == 2u16 {
                            break 'outer;
                        }
                        if j == 1u16 {
                            continue;
                        }
                        hits = hits + 1u16;
                    }
                    rows = rows + 1u16;
                }
                let mut acc: u16 = 0;
                'again: for i in 0u16..4 {
                    let mut j: u16 = 0;
                    while j < 4u16 {
                        j = j + 1u16;
                        if j == 2u16 {
                            continue 'again;
                        }
                        acc = acc + 1u16;
                    }
                    acc = acc + 10u16;
                }
                halt(acc * 1000u16 + rows * 100u16 + hits);
            }
        "#;
        // rows = 2 (i = 0, 1), hits = 6 (3 per row), acc = 4: `continue 'again`
        // leaves the inner loop before it can finish, so the +10 never runs
        assert_eq!(run_with_std_capped(source, 60_000), 4 * 1000 + 2 * 100 + 6);
    }

    #[test]
    fn compound_assignments_run() {
        let source = r#"
            struct P { scale: u16 }

            fn main() {
                let mut a: u16 = 3;
                a *= 5u16;              // 15
                a /= 2u16;              // 7
                a %= 4u16;              // 3
                let mut buf: Buf<u16, 3> = Buf::new([1, 2, 3]);
                let mut view = buf.as_array();
                view[1u16] *= 4u16;
                let mut p: P = P { scale: 2 };
                p.scale *= 7u16;
                halt(a + buf.as_array()[1u16] + p.scale);
            }
        "#;
        // a = ((3 * 5) / 2) % 4 = 3, buf[1] = 8, p.scale = 14
        assert_eq!(run_with_std_capped(source, 20_000), 25);
    }

    #[test]
    fn aggregate_statics_land_in_the_data_section() {
        let source = r#"
            struct Sprite { x: u16, w: u16 }
            struct Level { count: u16, origin: Sprite, tags: Buf<u16, 2> }

            static TABLE: Buf<Sprite, 2> =
                Buf::new([Sprite { x: 3, w: 5 }, Sprite { x: 7, w: 9 }]);
            static LEVEL: Level =
                Level { count: 4, origin: Sprite { x: 1, w: 2 }, tags: Buf::new([8, 6]) };
            static PAIR: (u16, u16) = (10, 20);

            fn main() {
                let view = TABLE.as_array();
                view[1u16].w = 11u16;              // writable through a view
                let copy: Sprite = TABLE.as_array()[0u16];
                halt(copy.x + view[1u16].w + LEVEL.count + LEVEL.origin.x
                    + LEVEL.tags[1u16] + PAIR.0 + PAIR.1);
            }
        "#;
        // copy.x = 3, view[1].w = 11, count = 4, origin.x = 1, tags[1] = 6, PAIR = (10, 20)
        assert_eq!(
            run_with_std_capped(source, 20_000),
            3 + 11 + 4 + 1 + 6 + 10 + 20
        );
    }

    #[test]
    fn enums_are_words_and_carry_state() {
        let source = r#"
            #[derive(PartialEq)]
            enum Trace { Idle, Run, Halt }
            struct Job { state: Trace, ticks: u16 }

            fn next(s: Trace) -> Trace {
                if s == Trace::Idle { Trace::Run } else { Trace::Halt }
            }

            fn main() {
                let mut j: Job = Job { state: Trace::Idle, ticks: 0 };
                let first = j.state as u16;
                j.state = next(j.state);
                let second = j.state as u16;
                let third = next(j.state) as u16;
                let mut buf: Buf<Trace, 2> = Buf::new([Trace::Idle, Trace::Halt]);
                let mut v = buf.as_array();
                v[0u16] = j.state;
                let r0 = v[0u16] as u16;
                let r1 = v[1u16] as u16;
                halt(first * 1000u16 + second * 100u16 + third * 10u16 + r0 + r1);
            }
        "#;
        // Idle = 0, Run = 1, Halt = 2: first(0) second(1) third(2) r0(1) r1(2)
        assert_eq!(run_with_std_capped(source, 40_000), 100 + 20 + 1 + 2);
    }

    #[test]
    fn match_lowers_to_a_branch_chain() {
        let source = r#"
            #[derive(PartialEq)]
            enum State { Idle, Run, Done }

            fn step(s: State) -> State {
                let mut next: State = State::Idle;
                match s {
                    State::Idle => { next = State::Run; }
                    State::Run => { next = State::Done; }
                    State::Done => { next = State::Idle; }
                }
                next
            }

            fn main() {
                let mut s: State = State::Idle;
                let mut acc: u16 = 0;
                let mut i: u16 = 0;
                while i < 6u16 {
                    i = i + 1u16;
                    s = step(s);
                    acc = acc + (s as u16);
                    match i {
                        3u16 => { acc = acc + 100u16; }
                        6u16 => { break; }
                        _ => { acc = acc + 1u16; }
                    }
                }
                halt(acc);
            }
        "#;
        // states cycle Run(1) Done(2) Idle(0) Run(1) Done(2) Idle(0):
        // acc = 1+2+0+1+2+0 = 6, plus a +1 on i=1,2,4,5 and +100 on i=3
        assert_eq!(run_with_std_capped(source, 60_000), 6 + 4 + 100);
    }

    fn compile(source: &str, options: CompilerOptions) -> CpuV3Program {
        let program = parse_source_with(source, options.data_base).unwrap();
        super::compile(program, &options, "main")
    }

    #[test]
    fn stored_boolean_operators_short_circuit_side_effects() {
        assert_source_in_both_modes(
            r#"
            static COUNT: u16 = 0;
            fn side(value: bool) -> bool {
                addr_of(&COUNT).write(0, COUNT + 1u16);
                value
            }
            fn choose(value: bool) -> bool { value || side(true) }
            fn main() {
                let a = false && side(true);
                let b = true || side(false);
                let c = side(true) && side(false);
                let d = side(false) || side(true);
                let e = !(false && side(true)) && choose(true);
                halt(COUNT * 100u16 + (a as u16) + (b as u16) * 2u16
                    + (c as u16) * 4u16 + (d as u16) * 8u16 + (e as u16) * 16u16);
            }
        "#,
            426,
        );
    }

    #[test]
    fn aggregate_assignments_preserve_the_rhs_and_evaluate_it_first() {
        assert_source_in_both_modes(
            r#"
            struct P { x: u16, y: u16 }
            static INDEX: u16 = 0;
            fn swap(p: Array<P>) -> P { P { x: p[0u16].y, y: p[0u16].x } }
            fn make() -> P { addr_of(&INDEX).write(0, 1); P { x: 7, y: 8 } }
            fn main() {
                let mut p: P = P { x: 1, y: 2 };
                p = P { x: p.y, y: p.x };
                if p.x != 2u16 || p.y != 1u16 { halt(10); }
                p = swap(view_of(&p));
                if p.x != 1u16 || p.y != 2u16 { halt(11); }
                let mut t: (u16, u16) = (3, 4);
                t = (t.1, t.0);
                if t.0 != 4u16 || t.1 != 3u16 { halt(12); }
                let mut b: Buf<u16, 2> = Buf::new([5, 6]);
                b = Buf::new([b[1u16], b[0u16]]);
                if b[0u16] != 6u16 || b[1u16] != 5u16 { halt(13); }
                let mut rows: Buf<P, 2> = Buf::new([P { x: 0, y: 0 }; 2]);
                rows[INDEX] = make();
                if rows[0u16].x != 0u16 || rows[1u16].x != 7u16 { halt(14); }
                halt(1);
            }
        "#,
            1,
        );
    }

    #[test]
    fn owned_buffers_and_view_fields_use_their_element_addresses() {
        assert_source_in_both_modes(
            r#"
            struct Holder { data: Array<u16>, signed: Array<i16> }
            static GLOBAL: Buf<u16, 2> = Buf::new([10, 20]);
            fn main() {
                let mut b: Buf<u16, 2> = Buf::new([41, 42]);
                let mut s: Buf<i16, 2> = Buf::new([-3, -4]);
                b[0u16] += 1u16;
                s[1i16] = -5;
                let mut h: Holder = Holder { data: b.as_array(), signed: s.as_array() };
                if h.data[1u16] != 42u16 || h.signed[1u16] != -5i16 { halt(10); }
                h.data[0u16] = 50;
                h.signed[1u16] += 2i16;
                let i: u16 = 1;
                GLOBAL.as_array()[i] = b[i];
                halt(b[0u16] + GLOBAL[i] + (s[1u16] + 3i16) as u16);
            }
        "#,
            92,
        );
    }

    #[test]
    fn buffer_struct_layout_dependencies_ignore_name_order() {
        assert_source_in_both_modes(
            r#"
            struct A { data: Buf<Z, 2> }
            struct Z { x: u16, y: u16 }
            fn main() {
                let a: A = A { data: Buf::new([Z { x: 1, y: 2 }, Z { x: 3, y: 4 }]) };
                halt(a.data[0u16].y + a.data[1u16].x);
            }
        "#,
            5,
        );
    }

    #[test]
    fn division_boundaries_match_host_quotients_and_remainders() {
        let mut source = String::from(
            r#"
            fn unsigned(a: u16, b: u16, q: u16, r: u16) -> bool {
                let mut cq = a; cq /= b;
                let mut cr = a; cr %= b;
                a / b == q && a % b == r && cq == q && cr == r
            }
            fn signed(a: i16, b: i16, q: i16, r: i16) -> bool {
                let mut cq = a; cq /= b;
                let mut cr = a; cr %= b;
                a / b == q && a % b == r && cq == q && cr == r
            }
            fn main() {
        "#,
        );
        for (a, b) in [
            (0u16, 0u16),
            (1, 0),
            (65535, 0),
            (65535, 1),
            (65535, 2),
            (65535, 32768),
            (65535, 65535),
            (32768, 65535),
            (32768, 32767),
            (12345, 257),
            (40000, 3),
        ] {
            let q = a.checked_div(b).unwrap_or(0);
            let r = a.checked_rem(b).unwrap_or(a);
            source.push_str(&format!(
                "if !unsigned({a}u16, {b}u16, {q}u16, {r}u16) {{ halt(10); }}\n"
            ));
        }
        for (a, b) in [
            (i16::MIN, -1i16),
            (i16::MIN, 1),
            (i16::MIN, i16::MIN),
            (i16::MIN, 0),
            (i16::MAX, -1),
            (i16::MAX, i16::MIN),
            (-1000, 7),
            (1000, -7),
            (-1000, -7),
            (-1, 2),
            (1, -2),
            (0, -1),
        ] {
            let q = if b == 0 { 0 } else { a.wrapping_div(b) };
            let r = if b == 0 { a } else { a.wrapping_rem(b) };
            source.push_str(&format!(
                "if !signed({a}i16, {b}i16, {q}i16, {r}i16) {{ halt(11); }}\n"
            ));
        }
        source.push_str("halt(1); }");
        assert_source_in_both_modes(&source, 1);
    }

    #[test]
    fn recursive_sret_preserves_all_argument_registers_and_nested_destinations() {
        assert_source_in_both_modes(
            r#"
            struct Record { sum: u16, values: Buf<u16, 3> }
            struct Outer { item: Record, sentinel: u16 }
            fn make(a: u16, b: u16, c: u16, d: u16, e: u16) -> Record {
                Record { sum: a + b + c + d + e, values: Buf::new([a, b, c]) }
            }
            fn recur(n: u16, a: u16, b: u16, c: u16, d: u16) -> Record {
                if n == 0u16 { return make(a, b, c, d, 99u16); }
                let result: Record = recur(n - 1u16, a + 1u16, b + 2u16, c + 3u16, d + 4u16);
                return result;
            }
            fn main() {
                let mut outer: Outer = Outer { item: recur(3u16, 1u16, 2u16, 3u16, 4u16), sentinel: 777 };
                let saved: Record = outer.item;
                outer.item = recur(0u16, 9u16, 8u16, 7u16, 6u16);
                if saved.sum != 139u16 || saved.values[0u16] != 4u16
                    || saved.values[1u16] != 8u16 || saved.values[2u16] != 12u16 { halt(10); }
                if outer.item.sum != 129u16 || outer.item.values[0u16] != 9u16
                    || outer.item.values[1u16] != 8u16 || outer.item.values[2u16] != 7u16
                    || outer.sentinel != 777u16 { halt(11); }
                halt(1);
            }
        "#,
            1,
        );
    }

    #[test]
    fn match_evaluates_once_and_composes_with_labeled_loop_exits() {
        assert_source_in_both_modes(
            r#"
            static CALLS: u16 = 0;
            fn observe(x: u16) -> u16 { addr_of(&CALLS).write(0, CALLS + 1u16); x }
            fn classify(x: i16) -> u16 {
                let mut result: u16 = 0;
                match x {
                    -32768i16 => { result = 3; }
                    -1i16 => { result = 5; }
                    _ => { result = 7; }
                }
                result
            }
            fn main() {
                let mut total: u16 = 0;
                'rows: for row in 0u16..4 {
                    for col in 0u16..3 {
                        match observe(row) {
                            1u16 => { continue 'rows; }
                            3u16 => { break 'rows; }
                            _ => { total += row * 10u16 + col; }
                        }
                    }
                }
                // row 0 contributes 3, row 2 contributes 63; 3+1+3+1 observations.
                halt(total + CALLS * 100u16 + classify(-32768i16) + classify(-1i16) + classify(0i16));
            }
        "#,
            881,
        );
    }

    #[test]
    fn enum_buffer_and_bool_tuple_survive_indirect_calls() {
        assert_source_in_both_modes(
            r#"
            #[derive(PartialEq)] enum State { Idle, Run, Done }
            fn next(s: State) -> State {
                if s == State::Idle { State::Run } else { State::Done }
            }
            fn flags(s: State) -> (bool, bool, u16) { (s == State::Run, s == State::Done, s as u16) }
            fn main() {
                let mut states: Buf<State, 3> = Buf::new([State::Idle, State::Run, State::Done]);
                let advance: fn(State) -> State = next;
                let mut total: u16 = 0;
                for i in 0u16..3 {
                    states[i] = advance(states[i]);
                    let (running, done, tag) = flags(states[i]);
                    if running { total += 10u16; }
                    if done { total += 100u16; }
                    total += tag;
                }
                halt(total);
            }
        "#,
            215,
        );
    }

    fn assert_source_in_both_modes(source: &str, expected: u16) {
        for opt in [rcc::Opts::default(), rcc::Opts::disabled()] {
            let options = CompilerOptions {
                opt,
                ..CompilerOptions::default()
            };
            let program = rcc::frontend::compile_program_named(
                "<regression>",
                source,
                &options,
                &mut |name| Err(format!("unknown module `{name}`")),
            )
            .unwrap();
            let compiled = super::compile(program, &options, "main");
            assert_eq!(execute_capped(compiled, 100_000).0, expected);
        }
    }

    /// Compile with the rcc standard library appended. `compile` above is
    /// parse-only, so operators that lower to a library call (such as `/`) and
    /// the library modules themselves need this entry point.
    fn compile_with_std(source: &str) -> CpuV3Program {
        let options = CompilerOptions::default();
        let program =
            rcc::frontend::compile_program_named("<test>", source, &options, &mut |name| {
                Err(format!("unknown module `{name}`"))
            })
            .unwrap();
        super::compile(program, &options, "main")
    }

    fn execute(program: CpuV3Program) -> (u16, cpu_v3::CpuV3Sim) {
        execute_capped(program, 10_000)
    }

    fn execute_capped(program: CpuV3Program, max_cycles: usize) -> (u16, cpu_v3::CpuV3Sim) {
        let mut machine = cpu_v3::CpuV3Sim::default();
        machine
            .load_program(program.code_base, &program.words)
            .unwrap();
        if program.code_base != 0 {
            let mut bootstrap = cpu_v3::load_immediate16(REG_TMP, program.code_base).to_vec();
            bootstrap.push(cpu_v3::jump_register(REG_TMP));
            machine.load_program(0, &bootstrap).unwrap();
        }
        let signal = match machine.run(max_cycles).unwrap() {
            cpu_v3::RunOutcome::Halted { signal, .. } => signal,
            outcome => panic!("CpuV3 program did not halt: {outcome:?}"),
        };
        (signal, machine)
    }

    fn run(source: &str) -> u16 {
        run_with_options(source, CompilerOptions::default()).0
    }

    fn run_with_options(source: &str, options: CompilerOptions) -> (u16, cpu_v3::CpuV3Sim) {
        execute(compile(source, options))
    }

    fn run_with_std(source: &str) -> u16 {
        execute(compile_with_std(source)).0
    }

    /// like `run_with_std`, for a program that needs more than the default cap
    fn run_with_std_capped(source: &str, max_cycles: usize) -> u16 {
        execute_capped(compile_with_std(source), max_cycles).0
    }

    fn disasm(program: &CpuV3Program) -> Vec<cpu_v3::DisasmLine> {
        cpu_v3::disassemble_words(&program.words, program.code_base)
    }

    fn mnemonics(program: &CpuV3Program) -> Vec<String> {
        disasm(program).into_iter().map(|line| line.text).collect()
    }

    fn has_prefix(program: &CpuV3Program, prefix: &str) -> bool {
        mnemonics(program).iter().any(|m| m.starts_with(prefix))
    }

    #[test]
    fn frontend_ir_runs_arithmetic_loop_and_signed_comparisons() {
        let source = r#"
            fn main() {
                let mut sum: u16 = 0;
                let mut i: u16 = 5;
                while i != 0 { sum = sum + i; i = i - 1; }
                let low: i16 = -32768;
                let high: i16 = 32767;
                if low < high { halt(sum); } else { halt(99); }
            }
        "#;
        assert_eq!(run(source), 15);
    }

    #[test]
    fn direct_and_indirect_calls_follow_the_cpu_v3_abi() {
        let source = r#"
            fn add(a: u16, b: u16) -> u16 { a + b }
            fn main() {
                let f: fn(u16, u16) -> u16 = add;
                halt(add(7, 8) + f(10, 20));
            }
        "#;
        assert_eq!(run(source), 45);
    }

    #[test]
    fn nonzero_code_base_relocates_entry_and_function_addresses() {
        let source = r#"
            fn add(a: u16, b: u16) -> u16 { a + b }
            fn main() {
                let f: fn(u16, u16) -> u16 = add;
                halt(f(20, 22));
            }
        "#;
        let options = CompilerOptions {
            code_base: 0x0200,
            ..CompilerOptions::default()
        };
        let program = compile(source, options.clone());
        assert_eq!(program.code_base, 0x0200);
        assert!(program.listing.contains("main @ 0x0200"));
        assert_eq!(run_with_options(source, options).0, 42);
    }

    #[test]
    fn unsigned_comparisons_handle_values_near_the_wrap_boundary() {
        let source = r#"
            fn main() {
                let high: u16 = 0xffff;
                let low: u16 = 15;
                if high > low && low < high && high >= 0xffff && low <= 15 {
                    halt(7);
                } else {
                    halt(99);
                }
            }
        "#;
        assert_eq!(run(source), 7);
    }

    #[test]
    fn local_arrays_and_bit_intrinsics_use_the_new_stack_and_operations() {
        let source = r#"
            fn main() {
                let mut words: Buf<u16, 4> = Buf::new([1, 2, 4, 8]);
                let mut view = words.as_array();
                view[2u16] = 0x800f;
                halt(view[0u16] + cnt1(view[2u16]) + log2(view[2u16]));
            }
        "#;
        assert_eq!(run(source), 21);
    }

    #[test]
    fn device_intrinsics_use_the_dedicated_device_path() {
        let source = r#"
            const ECHO_DEVICE: u16 = 1 + 1;
            const ECHO_CHANNEL: u16 = 1 + 2;
            fn main() {
                dev_send(ECHO_DEVICE, ECHO_CHANNEL, 0x1234);
                halt(dev_recv(ECHO_DEVICE, ECHO_CHANNEL));
            }
        "#;
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
        let program = compile(source, CompilerOptions::default());
        let mut machine = cpu_v3::CpuV3Sim::default();
        machine.load_program(0, &program.words).unwrap();
        machine.attach_device(2, Box::new(EchoDevice([0; 16])));
        let signal = match machine.run(10_000).unwrap() {
            cpu_v3::RunOutcome::Halted { signal, .. } => signal,
            outcome => panic!("CpuV3 program did not halt: {outcome:?}"),
        };
        assert_eq!(signal, 0x1234);
        assert_eq!(machine.memory(0xff23), 0);
        assert_eq!(machine.device::<EchoDevice>(2).unwrap().0[3], 0x1234);
    }

    #[test]
    fn segment_intrinsics_switch_data_and_code_segments() {
        let source = r#"
            fn main() {
                mtsr_dseg(1);
                let mut a = Ptr::from_addr(0x0010).as_u16_array();
                a[0u16] = 0x1234;
                jseg(2, 0x0020);
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        let mut machine = cpu_v3::CpuV3Sim::default();
        machine
            .load_program(program.code_base, &program.words)
            .unwrap();
        let mut app = cpu_v3::load_immediate16(0, 0x77).to_vec();
        app.push(cpu_v3::halt());
        machine.load_segment(2, 0x0020, &app).unwrap();
        assert!(matches!(
            machine.run(1_000),
            Ok(cpu_v3::RunOutcome::Halted { signal: 0x77, .. })
        ));
        assert_eq!(machine.code_segment(), 2);
        assert_eq!(machine.data_segment(), 1);
        assert_eq!(
            machine.physical_memory(cpu_v3::PhysicalWordAddress::new(0x0001_0010)),
            0x1234
        );
    }

    #[test]
    fn cache_handoff_is_one_terminal_ir_operation_and_two_adjacent_words() {
        let source = r#"
            fn main() {
                dcache_invalidate_all();
                let cseg: u16 = 2;
                let target: u16 = 0x0020;
                icache_invalidate_delayed_and_jump(cseg, target);
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert_eq!(program.words.last(), Some(&cpu_v3::halt()));
        let tail = &program.words[program.words.len() - 3..program.words.len() - 1];
        assert_eq!(tail[0] & 0xfff0, 0x7800);
        assert_eq!(tail[1] & 0xff00, 0x6f00);
        assert_eq!(tail[0] & 0x000f, (tail[1] >> 4) & 0x000f);
        assert!(program.words[..program.words.len() - 3]
            .iter()
            .any(|word| word & 0xfff0 == 0x7810));
    }

    #[test]
    fn segment_switch_mirror_loop_copies_across_data_segments() {
        let source = r#"
            fn main() {
                let desc = Ptr::from_addr(0x1000).as_u16_array();
                let mut handoff = Ptr::from_addr(0x0100).as_u16_array();
                let hseg: u16 = 2;
                let mut i: u16 = 0;
                while i < 32 {
                    let w = desc[i];
                    mtsr_dseg(hseg);
                    handoff[i] = w;
                    mtsr_dseg(0);
                    i = i + 1;
                }
                halt(0);
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        let mut machine = cpu_v3::CpuV3Sim::default();
        machine
            .load_program(program.code_base, &program.words)
            .unwrap();
        let source_words: [u16; 32] = std::array::from_fn(|i| 0x100 + i as u16);
        machine.load_program(0x1000, &source_words).unwrap();
        match machine.run(100_000) {
            Ok(cpu_v3::RunOutcome::Halted { .. }) => {}
            other => panic!("mirror loop failed: {other:?}"),
        }
        for (i, &w) in source_words.iter().enumerate() {
            assert_eq!(
                machine.physical_memory(cpu_v3::PhysicalWordAddress::new(0x0002_0100 + i as u32)),
                w,
                "word {i}"
            );
        }
        assert_eq!(machine.data_segment(), 0);
    }

    #[test]
    fn non_overlapping_static_data_initializes_unified_memory() {
        let source = "static VALUE: u16 = 77; fn main() { halt(VALUE); }";
        let options = CompilerOptions {
            data_base: 0x4000,
            ..CompilerOptions::default()
        };
        let (signal, machine) = run_with_options(source, options);
        assert_eq!(signal, 77);
        assert_eq!(machine.memory(0x4000), 77);
    }

    #[test]
    fn unified_code_and_static_data_overlap_is_rejected() {
        let source = "static VALUE: u16 = 7; fn main() { halt(VALUE); }";
        let options = CompilerOptions {
            data_base: 0,
            ..CompilerOptions::default()
        };
        let result = std::panic::catch_unwind(|| compile(source, options));
        let error = result.expect_err("overlapping CpuV3 code and static data must fail");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(message.contains("CpuV3 unified-memory image"), "{message}");
    }

    #[test]
    fn device_indices_above_seven_are_rejected() {
        let result = std::panic::catch_unwind(|| {
            compile(
                "const INVALID_DEVICE: u16 = 8;\nconst TEST_CHANNEL: u16 = 0;\nfn main() { dev_send(INVALID_DEVICE, TEST_CHANNEL, 0); halt(0); }",
                CompilerOptions::default(),
            )
        });
        let error = result.expect_err("CpuV3 v0.6 supports only eight devices");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(message.contains("device index 8"), "{message}");
    }

    #[test]
    fn zero_stack_pointer_denotes_the_top_of_the_segment() {
        let source = r#"
            fn main() {
                let words: Buf<u16, 2> = Buf::new([0x1234, 0x4321]);
                let view = words.as_array();
                halt(view[0u16] + view[1u16]);
            }
        "#;
        assert_eq!(run(source), 0x5555);
    }

    #[test]
    fn dynamic_shifts_select_register_count_encodings() {
        // Parameters keep the operands unknown so the optimizer cannot fold
        // the operations into a single constant.
        let source = r#"
            fn sh(a: u16, n: u16) -> u16 { a << n }
            fn lsr(a: u16, n: u16) -> u16 { a >> n }
            fn asr(a: i16, n: u16) -> i16 { a >> n }
            fn main() {
                let a: u16 = 3;
                let n: u16 = 1;
                let s: i16 = -8;
                halt(sh(a, n) + lsr(a, n) + (asr(s, n) as u16));
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(has_prefix(&program, "shl "), "{}", program.listing);
        assert!(has_prefix(&program, "shr "), "{}", program.listing);
        assert!(has_prefix(&program, "asr "), "{}", program.listing);
        assert!(!has_prefix(&program, "shli"), "{}", program.listing);
        // 6 + 1 + 0xfffc wraps to 3
        assert_eq!(run(source), 3);
    }

    #[test]
    fn constant_shifts_select_immediate_encodings() {
        let source = r#"
            fn sh(a: u16) -> u16 { a << 2 }
            fn shr(a: u16) -> u16 { a >> 1 }
            fn asr(a: i16) -> i16 { a >> 1 }
            fn main() {
                let a: u16 = 3;
                let s: i16 = -8;
                halt(sh(a) + shr(a) + (asr(s) as u16));
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(has_prefix(&program, "shli"), "{}", program.listing);
        assert!(has_prefix(&program, "shri"), "{}", program.listing);
        assert!(has_prefix(&program, "asri"), "{}", program.listing);
        // 12 + 1 + 0xfffc wraps to 9
        assert_eq!(run(source), 9);
    }

    #[test]
    fn immediate_arithmetic_families_select_short_and_wide_forms() {
        let source = r#"
            fn calc(x: u16) -> u16 {
                let a = x + 5;
                let b = a - 3;
                let c = b & 0x00ff; // does not fit u4: wide PFX12 form
                let d = c | 6;
                d ^ 7
            }
            fn main() { halt(calc(100)); }
        "#;
        let program = compile(source, CompilerOptions::default());
        for mnemonic in ["addi", "subi", "andi", "ori", "xori"] {
            assert!(
                has_prefix(&program, mnemonic),
                "missing {mnemonic}\n{}",
                program.listing
            );
        }
        assert!(
            disasm(&program)
                .iter()
                .any(|line| line.wide && line.text.starts_with("andi")),
            "andi 0x00ff must use the wide form\n{}",
            program.listing
        );
        // 100 + 5 - 3 = 102; & 0xff = 102; | 6 = 102; ^ 7 = 97
        assert_eq!(run(source), 97);
    }

    #[test]
    fn wide_immediate_comparisons_use_pfx12() {
        let source = r#"
            fn wide(x: u16) { if x < 1000 { halt(1); } else { halt(0); } }
            fn main() { wide(500); }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(
            disasm(&program)
                .iter()
                .any(|line| line.wide && line.text.starts_with("cmpui")),
            "{}",
            program.listing
        );
        assert_eq!(run(source), 1);
    }

    #[test]
    fn constant_on_the_left_swaps_the_condition() {
        // 40 > x must become x < 40 (not x >= 40), and -8 <= s must become
        // s >= -8, so the immediate compare keeps the written meaning.
        let source = r#"
            fn above(x: u16) { if 40 > x { halt(1); } else { halt(0); } }
            fn at_least(s: i16) { if -8 <= s { halt(2); } else { halt(0); } }
            fn main() { above(30); at_least(-5); }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(has_prefix(&program, "cmpui"), "{}", program.listing);
        assert!(has_prefix(&program, "cmpsi"), "{}", program.listing);
        // above(30): 40 > 30 is true
        assert_eq!(run(source), 1);
    }

    #[test]
    fn immediate_selection_runs_with_optimizations_disabled() {
        // Legalization and immediate selection must not depend on the
        // optimizer; only the CFG-changing diamond conversion is opt-gated.
        let source = r#"
            fn calc(x: u16) -> u16 { x + 5 }
            fn main() { halt(calc(1)); }
        "#;
        let options = CompilerOptions {
            opt: rcc::Opts::disabled(),
            ..CompilerOptions::default()
        };
        let program = compile(source, options.clone());
        assert!(has_prefix(&program, "addi"), "{}", program.listing);
        assert_eq!(run_with_options(source, options).0, 6);
    }

    #[test]
    fn constant_comparisons_select_immediate_encodings() {
        // Statement branches stay branches (no diamond conversion), so the
        // compare must select CMPSI/CMPUI rather than a register compare.
        let source = r#"
            fn ucmp(x: u16) { if x < 40 { halt(1); } else { halt(0); } }
            fn scmp(s: i16) { if s >= -8 { halt(2); } else { halt(0); } }
            fn main() { ucmp(30); scmp(-5); }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(has_prefix(&program, "cmpui"), "{}", program.listing);
        assert!(has_prefix(&program, "cmpsi"), "{}", program.listing);
        assert_eq!(run(source), 1);
    }

    #[test]
    fn destructive_operations_copy_the_source_when_both_remain_live() {
        // `a` is live across the shift and multiply, so the destructive forms
        // cannot reuse its register and must insert a MOV first.
        let source = r#"
            fn mix(x: u16) -> u16 {
                let a = x & 0x00ff;
                let b = a + 1;
                let s = a << 1;
                let m = a * 3;
                a + b + s + m
            }
            fn main() { halt(mix(100)); }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(has_prefix(&program, "mov "), "{}", program.listing);
        assert!(has_prefix(&program, "shli"), "{}", program.listing);
        assert!(has_prefix(&program, "muli"), "{}", program.listing);
        // a = 100, b = 101, s = 200, m = 300
        assert_eq!(run(source), 701);
    }

    #[test]
    fn if_value_diamonds_become_boolean_comparisons_and_conditional_moves() {
        let source = r#"
            fn classify(x: u16) -> u16 {
                let flag = if x < 40 { 1 } else { 0 };
                let m = if x < 40 { x } else { 0 };
                let n = if x > 40 { x } else { 7 };
                flag + m + n
            }
            fn main() { halt(classify(30)); }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(has_prefix(&program, "sltui"), "{}", program.listing);
        assert!(
            ["moveq", "movne", "movlt", "movge", "movgt", "movle"]
                .iter()
                .any(|m| has_prefix(&program, m)),
            "no conditional move\n{}",
            program.listing
        );
        // flag = 1, m = 30, n = 7
        assert_eq!(run(source), 38);
    }

    #[test]
    fn if_value_arms_with_one_simple_instruction_use_a_conditional_move() {
        // `c3 - 1` is a single pure instruction, so the else arm stays
        // if-convertible; the countdown wraps 0 -> 2.
        let source = r#"
            fn countdown(c3: u16) -> u16 {
                if c3 == 0 { 2 } else { c3 - 1 }
            }
            fn main() {
                halt(countdown(0) * 100 + countdown(5));
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(
            ["moveq", "movne", "movlt", "movge", "movgt", "movle"]
                .iter()
                .any(|m| has_prefix(&program, m)),
            "no conditional move\n{}",
            program.listing
        );
        assert!(has_prefix(&program, "subi"), "{}", program.listing);
        // countdown(0) = 2, countdown(5) = 4
        assert_eq!(run(source), 204);
    }

    #[test]
    fn if_value_with_a_register_false_arm_keeps_the_conditional_move() {
        // Regression: the `Mov dst, false` must survive CSE copy propagation,
        // which previously aliased it with the redefined result and dropped the
        // conditional write.
        let source = r#"
            fn clamp(x: u16) -> u16 { if x == 0 { 2 } else { x } }
            fn main() { halt(clamp(0) * 100 + clamp(5)); }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(
            ["moveq", "movne", "movlt", "movge", "movgt", "movle"]
                .iter()
                .any(|m| has_prefix(&program, m)),
            "no conditional move\n{}",
            program.listing
        );
        // clamp(0) = 2, clamp(5) = 5
        assert_eq!(run(source), 205);
    }

    #[test]
    fn multiply_lowers_to_hardware_windows_and_muli() {
        let source = r#"
            fn products(a: u16, b: u16) -> u16 {
                let lo = a * b;
                let w8 = mul8(a, b);
                let w16 = mul16(a, b);
                let m = a * 9;
                lo ^ w8 ^ w16 ^ m
            }
            fn main() { halt(products(0x00ff, 0x00ff)); }
        "#;
        let program = compile(source, CompilerOptions::default());
        for mnemonic in ["mul0", "mul8", "mul16", "muli"] {
            assert!(
                has_prefix(&program, mnemonic),
                "missing {mnemonic}\n{}",
                program.listing
            );
        }
        // 0xfe01 ^ 0x00fe ^ 0x0000 ^ 0x08f7 = 0xf608
        assert_eq!(run(source), 0xf608);
    }

    #[test]
    fn byte_sign_extend_and_leading_zero_count_use_their_instructions() {
        let source = r#"
            fn extend(x: u16) -> i16 { sextb(x) }
            fn bits(x: u16) -> u16 { clz(x) }
            fn main() {
                let a: i16 = extend(0x00ff); // -1
                let b: u16 = bits(0x8000);   // 0
                let c: u16 = bits(0x0001);   // 15
                halt((a as u16) + b + c);
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert!(has_prefix(&program, "sextb"), "{}", program.listing);
        assert!(has_prefix(&program, "clz"), "{}", program.listing);
        // 0xffff + 0 + 15 wraps to 14
        assert_eq!(run(source), 14);
    }

    #[test]
    fn halt_and_signal_use_the_signal_encoding() {
        let source = r#"
            fn main() {
                signal(1, 5);
                signal(15, 6);
                halt(7);
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        let signals = mnemonics(&program)
            .iter()
            .filter(|m| m.starts_with("signal"))
            .count();
        assert_eq!(signals, 2, "{}", program.listing);
        assert!(has_prefix(&program, "halt"), "{}", program.listing);
        assert_eq!(run(source), 7);
    }

    #[test]
    fn special_register_reads_lower_to_mfsr() {
        let source = r#"
            fn main() {
                let c = read_cseg();
                let d = read_dseg();
                signal(1, c + d);
                halt(c + d);
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert_eq!(
            mnemonics(&program)
                .iter()
                .filter(|m| m.starts_with("mfsr"))
                .count(),
            2,
            "{}",
            program.listing
        );
        assert_eq!(run(source), 0);
    }

    #[test]
    fn branch_relaxation_selects_short_and_wide_forms() {
        // near target: the conditional branch relaxes to the one-word form
        let near = compile(
            r#"
                fn pick(x: u16) { if x == 1 { halt(7); } else { halt(9); } }
                fn main() { pick(1); }
            "#,
            CompilerOptions::default(),
        );
        assert!(
            disasm(&near).iter().any(|line| {
                !line.wide
                    && ["beq", "bne", "blt", "bge", "bgt", "ble"]
                        .iter()
                        .any(|c| line.text.starts_with(c))
            }),
            "expected a short branch\n{}",
            near.listing
        );

        // far target: a 150-instruction loop body makes the back edge exceed
        // the signed 8-bit range, so it must stay in the wide form
        let mut body = String::new();
        for _ in 0..150 {
            body.push_str("acc = acc + x;\n");
        }
        let far_source = format!(
            "fn loopy(x: u16) {{ let mut i: u16 = 0; let mut acc: u16 = 0; \
             while i < 10 {{ {body} i = i + 1; }} halt(acc); }}\nfn main() {{ loopy(1); }}"
        );
        let far = compile(&far_source, CompilerOptions::default());
        assert!(
            disasm(&far).iter().any(|line| {
                line.wide
                    && (["beq", "bne", "blt", "bge", "bgt", "ble", "jrel"]
                        .iter()
                        .any(|c| line.text.starts_with(c)))
            }),
            "expected a wide branch/jump\n{}",
            far.listing
        );
        // 10 iterations * 150 additions of x = 1
        assert_eq!(run(&far_source), 1500);
    }

    #[test]
    fn invalid_constant_arguments_are_source_diagnostics() {
        // Invalid device/channel constants are rejected by the frontend with a
        // source diagnostic instead of reaching the lowering asserts.
        for (source, expected) in [
            (
                "const D: u16 = 8; fn main() { dev_recv(D, 0); halt(0); }",
                "device index 8",
            ),
            (
                "const C: u16 = 20; fn main() { dev_recv(0, C); halt(0); }",
                "channel 20",
            ),
            ("fn main() { dev_send(0, 16, 1); halt(0); }", "channel 16"),
        ] {
            let error = parse_source_with(source, 0)
                .err()
                .expect("the invalid constant must be rejected");
            assert!(
                error.to_string().contains(expected),
                "expected {expected:?} in:\n{error}"
            );
        }
    }

    #[test]
    fn fallible_compile_succeeds_for_valid_programs() {
        let options = CompilerOptions::default();
        let program = parse_source_with("fn main() { halt(1); }", options.data_base).unwrap();
        let compiled =
            super::try_compile(program, &options, "main").expect("a well-formed program compiles");
        assert_eq!(compiled.words.last(), Some(&cpu_v3::halt()));
        assert_eq!(
            validate_program(&compiled.words, compiled.code_base),
            Ok(())
        );
    }

    #[test]
    fn backend_error_display_is_a_compiler_message() {
        assert_eq!(
            BackendError::Validation(ProgramValidationError::InvalidInstruction {
                address: 0x10,
                word: 0xe800,
            })
            .to_string(),
            "illegal instruction 0xe800 at 0x0010"
        );
        assert_eq!(
            BackendError::UnknownFunction("ghost".to_string()).to_string(),
            "unknown function `ghost`"
        );
    }

    #[test]
    fn program_validator_accepts_compiled_output() {
        let source = r#"
            fn main() {
                let a: u16 = 0x1234;
                let b = a + 5;
                signal(1, b);
                halt(b);
            }
        "#;
        let program = compile(source, CompilerOptions::default());
        assert_eq!(validate_program(&program.words, program.code_base), Ok(()));
    }

    #[test]
    fn program_validator_rejects_illegal_and_malformed_images() {
        // the reserved revision-0.7 HALT word
        assert_eq!(
            validate_program(&[0xe800], 0),
            Err(ProgramValidationError::InvalidInstruction {
                address: 0,
                word: 0xe800
            })
        );
        // a prefix followed by a non-consumer, and a trailing prefix
        assert_eq!(
            validate_program(&[cpu_v3::prefix12(0), cpu_v3::halt()], 0),
            Err(ProgramValidationError::DanglingPrefix {
                address: 0,
                next: Some(cpu_v3::halt())
            })
        );
        assert_eq!(
            validate_program(&[cpu_v3::prefix12(0)], 0),
            Err(ProgramValidationError::DanglingPrefix {
                address: 0,
                next: None
            })
        );
        // relative consumers must zero payload12[11:8]
        assert_eq!(
            validate_program(
                &[
                    cpu_v3::prefix12(0x100),
                    cpu_v3::branch(cpu_v3::TestCondition::Equal, 0)
                ],
                0
            ),
            Err(ProgramValidationError::RelativePrefixHighBits {
                address: 0,
                payload: 0x100
            })
        );
        assert_eq!(
            validate_program(
                &[
                    cpu_v3::prefix12(0x001),
                    cpu_v3::branch(cpu_v3::TestCondition::Equal, 0)
                ],
                0
            ),
            Ok(())
        );
    }
}
