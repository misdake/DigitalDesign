//! CpuV3 lowering and fixed-width symbolic linking.
//!
//! Control-flow references always use the two-word wide form in this first
//! backend. This keeps layout deterministic while the ISA and hardware settle;
//! shortening is a later size optimization, not a correctness requirement.

mod options;

pub use options::CompilerOptions;

use crate as cpu_v3;
use crate::{AluOp, ImmediateOp, TestCondition, Word};
use rcc::*;
use std::collections::{HashMap, HashSet};

const REG_TMP: u8 = 12;
const REG_SP: u8 = 13;
const REG_LINK: u8 = 14;

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
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CpuV3Program {
    pub code_base: Word,
    pub words: Vec<Word>,
    pub listing: String,
}

#[derive(Clone)]
enum Line {
    Word(Word),
    Label(usize),
    Branch {
        condition: TestCondition,
        target: usize,
    },
    Jump(usize),
    Call(FuncName),
    LoadFunctionAddress {
        function: FuncName,
        dst: u8,
    },
}

impl Line {
    fn size(&self) -> usize {
        match self {
            Self::Word(_) => 1,
            Self::Label(_) => 0,
            Self::Branch { .. }
            | Self::Jump(_)
            | Self::Call(_)
            | Self::LoadFunctionAddress { .. } => 2,
        }
    }
}

struct LoweredFunction {
    name: FuncName,
    lines: Vec<Line>,
    static_addresses: Vec<u16>,
}

/// Compile target-independent RCC IR for the CpuV3 ABI and ISA.
pub fn compile(
    program: rcc::frontend::Program,
    options: &CompilerOptions,
    main: FuncName,
) -> CpuV3Program {
    let functions = program
        .funcs
        .into_iter()
        .map(|function| (function.name, function))
        .collect();
    compile_ir(functions, options, main)
}

fn compile_ir(
    functions: HashMap<FuncName, IrFunc>,
    options: &CompilerOptions,
    main: FuncName,
) -> CpuV3Program {
    assert!(
        options.stack_init != 0 && options.stack_init <= cpu_v3::MMIO_BASE,
        "CpuV3 stack_init must be in 0x0001..={:#06x}; zero wraps into the MMIO page",
        cpu_v3::MMIO_BASE
    );
    let reachable = reachable_functions(&functions, main);
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
            .unwrap_or_else(|| panic!("unknown function `{name}`"))
            .clone();
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

    link(lowered, options)
}

fn reachable_functions(functions: &HashMap<FuncName, IrFunc>, main: FuncName) -> HashSet<FuncName> {
    let mut reachable = HashSet::from([main]);
    let mut work = vec![main];
    while let Some(name) = work.pop() {
        let function = functions
            .get(name)
            .unwrap_or_else(|| panic!("unknown function `{name}`"));
        for block in function.rpo() {
            for instruction in &function.blocks[block].insts {
                if let Instr::Call { func, .. } | Instr::LoadFuncAddr { func, .. } = instruction {
                    assert!(functions.contains_key(func), "unknown function `{func}`");
                    if reachable.insert(*func) {
                        work.push(*func);
                    }
                }
            }
        }
    }
    reachable
}

fn lower_function(
    function: &IrFunc,
    allocation: &Allocation,
    is_main: bool,
    stack_init: u16,
) -> LoweredFunction {
    let mut lines = vec![];
    let mut static_addresses = vec![];
    let register = |vreg: VReg| allocation.reg[&vreg];

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
        lines.push(Line::Label(block_id));
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
                    (register(value), register(phi.dst))
                })
                .collect::<Vec<_>>();
            emit_parallel_moves(&mut lines, &moves);
        }

        for instruction in &block.insts {
            lower_instruction(
                instruction,
                &register,
                allocation,
                &mut lines,
                &mut static_addresses,
            );
        }

        match block
            .term
            .as_ref()
            .expect("reachable block is unterminated")
        {
            Terminator::Jmp { target } => {
                emit_edge_moves(function, block_id, *target, &register, &mut lines);
                if layout.get(layout_index + 1) != Some(target) {
                    lines.push(Line::Jump(*target));
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
                        lines.push(Line::Jump(*if_true));
                    }
                } else {
                    // Critical edges were split by allocation, so neither
                    // conditional successor needs edge-local phi moves here.
                    let condition = lower_comparison(cmp, &register, &mut lines);
                    if next == Some(*if_false) {
                        lines.push(Line::Branch {
                            condition,
                            target: *if_true,
                        });
                    } else if next == Some(*if_true) {
                        lines.push(Line::Branch {
                            condition: condition.invert(),
                            target: *if_false,
                        });
                    } else {
                        lines.push(Line::Branch {
                            condition,
                            target: *if_true,
                        });
                        lines.push(Line::Jump(*if_false));
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
                lines.push(Line::Word(cpu_v3::jump_register(REG_LINK)));
            }
            Terminator::Halt { signal } => {
                let signal = register(*signal);
                if signal != 0 {
                    lines.push(Line::Word(cpu_v3::move_register(0, signal)));
                }
                lines.push(Line::Word(cpu_v3::halt()));
            }
        }
    }

    LoweredFunction {
        name: function.name,
        lines,
        static_addresses,
    }
}

