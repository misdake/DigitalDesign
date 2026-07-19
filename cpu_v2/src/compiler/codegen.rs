//! codegen: lower an allocated IrFunc to machine instructions via the shared
//! Assembler. branch offsets are checked against the ISA's i8 range (±128);
//! out-of-range branches are a hard error here (branch relaxation is M5).

use crate::isa::*;
use crate::compiler::{Assembler, InstructionSlot, Relocation};
use crate::compiler::ir::*;
use crate::compiler::regalloc::*;
use std::collections::HashMap;

/// one emitted line: concrete instruction, unresolved branch, label marker,
/// far jump, or a reserved slot (filled by the linker, e.g. call sequences)
enum Line {
    Inst(Instruction, Option<u32>),
    /// conditional branch (j_cc) to a block/label
    Branch { cond: Cond, target: usize },
    /// unconditional jump to a block/label
    Jump { target: usize },
    /// zero-width address marker (block ids and relaxation targets)
    Label(usize),
    /// zero-width comment attached to the address of the next emitted line
    Comment(String),
    /// far jump: load_lo + load_hi + jmp_reg tmp (3 slots, absolute target)
    AbsJump { target: usize },
    /// call placeholder: 3 reserved slots filled by the linker
    Call3 { func: crate::compiler::FuncName },
    /// load function address: 2 reserved slots filled by the linker
    LoadAddr2 { func: crate::compiler::FuncName, reg: u8 },
}

fn line_size(line: &Line) -> usize {
    match line {
        Line::Inst(_, _) | Line::Branch { .. } | Line::Jump { .. } => 1,
        Line::AbsJump { .. } => 3,
        Line::Call3 { .. } => 3,
        Line::LoadAddr2 { .. } => 2,
        Line::Label(_) | Line::Comment(_) => 0,
    }
}

/// expand out-of-range branches until everything fits the i8 offset range:
/// conditional -> inverted short branch over an AbsJump; unconditional -> AbsJump.
fn relax(lines: &mut Vec<Line>, func: &str) {
    let mut fresh_label = 1usize << 60;
    for _ in 0..64 {
        // current label addresses (relative to function start)
        let mut label_addr: HashMap<usize, usize> = HashMap::new();
        let mut addr = 0usize;
        for line in lines.iter() {
            if let Line::Label(l) = line {
                label_addr.insert(*l, addr);
            } else {
                addr += line_size(line);
            }
        }
        // find the first out-of-range branch and expand it
        let mut addr = 0usize;
        let mut expanded = false;
        for i in 0..lines.len() {
            let size = line_size(&lines[i]);
            match &lines[i] {
                Line::Branch { cond, target } => {
                    let offset = label_addr[target] as i64 - addr as i64;
                    if !(-128..=127).contains(&offset) || offset == 0 {
                        let (inv, t) = (cond.invert(), *target);
                        let skip = fresh_label;
                        fresh_label += 1;
                        lines.splice(
                            i..i + 1,
                            [
                                Line::Branch { cond: inv, target: skip },
                                Line::AbsJump { target: t },
                                Line::Label(skip),
                            ],
                        );
                        expanded = true;
                        break;
                    }
                }
                Line::Jump { target } => {
                    let offset = label_addr[target] as i64 - addr as i64;
                    if !(-128..=127).contains(&offset) || offset == 0 {
                        let t = *target;
                        lines.splice(i..i + 1, [Line::AbsJump { target: t }]);
                        expanded = true;
                        break;
                    }
                }
                _ => {}
            }
            addr += size;
        }
        if !expanded {
            return;
        }
    }
    panic!("branch relaxation did not converge in {func}")
}

pub struct EmittedFunc {
    pub relocations: Vec<Relocation>,
    /// number of instructions emitted
    pub len: usize,
    /// (absolute address, comment) pairs for the disassembly listing
    pub comments: Vec<(usize, String)>,
    /// (absolute address, source line) for the debugger's line table
    pub line_map: Vec<(usize, u32)>,
}

