//! register allocation for compiler IR.
//!
//! pipeline per function:
//! 1. ABI shims: params/call args/call results/ret values are copied through
//!    fresh vregs pinned to their ABI registers (short fixed intervals), so
//!    the allocator itself is fully uniform.
//! 2. critical edge splitting, so phi operand moves always have a safe
//!    insertion point (pred with single successor).
//! 3. linear scan over live intervals (RPO numbering); intervals crossing a
//!    call are restricted to callee-save regs or the stack. spilling rewrites
//!    the IR (explicit LoadSp/StoreSp) and rescans until fixpoint.
//! 4. frame layout: [callee-save saves][spill slots], sized by actual need.

use crate::compiler::ir::*;
use std::collections::{BTreeSet, HashMap, HashSet};

pub const REG_RA: u8 = 13;
pub const REG_SP: u8 = 14;
pub const REG_TMP: u8 = 15;
pub const RET_REGS: [u8; 2] = [0, 1];
pub const ARG_REGS: [u8; 6] = [2, 3, 4, 5, 6, 7];
pub const CALLER_SAVED: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
pub const CALLEE_SAVED: [u8; 5] = [8, 9, 10, 11, 12];
pub const MAX_FRAME: usize = 255;

/// result of register allocation for one function
pub struct Allocation {
    /// final register for every vreg (spilled vregs have been rewritten to
    /// explicit LoadSp/StoreSp, so every remaining vreg has a register)
    pub reg: HashMap<VReg, u8>,
    /// callee-save regs actually used; saved/restored at frame slots 0..k
    pub callee_saved: Vec<u8>,
    /// number of spill frame slots used
    pub spill_slots: u8,
}
impl Allocation {
    pub fn frame_size(&self) -> usize {
        self.callee_saved.len() + self.spill_slots as usize
    }
    /// frame slot index of a spill slot (they follow the callee-save area)
    pub fn spill_frame_slot(&self, slot: u8) -> u8 {
        self.callee_saved.len() as u8 + slot
    }
}

pub fn allocate(func: &IrFunc, coalesce: bool) -> (IrFunc, Allocation) {
    let mut f = func.clone();
    let abi = insert_abi_shims(&mut f);
    split_critical_edges(&mut f);

    let mut spill_slots = 0u8;
    for _ in 0..8 {
        let intervals = compute_intervals(&f);
        let affinity = if coalesce {
            compute_affinity(&f)
        } else {
            HashMap::new()
        };
        let scan = linear_scan(&intervals, &abi, &affinity);
        if scan.spilled.is_empty() {
            let mut callee_saved: Vec<u8> = scan
                .reg
                .values()
                .filter(|r| CALLEE_SAVED.contains(r))
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            // non-leaf functions must also preserve ra (calls clobber r13)
            if f.blocks.iter().any(|b| {
                b.insts
                    .iter()
                    .any(|i| matches!(i, Instr::Call { .. } | Instr::CallPtr { .. }))
            }) {
                callee_saved.push(REG_RA);
            }
            let alloc = Allocation {
                reg: scan.reg,
                callee_saved,
                spill_slots,
            };
            assert!(
                alloc.frame_size() <= MAX_FRAME,
                "function {} needs {} frame slots, max {MAX_FRAME}",
                f.name,
                alloc.frame_size()
            );
            return (f, alloc);
        }
        rewrite_spills(&mut f, &scan.spilled, &mut spill_slots);
    }
    panic!("register allocation for {} did not converge", f.name)
}

// ---------------------------------------------------------------------------
// instruction use/def helpers
// ---------------------------------------------------------------------------