fn lower_instruction(
    instruction: &Instr,
    register: &dyn Fn(VReg) -> u8,
    allocation: &Allocation,
    lines: &mut Vec<Line>,
    static_addresses: &mut Vec<u16>,
) {
    match instruction {
        Instr::Bin { dst, op, lhs, rhs } => {
            let operation = match op {
                BinOp::Add => AluOp::Add,
                BinOp::Sub => AluOp::Sub,
                BinOp::And => AluOp::And,
                BinOp::Or => AluOp::Or,
                BinOp::Xor => AluOp::Xor,
            };
            lines.push(Line::Word(cpu_v3::alu(
                operation,
                register(*dst),
                register(*lhs),
                register(*rhs),
            )));
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
            if dst != src {
                lines.push(Line::Word(cpu_v3::move_register(dst, src)));
            }
            let operation = match op {
                ShiftOp::Lsl => ImmediateOp::ShiftLeft,
                ShiftOp::Lsr => ImmediateOp::ShiftRightLogical,
                ShiftOp::Asr => ImmediateOp::ShiftRightArithmetic,
            };
            lines.push(Line::Word(cpu_v3::immediate_unsigned(
                operation, dst, *amount,
            )));
        }
        Instr::Mov { dst, src } => {
            let dst = register(*dst);
            let src = register(*src);
            if dst != src {
                lines.push(Line::Word(cpu_v3::move_register(dst, src)));
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
        Instr::Call { func, .. } => lines.push(Line::Call(func)),
        Instr::LoadFuncAddr { dst, func } => lines.push(Line::LoadFunctionAddress {
            function: func,
            dst: register(*dst),
        }),
        Instr::CallPtr { .. } => lines.push(Line::Word(cpu_v3::jump_and_link_register(REG_TMP))),
        Instr::DevRecv {
            dst,
            device,
            channel,
        } => {
            check_device(*device);
            lines.push(Line::Word(cpu_v3::device_receive(
                register(*dst),
                *device,
                *channel,
            )));
        }
        Instr::DevSend {
            device,
            channel,
            src,
        } => {
            check_device(*device);
            lines.push(Line::Word(cpu_v3::device_send(
                register(*src),
                *device,
                *channel,
            )));
        }
        Instr::MtsrDseg { src } => {
            lines.push(Line::Word(cpu_v3::write_data_segment(register(*src))))
        }
        Instr::Jseg { cseg, target } => lines.push(Line::Word(cpu_v3::jump_segment(
            register(*cseg),
            register(*target),
        ))),
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
            lines.push(Line::Word(cpu_v3::move_register(dst, REG_SP)));
            emit_immediate(
                lines,
                ImmediateOp::Add,
                dst,
                u16::from(allocation.callee_saved.len() as u8 + *slot),
                true,
            );
        }
    }
}

fn lower_unary(dst: u8, operation: UnOp, src: u8, lines: &mut Vec<Line>) {
    match operation {
        UnOp::Inv => lines.push(Line::Word(cpu_v3::not(dst, src))),
        UnOp::Neg => lines.push(Line::Word(cpu_v3::negate(dst, src))),
        UnOp::Cnt1 => lines.push(Line::Word(cpu_v3::population_count(dst, src))),
        UnOp::Log2 => {
            // log2(0) is defined by rcc as zero.
            lines.push(Line::Word(cpu_v3::leading_zeros(dst, src)));
            emit_load_immediate(lines, REG_TMP, 15);
            lines.push(Line::Word(cpu_v3::alu(AluOp::Sub, dst, REG_TMP, dst)));
            let done = usize::MAX - lines.len();
            emit_test_nonzero(lines, src);
            lines.push(Line::Branch {
                condition: TestCondition::NotEqual,
                target: done,
            });
            emit_load_immediate(lines, dst, 0);
            lines.push(Line::Label(done));
        }
        UnOp::Not0 => {
            if dst != src {
                lines.push(Line::Word(cpu_v3::move_register(dst, src)));
            }
            lines.push(Line::Word(cpu_v3::immediate_signed(
                ImmediateOp::CompareEqual,
                dst,
                0,
            )));
            lines.push(Line::Word(cpu_v3::immediate_unsigned(
                ImmediateOp::Xor,
                dst,
                1,
            )));
        }
    }
}

fn check_device(device: u8) {
    assert!(
        device < 8,
        "CpuV3 device index {device} exceeds the ISA v0.5 limit of 8 devices"
    );
}

/// Emits the generic "test a value against zero" comparison used when a
/// branch tests a plain value rather than a comparison outcome.
fn emit_test_nonzero(lines: &mut Vec<Line>, test: u8) {
    lines.push(Line::Word(cpu_v3::immediate_signed(
        ImmediateOp::CompareSigned,
        test,
        0,
    )));
}

/// Lowers a branch comparison directly to a CMP-class instruction feeding a
/// conditional branch; no 0/1 value is materialized. Returns the condition
/// that branches when `cmp` holds.
fn lower_comparison(
    comparison: &Cmp,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Vec<Line>,
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
            lines.push(Line::Word(cpu_v3::compare_signed(lhs, lhs)));
            return TestCondition::Equal;
        }
        CompareOp::Never => {
            lines.push(Line::Word(cpu_v3::compare_signed(lhs, lhs)));
            return TestCondition::NotEqual;
        }
    };
    match comparison.rhs {
        CmpRhs::Reg(rhs) => {
            let rhs = register(rhs);
            lines.push(Line::Word(if comparison.signed {
                cpu_v3::compare_signed(lhs, rhs)
            } else {
                cpu_v3::compare_unsigned(lhs, rhs)
            }));
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

fn emit_edge_moves(
    function: &IrFunc,
    predecessor: BlockId,
    target: BlockId,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Vec<Line>,
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
            (register(value), register(phi.dst))
        })
        .collect::<Vec<_>>();
    emit_parallel_moves(lines, &moves);
}