/// parallel move between registers, dst-centric (sources may repeat).
/// returns (from, to) moves breaking cycles via `tmp`.
fn parallel_moves(moves: &[(u8, u8)], tmp: u8) -> Vec<(u8, u8)> {
    let mut pending: Vec<(u8, u8)> = moves.iter().copied().filter(|&(f, t)| f != t).collect();
    // dsts should be unique; keep first occurrence defensively
    pending.dedup_by(|b, a| a.1 == b.1);

    let mut out = vec![];
    while !pending.is_empty() {
        let mut progressed = false;
        let mut i = 0;
        while i < pending.len() {
            let (_, t) = pending[i];
            // safe to emit when no other pending move reads t as its source
            if !pending.iter().enumerate().any(|(j, &(f2, _))| j != i && f2 == t) {
                out.push(pending.remove(i));
                progressed = true;
            } else {
                i += 1;
            }
        }
        if !progressed {
            // cycle: save the target's value to tmp, redirect its readers
            let (_, t) = pending[0];
            out.push((t, tmp));
            for m in pending.iter_mut() {
                if m.0 == t {
                    m.0 = tmp;
                }
            }
        }
    }
    out
}

fn hi_lo(v: u8) -> (u8, u8) {
    (v >> 4, v & 0xf)
}

/// compile one function into the assembler at `start_address`
pub fn compile_function(
    f: &IrFunc,
    alloc: &Allocation,
    asm: &mut Assembler,
    start_address: usize,
    stack_init: u16,
) -> EmittedFunc {
    let layout = f.rpo();
    let mut lines: Vec<Line> = vec![];

    let reg = |v: VReg| alloc.reg[&v];

    // entry-point stack pointer override (before the prologue)
    if stack_init != 0 {
        let (hi, lo) = hi_lo(stack_init as u8);
        lines.push(Line::Inst(load_lo(hi, lo, REG_SP), None));
        if stack_init > 255 {
            let (hi, lo) = hi_lo((stack_init >> 8) as u8);
            lines.push(Line::Inst(load_hi(hi, lo, REG_SP), None));
        }
    }

    // prologue
    if alloc.frame_size() > 0 {
        let (hi, lo) = hi_lo(alloc.frame_size() as u8);
        lines.push(Line::Inst(sp_sub(hi, lo), None));
        for (slot, &r) in alloc.callee_saved.iter().enumerate() {
            let (hi, lo) = hi_lo(slot as u8);
            lines.push(Line::Inst(store_sp(hi, lo, r), None));
        }
    }

    for (idx, &b) in layout.iter().enumerate() {
        lines.push(Line::Label(b));
        let mut comment = String::new();
        if let Some(note) = f.block_notes[b] {
            comment = note.to_string();
        }
        if let Some(line) = f.block_lines[b] {
            if !comment.is_empty() {
                comment.push_str(", ");
            }
            comment.push_str(&format!("line {line}"));
        }
        if !comment.is_empty() {
            lines.push(Line::Comment(comment));
        }
        let block = &f.blocks[b];

        // phi moves for a single-pred block go at the block top
        if block.preds.len() == 1 && !block.phis.is_empty() {
            let p = block.preds[0];
            let moves: Vec<(u8, u8)> = block
                .phis
                .iter()
                .filter_map(|phi| {
                    phi.args
                        .iter()
                        .find(|(pred, _)| *pred == p)
                        .map(|(_, v)| (reg(*v), reg(phi.dst)))
                })
                .collect();
            for (from, to) in parallel_moves(&moves, REG_TMP) {
                lines.push(Line::Inst(mov(from, to), None));
            }
        }

        for (inst, ir_line) in block.insts.iter().zip(&block.lines) {
            emit_inst(inst, *ir_line, &reg, alloc.callee_saved.len() as u8, alloc.local_slots, &mut lines);
        }

        match &block.term {
            Some(Terminator::Jmp { target }) => {
                emit_edge_moves(f, b, *target, &reg, &mut lines);
                if layout.get(idx + 1) != Some(target) {
                    lines.push(Line::Jump { target: *target });
                }
            }
            Some(Terminator::Br { cmp, if_true, if_false }) => {
                emit_cmp(cmp, &reg, &mut lines);
                let next = layout.get(idx + 1);
                if if_true == if_false && next == Some(if_true) {
                    // both targets are the fallthrough block
                } else if next == Some(if_false) {
                    lines.push(Line::Branch {
                        cond: cmp.cond,
                        target: *if_true,
                    });
                } else if next == Some(if_true) {
                    lines.push(Line::Branch {
                        cond: cmp.cond.invert(),
                        target: *if_false,
                    });
                } else {
                    lines.push(Line::Branch {
                        cond: cmp.cond,
                        target: *if_true,
                    });
                    lines.push(Line::Jump { target: *if_false });
                }
            }
            Some(Terminator::Ret { .. }) => {
                // values are already in r0/r1 via ABI shim movs
                for (slot, &r) in alloc.callee_saved.iter().enumerate() {
                    let (hi, lo) = hi_lo(slot as u8);
                    lines.push(Line::Inst(load_sp(hi, lo, r), None));
                }
                if alloc.frame_size() > 0 {
                    let (hi, lo) = hi_lo(alloc.frame_size() as u8);
                    lines.push(Line::Inst(sp_add(hi, lo), None));
                }
                lines.push(Line::Inst(jmp_reg(REG_RA), None));
            }
            Some(Terminator::Halt { signal }) => {
                lines.push(Line::Inst(halt(reg(*signal)), None));
            }
            None => unreachable!("unterminated reachable block b{b}"),
        }
    }

    // expand out-of-range branches, then write everything into the assembler
    relax(&mut lines, f.name);

    let mut label_addr: HashMap<usize, usize> = HashMap::new();
    {
        let mut a = start_address;
        for line in &lines {
            if let Line::Label(l) = line {
                label_addr.insert(*l, a);
            } else {
                a += line_size(line);
            }
        }
    }
    let mut addr = start_address;
    let mut written = 0usize;
    let mut relocations = vec![];
    let mut comments = vec![];
    let mut line_map = vec![];
    for line in &lines {
        match line {
            Line::Label(_) => {}
            Line::Comment(text) => {
                comments.push((addr, text.clone()));
            }
            Line::Inst(inst, line) => {
                if let Some(line) = line {
                    line_map.push((addr, *line));
                }
                asm.inst_at(*inst, addr);
                addr += 1;
                written += 1;
            }
            Line::Branch { cond, target } => {
                let (hi, lo) = branch_offset(addr, label_addr[target], f.name);
                let inst = match cond {
                    Cond::Never => panic!("branch with Cond::Never"),
                    Cond::Greater => jg(hi, lo),
                    Cond::Equal => je(hi, lo),
                    Cond::Less => jl(hi, lo),
                    Cond::GreaterEqual => jge(hi, lo),
                    Cond::LessEqual => jle(hi, lo),
                    Cond::NotEqual => jne(hi, lo),
                    Cond::Always => jmp(hi, lo),
                };
                asm.inst_at(inst, addr);
                addr += 1;
                written += 1;
            }
            Line::Jump { target } => {
                let (hi, lo) = branch_offset(addr, label_addr[target], f.name);
                asm.inst_at(jmp(hi, lo), addr);
                addr += 1;
                written += 1;
            }
            Line::AbsJump { target } => {
                let t = label_addr[target] as u16;
                let (hi, lo) = hi_lo(t as u8);
                asm.inst_at(load_lo(hi, lo, REG_TMP), addr);
                let (hi, lo) = hi_lo((t >> 8) as u8);
                asm.inst_at(load_hi(hi, lo, REG_TMP), addr + 1);
                asm.inst_at(jmp_reg(REG_TMP), addr + 2);
                addr += 3;
                written += 3;
            }
            Line::Call3 { func } => {
                relocations.push(Relocation {
                    func_name: func,
                    kind: crate::compiler::RelocKind::Call3,
                    slots: vec![
                        InstructionSlot::new(addr),
                        InstructionSlot::new(addr + 1),
                        InstructionSlot::new(addr + 2),
                    ],
                });
                // left invalid; the linker fills them during relocation
                addr += 3;
                written += 3;
            }
            Line::LoadAddr2 { func, reg } => {
                relocations.push(Relocation {
                    func_name: func,
                    kind: crate::compiler::RelocKind::LoadAddr { reg: *reg },
                    slots: vec![InstructionSlot::new(addr), InstructionSlot::new(addr + 1)],
                });
                addr += 2;
                written += 2;
            }
        }
    }

    EmittedFunc {
        relocations,
        len: written,
        comments,
        line_map,
    }
}

