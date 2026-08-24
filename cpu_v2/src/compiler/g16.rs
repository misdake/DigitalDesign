//! G16 lowering and fixed-width symbolic linking.
//!
//! Control-flow references always use the two-word wide form in this first
//! backend. This keeps layout deterministic while the ISA and hardware settle;
//! shortening is a later size optimization, not a correctness requirement.

use crate::compiler::ir::*;
use crate::compiler::passes::optimize;
use crate::compiler::regalloc::{allocate_with_convention, Allocation, G16_REGISTER_CONVENTION};
use crate::compiler::{CompilerOptions, FuncName};
use crate::g16::{self, AluOp, BranchCondition, ImmediateOp, Word};
use crate::Cond;
use std::collections::{HashMap, HashSet};

const REG_TMP: u8 = 12;
const REG_SP: u8 = 13;
const REG_LINK: u8 = 14;
const REG_GLOBAL: u8 = 15;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct G16Program {
    pub code_base: Word,
    pub words: Vec<Word>,
    pub listing: String,
}

#[derive(Clone)]
enum Line {
    Word(Word),
    Label(usize),
    Branch {
        condition: BranchCondition,
        test: u8,
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

pub(crate) fn compile_program(
    functions: HashMap<FuncName, IrFunc>,
    options: &CompilerOptions,
    main: FuncName,
) -> G16Program {
    assert!(
        options.stack_init != 0 && options.stack_init <= g16::MMIO_BASE,
        "G16 stack_init must be in 0x0001..={:#06x}; zero wraps into the MMIO page",
        g16::MMIO_BASE
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
            allocate_with_convention(&function, options.opt.coalesce, G16_REGISTER_CONVENTION);
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

    if is_main {
        if stack_init != 0 {
            emit_load_immediate(&mut lines, REG_SP, stack_init);
        }
        emit_load_immediate(&mut lines, REG_GLOBAL, g16::MMIO_BASE);
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
                    let (condition, test) = lower_comparison(cmp, &register, &mut lines);
                    if next == Some(*if_false) {
                        lines.push(Line::Branch {
                            condition,
                            test,
                            target: *if_true,
                        });
                    } else if next == Some(*if_true) {
                        lines.push(Line::Branch {
                            condition: condition.invert(),
                            test,
                            target: *if_false,
                        });
                    } else {
                        lines.push(Line::Branch {
                            condition,
                            test,
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
                lines.push(Line::Word(g16::jump_register(REG_LINK)));
            }
            Terminator::Halt { signal } => {
                let signal = register(*signal);
                if signal != 0 {
                    lines.push(Line::Word(g16::move_register(0, signal)));
                }
                lines.push(Line::Word(g16::halt()));
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
            lines.push(Line::Word(g16::alu(
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
                lines.push(Line::Word(g16::move_register(dst, src)));
            }
            let operation = match op {
                ShiftOp::Lsl => ImmediateOp::ShiftLeft,
                ShiftOp::Lsr => ImmediateOp::ShiftRightLogical,
                ShiftOp::Asr => ImmediateOp::ShiftRightArithmetic,
            };
            lines.push(Line::Word(g16::immediate_unsigned(operation, dst, *amount)));
        }
        Instr::Mov { dst, src } => {
            let dst = register(*dst);
            let src = register(*src);
            if dst != src {
                lines.push(Line::Word(g16::move_register(dst, src)));
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
            emit_load_immediate(lines, REG_GLOBAL, *addr);
            emit_load_immediate(lines, REG_TMP, *value);
            emit_store(lines, REG_TMP, REG_GLOBAL, 0);
            emit_load_immediate(lines, REG_GLOBAL, g16::MMIO_BASE);
        }
        Instr::Call { func, .. } => lines.push(Line::Call(func)),
        Instr::LoadFuncAddr { dst, func } => lines.push(Line::LoadFunctionAddress {
            function: func,
            dst: register(*dst),
        }),
        Instr::CallPtr { .. } => {
            lines.push(Line::Word(g16::jump_and_link_register(REG_LINK, REG_TMP)))
        }
        Instr::DevRecv {
            dst,
            device,
            channel,
        } => emit_load(
            lines,
            register(*dst),
            REG_GLOBAL,
            i16::from(*device) * 16 + i16::from(*channel),
        ),
        Instr::DevSend {
            device,
            channel,
            src,
        } => emit_store(
            lines,
            register(*src),
            REG_GLOBAL,
            i16::from(*device) * 16 + i16::from(*channel),
        ),
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
            lines.push(Line::Word(g16::move_register(dst, REG_SP)));
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
        UnOp::Inv => lines.push(Line::Word(g16::not(dst, src))),
        UnOp::Neg => lines.push(Line::Word(g16::negate(dst, src))),
        UnOp::Cnt1 => lines.push(Line::Word(g16::population_count(dst, src))),
        UnOp::Log2 => {
            // log2(0) is defined by rcc as zero.
            lines.push(Line::Word(g16::leading_zeros(dst, src)));
            emit_load_immediate(lines, REG_TMP, 15);
            lines.push(Line::Word(g16::alu(AluOp::Sub, dst, REG_TMP, dst)));
            let done = usize::MAX - lines.len();
            lines.push(Line::Branch {
                condition: BranchCondition::NonZero,
                test: src,
                target: done,
            });
            emit_load_immediate(lines, dst, 0);
            lines.push(Line::Label(done));
        }
        UnOp::Not0 => {
            if dst != src {
                lines.push(Line::Word(g16::move_register(dst, src)));
            }
            lines.push(Line::Word(g16::immediate_signed(
                ImmediateOp::CompareEqual,
                dst,
                0,
            )));
            lines.push(Line::Word(g16::immediate_unsigned(
                ImmediateOp::Xor,
                dst,
                1,
            )));
        }
    }
}

fn lower_comparison(
    comparison: &Cmp,
    register: &dyn Fn(VReg) -> u8,
    lines: &mut Vec<Line>,
) -> (BranchCondition, u8) {
    let lhs = register(comparison.lhs);
    match comparison.rhs {
        CmpRhs::Reg(rhs) => lower_register_comparison(
            comparison.cond,
            comparison.signed,
            lhs,
            register(rhs),
            lines,
        ),
        CmpRhs::Imm(rhs) => {
            lower_immediate_comparison(comparison.cond, comparison.signed, lhs, rhs, lines)
        }
    }
}

fn lower_register_comparison(
    condition: Cond,
    signed: bool,
    lhs: u8,
    rhs: u8,
    lines: &mut Vec<Line>,
) -> (BranchCondition, u8) {
    if matches!(condition, Cond::Equal | Cond::NotEqual) {
        lines.push(Line::Word(g16::alu(AluOp::Xor, REG_TMP, lhs, rhs)));
        return (
            if matches!(condition, Cond::Equal) {
                BranchCondition::Zero
            } else {
                BranchCondition::NonZero
            },
            REG_TMP,
        );
    }
    let (left, right, invert) = match condition {
        Cond::Less => (lhs, rhs, false),
        Cond::GreaterEqual => (lhs, rhs, true),
        Cond::Greater => (rhs, lhs, false),
        Cond::LessEqual => (rhs, lhs, true),
        Cond::Always => return (BranchCondition::Even, REG_TMP),
        Cond::Never | Cond::Equal | Cond::NotEqual => unreachable!(),
    };
    lines.push(Line::Word(g16::move_register(REG_TMP, left)));
    lines.push(Line::Word(if signed {
        g16::set_less_than_signed(REG_TMP, right)
    } else {
        g16::set_less_than_unsigned(REG_TMP, right)
    }));
    (
        if invert {
            BranchCondition::Zero
        } else {
            BranchCondition::NonZero
        },
        REG_TMP,
    )
}

fn lower_immediate_comparison(
    condition: Cond,
    signed: bool,
    lhs: u8,
    rhs: u16,
    lines: &mut Vec<Line>,
) -> (BranchCondition, u8) {
    if matches!(condition, Cond::Equal | Cond::NotEqual) {
        lines.push(Line::Word(g16::move_register(REG_TMP, lhs)));
        emit_immediate(lines, ImmediateOp::CompareEqual, REG_TMP, rhs, true);
        return (
            if matches!(condition, Cond::Equal) {
                BranchCondition::NonZero
            } else {
                BranchCondition::Zero
            },
            REG_TMP,
        );
    }

    let direct = matches!(condition, Cond::Less | Cond::GreaterEqual);
    if direct {
        lines.push(Line::Word(g16::move_register(REG_TMP, lhs)));
        emit_immediate(
            lines,
            if signed {
                ImmediateOp::CompareLessThanSigned
            } else {
                ImmediateOp::CompareLessThanUnsigned
            },
            REG_TMP,
            rhs,
            signed,
        );
    } else {
        emit_load_immediate(lines, REG_TMP, rhs);
        lines.push(Line::Word(if signed {
            g16::set_less_than_signed(REG_TMP, lhs)
        } else {
            g16::set_less_than_unsigned(REG_TMP, lhs)
        }));
    }
    let invert = matches!(condition, Cond::GreaterEqual | Cond::LessEqual);
    (
        if invert {
            BranchCondition::Zero
        } else {
            BranchCondition::NonZero
        },
        REG_TMP,
    )
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
            lines.push(Line::Word(g16::move_register(to, from)));
        } else {
            let target = pending[0].1;
            lines.push(Line::Word(g16::move_register(REG_TMP, target)));
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
        lines.push(Line::Word(g16::immediate_unsigned(
            ImmediateOp::LoadUnsigned,
            dst,
            value as u8,
        )));
    } else if value >= 0xfff8 {
        lines.push(Line::Word(g16::immediate_signed(
            ImmediateOp::LoadSigned,
            dst,
            value as i16,
        )));
    } else {
        lines.extend(g16::load_immediate16(dst, value).map(Line::Word));
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
        lines.push(Line::Word(g16::immediate_signed(
            operation,
            dst,
            value as i16,
        )));
    } else if !signed_short && value <= 15 {
        lines.push(Line::Word(g16::immediate_unsigned(
            operation,
            dst,
            value as u8,
        )));
    } else {
        let consumer = 0xa000 | ((operation as u16) << 8) | (u16::from(dst) << 4);
        lines.extend(g16::prefixed(consumer, value).map(Line::Word));
    }
}

fn emit_load(lines: &mut Vec<Line>, dst: u8, base: u8, offset: i16) {
    if (-8..=7).contains(&offset) {
        lines.push(Line::Word(g16::load(dst, base, offset)));
    } else {
        lines.extend(g16::prefixed(g16::load(dst, base, 0), offset as u16).map(Line::Word));
    }
}

fn emit_store(lines: &mut Vec<Line>, src: u8, base: u8, offset: i16) {
    if (-8..=7).contains(&offset) {
        lines.push(Line::Word(g16::store(src, base, offset)));
    } else {
        lines.extend(g16::prefixed(g16::store(src, base, 0), offset as u16).map(Line::Word));
    }
}

fn link(functions: Vec<LoweredFunction>, options: &CompilerOptions) -> G16Program {
    let mut function_addresses = HashMap::new();
    let code_base = usize::from(options.code_base);
    let mut cursor = code_base;
    for function in &functions {
        function_addresses.insert(function.name, cursor);
        cursor += function.lines.iter().map(Line::size).sum::<usize>() + 1;
    }
    assert!(
        cursor <= 1 << 16,
        "G16 program at code_base {:#06x} exceeds the 64K code-segment window",
        options.code_base
    );
    assert!(
        cursor <= usize::from(options.data_base),
        "G16 unified-memory image uses {cursor} code words and crosses data_base {:#06x}",
        options.data_base
    );
    assert!(
        cursor <= usize::from(options.stack_init),
        "G16 unified-memory image uses {cursor} code words and crosses stack_init {:#06x}",
        options.stack_init
    );
    let heap_end = options
        .heap_begin
        .checked_add(options.heap_size)
        .expect("G16 heap range wraps the address space");
    assert!(
        heap_end <= options.stack_init,
        "G16 heap {:#06x}..{heap_end:#06x} overlaps the stack/MMIO boundary {:#06x}",
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
            "G16 unified-memory image uses {cursor} code words but static data starts at {address:#06x}; select a non-overlapping data_base"
        );
    }
    if let Some(address) = static_addresses
        .iter()
        .copied()
        .find(|address| *address >= options.heap_begin)
    {
        panic!(
            "G16 static data reaches {address:#06x}, overlapping heap_begin {:#06x}",
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
                    "duplicate G16 label"
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
                Line::Branch {
                    condition,
                    test,
                    target,
                } => {
                    let offset = relative_offset(code_base + words.len() + 2, labels[target]);
                    words.extend(wide_branch(*condition, *test, offset));
                }
                Line::Jump(target) => {
                    let offset = relative_offset(code_base + words.len() + 2, labels[target]);
                    words.extend(wide_jump(None, offset));
                }
                Line::Call(function) => {
                    let offset =
                        relative_offset(code_base + words.len() + 2, function_addresses[function]);
                    words.extend(wide_jump(Some(REG_LINK), offset));
                }
                Line::LoadFunctionAddress { function, dst } => {
                    words.extend(g16::load_immediate16(
                        *dst,
                        function_addresses[function] as u16,
                    ));
                }
            }
        }
        words.push(g16::halt());
        for (offset, word) in words[local_start..].iter().enumerate() {
            listing.push_str(&format!("  {:04x}: {:04x}\n", start + offset, word));
        }
    }
    G16Program {
        code_base: options.code_base,
        words,
        listing,
    }
}

fn relative_offset(from: usize, to: usize) -> i16 {
    (to as u16).wrapping_sub(from as u16) as i16
}

fn wide_branch(condition: BranchCondition, test: u8, offset: i16) -> [Word; 2] {
    g16::prefixed(g16::branch(condition, test, 0), offset as u16)
}

fn wide_jump(link: Option<u8>, offset: i16) -> [Word; 2] {
    let bits = offset as u16;
    let link = link.unwrap_or(15);
    [
        g16::immediate_high12((bits >> 8) & 0xff),
        0xc000 | (u16::from(link) << 8) | (bits & 0xff),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::parse_source_with;

    fn compile(source: &str, options: CompilerOptions) -> G16Program {
        let program = parse_source_with(source, options.data_base).unwrap();
        let mut compiler = crate::Compiler::new();
        compiler.opts = options;
        for function in program.funcs {
            compiler.add_func(function);
        }
        compiler.finish_g16("main")
    }

    fn run(source: &str) -> u16 {
        run_with_options(source, CompilerOptions::g16()).0
    }

    fn run_with_options(source: &str, options: CompilerOptions) -> (u16, g16::Machine) {
        let program = compile(source, options);
        let mut machine = g16::Machine::default();
        machine
            .load_program(program.code_base, &program.words)
            .unwrap();
        if program.code_base != 0 {
            let mut bootstrap = g16::load_immediate16(REG_TMP, program.code_base).to_vec();
            bootstrap.push(g16::jump_register(REG_TMP));
            machine.load_program(0, &bootstrap).unwrap();
        }
        let signal = match machine.run(10_000).unwrap() {
            g16::RunOutcome::Halted { signal, .. } => signal,
            outcome => panic!("G16 program did not halt: {outcome:?}"),
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
    fn direct_and_indirect_calls_follow_the_g16_abi() {
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
            ..CompilerOptions::g16()
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
    fn non_overlapping_static_data_initializes_unified_memory() {
        let source = "static VALUE: u16 = 77; fn main() { halt(VALUE); }";
        let options = CompilerOptions {
            data_base: 0x4000,
            ..CompilerOptions::g16()
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
            ..CompilerOptions::g16()
        };
        let result = std::panic::catch_unwind(|| compile(source, options));
        let error = result.expect_err("overlapping G16 code and static data must fail");
        let message = error
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| error.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(message.contains("G16 unified-memory image"), "{message}");
    }

    #[test]
    fn legacy_wrapping_stack_configuration_is_rejected_for_g16() {
        let result = std::panic::catch_unwind(|| {
            compile("fn main() { halt(0); }", CompilerOptions::default())
        });
        let error = result.expect_err("G16 must reject a stack that wraps into MMIO");
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