fn emit_parallel_moves(lines: &mut Vec<Line>, moves: &[(u8, u8)]) {
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
            lines.push(Line::Word(cpu_v3::move_register(to, from)));
        } else {
            let target = pending[0].1;
            lines.push(Line::Word(cpu_v3::move_register(REG_TMP, target)));
            for (from, _) in &mut pending {
                if *from == target {
                    *from = REG_TMP;
                }
            }
        }
    }
}

fn emit_load_immediate(lines: &mut Vec<Line>, dst: u8, value: u16) {
    if value <= 15 {
        lines.push(Line::Word(cpu_v3::immediate_unsigned(
            ImmediateOp::LoadUnsigned,
            dst,
            value as u8,
        )));
    } else if value >= 0xfff8 {
        lines.push(Line::Word(cpu_v3::immediate_signed(
            ImmediateOp::LoadSigned,
            dst,
            value as i16,
        )));
    } else {
        lines.extend(cpu_v3::load_immediate16(dst, value).map(Line::Word));
    }
}

fn emit_immediate(
    lines: &mut Vec<Line>,
    operation: ImmediateOp,
    dst: u8,
    value: u16,
    signed_short: bool,
) {
    if signed_short && (-8..=7).contains(&(value as i16)) {
        lines.push(Line::Word(cpu_v3::immediate_signed(
            operation,
            dst,
            value as i16,
        )));
    } else if !signed_short && value <= 15 {
        lines.push(Line::Word(cpu_v3::immediate_unsigned(
            operation,
            dst,
            value as u8,
        )));
    } else {
        let consumer = 0xa000 | ((operation as u16) << 8) | (u16::from(dst) << 4);
        lines.extend(cpu_v3::prefixed(consumer, value).map(Line::Word));
    }
}

fn emit_load(lines: &mut Vec<Line>, dst: u8, base: u8, offset: i16) {
    if (-8..=7).contains(&offset) {
        lines.push(Line::Word(cpu_v3::load(dst, base, offset)));
    } else {
        lines.extend(cpu_v3::prefixed(cpu_v3::load(dst, base, 0), offset as u16).map(Line::Word));
    }
}

fn emit_store(lines: &mut Vec<Line>, src: u8, base: u8, offset: i16) {
    if (-8..=7).contains(&offset) {
        lines.push(Line::Word(cpu_v3::store(src, base, offset)));
    } else {
        lines.extend(cpu_v3::prefixed(cpu_v3::store(src, base, 0), offset as u16).map(Line::Word));
    }
}