fn branch_offset(from: usize, to: usize, func: &str) -> (u8, u8) {
    let offset = to as i64 - from as i64;
    assert!(
        (-128..=127).contains(&offset) && offset != 0,
        "branch still out of range after relaxation in function {func}: \
         from 0x{from:04x} to 0x{to:04x} (offset {offset})"
    );
    let v = offset as i8 as u8;
    (v >> 4, v & 0xf)
}

/// phi moves for edge b -> target, placed at the end of b (before its jump)
fn emit_edge_moves(
    f: &IrFunc,
    b: BlockId,
    target: BlockId,
    reg: &dyn Fn(VReg) -> u8,
    lines: &mut Vec<Line>,
) {
    let tblock = &f.blocks[target];
    if tblock.phis.is_empty() || tblock.preds.len() == 1 {
        return; // single-pred case handled at the target's top
    }
    let moves: Vec<(u8, u8)> = tblock
        .phis
        .iter()
        .map(|phi| {
            let (_, v) = phi
                .args
                .iter()
                .find(|(pred, _)| *pred == b)
                .unwrap_or_else(|| panic!("no phi arg for edge b{b} -> b{target}"));
            (reg(*v), reg(phi.dst))
        })
        .collect();
    for (from, to) in parallel_moves(&moves, REG_TMP) {
        lines.push(Line::Inst(mov(from, to), None));
    }
}

