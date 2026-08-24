//! Lower an allocated IR function to symbolic machine lines. Absolute
//! addresses and variable-width direct calls are resolved by the linker.

use crate::compiler::ir::*;
use crate::compiler::regalloc::*;
use crate::isa::*;
use std::collections::{HashMap, HashSet};

/// One symbolic machine line retained until whole-program layout is stable.
#[derive(Clone)]
pub(crate) enum MachineLine {
    Inst(Instruction, Option<u32>),
    /// conditional branch (j_cc) to a block/label
    Branch {
        cond: Cond,
        target: usize,
        line: Option<u32>,
    },
    /// unconditional jump to a block/label
    Jump {
        target: usize,
        line: Option<u32>,
    },
    /// zero-width address marker (block ids and relaxation targets)
    Label(usize),
    /// zero-width comment attached to the address of the next emitted line
    Comment(String),
    /// compiler-generated initialization range markers
    SectionStart {
        name: String,
        detail: String,
    },
    SectionEnd,
    /// far jump: load_lo + load_hi + jmp_reg tmp (3 slots, absolute target)
    AbsJump {
        target: usize,
        line: Option<u32>,
    },
    /// direct call whose one- or three-word encoding is selected by the linker
    DirectCall {
        func: crate::compiler::FuncName,
        id: usize,
        /// Estimated execution weight used by automatic table selection.
        weight: usize,
        line: Option<u32>,
    },
    /// function-table call: 1 slot filled with call_abs by the linker
    CallAbs1 {
        func: crate::compiler::FuncName,
        index: u8,
        line: Option<u32>,
    },
    /// load function address: two words filled with the final absolute address
    LoadAddr2 {
        func: crate::compiler::FuncName,
        reg: u8,
        line: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CallSite {
    pub caller: crate::compiler::FuncName,
    pub id: usize,
}

pub(crate) fn line_size(
    line: &MachineLine,
    caller: crate::compiler::FuncName,
    near_calls: &HashSet<CallSite>,
) -> usize {
    match line {
        MachineLine::Inst(_, _)
        | MachineLine::Branch { .. }
        | MachineLine::Jump { .. }
        | MachineLine::CallAbs1 { .. } => 1,
        MachineLine::AbsJump { .. } => 3,
        MachineLine::DirectCall { id, .. } => {
            if near_calls.contains(&CallSite { caller, id: *id }) {
                1
            } else {
                3
            }
        }
        MachineLine::LoadAddr2 { .. } => 2,
        MachineLine::Label(_)
        | MachineLine::Comment(_)
        | MachineLine::SectionStart { .. }
        | MachineLine::SectionEnd => 0,
    }
}

/// expand out-of-range branches until everything fits the i8 offset range:
/// conditional -> inverted short branch over an AbsJump; unconditional -> AbsJump.
pub(crate) fn relax(
    lines: &mut Vec<MachineLine>,
    func: crate::compiler::FuncName,
    near_calls: &HashSet<CallSite>,
) {
    let mut fresh_label = 1usize << 60;
    for _ in 0..64 {
        // current label addresses (relative to function start)
        let mut label_addr: HashMap<usize, usize> = HashMap::new();
        let mut addr = 0usize;
        for line in lines.iter() {
            if let MachineLine::Label(l) = line {
                label_addr.insert(*l, addr);
            } else {
                addr += line_size(line, func, near_calls);
            }
        }
        // find the first out-of-range branch and expand it
        let mut addr = 0usize;
        let mut expanded = false;
        for i in 0..lines.len() {
            let size = line_size(&lines[i], func, near_calls);
            match &lines[i] {
                MachineLine::Branch { cond, target, line } => {
                    let offset = label_addr[target] as i64 - addr as i64;
                    if !(-128..=127).contains(&offset) || offset == 0 {
                        let (inv, t) = (cond.invert(), *target);
                        let line = *line;
                        let skip = fresh_label;
                        fresh_label += 1;
                        lines.splice(
                            i..i + 1,
                            [
                                MachineLine::Branch {
                                    cond: inv,
                                    target: skip,
                                    line,
                                },
                                MachineLine::AbsJump { target: t, line },
                                MachineLine::Label(skip),
                            ],
                        );
                        expanded = true;
                        break;
                    }
                }
                MachineLine::Jump { target, line } => {
                    let offset = label_addr[target] as i64 - addr as i64;
                    if !(-128..=127).contains(&offset) || offset == 0 {
                        let t = *target;
                        let line = *line;
                        lines.splice(i..i + 1, [MachineLine::AbsJump { target: t, line }]);
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
    pub(crate) name: crate::compiler::FuncName,
    pub(crate) lines: Vec<MachineLine>,
}

fn block_is_in_cycle(function: &IrFunc, start: BlockId) -> bool {
    let mut seen = HashSet::new();
    let mut work = function.successors(start);
    while let Some(block) = work.pop() {
        if block == start {
            return true;
        }
        if seen.insert(block) {
            work.extend(function.successors(block));
        }
    }
    false
}

#[derive(Clone, Debug)]
pub struct InitSection {
    pub name: String,
    pub detail: String,
    pub addr: (usize, usize),
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
            if !pending
                .iter()
                .enumerate()
                .any(|(j, &(f2, _))| j != i && f2 == t)
            {
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

fn emit_load_u16(lines: &mut Vec<MachineLine>, value: u16, reg: u8) {
    let (hi, lo) = hi_lo(value as u8);
    lines.push(MachineLine::Inst(load_lo(hi, lo, reg), None));
    if value > 255 {
        let (hi, lo) = hi_lo((value >> 8) as u8);
        lines.push(MachineLine::Inst(load_hi(hi, lo, reg), None));
    }
}

fn start_init(lines: &mut Vec<MachineLine>, name: &str, detail: String) {
    lines.push(MachineLine::SectionStart {
        name: name.to_string(),
        detail,
    });
}

fn emit_static_data_init(block: &Block, restore_sp: u16, lines: &mut Vec<MachineLine>) {
    let mut current_page = None;
    for inst in &block.insts {
        let Instr::StoreStatic { addr, value } = inst else {
            panic!("static data initialization block contains a non-static store");
        };
        let page = *addr & 0xff00;
        if current_page != Some(page) {
            emit_load_u16(lines, page, REG_SP);
            current_page = Some(page);
        }
        emit_load_u16(lines, *value, REG_TMP);
        let (hi, lo) = hi_lo(*addr as u8);
        lines.push(MachineLine::Inst(store_sp(hi, lo, REG_TMP), None));
    }
    if current_page != Some(restore_sp) {
        emit_load_u16(lines, restore_sp, REG_SP);
    }
}

/// Lower one function without assigning any absolute instruction addresses.
pub fn compile_function(
    f: &IrFunc,
    alloc: &Allocation,
    stack_init: u16,
    function_table: &HashMap<crate::compiler::FuncName, u8>,
    initialize_function_table: bool,
) -> EmittedFunc {
    let layout = f.rpo();
    let mut lines: Vec<MachineLine> = vec![];

    let reg = |v: VReg| alloc.reg[&v];
    let frame_sp = stack_init.wrapping_sub(alloc.frame_size() as u16);

    // call_abs reads mem[0xff00 + index]. store_sp has a full u8 offset, so
    // use sp as the table base and initialize all 256 possible entries without
    // incrementing a separate base register every eight stores.
    if initialize_function_table && !function_table.is_empty() {
        let base_detail = if stack_init == crate::FUNCTION_TABLE_BASE {
            format!(
                "sp = {:#06x} (function-table base)",
                crate::FUNCTION_TABLE_BASE
            )
        } else {
            format!(
                "temporary sp = {:#06x} for function table",
                crate::FUNCTION_TABLE_BASE
            )
        };
        start_init(&mut lines, "stack", base_detail);
        emit_load_u16(&mut lines, crate::FUNCTION_TABLE_BASE, REG_SP);
        lines.push(MachineLine::SectionEnd);

        let mut entries: Vec<_> = function_table
            .iter()
            .map(|(&name, &index)| (index, name))
            .collect();
        entries.sort_by_key(|&(index, _)| index);
        start_init(
            &mut lines,
            "function-table",
            format!(
                "{} entries at {:#06x}..{:#06x}",
                entries.len(),
                crate::FUNCTION_TABLE_BASE,
                crate::FUNCTION_TABLE_BASE as usize + entries.len()
            ),
        );
        for (index, func) in entries {
            lines.push(MachineLine::Comment(format!(
                "function table[{index:02x}] = {func} @ mem[{:#06x}]",
                crate::FUNCTION_TABLE_BASE + index as u16
            )));
            lines.push(MachineLine::LoadAddr2 {
                func,
                reg: REG_TMP,
                line: None,
            });
            let (hi, lo) = hi_lo(index);
            lines.push(MachineLine::Inst(store_sp(hi, lo, REG_TMP), None));
        }
        lines.push(MachineLine::SectionEnd);
    }

    // Entry-point stack pointer override. When it is also the function-table
    // base, the preceding table setup has already established it.
    if stack_init != 0
        && (!initialize_function_table
            || function_table.is_empty()
            || stack_init != crate::FUNCTION_TABLE_BASE)
    {
        start_init(&mut lines, "stack", format!("sp = {stack_init:#06x}"));
        emit_load_u16(&mut lines, stack_init, REG_SP);
        lines.push(MachineLine::SectionEnd);
    }

    // prologue
    if alloc.frame_size() > 0 {
        let (hi, lo) = hi_lo(alloc.frame_size() as u8);
        lines.push(MachineLine::Inst(sp_sub(hi, lo), None));
        for (slot, &r) in alloc.callee_saved.iter().enumerate() {
            let (hi, lo) = hi_lo(slot as u8);
            lines.push(MachineLine::Inst(store_sp(hi, lo, r), None));
        }
    }

    for (idx, &b) in layout.iter().enumerate() {
        lines.push(MachineLine::Label(b));
        let init_detail = f.block_notes[b].and_then(|note| note.strip_prefix("global init: "));
        if let Some(detail) = init_detail {
            let name = detail
                .split(':')
                .next()
                .unwrap_or("runtime")
                .replace(' ', "-");
            start_init(&mut lines, &name, detail.to_string());
        }
        let mut comment = String::new();
        if let Some(note) = f.block_notes[b].filter(|_| init_detail.is_none()) {
            comment = note.to_string();
        }
        if let Some(line) = f.block_lines[b] {
            if !comment.is_empty() {
                comment.push_str(", ");
            }
            comment.push_str(&format!("line {line}"));
        }
        if !comment.is_empty() {
            lines.push(MachineLine::Comment(comment));
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
                lines.push(MachineLine::Inst(mov(from, to), f.block_lines[b]));
            }
        }

        if init_detail.is_some_and(|detail| detail.starts_with("static data:")) {
            emit_static_data_init(block, frame_sp, &mut lines);
        } else {
            let calls = CallLowering {
                caller: f.name,
                hot_block: block_is_in_cycle(f, b),
                function_table,
            };
            for (inst, ir_line) in block.insts.iter().zip(&block.lines) {
                emit_inst(
                    inst,
                    *ir_line,
                    &reg,
                    alloc.callee_saved.len() as u8,
                    alloc.local_slots,
                    &calls,
                    &mut lines,
                );
            }
        }

        match &block.term {
            Some(Terminator::Jmp { target }) => {
                emit_edge_moves(f, b, *target, block.term_line, &reg, &mut lines);
                if layout.get(idx + 1) != Some(target) {
                    lines.push(MachineLine::Jump {
                        target: *target,
                        line: block.term_line,
                    });
                }
            }
            Some(Terminator::Br {
                cmp,
                if_true,
                if_false,
            }) => {
                emit_cmp(cmp, block.term_line, &reg, &mut lines);
                let next = layout.get(idx + 1);
                if if_true == if_false && next == Some(if_true) {
                    // both targets are the fallthrough block
                } else if next == Some(if_false) {
                    lines.push(MachineLine::Branch {
                        cond: cmp.cond,
                        target: *if_true,
                        line: block.term_line,
                    });
                } else if next == Some(if_true) {
                    lines.push(MachineLine::Branch {
                        cond: cmp.cond.invert(),
                        target: *if_false,
                        line: block.term_line,
                    });
                } else {
                    lines.push(MachineLine::Branch {
                        cond: cmp.cond,
                        target: *if_true,
                        line: block.term_line,
                    });
                    lines.push(MachineLine::Jump {
                        target: *if_false,
                        line: block.term_line,
                    });
                }
            }
            Some(Terminator::Ret { .. }) => {
                // values are already in r0/r1 via ABI shim movs
                for (slot, &r) in alloc.callee_saved.iter().enumerate() {
                    let (hi, lo) = hi_lo(slot as u8);
                    lines.push(MachineLine::Inst(load_sp(hi, lo, r), block.term_line));
                }
                if alloc.frame_size() > 0 {
                    let (hi, lo) = hi_lo(alloc.frame_size() as u8);
                    lines.push(MachineLine::Inst(sp_add(hi, lo), block.term_line));
                }
                lines.push(MachineLine::Inst(jmp_reg(REG_RA), block.term_line));
            }
            Some(Terminator::Halt { signal }) => {
                lines.push(MachineLine::Inst(halt(reg(*signal)), block.term_line));
            }
            None => unreachable!("unterminated reachable block b{b}"),
        }
        if init_detail.is_some() {
            lines.push(MachineLine::SectionEnd);
        }
    }

    let mut next_call_id = 0;
    for line in &mut lines {
        if let MachineLine::DirectCall { id, .. } = line {
            *id = next_call_id;
            next_call_id += 1;
        }
    }

    EmittedFunc {
        name: f.name,
        lines,
    }
}

/// phi moves for edge b -> target, placed at the end of b (before its jump)
fn emit_edge_moves(
    f: &IrFunc,
    b: BlockId,
    target: BlockId,
    line: Option<u32>,
    reg: &dyn Fn(VReg) -> u8,
    lines: &mut Vec<MachineLine>,
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
        lines.push(MachineLine::Inst(mov(from, to), line));
    }
}

fn emit_cmp(cmp: &Cmp, line: Option<u32>, reg: &dyn Fn(VReg) -> u8, lines: &mut Vec<MachineLine>) {
    let lhs = reg(cmp.lhs);
    match (&cmp.rhs, cmp.signed) {
        (CmpRhs::Reg(r), false) => lines.push(MachineLine::Inst(cmp_r(reg(*r), lhs), line)),
        (CmpRhs::Reg(r), true) => lines.push(MachineLine::Inst(cmp_s(reg(*r), lhs), line)),
        (CmpRhs::Imm(v), false) if *v <= 15 => {
            lines.push(MachineLine::Inst(cmp_i(*v as u8, lhs), line));
        }
        (CmpRhs::Imm(v), true) if (-8..=7).contains(&(*v as i16)) => {
            lines.push(MachineLine::Inst(cmp_si((*v as u8) & 0xf, lhs), line));
        }
        (CmpRhs::Imm(v), signed) => {
            // materialize the immediate, then compare registers
            let (hi, lo) = hi_lo(*v as u8);
            lines.push(MachineLine::Inst(load_lo(hi, lo, REG_TMP), line));
            if *v > 255 {
                let (hi, lo) = hi_lo((*v >> 8) as u8);
                lines.push(MachineLine::Inst(load_hi(hi, lo, REG_TMP), line));
            }
            if signed {
                lines.push(MachineLine::Inst(cmp_s(REG_TMP, lhs), line));
            } else {
                lines.push(MachineLine::Inst(cmp_r(REG_TMP, lhs), line));
            }
        }
    }
}

struct CallLowering<'a> {
    caller: crate::compiler::FuncName,
    hot_block: bool,
    function_table: &'a HashMap<crate::compiler::FuncName, u8>,
}

fn emit_inst(
    inst: &Instr,
    ir_line: Option<u32>,
    reg: &dyn Fn(VReg) -> u8,
    local_base: u8,
    n_locals: u8,
    calls: &CallLowering<'_>,
    lines: &mut Vec<MachineLine>,
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
            lines.push(MachineLine::Inst(
                f(reg(*lhs), reg(*rhs), reg(*dst)),
                ir_line,
            ));
        }
        Instr::Un { dst, op, src } => {
            let f = match op {
                UnOp::Inv => inv,
                UnOp::Neg => neg,
                UnOp::Not0 => not0,
                UnOp::Cnt1 => cnt1,
                UnOp::Log2 => log2,
            };
            lines.push(MachineLine::Inst(f(reg(*src), reg(*dst)), ir_line));
        }
        Instr::Shift {
            dst,
            op,
            src,
            amount,
        } => {
            if reg(*dst) != reg(*src) {
                lines.push(MachineLine::Inst(mov(reg(*src), reg(*dst)), ir_line));
            }
            let f = match op {
                ShiftOp::Lsl => lsl,
                ShiftOp::Lsr => lsr,
                ShiftOp::Asr => asr,
            };
            lines.push(MachineLine::Inst(f(*amount, reg(*dst)), ir_line));
        }
        Instr::Mov { dst, src } => {
            if reg(*dst) != reg(*src) {
                lines.push(MachineLine::Inst(mov(reg(*src), reg(*dst)), ir_line));
            }
        }
        Instr::LoadImm { dst, value } => {
            let (hi, lo) = hi_lo(*value as u8);
            lines.push(MachineLine::Inst(load_lo(hi, lo, reg(*dst)), ir_line));
            if *value > 255 {
                let (hi, lo) = hi_lo((*value >> 8) as u8);
                lines.push(MachineLine::Inst(load_hi(hi, lo, reg(*dst)), ir_line));
            }
        }
        Instr::LoadMem { dst, base, offset } => {
            let dst = reg(*dst);
            emit_mem(
                lines,
                *base,
                *offset,
                ir_line,
                reg,
                |lines, base_reg, off| {
                    lines.push(MachineLine::Inst(load_mem(base_reg, off, dst), ir_line));
                },
            );
        }
        Instr::StoreMem { base, offset, src } => {
            let src = reg(*src);
            emit_mem(
                lines,
                *base,
                *offset,
                ir_line,
                reg,
                |lines, base_reg, off| {
                    lines.push(MachineLine::Inst(store_mem(base_reg, src, off), ir_line));
                },
            );
        }
        Instr::StoreStatic { .. } => {
            unreachable!("static stores are emitted as a grouped initialization section")
        }
        Instr::Call { func, .. } => {
            // args/results already moved by ABI shims
            lines.push(MachineLine::Comment(format!("call {func}")));
            if let Some(&index) = calls.function_table.get(func) {
                lines.push(MachineLine::CallAbs1 {
                    func,
                    index,
                    line: ir_line,
                });
            } else {
                lines.push(MachineLine::DirectCall {
                    func,
                    id: 0,
                    weight: if *func == calls.caller || calls.hot_block {
                        4
                    } else {
                        1
                    },
                    line: ir_line,
                });
            }
        }
        Instr::LoadFuncAddr { dst, func } => {
            lines.push(MachineLine::Comment(format!("&{func}")));
            lines.push(MachineLine::LoadAddr2 {
                func,
                reg: reg(*dst),
                line: ir_line,
            });
        }
        Instr::CallPtr { .. } => {
            // addr/args/results already moved by ABI shims (addr is in tmp)
            lines.push(MachineLine::Inst(call_reg(REG_TMP), ir_line));
        }
        Instr::DevRecv {
            dst,
            device,
            channel,
        } => {
            assert!(
                *device <= 15 && *channel <= 15,
                "device/channel out of u4 range"
            );
            lines.push(MachineLine::Inst(
                dev_recv(*device, *channel, reg(*dst)),
                ir_line,
            ));
        }
        Instr::DevSend {
            device,
            channel,
            src,
        } => {
            assert!(
                *device <= 15 && *channel <= 15,
                "device/channel out of u4 range"
            );
            lines.push(MachineLine::Inst(
                dev_send(*device, *channel, reg(*src)),
                ir_line,
            ));
        }
        Instr::MtsrDseg { .. } | Instr::Jseg { .. } => {
            panic!("mtsr_dseg/jseg are G16-only intrinsics; the v2.6 ISA has no segment registers")
        }
        Instr::LoadSp { dst, slot } => {
            let (hi, lo) = hi_lo(local_base + n_locals + *slot);
            lines.push(MachineLine::Inst(load_sp(hi, lo, reg(*dst)), ir_line));
        }
        Instr::StoreSp { slot, src } => {
            let (hi, lo) = hi_lo(local_base + n_locals + *slot);
            lines.push(MachineLine::Inst(store_sp(hi, lo, reg(*src)), ir_line));
        }
        Instr::LoadLocal { dst, slot } => {
            let (hi, lo) = hi_lo(local_base + *slot);
            lines.push(MachineLine::Inst(load_sp(hi, lo, reg(*dst)), ir_line));
        }
        Instr::StoreLocal { slot, src } => {
            let (hi, lo) = hi_lo(local_base + *slot);
            lines.push(MachineLine::Inst(store_sp(hi, lo, reg(*src)), ir_line));
        }
        Instr::AddrOfLocal { dst, slot } => {
            // dst = sp + frame offset of the local slot
            let (hi, lo) = hi_lo(local_base + *slot);
            lines.push(MachineLine::Inst(mov(REG_SP, reg(*dst)), ir_line));
            if local_base + *slot > 0 {
                lines.push(MachineLine::Inst(load_lo(hi, lo, REG_TMP), ir_line));
                lines.push(MachineLine::Inst(
                    add(reg(*dst), REG_TMP, reg(*dst)),
                    ir_line,
                ));
            }
        }
    }
}

/// emit a load/store with base + offset, legalizing the i4 offset range
/// (-8..=7) via tmp if needed. `emit` gets (base_reg, raw_i4_offset).
fn emit_mem(
    lines: &mut Vec<MachineLine>,
    base: VReg,
    offset: i16,
    line: Option<u32>,
    reg: &dyn Fn(VReg) -> u8,
    mut emit: impl FnMut(&mut Vec<MachineLine>, u8, u8),
) {
    if (-8..=7).contains(&offset) {
        emit(lines, reg(base), (offset as u8) & 0xf);
    } else {
        // address = base + offset via tmp
        let (hi, lo) = hi_lo(offset as u8);
        lines.push(MachineLine::Inst(load_lo(hi, lo, REG_TMP), line));
        if (offset >> 8) != 0 {
            let (hi, lo) = hi_lo((offset >> 8) as u8);
            lines.push(MachineLine::Inst(load_hi(hi, lo, REG_TMP), line));
        }
        lines.push(MachineLine::Inst(add(reg(base), REG_TMP, REG_TMP), line));
        emit(lines, REG_TMP, 0);
    }
}
