//! optimization passes on IrFunc (run before register allocation).
//! all passes are individually switchable via `Opts`.

use crate::compiler::builder::remove_trivial_phis;
use crate::compiler::ir::*;
use crate::CompareOp;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct Opts {
    pub const_prop: bool,
    pub cse: bool,
    pub dce: bool,
    /// register coalescing hints (used by the allocator, not a pass here)
    pub coalesce: bool,
}
impl Default for Opts {
    fn default() -> Self {
        Self {
            const_prop: true,
            cse: true,
            dce: true,
            coalesce: true,
        }
    }
}
impl Opts {
    pub fn disabled() -> Self {
        Self {
            const_prop: false,
            cse: false,
            dce: false,
            coalesce: false,
        }
    }

    pub fn is_disabled(&self) -> bool {
        !self.const_prop && !self.cse && !self.dce && !self.coalesce
    }
}

pub fn optimize(f: &mut IrFunc, opts: &Opts) {
    let mut hoisted_return_joins = HashSet::new();
    for _ in 0..4 {
        let mut changed = false;
        if opts.const_prop {
            changed |= const_prop(f);
        }
        if opts.cse {
            changed |= cse(f);
        }
        if opts.dce {
            changed |= dce(f);
        }
        changed |= remove_trivial_phis(f);
        if !opts.is_disabled() {
            changed |= hoist_constant_return_arm(f, &mut hoisted_return_joins);
        }
        if !changed {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// substitution helper (uses only, never defs)
// ---------------------------------------------------------------------------

pub(crate) fn subst_uses(f: &mut IrFunc, replace: &HashMap<VReg, VReg>) {
    let mut subst = |v: &mut VReg| {
        while let Some(&r) = replace.get(v) {
            *v = r;
        }
    };
    for b in &mut f.blocks {
        for phi in &mut b.phis {
            for (_, v) in &mut phi.args {
                subst(v);
            }
        }
        for inst in &mut b.insts {
            inst.for_each_use_mut(&mut subst);
        }
        if let Some(term) = &mut b.term {
            match term {
                Terminator::Jmp { .. } => {}
                Terminator::Br { cmp, .. } => {
                    subst(&mut cmp.lhs);
                    if let CmpRhs::Reg(r) = &mut cmp.rhs {
                        subst(r);
                    }
                }
                Terminator::Ret { values } => values.iter_mut().for_each(&subst),
                Terminator::Halt { signal } => subst(signal),
                Terminator::IcacheInvalidateDelayedAndJump { cseg, target } => {
                    subst(cseg);
                    subst(target);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// constant propagation / folding (+ branch resolution)
// ---------------------------------------------------------------------------

fn fold_bin(op: BinOp, a: u16, b: u16) -> u16 {
    match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::And => a & b,
        BinOp::Or => a | b,
        BinOp::Xor => a ^ b,
    }
}
fn fold_mul(window: MulWindow, a: u16, b: u16) -> u16 {
    let product = u32::from(a) * u32::from(b);
    let shift = match window {
        MulWindow::Low => 0,
        MulWindow::Shift8 => 8,
        MulWindow::Shift16 => 16,
    };
    (product >> shift) as u16
}
fn fold_un(op: UnOp, a: u16) -> Option<u16> {
    Some(match op {
        UnOp::Inv => !a,
        UnOp::Neg => (a as i16).wrapping_neg() as u16,
        UnOp::Not0 => (a != 0) as u16,
        UnOp::Cnt1 => a.count_ones() as u16,
        UnOp::Log2 => {
            if a == 0 {
                0
            } else {
                a.ilog2() as u16
            }
        }
        UnOp::Sextb => (((a << 8) as i16) >> 8) as u16,
        UnOp::Clz => a.leading_zeros() as u16,
    })
}
fn fold_shift(op: ShiftOp, a: u16, amount: u8) -> u16 {
    let amount = u32::from(amount & 15);
    match op {
        ShiftOp::Lsl => a.wrapping_shl(amount),
        ShiftOp::Lsr => a.wrapping_shr(amount),
        ShiftOp::Asr => ((a as i16) >> amount) as u16,
    }
}

/// the constant value of an operand, if known
fn operand_konst(operand: &IntOperand, konst: &[Option<u16>]) -> Option<u16> {
    match operand {
        IntOperand::Imm(value) => Some(*value),
        IntOperand::Reg(v) => konst[*v as usize],
    }
}

fn const_prop(f: &mut IrFunc) -> bool {
    // ----- sparse constant analysis (monotone fixpoint) -----
    fn learn(konst: &mut [Option<u16>], v: VReg, x: u16, changed: &mut bool) {
        if konst[v as usize].is_none() {
            konst[v as usize] = Some(x);
            *changed = true;
        }
    }
    let mut konst: Vec<Option<u16>> = vec![None; f.vreg_count as usize];
    loop {
        let mut changed = false;
        for b in &f.blocks {
            for phi in &b.phis {
                let mut vals = phi.args.iter().map(|&(_, v)| konst[v as usize]);
                if let Some(Some(x)) = vals.next().filter(|first| vals.all(|v| v == *first)) {
                    learn(&mut konst, phi.dst, x, &mut changed);
                }
            }
            for inst in &b.insts {
                match inst {
                    Instr::LoadImm { dst, value } => learn(&mut konst, *dst, *value, &mut changed),
                    Instr::Mov { dst, src } => {
                        if let Some(x) = konst[*src as usize] {
                            learn(&mut konst, *dst, x, &mut changed)
                        }
                    }
                    Instr::Bin { dst, op, lhs, rhs } => {
                        if let (Some(a), Some(b)) =
                            (konst[*lhs as usize], operand_konst(rhs, &konst))
                        {
                            learn(&mut konst, *dst, fold_bin(*op, a, b), &mut changed);
                        }
                    }
                    Instr::Mul {
                        dst,
                        window,
                        lhs,
                        rhs,
                    } => {
                        if let (Some(a), Some(b)) =
                            (konst[*lhs as usize], operand_konst(rhs, &konst))
                        {
                            learn(&mut konst, *dst, fold_mul(*window, a, b), &mut changed);
                        }
                    }
                    Instr::Un { dst, op, src } => {
                        if let Some(a) = konst[*src as usize] {
                            if let Some(x) = fold_un(*op, a) {
                                learn(&mut konst, *dst, x, &mut changed);
                            }
                        }
                    }
                    Instr::Shift {
                        dst,
                        op,
                        src,
                        amount,
                    } => {
                        if let (Some(a), Some(n)) =
                            (konst[*src as usize], operand_konst(amount, &konst))
                        {
                            learn(&mut konst, *dst, fold_shift(*op, a, n as u8), &mut changed);
                        }
                    }
                    _ => {}
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut changed = false;

    // ----- rewrite folded defs into LoadImm -----
    for b in &mut f.blocks {
        for inst in &mut b.insts {
            let (dst, value) = match inst {
                Instr::Bin { dst, .. }
                | Instr::Mul { dst, .. }
                | Instr::Un { dst, .. }
                | Instr::Shift { dst, .. }
                | Instr::Mov { dst, .. } => (*dst, konst[*dst as usize]),
                _ => continue,
            };
            if let Some(value) = value {
                *inst = Instr::LoadImm { dst, value };
                changed = true;
            }
        }
    }

    // Keep encodable constants as immediates at their use sites. Larger
    // constants stay in vregs so loop-invariant loads remain outside loops;
    // otherwise codegen would materialize them into REG_TMP at every compare.
    for block in &mut f.blocks {
        let Some(Terminator::Br { cmp, .. }) = &mut block.term else {
            continue;
        };
        let CmpRhs::Reg(rhs) = cmp.rhs else {
            continue;
        };
        let Some(value) = konst[rhs as usize] else {
            continue;
        };
        let encodable = if cmp.signed {
            (-8..=7).contains(&(value as i16))
        } else {
            value <= 15
        };
        if encodable {
            cmp.rhs = CmpRhs::Imm(value);
            changed = true;
        }
    }

    changed |= use_unsigned_cmp_after_nonnegative_guard(f, &konst);

    // ----- resolve constant branches -----
    for b in 0..f.blocks.len() {
        let (cmp, if_true, if_false) = match &f.blocks[b].term {
            Some(Terminator::Br {
                cmp,
                if_true,
                if_false,
            }) => (*cmp, *if_true, *if_false),
            _ => continue,
        };
        let Some(lhs) = konst[cmp.lhs as usize] else {
            continue;
        };
        let rhs = match cmp.rhs {
            CmpRhs::Imm(v) => v,
            CmpRhs::Reg(r) => match konst[r as usize] {
                Some(v) => v,
                None => continue,
            },
        };
        let ordering = if cmp.signed {
            (lhs as i16).cmp(&(rhs as i16))
        } else {
            lhs.cmp(&rhs)
        };
        let taken = match cmp.cond {
            CompareOp::Never => false,
            CompareOp::Greater => ordering.is_gt(),
            CompareOp::Equal => ordering.is_eq(),
            CompareOp::Less => ordering.is_lt(),
            CompareOp::GreaterEqual => !ordering.is_lt(),
            CompareOp::NotEqual => !ordering.is_eq(),
            CompareOp::LessEqual => !ordering.is_gt(),
            CompareOp::Always => true,
        };
        let (target, dead) = if taken {
            (if_true, if_false)
        } else {
            (if_false, if_true)
        };
        f.blocks[b].term = Some(Terminator::Jmp { target });
        // remove the dead CFG edge
        f.blocks[dead].preds.retain(|&p| p != b);
        for phi in &mut f.blocks[dead].phis {
            phi.args.retain(|&(p, _)| p != b);
        }
        changed = true;
    }

    changed
}

/// A signed lower-bound guard can make a following small positive comparison
/// eligible for the unsigned cmp_i instruction. Restrict this to a block with
/// one predecessor, so the range fact is guaranteed on every path.
fn use_unsigned_cmp_after_nonnegative_guard(f: &mut IrFunc, konst: &[Option<u16>]) -> bool {
    let mut rewrites = vec![];
    for block in 0..f.blocks.len() {
        let [pred] = f.blocks[block].preds.as_slice() else {
            continue;
        };
        let Some(Terminator::Br {
            cmp: guard,
            if_true,
            if_false,
        }) = &f.blocks[*pred].term
        else {
            continue;
        };
        if !guard.signed || if_true == if_false {
            continue;
        }
        let CmpRhs::Imm(raw_bound) = guard.rhs else {
            continue;
        };
        let relation = if *if_true == block {
            guard.cond
        } else if *if_false == block {
            guard.cond.invert()
        } else {
            continue;
        };
        let bound = raw_bound as i16;
        let proves_nonnegative = match relation {
            CompareOp::GreaterEqual | CompareOp::Equal => bound >= 0,
            CompareOp::Greater => bound >= -1,
            _ => false,
        };
        if !proves_nonnegative {
            continue;
        }
        let Some(Terminator::Br { cmp, .. }) = &f.blocks[block].term else {
            continue;
        };
        let rhs = match cmp.rhs {
            CmpRhs::Imm(rhs) => rhs,
            CmpRhs::Reg(rhs) => match konst[rhs as usize] {
                Some(value) => value,
                None => continue,
            },
        };
        if cmp.signed && cmp.lhs == guard.lhs && rhs <= 15 && (rhs as i16) >= 0 {
            rewrites.push((block, rhs));
        }
    }
    for &(block, rhs) in &rewrites {
        let Some(Terminator::Br { cmp, .. }) = &mut f.blocks[block].term else {
            unreachable!();
        };
        cmp.rhs = CmpRhs::Imm(rhs);
        cmp.signed = false;
    }
    !rewrites.is_empty()
}

fn compute_dominators(f: &IrFunc, rpo: &[BlockId]) -> Vec<HashSet<BlockId>> {
    let mut doms: Vec<HashSet<BlockId>> = vec![HashSet::new(); f.blocks.len()];
    doms[f.entry].insert(f.entry);
    loop {
        let mut changed = false;
        for &block in rpo {
            if block == f.entry {
                continue;
            }
            let mut intersection: Option<HashSet<BlockId>> = None;
            for &pred in &f.blocks[block].preds {
                if doms[pred].is_empty() {
                    continue;
                }
                intersection = Some(match intersection {
                    None => doms[pred].clone(),
                    Some(current) => current.intersection(&doms[pred]).copied().collect(),
                });
            }
            let mut next = intersection.unwrap_or_default();
            next.insert(block);
            if next != doms[block] {
                doms[block] = next;
                changed = true;
            }
        }
        if !changed {
            return doms;
        }
    }
}

/// Turn a constant arm of a return diamond into a default value established
/// before the condition. All branches for that arm can then target the common
/// return block directly, allowing the successful arm to fall through after
/// overwriting the return register.
fn hoist_constant_return_arm(f: &mut IrFunc, transformed: &mut HashSet<BlockId>) -> bool {
    let rpo = f.rpo();
    let doms = compute_dominators(f, &rpo);

    for join in rpo {
        // Hoisting both constant arms would make the later default overwrite
        // the earlier one before the condition executes.
        if transformed.contains(&join) {
            continue;
        }
        let return_value = match &f.blocks[join].term {
            Some(Terminator::Ret { values }) if values.len() == 1 => values[0],
            _ => continue,
        };
        if !f.blocks[join].insts.is_empty() || f.blocks[join].phis.len() != 1 {
            continue;
        }
        let phi = &f.blocks[join].phis[0];
        if phi.dst != return_value {
            continue;
        }
        for &(constant_block, incoming) in &phi.args {
            let constant = &f.blocks[constant_block];
            let value = match constant.insts.as_slice() {
                [Instr::LoadImm { dst, value }] if *dst == incoming => *value,
                _ => continue,
            };
            if !constant.phis.is_empty()
                || !matches!(constant.term, Some(Terminator::Jmp { target }) if target == join)
                || constant.preds.is_empty()
            {
                continue;
            }
            let Some(hoist) = doms[constant_block]
                .iter()
                .filter(|&&block| block != constant_block)
                .max_by_key(|&&block| doms[block].len())
                .copied()
            else {
                continue;
            };
            let old_preds = f.blocks[constant_block].preds.clone();

            f.blocks[hoist].insts.push(Instr::LoadImm {
                dst: incoming,
                value,
            });
            // This is scheduled initialization of the result, not execution
            // of the source-level else arm.
            let scheduled_line = f.blocks[hoist].term_line.or(f.block_lines[hoist]);
            f.blocks[hoist].lines.push(scheduled_line);

            for &pred in &old_preds {
                match &mut f.blocks[pred].term {
                    Some(Terminator::Jmp { target }) => {
                        if *target == constant_block {
                            *target = join;
                        }
                    }
                    Some(Terminator::Br {
                        if_true, if_false, ..
                    }) => {
                        if *if_true == constant_block {
                            *if_true = join;
                        }
                        if *if_false == constant_block {
                            *if_false = join;
                        }
                    }
                    _ => {}
                }
            }
            f.blocks[constant_block].insts.clear();
            f.blocks[constant_block].lines.clear();
            f.blocks[constant_block].preds.clear();

            f.blocks[join].preds.retain(|&pred| pred != constant_block);
            f.blocks[join].preds.extend(old_preds.iter().copied());
            let phi = &mut f.blocks[join].phis[0];
            phi.args.retain(|&(pred, _)| pred != constant_block);
            phi.args
                .extend(old_preds.into_iter().map(|pred| (pred, incoming)));
            transformed.insert(join);
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// CSE / GVN (dominator-scoped) + copy propagation
// ---------------------------------------------------------------------------

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
enum Key {
    Bin(BinOp, VReg, IntOperand),
    Mul(MulWindow, VReg, IntOperand),
    Un(UnOp, VReg),
    Shift(ShiftOp, VReg, IntOperand),
    Imm(u16),
}

fn canon_operand(replace: &HashMap<VReg, VReg>, operand: IntOperand) -> IntOperand {
    match operand {
        IntOperand::Imm(value) => IntOperand::Imm(value),
        IntOperand::Reg(v) => {
            let mut v = v;
            while let Some(&r) = replace.get(&v) {
                v = r;
            }
            IntOperand::Reg(v)
        }
    }
}

fn cse(f: &mut IrFunc) -> bool {
    // dominators (simple iterative)
    let rpo = f.rpo();
    let doms = compute_dominators(f, &rpo);
    // dom tree children (idom = strict dominator with the largest dom set)
    let mut children: Vec<Vec<BlockId>> = vec![vec![]; f.blocks.len()];
    for &b in &rpo {
        if b == f.entry {
            continue;
        }
        if let Some(idom) = doms[b]
            .iter()
            .filter(|&&d| d != b)
            .max_by_key(|&&d| doms[d].len())
            .copied()
        {
            children[idom].push(b);
        }
    }

    // Copy/CSE aliasing is only sound for a vreg with a single definition. The
    // if-conversion pass deliberately redefines its destination (a Mov/LoadImm
    // feeding a CMov), and aliasing that vreg would replace the uses after the
    // conditional write and drop it.
    let mut seen_defs: HashSet<VReg> = HashSet::new();
    let mut multi_def: HashSet<VReg> = HashSet::new();
    for b in &f.blocks {
        for inst in &b.insts {
            for def in crate::compiler::regalloc::inst_defs(inst) {
                if !seen_defs.insert(def) {
                    multi_def.insert(def);
                }
            }
        }
    }

    fn canon(replace: &HashMap<VReg, VReg>, mut v: VReg) -> VReg {
        while let Some(&r) = replace.get(&v) {
            v = r;
        }
        v
    }
    fn pure_key(inst: &Instr, replace: &HashMap<VReg, VReg>) -> Option<(Key, VReg)> {
        match inst {
            Instr::Bin { dst, op, lhs, rhs } => {
                let (mut a, mut b) = (canon(replace, *lhs), canon_operand(replace, *rhs));
                if matches!(op, BinOp::Add | BinOp::And | BinOp::Or | BinOp::Xor) {
                    if let IntOperand::Reg(rb) = b {
                        if a > rb {
                            b = IntOperand::Reg(a);
                            a = rb;
                        }
                    }
                }
                Some((Key::Bin(*op, a, b), *dst))
            }
            Instr::Mul {
                dst,
                window,
                lhs,
                rhs,
            } => {
                // every multiply window is commutative
                let (mut a, mut b) = (canon(replace, *lhs), canon_operand(replace, *rhs));
                if let IntOperand::Reg(rb) = b {
                    if a > rb {
                        b = IntOperand::Reg(a);
                        a = rb;
                    }
                }
                Some((Key::Mul(*window, a, b), *dst))
            }
            Instr::Un { dst, op, src } => Some((Key::Un(*op, canon(replace, *src)), *dst)),
            Instr::Shift {
                dst,
                op,
                src,
                amount,
            } => Some((
                Key::Shift(*op, canon(replace, *src), canon_operand(replace, *amount)),
                *dst,
            )),
            Instr::LoadImm { dst, value } => Some((Key::Imm(*value), *dst)),
            _ => None,
        }
    }

    let mut replace: HashMap<VReg, VReg> = HashMap::new();
    let mut changed = false;
    fn walk(
        b: BlockId,
        f: &IrFunc,
        children: &[Vec<BlockId>],
        multi_def: &HashSet<VReg>,
        scope: &mut HashMap<Key, VReg>,
        replace: &mut HashMap<VReg, VReg>,
        changed: &mut bool,
    ) {
        let mut added = vec![];
        for inst in &f.blocks[b].insts {
            match inst {
                Instr::Mov { dst, src } | Instr::FpuMov { dst, src } => {
                    if !multi_def.contains(dst) {
                        replace.insert(*dst, canon(replace, *src));
                        *changed = true;
                    }
                }
                _ => {
                    if let Some((key, dst)) = pure_key(inst, replace) {
                        if let Some(&found) = scope.get(&key) {
                            if !multi_def.contains(&dst) {
                                replace.insert(dst, found);
                                *changed = true;
                            }
                        } else {
                            scope.insert(key.clone(), dst);
                            added.push(key);
                        }
                    }
                }
            }
        }
        for &c in &children[b] {
            walk(c, f, children, multi_def, scope, replace, changed);
        }
        for k in added {
            scope.remove(&k);
        }
    }
    {
        let mut scope = HashMap::new();
        walk(
            f.entry,
            f,
            &children,
            &multi_def,
            &mut scope,
            &mut replace,
            &mut changed,
        );
    }

    if changed {
        subst_uses(f, &replace);
    }
    changed
}

// ---------------------------------------------------------------------------
// dead code elimination
// ---------------------------------------------------------------------------

fn dce(f: &mut IrFunc) -> bool {
    let mut useful: HashSet<VReg> = HashSet::new();
    // seed: side-effecting instructions and terminators
    loop {
        let mut changed = false;
        let mark = |v: VReg, useful: &mut HashSet<VReg>, changed: &mut bool| {
            if useful.insert(v) {
                *changed = true;
            }
        };
        for b in &f.blocks {
            for phi in &b.phis {
                if useful.contains(&phi.dst) {
                    for &(_, v) in &phi.args {
                        mark(v, &mut useful, &mut changed);
                    }
                }
            }
            for inst in &b.insts {
                // SIGNAL is an observable compiler barrier and never dies
                let root = matches!(
                    inst,
                    Instr::StoreMem { .. }
                        | Instr::StoreStatic { .. }
                        | Instr::StoreSp { .. }
                        | Instr::StoreLocal { .. }
                        | Instr::FpuStore { .. }
                        | Instr::Call { .. }
                        | Instr::CallPtr { .. }
                        | Instr::DevSend { .. }
                        | Instr::DevRecv { .. }
                        | Instr::DcacheInvalidateAll
                        | Instr::MtsrDseg { .. }
                        | Instr::Jseg { .. }
                        | Instr::Signal { .. }
                );
                let defs = crate::compiler::regalloc::inst_defs(inst);
                if root || defs.iter().any(|d| useful.contains(d)) {
                    for u in crate::compiler::regalloc::inst_uses(inst) {
                        mark(u, &mut useful, &mut changed);
                    }
                }
            }
            if let Some(term) = &b.term {
                for u in match term {
                    Terminator::Jmp { .. } => vec![],
                    Terminator::Br { cmp, .. } => match &cmp.rhs {
                        CmpRhs::Reg(r) => vec![cmp.lhs, *r],
                        CmpRhs::Imm(_) => vec![cmp.lhs],
                    },
                    Terminator::Ret { values } => values.clone(),
                    Terminator::Halt { signal } => vec![*signal],
                    Terminator::IcacheInvalidateDelayedAndJump { cseg, target } => {
                        vec![*cseg, *target]
                    }
                } {
                    mark(u, &mut useful, &mut changed);
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut changed = false;
    for b in &mut f.blocks {
        let before = b.insts.len() + b.phis.len();
        b.phis.retain(|p| useful.contains(&p.dst));
        let keep = |inst: &Instr| {
            let defs = crate::compiler::regalloc::inst_defs(inst);
            // removable instructions are those without side effects
            let removable = matches!(
                inst,
                Instr::Bin { .. }
                    | Instr::Mul { .. }
                    | Instr::Un { .. }
                    | Instr::Shift { .. }
                    | Instr::Mov { .. }
                    | Instr::LoadImm { .. }
                    | Instr::LoadMem { .. }
                    | Instr::LoadSp { .. }
                    | Instr::LoadLocal { .. }
                    | Instr::AddrOfLocal { .. }
                    | Instr::Mfsr { .. }
                    | Instr::Bool { .. }
                    | Instr::CMov { .. }
                    | Instr::FpuBin { .. }
                    | Instr::FpuUn { .. }
                    | Instr::FpuMov { .. }
                    | Instr::FpuFromInt { .. }
                    | Instr::FpuFromLo { .. }
                    | Instr::FpuFromHi { .. }
                    | Instr::FpuToInt { .. }
                    | Instr::FpuToLo { .. }
                    | Instr::FpuToHi { .. }
                    | Instr::FpuLoad { .. }
                    | Instr::AddrOfFpuSpill { .. }
            );
            !removable || defs.iter().any(|d| useful.contains(d))
        };
        let insts = std::mem::take(&mut b.insts);
        let lines = std::mem::take(&mut b.lines);
        for (inst, line) in insts.into_iter().zip(lines) {
            if keep(&inst) {
                b.insts.push(inst);
                b.lines.push(line);
            }
        }
        changed |= before != b.insts.len() + b.phis.len();
    }
    changed
}

// ---------------------------------------------------------------------------
// safe if-conversion (CpuV3): simple one-instruction diamonds become a
// Boolean-producing comparison or a conditional move
// ---------------------------------------------------------------------------

/// One arm of a convertible diamond, seen from its phi argument: the arm is
/// empty (the phi uses an earlier value directly), a single LoadImm, a single
/// Mov, or a single pure value instruction; it has exactly one predecessor and
/// ends in a plain jump.
#[derive(Clone)]
enum ArmValue {
    /// value vreg used as-is
    Reg(VReg),
    /// constant produced by the arm's own LoadImm
    Imm(u16),
    /// value produced by a single pure instruction, which is hoisted into the
    /// branch block (the instruction defines the phi argument)
    Expr(Instr),
}

/// the vreg an instruction defines, if it is one of the value-producing forms
/// this pass can hoist
fn hoistable_def(inst: &Instr) -> Option<VReg> {
    match inst {
        Instr::Bin { dst, .. }
        | Instr::Mul { dst, .. }
        | Instr::Shift { dst, .. }
        | Instr::Un { dst, .. } => Some(*dst),
        _ => None,
    }
}

/// whether an instruction is side-effect-free, non-faulting, and lowers
/// without an internal branch (so it can run unconditionally before the
/// compare). `Log2` is excluded because it expands to a branch.
fn hoistable_value(inst: &Instr) -> bool {
    match inst {
        Instr::Bin { .. } | Instr::Mul { .. } | Instr::Shift { .. } => true,
        Instr::Un { op, .. } => !matches!(op, UnOp::Log2),
        _ => false,
    }
}

fn diamond_arm(
    f: &IrFunc,
    block: BlockId,
    pred: BlockId,
    phi_arg: VReg,
) -> Option<(ArmValue, BlockId)> {
    let arm = &f.blocks[block];
    if !arm.phis.is_empty() || arm.preds.as_slice() != [pred] {
        return None;
    }
    let Some(Terminator::Jmp { target }) = arm.term else {
        return None;
    };
    match arm.insts.as_slice() {
        [] => Some((ArmValue::Reg(phi_arg), target)),
        [Instr::LoadImm { dst, value }] if *dst == phi_arg => Some((ArmValue::Imm(*value), target)),
        [Instr::Mov { dst, src }] if *dst == phi_arg => Some((ArmValue::Reg(*src), target)),
        [inst] if hoistable_def(inst) == Some(phi_arg) && hoistable_value(inst) => {
            Some((ArmValue::Expr(inst.clone()), target))
        }
        _ => None,
    }
}

/// Convert simple if-expression diamonds to `Bool`/`CMov` instructions.
///
/// Eligible shape (each arm is empty, a single LoadImm, a single Mov, or a
/// single pure value instruction, so there are no side effects, no faults, and
/// no nested control flow):
///
/// ```text
/// B: br cmp, T, F
/// T: ...; jmp J
/// F: ...; jmp J
/// J: dst = phi [(T, vT), (F, vF)]   (J's preds are exactly T and F)
/// ```
///
/// The converted form (compare + move + conditional move, or one Boolean
/// comparison) never exceeds the branchy form (compare + branch + jump + arm
/// words) in static word count. Only the CpuV3 backend runs this pass, and
/// only with optimizations enabled; CpuV2 keeps the branchy expansion.
/// Resolve a `CmpRhs::Reg` back to `CmpRhs::Imm` when the register is defined
/// by a single LoadImm, so constant comparisons in converted diamonds select
/// the immediate encodings.
fn resolve_cmp_imm(f: &IrFunc, cmp: &Cmp) -> Cmp {
    let mut cmp = *cmp;
    if let CmpRhs::Reg(v) = cmp.rhs {
        let value = f.blocks.iter().flat_map(|b| &b.insts).find_map(|inst| {
            if let Instr::LoadImm { dst, value } = inst {
                (*dst == v).then_some(*value)
            } else {
                None
            }
        });
        if let Some(value) = value {
            cmp.rhs = CmpRhs::Imm(value);
        }
    }
    cmp
}

pub fn convert_diamonds(f: &mut IrFunc) -> bool {
    let mut changed = false;
    for b in 0..f.blocks.len() {
        let Some(Terminator::Br {
            cmp,
            if_true,
            if_false,
        }) = f.blocks[b].term.clone()
        else {
            continue;
        };
        if if_true == if_false || if_true == b || if_false == b {
            continue;
        }
        // only the six real predicates map to hardware conditions
        if matches!(cmp.cond, CompareOp::Never | CompareOp::Always) {
            continue;
        }
        // find the join: one block with exactly these two preds and one phi
        // selecting between the two arm values
        let mut converted = None;
        for join in 0..f.blocks.len() {
            if join == b || join == if_true || join == if_false {
                continue;
            }
            if f.blocks[join].preds.as_slice() != [if_true, if_false]
                && f.blocks[join].preds.as_slice() != [if_false, if_true]
            {
                continue;
            }
            if f.blocks[join].phis.len() != 1 {
                continue;
            }
            let phi = &f.blocks[join].phis[0];
            let v_true = phi
                .args
                .iter()
                .find(|&&(p, _)| p == if_true)
                .map(|&(_, v)| v);
            let v_false = phi
                .args
                .iter()
                .find(|&&(p, _)| p == if_false)
                .map(|&(_, v)| v);
            let (Some(v_true), Some(v_false)) = (v_true, v_false) else {
                continue;
            };
            let (Some((true_value, _)), Some((false_value, _))) = (
                diamond_arm(f, if_true, b, v_true),
                diamond_arm(f, if_false, b, v_false),
            ) else {
                continue;
            };
            // Only GPR diamonds are converted: the Boolean/CMov lowering is a
            // GPR form, and an FPU phi would otherwise be rewritten to a plain
            // `Mov` between F registers.
            if f.class_of(phi.dst) != RegClass::Gpr {
                continue;
            }
            converted = Some((join, phi.dst, true_value, false_value));
            break;
        }
        let Some((join, dst, true_value, false_value)) = converted else {
            continue;
        };
        let cmp = resolve_cmp_imm(f, &cmp);
        let line = f.blocks[b].term_line;

        // Boolean materialization: arms are exactly 1 and 0 (in any order)
        let bool_cmp = match (&true_value, &false_value) {
            (ArmValue::Imm(1), ArmValue::Imm(0)) => Some(cmp),
            (ArmValue::Imm(0), ArmValue::Imm(1)) => {
                let mut inverted = cmp;
                inverted.cond = inverted.cond.invert();
                Some(inverted)
            }
            _ => None,
        };
        let mut insts = vec![];
        if let Some(bool_cmp) = bool_cmp {
            insts.push(Instr::Bool { dst, cmp: bool_cmp });
        } else {
            // Hoist any arm that computes its value: a pure single instruction
            // can run unconditionally before the compare. Arms are independent
            // in SSA, so the order between them is irrelevant.
            let mut arm_def = |arm: &ArmValue| -> Option<VReg> {
                if let ArmValue::Expr(inst) = arm {
                    insts.push(inst.clone());
                    hoistable_def(inst)
                } else {
                    None
                }
            };
            let true_def = arm_def(&true_value);
            let false_def = arm_def(&false_value);
            // dst = false value; dst = true value when the condition holds
            match &false_value {
                ArmValue::Imm(value) => insts.push(Instr::LoadImm { dst, value: *value }),
                ArmValue::Reg(src) => insts.push(Instr::Mov { dst, src: *src }),
                ArmValue::Expr(_) => insts.push(Instr::Mov {
                    dst,
                    src: false_def.expect("computed arm has a defined value"),
                }),
            }
            let src = match true_value {
                ArmValue::Imm(value) => {
                    let tmp = f.fresh_vreg(crate::RegClass::Gpr);
                    insts.push(Instr::LoadImm { dst: tmp, value });
                    tmp
                }
                ArmValue::Reg(src) => src,
                ArmValue::Expr(_) => true_def.expect("computed arm has a defined value"),
            };
            insts.push(Instr::CMov { dst, cmp, src });
        }

        let block = &mut f.blocks[b];
        for inst in insts {
            block.insts.push(inst);
            block.lines.push(line);
        }
        block.term = Some(Terminator::Jmp { target: join });
        for dead in [if_true, if_false] {
            // The arm blocks become unreachable: drop their terminator too so no
            // later pass can rediscover an edge from them to the join.
            let arm = &mut f.blocks[dead];
            arm.insts.clear();
            arm.lines.clear();
            arm.phis.clear();
            arm.preds.clear();
            arm.term = None;
            arm.term_line = None;
        }
        f.blocks[join].phis.clear();
        f.blocks[join].preds = vec![b];
        changed = true;
    }
    changed
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::builder::FuncBuilder;
    use crate::CompareOp;

    fn opt(f: &mut IrFunc) {
        optimize(f, &Opts::default());
    }

    #[test]
    fn test_const_prop() {
        let (mut b, _) = FuncBuilder::new("f", 0, 1);
        let x = b.load_imm(2);
        let y = b.load_imm(3);
        let z = b.bin(BinOp::Add, x, y);
        b.ret(&[z]);
        let mut f = b.finish();
        opt(&mut f);
        let text = f.to_string();
        assert!(text.contains("imm 5"), "{text}");
        assert!(!text.contains("add "), "{text}");
    }

    #[test]
    fn test_branch_fold() {
        let (mut b, _) = FuncBuilder::new("f", 0, 1);
        let x = b.load_imm(2);
        let y = b.load_imm(3);
        let cmp = b.cmp(x, CmpRhs::Reg(y), CompareOp::Less);
        let r = b.new_var();
        b.if_else(
            cmp,
            |b| {
                let v = b.load_imm(10);
                b.set(r, v);
            },
            |b| {
                let v = b.load_imm(20);
                b.set(r, v);
            },
        );
        let v = b.get(r);
        b.ret(&[v]);
        let mut f = b.finish();
        opt(&mut f);
        let text = f.to_string();
        // 2 < 3 is always true: no conditional branch remains
        assert!(!text.contains("br "), "{text}");
        assert!(text.contains("imm 10"), "{text}");
    }

    #[test]
    fn test_cse() {
        let (mut b, params) = FuncBuilder::new("f", 2, 1);
        let (x, y) = (b.get(params[0]), b.get(params[1]));
        let s1 = b.bin(BinOp::Add, x, y);
        let s2 = b.bin(BinOp::Add, x, y);
        let z = b.bin(BinOp::Add, s1, s2);
        b.ret(&[z]);
        let mut f = b.finish();
        opt(&mut f);
        // s2 is replaced by s1; only two adds remain (s1's and z = s1 + s1)
        let adds = f.blocks[0]
            .insts
            .iter()
            .filter(|i| matches!(i, Instr::Bin { .. }))
            .count();
        assert_eq!(adds, 2, "{}", f);
    }

    #[test]
    fn test_dce() {
        let (mut b, _) = FuncBuilder::new("f", 0, 1);
        let _dead = b.load_imm(42);
        let x = b.load_imm(7);
        b.ret(&[x]);
        let mut f = b.finish();
        opt(&mut f);
        let text = f.to_string();
        assert!(!text.contains("42"), "{text}");
    }

    #[test]
    fn test_const_prop_in_loop() {
        // loop-invariant computation is folded; the loop's own phis stay
        let (mut b, params) = FuncBuilder::new("sum", 1, 1);
        let n = params[0];
        let sum = b.new_var();
        let zero = b.load_imm(0);
        b.set(sum, zero);
        let factor = {
            let two = b.load_imm(2);
            let three = b.load_imm(3);
            b.bin(BinOp::Add, two, three) // 5, loop-invariant
        };
        b.while_loop(
            |b| {
                let s = b.get(sum);
                let n = b.get(n);
                b.cmp(s, CmpRhs::Reg(n), CompareOp::Less)
            },
            |b| {
                let s = b.get(sum);
                let s = b.bin(BinOp::Add, s, factor);
                b.set(sum, s);
            },
        );
        let s = b.get(sum);
        b.ret(&[s]);
        let mut f = b.finish();
        opt(&mut f);
        let text = f.to_string();
        assert!(text.contains("imm 5"), "{text}");
    }

    #[test]
    fn test_unencodable_compare_constant_stays_in_a_register() {
        let (mut b, params) = FuncBuilder::new("f", 1, 1);
        let x = b.get(params[0]);
        let limit = b.load_imm(16);
        let result = b.new_var();
        let cmp = b.cmp(x, CmpRhs::Reg(limit), CompareOp::Less);
        b.if_else(
            cmp,
            |b| {
                let value = b.load_imm(1);
                b.set(result, value);
            },
            |b| {
                let value = b.load_imm(0);
                b.set(result, value);
            },
        );
        let value = b.get(result);
        b.ret(&[value]);
        let mut f = b.finish();
        opt(&mut f);

        assert!(f.blocks.iter().any(|block| block
            .insts
            .iter()
            .any(|inst| matches!(inst, Instr::LoadImm { dst, value: 16 } if *dst == limit))));
        assert!(f.blocks.iter().any(|block| matches!(
            block.term,
            Some(Terminator::Br {
                cmp: Cmp {
                    rhs: CmpRhs::Reg(rhs),
                    ..
                },
                ..
            }) if rhs == limit
        )));
    }
}