fn emit_cmp(cmp: &Cmp, reg: &dyn Fn(VReg) -> u8, lines: &mut Vec<Line>) {
    let lhs = reg(cmp.lhs);
    match (&cmp.rhs, cmp.signed) {
        (CmpRhs::Reg(r), false) => lines.push(Line::Inst(cmp_r(reg(*r), lhs), None)),
        (CmpRhs::Reg(r), true) => lines.push(Line::Inst(cmp_s(reg(*r), lhs), None)),
        (CmpRhs::Imm(v), false) if *v <= 15 => {
            lines.push(Line::Inst(cmp_i(*v as u8, lhs), None));
        }
        (CmpRhs::Imm(v), true) if (-8..=7).contains(&(*v as i16)) => {
            lines.push(Line::Inst(cmp_si((*v as u8) & 0xf, lhs), None));
        }
        (CmpRhs::Imm(v), signed) => {
            // materialize the immediate, then compare registers
            let (hi, lo) = hi_lo(*v as u8);
            lines.push(Line::Inst(load_lo(hi, lo, REG_TMP), None));
            if *v > 255 {
                let (hi, lo) = hi_lo((*v >> 8) as u8);
                lines.push(Line::Inst(load_hi(hi, lo, REG_TMP), None));
            }
            if signed {
                lines.push(Line::Inst(cmp_s(REG_TMP, lhs), None));
            } else {
                lines.push(Line::Inst(cmp_r(REG_TMP, lhs), None));
            }
        }
    }
}

