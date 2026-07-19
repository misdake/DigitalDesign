//! optimization passes on IrFunc (run before register allocation).
//! all passes are individually switchable via `Opts`.

use crate::compiler::builder::remove_trivial_phis;
use crate::compiler::ir::*;
use crate::sim::{calc_flags, calc_flags_signed};
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

pub fn optimize(f: &mut IrFunc, opts: &Opts) {
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
        if !changed {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// substitution helper (uses only, never defs)
// ---------------------------------------------------------------------------

pub(crate) fn subst_uses(f: &mut IrFunc, replace: &HashMap<VReg, VReg>) {
    let subst = |v: &mut VReg| {
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
            match inst {
                Instr::Bin { lhs, rhs, .. } => {
                    subst(lhs);
                    subst(rhs);
                }
                Instr::Un { src, .. } | Instr::Shift { src, .. } | Instr::Mov { src, .. } => {
                    subst(src)
                }
                Instr::LoadImm { .. } | Instr::DevRecv { .. } | Instr::LoadSp { .. } | Instr::LoadLocal { .. } | Instr::AddrOfLocal { .. } => {}
                Instr::LoadMem { base, .. } => subst(base),
                Instr::StoreMem { base, src, .. } => {
                    subst(base);
                    subst(src);
                }
                Instr::Call { args, .. } => args.iter_mut().for_each(&subst),
                Instr::LoadFuncAddr { .. } => {}
                Instr::CallPtr { addr, args, .. } => {
                    subst(addr);
                    args.iter_mut().for_each(&subst);
                }
                Instr::DevSend { src, .. } | Instr::StoreSp { src, .. } | Instr::StoreLocal { src, .. } => subst(src),
            }
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
fn fold_un(op: UnOp, a: u16) -> Option<u16> {
    Some(match op {
        UnOp::Inv => !a,
        UnOp::Neg => (a as i16).wrapping_neg() as u16,
        UnOp::Not0 => (a != 0) as u16,
        UnOp::Cnt1 => a.count_ones() as u16,
        UnOp::Log2 => {
            if a == 0 {
                return None; // hardware result undefined for 0
            }
            a.ilog2() as u16
        }
    })
}
fn fold_shift(op: ShiftOp, a: u16, amount: u8) -> u16 {
    match op {
        ShiftOp::Lsl => a.wrapping_shl(amount as u32),
        ShiftOp::Lsr => a.wrapping_shr(amount as u32),
        ShiftOp::Asr => ((a as i16) >> amount) as u16,
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
                        if let (Some(a), Some(b)) = (konst[*lhs as usize], konst[*rhs as usize]) {
                            learn(&mut konst, *dst, fold_bin(*op, a, b), &mut changed);
                        }
                    }
                    Instr::Un { dst, op, src } => {
                        if let Some(a) = konst[*src as usize] {
                            if let Some(x) = fold_un(*op, a) {
                                learn(&mut konst, *dst, x, &mut changed);
                            }
                        }
                    }
                    Instr::Shift { dst, op, src, amount } => {
                        if let Some(a) = konst[*src as usize] {
                            learn(&mut konst, *dst, fold_shift(*op, a, *amount), &mut changed);
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
        let flags = if cmp.signed {
            calc_flags_signed(lhs, rhs)
        } else {
            calc_flags(lhs, rhs)
        };
        let taken = (cmp.cond as u8) & flags != 0;
        let (target, dead) = if taken { (if_true, if_false) } else { (if_false, if_true) };
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

// ---------------------------------------------------------------------------
// CSE / GVN (dominator-scoped) + copy propagation
// ---------------------------------------------------------------------------

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
enum Key {
    Bin(BinOp, VReg, VReg),
    Un(UnOp, VReg),
    Shift(ShiftOp, VReg, u8),
    Imm(u16),
}

fn cse(f: &mut IrFunc) -> bool {
    // dominators (simple iterative)
    let rpo = f.rpo();
    let mut doms: Vec<HashSet<BlockId>> = vec![HashSet::new(); f.blocks.len()];
    doms[f.entry].insert(f.entry);
    loop {
        let mut changed = false;
        for &b in &rpo {
            if b == f.entry {
                continue;
            }
            let mut d: Option<HashSet<BlockId>> = None;
            for &p in &f.blocks[b].preds {
                if doms[p].is_empty() {
                    continue; // pred not yet numbered (back edge, loop header)
                }
                d = Some(match d {
                    None => doms[p].clone(),
                    Some(acc) => acc.intersection(&doms[p]).copied().collect(),
                });
            }
            let mut d = d.unwrap_or_default();
            d.insert(b);
            if d != doms[b] {
                doms[b] = d;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
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

    fn canon(replace: &HashMap<VReg, VReg>, mut v: VReg) -> VReg {
        while let Some(&r) = replace.get(&v) {
            v = r;
        }
        v
    }
    fn pure_key(inst: &Instr, replace: &HashMap<VReg, VReg>) -> Option<(Key, VReg)> {
        match inst {
            Instr::Bin { dst, op, lhs, rhs } => {
                let (mut a, mut b) = (canon(replace, *lhs), canon(replace, *rhs));
                if matches!(op, BinOp::Add | BinOp::And | BinOp::Or | BinOp::Xor) && a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                Some((Key::Bin(*op, a, b), *dst))
            }
            Instr::Un { dst, op, src } => Some((Key::Un(*op, canon(replace, *src)), *dst)),
            Instr::Shift { dst, op, src, amount } => {
                Some((Key::Shift(*op, canon(replace, *src), *amount), *dst))
            }
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
        scope: &mut HashMap<Key, VReg>,
        replace: &mut HashMap<VReg, VReg>,
        changed: &mut bool,
    ) {
        let mut added = vec![];
        for inst in &f.blocks[b].insts {
            match inst {
                Instr::Mov { dst, src } => {
                    replace.insert(*dst, canon(replace, *src));
                    *changed = true;
                }
                _ => {
                    if let Some((key, dst)) = pure_key(inst, replace) {
                        if let Some(&found) = scope.get(&key) {
                            replace.insert(dst, found);
                            *changed = true;
                        } else {
                            scope.insert(key.clone(), dst);
                            added.push(key);
                        }
                    }
                }
            }
        }
        for &c in &children[b] {
            walk(c, f, children, scope, replace, changed);
        }
        for k in added {
            scope.remove(&k);
        }
    }
    {
        let mut scope = HashMap::new();
        walk(f.entry, f, &children, &mut scope, &mut replace, &mut changed);
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
                let root = matches!(
                    inst,
                    Instr::StoreMem { .. }
                        | Instr::StoreSp { .. }
                        | Instr::StoreLocal { .. }
                        | Instr::Call { .. }
                        | Instr::CallPtr { .. }
                        | Instr::DevSend { .. }
                        | Instr::DevRecv { .. }
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
                    | Instr::Un { .. }
                    | Instr::Shift { .. }
                    | Instr::Mov { .. }
                    | Instr::LoadImm { .. }
                    | Instr::LoadMem { .. }
                    | Instr::LoadSp { .. }
                    | Instr::LoadLocal { .. }
                    | Instr::AddrOfLocal { .. }
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
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isa::Cond;
    use crate::compiler::builder::FuncBuilder;

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
        let cmp = b.cmp(x, CmpRhs::Reg(y), Cond::Less);
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
                b.cmp(s, CmpRhs::Reg(n), Cond::Less)
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
}