pub(crate) fn inst_uses(inst: &Instr) -> Vec<VReg> {
    match inst {
        Instr::Bin { lhs, rhs, .. } => vec![*lhs, *rhs],
        Instr::Un { src, .. } | Instr::Shift { src, .. } | Instr::Mov { src, .. } => vec![*src],
        Instr::LoadImm { .. } | Instr::DevRecv { .. } | Instr::LoadSp { .. } | Instr::LoadFuncAddr { .. } => {
            vec![]
        }
        Instr::LoadMem { base, .. } => vec![*base],
        Instr::StoreMem { base, src, .. } => vec![*base, *src],
        Instr::Call { args, .. } => args.clone(),
        Instr::CallPtr { addr, args, .. } => {
            let mut u = vec![*addr];
            u.extend_from_slice(args);
            u
        }
        Instr::DevSend { src, .. } | Instr::StoreSp { src, .. } => vec![*src],
    }
}
pub(crate) fn inst_defs(inst: &Instr) -> Vec<VReg> {
    match inst {
        Instr::Bin { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Shift { dst, .. }
        | Instr::Mov { dst, .. }
        | Instr::LoadImm { dst, .. }
        | Instr::LoadMem { dst, .. }
        | Instr::DevRecv { dst, .. }
        | Instr::LoadFuncAddr { dst, .. }
        | Instr::LoadSp { dst, .. } => vec![*dst],
        Instr::StoreMem { .. } | Instr::DevSend { .. } | Instr::StoreSp { .. } => vec![],
        Instr::Call { rets, .. } | Instr::CallPtr { rets, .. } => rets.clone(),
    }
}
fn term_uses(term: &Terminator) -> Vec<VReg> {
    match term {
        Terminator::Jmp { .. } => vec![],
        Terminator::Br { cmp, .. } => match &cmp.rhs {
            CmpRhs::Reg(r) => vec![cmp.lhs, *r],
            CmpRhs::Imm(_) => vec![cmp.lhs],
        },
        Terminator::Ret { values } => values.clone(),
        Terminator::Halt { signal } => vec![*signal],
    }
}

fn replace_all_uses(f: &mut IrFunc, from: VReg, to: VReg) {
    let subst = |v: &mut VReg| {
        if *v == from {
            *v = to;
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
                Instr::Un { src, .. }
                | Instr::Shift { src, .. }
                | Instr::Mov { src, .. }
                | Instr::DevSend { src, .. }
                | Instr::StoreSp { src, .. } => subst(src),
                Instr::LoadImm { .. } | Instr::DevRecv { .. } | Instr::LoadSp { .. } => {}
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
// step 1: ABI shims
// ---------------------------------------------------------------------------

struct AbiInfo {
    /// vregs forced into a fixed register (short intervals by construction)
    pinned: HashMap<VReg, u8>,
}

fn insert_abi_shims(f: &mut IrFunc) -> AbiInfo {
    let mut pinned = HashMap::new();
    let fresh = |f: &mut IrFunc| {
        let v = f.vreg_count;
        f.vreg_count += 1;
        v
    };

    assert!(
        f.params.len() <= ARG_REGS.len(),
        "function {} has {} params, max {}",
        f.name,
        f.params.len(),
        ARG_REGS.len()
    );
    assert!(f.n_rets <= RET_REGS.len(), "function {} has too many return values", f.name);

    // params: pin to ARG_REGS; copy to a fresh vreg for all real uses
    let params = f.params.clone();
    for (i, p) in params.iter().enumerate() {
        pinned.insert(*p, ARG_REGS[i]);
        let used = f.blocks.iter().any(|b| {
            b.phis.iter().any(|phi| phi.args.iter().any(|(_, v)| v == p))
                || b.insts.iter().any(|inst| inst_uses(inst).contains(p))
                || b.term.as_ref().is_some_and(|t| term_uses(t).contains(p))
        });
        if used {
            let p2 = fresh(f);
            replace_all_uses(f, *p, p2);
            f.blocks[f.entry].insts.insert(0, Instr::Mov { dst: p2, src: *p });
        }
    }

    // calls and rets
    for b in 0..f.blocks.len() {
        let mut new_insts = Vec::with_capacity(f.blocks[b].insts.len());
        for inst in std::mem::take(&mut f.blocks[b].insts) {
            if let Instr::Call { func, args, rets } = inst {
                assert!(
                    args.len() <= ARG_REGS.len() && rets.len() <= RET_REGS.len(),
                    "call {func} exceeds ABI register count"
                );
                let mut pinned_args = Vec::with_capacity(args.len());
                for (j, a) in args.iter().enumerate() {
                    let alpha = fresh(f);
                    pinned.insert(alpha, ARG_REGS[j]);
                    new_insts.push(Instr::Mov { dst: alpha, src: *a });
                    pinned_args.push(alpha);
                }
                let mut pinned_rets = Vec::with_capacity(rets.len());
                let mut result_movs = Vec::with_capacity(rets.len());
                for (k, r) in rets.iter().enumerate() {
                    let rho = fresh(f);
                    pinned.insert(rho, RET_REGS[k]);
                    pinned_rets.push(rho);
                    result_movs.push(Instr::Mov { dst: *r, src: rho });
                }
                new_insts.push(Instr::Call {
                    func,
                    args: pinned_args,
                    rets: pinned_rets,
                });
                new_insts.extend(result_movs);
            } else if let Instr::CallPtr { addr, args, rets } = inst {
                assert!(
                    args.len() <= ARG_REGS.len() && rets.len() <= RET_REGS.len(),
                    "indirect call exceeds ABI register count"
                );
                // the target address goes to tmp (r15): it must survive the
                // argument parallel move, which only touches r2..r7
                let alpha_addr = fresh(f);
                pinned.insert(alpha_addr, REG_TMP);
                new_insts.push(Instr::Mov {
                    dst: alpha_addr,
                    src: addr,
                });
                let mut pinned_args = Vec::with_capacity(args.len());
                for (j, a) in args.iter().enumerate() {
                    let alpha = fresh(f);
                    pinned.insert(alpha, ARG_REGS[j]);
                    new_insts.push(Instr::Mov { dst: alpha, src: *a });
                    pinned_args.push(alpha);
                }
                let mut pinned_rets = Vec::with_capacity(rets.len());
                let mut result_movs = Vec::with_capacity(rets.len());
                for (k, r) in rets.iter().enumerate() {
                    let rho = fresh(f);
                    pinned.insert(rho, RET_REGS[k]);
                    pinned_rets.push(rho);
                    result_movs.push(Instr::Mov { dst: *r, src: rho });
                }
                new_insts.push(Instr::CallPtr {
                    addr: alpha_addr,
                    args: pinned_args,
                    rets: pinned_rets,
                });
                new_insts.extend(result_movs);
            } else {
                new_insts.push(inst);
            }
        }
        f.blocks[b].insts = new_insts;

        if let Some(Terminator::Ret { values }) = f.blocks[b].term.clone() {
            let mut pinned_values = Vec::with_capacity(values.len());
            for (k, v) in values.iter().enumerate() {
                let beta = fresh(f);
                pinned.insert(beta, RET_REGS[k]);
                f.blocks[b].insts.push(Instr::Mov { dst: beta, src: *v });
                pinned_values.push(beta);
            }
            f.blocks[b].term = Some(Terminator::Ret { values: pinned_values });
        }
    }

    AbiInfo { pinned }
}

// ---------------------------------------------------------------------------
// step 2: critical edge splitting
// ---------------------------------------------------------------------------

/// split every edge from a multi-successor pred to a multi-pred succ by a
/// trampoline block, so phi moves can always be placed at the end of a
/// single-successor pred.
fn split_critical_edges(f: &mut IrFunc) {
    for b in 0..f.blocks.len() {
        let (if_true, if_false) = match &f.blocks[b].term {
            Some(Terminator::Br { if_true, if_false, .. }) => (*if_true, *if_false),
            _ => continue,
        };
        let mut new_term = f.blocks[b].term.clone();
        for target in [if_true, if_false] {
            if f.blocks[target].preds.len() > 1 {
                let tramp = f.blocks.len();
                f.blocks.push(Block {
                    phis: vec![],
                    insts: vec![],
                    term: Some(Terminator::Jmp { target }),
                    preds: vec![b],
                });
                f.block_notes.push(None);
                // rewire the target's preds and phi args
                for p in &mut f.blocks[target].preds {
                    if *p == b {
                        *p = tramp;
                    }
                }
                for phi in &mut f.blocks[target].phis {
                    for (pred, _) in &mut phi.args {
                        if *pred == b {
                            *pred = tramp;
                        }
                    }
                }
                match &mut new_term {
                    Some(Terminator::Br { if_true: t, if_false: fl, .. }) => {
                        if *t == target {
                            *t = tramp;
                        }
                        if *fl == target {
                            *fl = tramp;
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        f.blocks[b].term = new_term;
    }
}

// ---------------------------------------------------------------------------
// step 3: intervals + linear scan
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Interval {
    start: u32,
    end: u32,
    crosses_call: bool,
}

/// position numbering: 2 per row (use at even, def at odd); phi defs at block
/// start + 1; phi args used at pred end. params are defined at position 0.
/// intervals are built from real liveness (backward dataflow), so values used
/// in a loop header stay live across the whole loop body.
fn compute_intervals(f: &IrFunc) -> Vec<Interval> {
    let n = f.vreg_count as usize;
    let rpo = f.rpo();

    // ----- position numbering + raw def/use positions -----
    let mut start = vec![u32::MAX; n];
    let mut end = vec![0u32; n];
    let mut calls: Vec<(u32, u32)> = vec![]; // (use pos, def pos)
    let mut block_start = vec![0u32; f.blocks.len()];
    let mut block_end = vec![0u32; f.blocks.len()];

    let def = |start: &mut Vec<u32>, end: &mut Vec<u32>, v: VReg, pos: u32| {
        start[v as usize] = start[v as usize].min(pos);
        end[v as usize] = end[v as usize].max(pos);
    };
    let mut pos = 0u32;
    for &p in &f.params {
        def(&mut start, &mut end, p, 0);
    }
    for &b in &rpo {
        block_start[b] = pos;
        let block = &f.blocks[b];
        for phi in &block.phis {
            def(&mut start, &mut end, phi.dst, pos + 1);
        }
        for inst in &block.insts {
            pos += 2;
            for u in inst_uses(inst) {
                end[u as usize] = end[u as usize].max(pos);
            }
            if let Instr::Call { .. } | Instr::CallPtr { .. } = inst {
                calls.push((pos, pos + 1));
            }
            for d in inst_defs(inst) {
                def(&mut start, &mut end, d, pos + 1);
            }
        }
        pos += 2;
        if let Some(term) = &block.term {
            for u in term_uses(term) {
                end[u as usize] = end[u as usize].max(pos);
            }
        }
        block_end[b] = pos;
    }
    // phi args are used at the end of their pred block
    for b in 0..f.blocks.len() {
        for phi in &f.blocks[b].phis {
            for (p, v) in &phi.args {
                end[*v as usize] = end[*v as usize].max(block_end[*p]);
            }
        }
    }

    // ----- per-block use/def sets -----
    let mut block_use: Vec<HashSet<VReg>> = vec![HashSet::new(); f.blocks.len()];
    let mut block_def: Vec<HashSet<VReg>> = vec![HashSet::new(); f.blocks.len()];
    let mut phi_defs: Vec<HashSet<VReg>> = vec![HashSet::new(); f.blocks.len()];
    for &b in &rpo {
        let block = &f.blocks[b];
        let mut use_b = HashSet::new();
        let mut def_b = HashSet::new();
        for phi in &block.phis {
            def_b.insert(phi.dst);
            phi_defs[b].insert(phi.dst);
        }
        let mut touch = |v: VReg, is_def: bool| {
            if is_def {
                def_b.insert(v);
            } else if !def_b.contains(&v) {
                use_b.insert(v);
            }
        };
        for inst in &block.insts {
            for u in inst_uses(inst) {
                touch(u, false);
            }
            for d in inst_defs(inst) {
                touch(d, true);
            }
        }
        if let Some(term) = &block.term {
            for u in term_uses(term) {
                touch(u, false);
            }
        }
        block_use[b] = use_b;
        block_def[b] = def_b;
    }

    // ----- backward liveness (phi args are live-out of their pred) -----
    let mut live_in: Vec<HashSet<VReg>> = vec![HashSet::new(); f.blocks.len()];
    let mut live_out: Vec<HashSet<VReg>> = vec![HashSet::new(); f.blocks.len()];
    loop {
        let mut changed = false;
        for &b in rpo.iter().rev() {
            let mut out: HashSet<VReg> = HashSet::new();
            for s in f.successors(b) {
                for &v in &live_in[s] {
                    if !phi_defs[s].contains(&v) {
                        out.insert(v);
                    }
                }
                for phi in &f.blocks[s].phis {
                    for (p, v) in &phi.args {
                        if *p == b {
                            out.insert(*v);
                        }
                    }
                }
            }
            let mut in_b = block_use[b].clone();
            for &v in &out {
                if !block_def[b].contains(&v) {
                    in_b.insert(v);
                }
            }
            if in_b != live_in[b] || out != live_out[b] {
                live_in[b] = in_b;
                live_out[b] = out;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // ----- intervals: raw def/use positions extended by live ranges -----
    (0..n as VReg)
        .map(|v| {
            let (mut s, mut e) = (start[v as usize], end[v as usize]);
            for &b in &rpo {
                if live_in[b].contains(&v) {
                    s = s.min(block_start[b]);
                }
                if live_out[b].contains(&v) {
                    e = e.max(block_end[b]);
                }
            }
            // no def: dead leftover of a spill rewrite (def was renamed); a
            // use without def is a genuine bug
            assert!(s != u32::MAX || e == 0, "vreg v{v} used but never defined");
            let crosses_call = s != u32::MAX && calls.iter().any(|&(cu, cd)| s < cu && e > cd);
            Interval {
                start: s,
                end: e,
                crosses_call,
            }
        })
        .collect()
}

/// coalescing hints: for `mov dst, src` the src's interval ends at the mov,
/// so giving src the dst's register eliminates the move. same for phi args.
/// (hint only — the allocator's usual checks still apply.)
fn compute_affinity(f: &IrFunc) -> HashMap<VReg, VReg> {
    let mut aff: HashMap<VReg, VReg> = HashMap::new();
    for b in &f.blocks {
        for inst in &b.insts {
            if let Instr::Mov { dst, src } = inst {
                aff.entry(*src).or_insert(*dst);
            }
        }
        for phi in &b.phis {
            for &(_, v) in &phi.args {
                aff.entry(v).or_insert(phi.dst);
            }
        }
    }
    aff
}

struct ScanResult {
    reg: HashMap<VReg, u8>,
    spilled: Vec<VReg>,
}

fn linear_scan(intervals: &[Interval], abi: &AbiInfo, affinity: &HashMap<VReg, VReg>) -> ScanResult {
    // fixed ranges per register (from pinned vregs), known upfront
    let mut fixed_ranges: HashMap<u8, Vec<(u32, u32)>> = HashMap::new();
    for (&v, &r) in &abi.pinned {
        fixed_ranges
            .entry(r)
            .or_default()
            .push((intervals[v as usize].start, intervals[v as usize].end));
    }
    for (r, ranges) in &fixed_ranges {
        let mut sorted = ranges.clone();
        sorted.sort();
        for w in sorted.windows(2) {
            assert!(
                w[1].0 > w[0].1,
                "pinned intervals on r{r} overlap: {w:?}"
            );
        }
    }
    let overlaps = |a: (u32, u32), b: (u32, u32)| !(a.1 < b.0 || b.1 < a.0);

    let mut order: Vec<VReg> = (0..intervals.len() as VReg)
        .filter(|&v| intervals[v as usize].start != u32::MAX) // skip dead vregs
        .collect();
    order.sort_by_key(|&v| intervals[v as usize].start);

    let mut reg: HashMap<VReg, u8> = HashMap::new();
    let mut spilled = vec![];
    // (end, vreg, reg); kept sorted by end
    let mut active: Vec<(u32, VReg, u8)> = vec![];

    for &v in &order {
        let iv = &intervals[v as usize];
        // expire intervals that no longer overlap
        active.retain(|&(e, av, _)| {
            if e < iv.start {
                debug_assert!(reg.contains_key(&av) || spilled.contains(&av));
                false
            } else {
                true
            }
        });

        if let Some(&r) = abi.pinned.get(&v) {
            reg.insert(v, r);
            active.push((iv.end, v, r));
            active.sort_by_key(|&(e, _, _)| e);
            continue;
        }

        let prefs: &[u8] = if iv.crosses_call {
            &CALLEE_SAVED
        } else {
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
        };
        let free = |r: u8| {
            !active.iter().any(|&(_, _, ar)| ar == r)
                && !fixed_ranges
                    .get(&r)
                    .is_some_and(|ranges| ranges.iter().any(|&fr| overlaps(fr, (iv.start, iv.end))))
        };
        // coalescing hint: prefer the affinity target's register
        let preferred = affinity.get(&v).and_then(|&a| {
            if let Some(&r) = abi.pinned.get(&a) {
                Some(r)
            } else {
                reg.get(&a).copied()
            }
        });
        let mut chosen = preferred.filter(|&r| {
            prefs.contains(&r) && free(r)
        });
        if chosen.is_none() {
            for &r in prefs {
                if free(r) {
                    chosen = Some(r);
                    break;
                }
            }
        }

        if let Some(r) = chosen {
            reg.insert(v, r);
            active.push((iv.end, v, r));
            active.sort_by_key(|&(e, _, _)| e);
        } else {
            // spill the active (non-fixed) interval with the latest end if it
            // outlives v; otherwise spill v itself
            let victim = active
                .iter()
                .enumerate()
                .filter(|(_, (_, av, _))| !abi.pinned.contains_key(av))
                .max_by_key(|(_, &(e, _, _))| e)
                .filter(|(_, &(e, _, _))| e > iv.end)
                .map(|(i, _)| i);
            if let Some(i) = victim {
                let (_, av, ar) = active.remove(i);
                reg.remove(&av);
                spilled.push(av);
                reg.insert(v, ar);
                active.push((iv.end, v, ar));
                active.sort_by_key(|&(e, _, _)| e);
            } else {
                spilled.push(v);
            }
        }
    }

    ScanResult { reg, spilled }
}

// ---------------------------------------------------------------------------
// step 4: spill rewriting (returns number of frame slots used)
// ---------------------------------------------------------------------------

/// rewrite spilled vregs into explicit LoadSp/StoreSp. each spilled vreg gets
/// a fresh frame slot from the persistent counter `next_slot` (monotonic
/// across fixpoint iterations, so slots assigned in earlier iterations are
/// never clobbered). TODO(M5): pack slots of non-overlapping spills.
fn rewrite_spills(f: &mut IrFunc, spilled: &[VReg], next_slot: &mut u8) {
    let mut slot_of: HashMap<VReg, u8> = HashMap::new();
    for &v in spilled {
        slot_of.insert(v, *next_slot);
        *next_slot += 1;
    }

    let spilled: HashSet<VReg> = spilled.iter().copied().collect();
    let fresh = |f: &mut IrFunc| {
        let v = f.vreg_count;
        f.vreg_count += 1;
        v
    };

    // loads/stores to append at the end of specific pred blocks (phi args);
    // collected in the first pass and applied in a second pass, because a
    // pred may be visited after the block holding the phi
    let mut edge_appends: HashMap<BlockId, Vec<Instr>> = HashMap::new();

    for b in 0..f.blocks.len() {
        // phis
        let phis = std::mem::take(&mut f.blocks[b].phis);
        let mut kept = vec![];
        for mut phi in phis {
            if spilled.contains(&phi.dst) {
                // dissolve into per-edge stores
                for (p, v) in phi.args {
                    let app = edge_appends.entry(p).or_default();
                    let src = if spilled.contains(&v) {
                        let t = fresh(f);
                        app.push(Instr::LoadSp {
                            dst: t,
                            slot: slot_of[&v],
                        });
                        t
                    } else {
                        v
                    };
                    app.push(Instr::StoreSp {
                        slot: slot_of[&phi.dst],
                        src,
                    });
                }
            } else {
                for (p, v) in &mut phi.args {
                    if spilled.contains(v) {
                        let t = fresh(f);
                        edge_appends.entry(*p).or_default().push(Instr::LoadSp {
                            dst: t,
                            slot: slot_of[v],
                        });
                        *v = t;
                    }
                }
                kept.push(phi);
            }
        }
        f.blocks[b].phis = kept;
    }

    for b in 0..f.blocks.len() {
        // instructions
        let mut new_insts = Vec::with_capacity(f.blocks[b].insts.len());
        for mut inst in std::mem::take(&mut f.blocks[b].insts) {
            // uses first
            match &mut inst {
                Instr::Bin { lhs, rhs, .. } => {
                    reload(lhs, &slot_of, &spilled, f, &mut new_insts);
                    reload(rhs, &slot_of, &spilled, f, &mut new_insts);
                }
                Instr::Un { src, .. } | Instr::Shift { src, .. } | Instr::Mov { src, .. } => {
                    reload(src, &slot_of, &spilled, f, &mut new_insts)
                }
                Instr::LoadImm { .. } | Instr::DevRecv { .. } | Instr::LoadSp { .. } => {}
                Instr::LoadMem { base, .. } => reload(base, &slot_of, &spilled, f, &mut new_insts),
                Instr::StoreMem { base, src, .. } => {
                    reload(base, &slot_of, &spilled, f, &mut new_insts);
                    reload(src, &slot_of, &spilled, f, &mut new_insts);
                }
                Instr::Call { args, .. } => {
                    for a in args {
                        reload(a, &slot_of, &spilled, f, &mut new_insts);
                    }
                }
                Instr::LoadFuncAddr { .. } => {}
                Instr::CallPtr { addr, args, .. } => {
                    reload(addr, &slot_of, &spilled, f, &mut new_insts);
                    for a in args {
                        reload(a, &slot_of, &spilled, f, &mut new_insts);
                    }
                }
                Instr::DevSend { src, .. } | Instr::StoreSp { src, .. } => {
                    reload(src, &slot_of, &spilled, f, &mut new_insts)
                }
            }
            new_insts.push(inst.clone());
            // defs after
            for d in inst_defs(&inst) {
                if spilled.contains(&d) {
                    let t = fresh(f);
                    // replace the just-pushed inst's def with t, then store
                    let last = new_insts.last_mut().unwrap();
                    for dd in defs_mut(last) {
                        if *dd == d {
                            *dd = t;
                        }
                    }
                    new_insts.push(Instr::StoreSp {
                        slot: slot_of[&d],
                        src: t,
                    });
                }
            }
        }
        f.blocks[b].insts = new_insts;

        // terminator uses (take the terminator out to avoid double-borrow of f)
        let mut taken_term = f.blocks[b].term.take();
        if let Some(term) = &mut taken_term {
            let mut pre = vec![];
            match term {
                Terminator::Jmp { .. } => {}
                Terminator::Br { cmp, .. } => {
                    reload(&mut cmp.lhs, &slot_of, &spilled, f, &mut pre);
                    if let CmpRhs::Reg(r) = &mut cmp.rhs {
                        reload(r, &slot_of, &spilled, f, &mut pre);
                    }
                }
                Terminator::Ret { values } => {
                    for v in values {
                        reload(v, &slot_of, &spilled, f, &mut pre);
                    }
                }
                Terminator::Halt { signal } => reload(signal, &slot_of, &spilled, f, &mut pre),
            }
            f.blocks[b].insts.extend(pre);
        }
        f.blocks[b].term = taken_term;
    }

    // edge appends targeted at pred blocks (from phis of successors)
    for (b, app) in edge_appends {
        f.blocks[b].insts.extend(app);
    }
}

fn reload(
    v: &mut VReg,
    slot_of: &HashMap<VReg, u8>,
    spilled: &HashSet<VReg>,
    f: &mut IrFunc,
    out: &mut Vec<Instr>,
) {
    if spilled.contains(v) {
        let t = {
            let t = f.vreg_count;
            f.vreg_count += 1;
            t
        };
        out.push(Instr::LoadSp {
            dst: t,
            slot: slot_of[v],
        });
        *v = t;
    }
}

fn defs_mut(inst: &mut Instr) -> Vec<&mut VReg> {
    match inst {
        Instr::Bin { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Shift { dst, .. }
        | Instr::Mov { dst, .. }
        | Instr::LoadImm { dst, .. }
        | Instr::LoadMem { dst, .. }
        | Instr::DevRecv { dst, .. }
        | Instr::LoadFuncAddr { dst, .. }
        | Instr::LoadSp { dst, .. } => vec![dst],
        Instr::StoreMem { .. } | Instr::DevSend { .. } | Instr::StoreSp { .. } => vec![],
        Instr::Call { rets, .. } | Instr::CallPtr { rets, .. } => rets.iter_mut().collect(),
    }
}