fn emit_inst(
    inst: &Instr,
    ir_line: Option<u32>,
    reg: &dyn Fn(VReg) -> u8,
    local_base: u8,
    n_locals: u8,
    lines: &mut Vec<Line>,
) {
    match inst {
        Instr::Bin { dst, op, lhs, rhs } => {
            let f = match op {
                BinOp::Add => add,
                BinOp::Sub => sub,
                BinOp::And => and,
                BinOp::Or => or,
                BinOp::Xor => xor,
            };
            lines.push(Line::Inst(f(reg(*lhs), reg(*rhs), reg(*dst)), ir_line));
        }
        Instr::Un { dst, op, src } => {
            let f = match op {
                UnOp::Inv => inv,
                UnOp::Neg => neg,
                UnOp::Not0 => not0,
                UnOp::Cnt1 => cnt1,
                UnOp::Log2 => log2,
            };
            lines.push(Line::Inst(f(reg(*src), reg(*dst)), ir_line));
        }
        Instr::Shift { dst, op, src, amount } => {
            if reg(*dst) != reg(*src) {
                lines.push(Line::Inst(mov(reg(*src), reg(*dst)), ir_line));
            }
            let f = match op {
                ShiftOp::Lsl => lsl,
                ShiftOp::Lsr => lsr,
                ShiftOp::Asr => asr,
            };
            lines.push(Line::Inst(f(*amount, reg(*dst)), ir_line));
        }
        Instr::Mov { dst, src } => {
            if reg(*dst) != reg(*src) {
                lines.push(Line::Inst(mov(reg(*src), reg(*dst)), ir_line));
            }
        }
        Instr::LoadImm { dst, value } => {
            let (hi, lo) = hi_lo(*value as u8);
            lines.push(Line::Inst(load_lo(hi, lo, reg(*dst)), ir_line));
            if *value > 255 {
                let (hi, lo) = hi_lo((*value >> 8) as u8);
                lines.push(Line::Inst(load_hi(hi, lo, reg(*dst)), ir_line));
            }
        }
        Instr::LoadMem { dst, base, offset } => {
            let dst = reg(*dst);
            emit_mem(lines, *base, *offset, reg, |lines, base_reg, off| {
                lines.push(Line::Inst(load_mem(base_reg, off, dst), ir_line));
            });
        }
        Instr::StoreMem { base, offset, src } => {
            let src = reg(*src);
            emit_mem(lines, *base, *offset, reg, |lines, base_reg, off| {
                lines.push(Line::Inst(store_mem(base_reg, src, off), ir_line));
            });
        }
        Instr::Call { func, .. } => {
            // args/results already moved by ABI shims; the linker fills 3 slots
            lines.push(Line::Comment(format!("call {func}")));
            lines.push(Line::Call3 { func });
        }
        Instr::LoadFuncAddr { dst, func } => {
            lines.push(Line::Comment(format!("&{func}")));
            lines.push(Line::LoadAddr2 { func, reg: reg(*dst) });
        }
        Instr::CallPtr { .. } => {
            // addr/args/results already moved by ABI shims (addr is in tmp)
            lines.push(Line::Inst(call_reg(REG_TMP), ir_line));
        }
        Instr::DevRecv { dst, device, channel } => {
            assert!(*device <= 15 && *channel <= 15, "device/channel out of u4 range");
            lines.push(Line::Inst(dev_recv(*device, *channel, reg(*dst)), ir_line));
        }
        Instr::DevSend { device, channel, src } => {
            assert!(*device <= 15 && *channel <= 15, "device/channel out of u4 range");
            lines.push(Line::Inst(dev_send(*device, *channel, reg(*src)), ir_line));
        }
        Instr::LoadSp { dst, slot } => {
            let (hi, lo) = hi_lo(local_base + n_locals + *slot);
            lines.push(Line::Inst(load_sp(hi, lo, reg(*dst)), ir_line));
        }
        Instr::StoreSp { slot, src } => {
            let (hi, lo) = hi_lo(local_base + n_locals + *slot);
            lines.push(Line::Inst(store_sp(hi, lo, reg(*src)), ir_line));
        }
        Instr::LoadLocal { dst, slot } => {
            let (hi, lo) = hi_lo(local_base + *slot);
            lines.push(Line::Inst(load_sp(hi, lo, reg(*dst)), ir_line));
        }
        Instr::StoreLocal { slot, src } => {
            let (hi, lo) = hi_lo(local_base + *slot);
            lines.push(Line::Inst(store_sp(hi, lo, reg(*src)), ir_line));
        }
        Instr::AddrOfLocal { dst, slot } => {
            // dst = sp + frame offset of the local slot
            let (hi, lo) = hi_lo(local_base + *slot);
            lines.push(Line::Inst(mov(REG_SP, reg(*dst)), ir_line));
            if local_base + *slot > 0 {
                lines.push(Line::Inst(load_lo(hi, lo, REG_TMP), ir_line));
                lines.push(Line::Inst(add(reg(*dst), REG_TMP, reg(*dst)), ir_line));
            }
        }
    }
}

/// emit a load/store with base + offset, legalizing the i4 offset range
/// (-8..=7) via tmp if needed. `emit` gets (base_reg, raw_i4_offset).
fn emit_mem(
    lines: &mut Vec<Line>,
    base: VReg,
    offset: i16,
    reg: &dyn Fn(VReg) -> u8,
    mut emit: impl FnMut(&mut Vec<Line>, u8, u8),
) {
    if (-8..=7).contains(&offset) {
        emit(lines, reg(base), (offset as u8) & 0xf);
    } else {
        // address = base + offset via tmp
        let (hi, lo) = hi_lo(offset as u8);
        lines.push(Line::Inst(load_lo(hi, lo, REG_TMP), None));
        if (offset >> 8) != 0 {
            let (hi, lo) = hi_lo((offset >> 8) as u8);
            lines.push(Line::Inst(load_hi(hi, lo, REG_TMP), None));
        }
        lines.push(Line::Inst(add(reg(base), REG_TMP, REG_TMP), None));
        emit(lines, REG_TMP, 0);
    }
}