fn link(functions: Vec<LoweredFunction>, options: &CompilerOptions) -> CpuV3Program {
    let mut function_addresses = HashMap::new();
    let code_base = usize::from(options.code_base);
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
    assert!(
        cursor <= usize::from(options.stack_init),
        "CpuV3 unified-memory image uses {cursor} code words and crosses stack_init {:#06x}",
        options.stack_init
    );
    let heap_end = options
        .heap_begin
        .checked_add(options.heap_size)
        .expect("CpuV3 heap range wraps the address space");
    assert!(
        heap_end <= options.stack_init,
        "CpuV3 heap {:#06x}..{heap_end:#06x} overlaps the stack/MMIO boundary {:#06x}",
        options.heap_begin,
        options.stack_init
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
    for function in &functions {
        let local_start = words.len();
        let start = code_base + local_start;
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
            match line {
                Line::Word(word) => words.push(*word),
                Line::Label(_) => continue,
                Line::Branch { condition, target } => {
                    let offset = relative_offset(code_base + words.len() + 2, labels[target]);
                    words.extend(wide_branch(*condition, offset));
                }
                Line::Jump(target) => {
                    let offset = relative_offset(code_base + words.len() + 2, labels[target]);
                    words.extend(wide_jump(offset));
                }
                Line::Call(function) => {
                    let offset =
                        relative_offset(code_base + words.len() + 2, function_addresses[function]);
                    words.extend(wide_call(offset));
                }
                Line::LoadFunctionAddress { function, dst } => {
                    words.extend(cpu_v3::load_immediate16(
                        *dst,
                        function_addresses[function] as u16,
                    ));
                }
            }
        }
        words.push(cpu_v3::halt());
        for (offset, word) in words[local_start..].iter().enumerate() {
            listing.push_str(&format!("  {:04x}: {:04x}\n", start + offset, word));
        }
    }
    CpuV3Program {
        code_base: options.code_base,
        words,
        listing,
    }
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

    fn compile(source: &str, options: CompilerOptions) -> CpuV3Program {
        let program = parse_source_with(source, options.data_base).unwrap();
        super::compile(program, &options, "main")
    }

    fn run(source: &str) -> u16 {
        run_with_options(source, CompilerOptions::default()).0
    }

    fn run_with_options(source: &str, options: CompilerOptions) -> (u16, cpu_v3::Machine) {
        let program = compile(source, options);
        let mut machine = cpu_v3::Machine::default();
        machine
            .load_program(program.code_base, &program.words)
            .unwrap();
        if program.code_base != 0 {
            let mut bootstrap = cpu_v3::load_immediate16(REG_TMP, program.code_base).to_vec();
            bootstrap.push(cpu_v3::jump_register(REG_TMP));
            machine.load_program(0, &bootstrap).unwrap();
        }
        let signal = match machine.run(10_000).unwrap() {
            cpu_v3::RunOutcome::Halted { signal, .. } => signal,
            outcome => panic!("CpuV3 program did not halt: {outcome:?}"),
        };
        (signal, machine)
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
                let mut words: [u16; 4] = [1, 2, 4, 8];
                let mut view = words.as_array();
                view[2u16] = 0x800f;
                halt(view[0u16] + cnt1(view[2u16]) + log2(view[2u16]));
            }
        "#;
        assert_eq!(run(source), 21);
    }

    #[test]
    fn device_intrinsics_address_the_fixed_mmio_page() {
        let source = r#"
            fn main() {
                dev_send(2, 3, 0x1234);
                halt(dev_recv(2, 3));
            }
        "#;
        let (signal, machine) = run_with_options(source, CompilerOptions::default());
        assert_eq!(signal, 0x1234);
        assert_eq!(machine.memory(0xff23), 0x1234);
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
        let mut machine = cpu_v3::Machine::default();
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
        let mut machine = cpu_v3::Machine::default();
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
                "fn main() { dev_send(8, 0, 0); halt(0); }",
                CompilerOptions::default(),
            )
        });
        let error = result.expect_err("CpuV3 v0.5 supports only eight devices");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(message.contains("device index 8"), "{message}");
    }

    #[test]
    fn legacy_wrapping_stack_configuration_is_rejected_for_cpu_v3() {
        let result = std::panic::catch_unwind(|| {
            compile(
                "fn main() { halt(0); }",
                CompilerOptions {
                    stack_init: 0,
                    ..CompilerOptions::default()
                },
            )
        });
        let error = result.expect_err("CpuV3 must reject a stack that wraps into MMIO");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            message.contains("zero wraps into the MMIO page"),
            "{message}"
        );
    }
}
