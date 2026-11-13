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
use crate::FuncName;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Register roles used by ABI shim insertion and linear scan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegisterConvention {
    pub return_registers: &'static [u8],
    pub argument_registers: &'static [u8],
    pub allocatable_registers: &'static [u8],
    pub caller_saved: &'static [u8],
    pub callee_saved: &'static [u8],
    pub link_register: u8,
    pub stack_register: u8,
    pub temporary_register: u8,
    pub maximum_frame_words: usize,
    /// optional FPU register file; targets without an FPU leave this `None`
    /// and assert when an Fpu-class vreg reaches allocation
    pub fpu: Option<FpuRegisterConvention>,
}

/// FPU register roles (all FPU registers are caller-saved; there is no
/// callee-saved FPU half, so intervals crossing a call always spill)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FpuRegisterConvention {
    pub return_registers: &'static [u8],
    pub argument_registers: &'static [u8],
    pub allocatable_registers: &'static [u8],
    /// excluded from allocation; used to break parallel-move cycles
    pub scratch_register: u8,
}

/// result of register allocation for one function
pub struct Allocation {
    /// final register for every vreg (spilled vregs have been rewritten to
    /// explicit LoadSp/StoreSp, so every remaining vreg has a register)
    pub reg: HashMap<VReg, u8>,
    /// callee-save regs actually used; saved/restored at frame slots 0..k
    pub callee_saved: Vec<u8>,
    /// frame-local slots assigned by the frontend (address-taken locals and
    /// local arrays); they follow the callee-save area
    pub local_slots: u8,
    /// number of spill frame slots used (after the local area)
    pub spill_slots: u8,
    /// number of FPU spill slots; each is 4 words and 4-aligned, in an area
    /// after the GPR spill area (preceded by up to 3 words of alignment
    /// padding)
    pub fpu_spill_slots: u8,
}
impl Allocation {
    pub fn frame_size(&self) -> usize {
        self.callee_saved.len()
            + self.local_slots as usize
            + self.spill_slots as usize
            + self.fpu_area_words()
    }
    /// words reserved for the FPU spill area (4 per slot plus alignment pad)
    pub fn fpu_area_words(&self) -> usize {
        if self.fpu_spill_slots == 0 {
            0
        } else {
            4 * self.fpu_spill_slots as usize + 3
        }
    }
    /// word offset from sp where the FPU spill alignment window starts
    pub fn fpu_area_offset(&self) -> u8 {
        self.callee_saved.len() as u8 + self.local_slots + self.spill_slots
    }
    /// frame slot index of a frontend local slot
    pub fn local_frame_slot(&self, slot: u8) -> u8 {
        self.callee_saved.len() as u8 + slot
    }
    /// frame slot index of a spill slot (they follow the local area)
    pub fn spill_frame_slot(&self, slot: u8) -> u8 {
        self.callee_saved.len() as u8 + self.local_slots + slot
    }
}

/// Narrow allocation failures needed by speculative optimizations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AllocationError {
    FrameCapacity {
        function: FuncName,
        words: usize,
        maximum: usize,
    },
    SpillCapacity {
        function: FuncName,
        class: RegClass,
    },
    NonConvergence {
        function: FuncName,
    },
}
impl AllocationError {
    pub fn is_resource_limit(&self) -> bool {
        !matches!(self, Self::NonConvergence { .. })
    }
}
impl std::fmt::Display for AllocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FrameCapacity {
                function,
                words,
                maximum,
            } => write!(
                f,
                "function {function} needs {words} frame slots, max {maximum}"
            ),
            Self::SpillCapacity { function, class } => write!(
                f,
                "function {function} exhausts {class:?} spill slot capacity"
            ),
            Self::NonConvergence { function } => {
                write!(f, "register allocation for {function} did not converge")
            }
        }
    }
}
impl std::error::Error for AllocationError {}

pub fn allocate_with_convention(
    func: &IrFunc,
    coalesce: bool,
    convention: RegisterConvention,
) -> (IrFunc, Allocation) {
    try_allocate_with_convention(func, coalesce, convention)
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Fallible capacity checking; malformed IR and ABI invariants are not recovered.
pub fn try_allocate_with_convention(
    func: &IrFunc,
    coalesce: bool,
    convention: RegisterConvention,
) -> Result<(IrFunc, Allocation), AllocationError> {
    validate_convention(convention);
    if convention.fpu.is_none() {
        assert!(
            func.vreg_class.iter().all(|&c| c == RegClass::Gpr),
            "function {} has FPU vregs but the target has no FPU register file",
            func.name
        );
    }
    let mut f = func.clone();
    let abi = insert_abi_shims(&mut f, convention);
    split_critical_edges(&mut f);

    let mut spill_slots = 0u8;
    let mut fpu_spill_slots = 0u8;
    for _ in 0..8 {
        let intervals = compute_intervals(&f);
        let affinity = if coalesce {
            compute_affinity(&f)
        } else {
            HashMap::new()
        };
        let scan = linear_scan(&f, &intervals, &abi, &affinity, convention);
        if scan.spilled.is_empty() {
            let mut callee_saved: Vec<u8> = scan
                .reg
                .values()
                .filter(|r| convention.callee_saved.contains(r))
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
                callee_saved.push(convention.link_register);
            }
            let alloc = Allocation {
                reg: scan.reg,
                callee_saved,
                local_slots: f.local_slots,
                spill_slots,
                fpu_spill_slots,
            };
            collapse_redundant_phi_trampolines(&mut f, &alloc.reg);
            forward_adjacent_stack_reloads(&mut f);
            deduplicate_allocated_immediates(&mut f, &alloc.reg);
            if alloc.frame_size() > convention.maximum_frame_words {
                return Err(AllocationError::FrameCapacity {
                    function: f.name,
                    words: alloc.frame_size(),
                    maximum: convention.maximum_frame_words,
                });
            }
            return Ok((f, alloc));
        }
        rewrite_spills(
            &mut f,
            &scan.spilled,
            &mut spill_slots,
            &mut fpu_spill_slots,
        )?;
    }
    Err(AllocationError::NonConvergence { function: f.name })
}

// Spill rewriting runs after ordinary optimization. For adjacent exact stack
// stores/loads, the stored value is still in its allocated register. A Mov
// preserves the load's definition and lowers to nothing when registers match.
fn forward_adjacent_stack_reloads(f: &mut IrFunc) {
    for block in &mut f.blocks {
        for i in 1..block.insts.len() {
            let replacement = match (&block.insts[i - 1], &block.insts[i]) {
                (Instr::StoreSp { src, slot: a }, Instr::LoadSp { dst, slot: b })
                | (Instr::StoreLocal { src, slot: a }, Instr::LoadLocal { dst, slot: b })
                    if a == b =>
                {
                    Some(Instr::Mov {
                        dst: *dst,
                        src: *src,
                    })
                }
                _ => None,
            };
            if let Some(inst) = replacement {
                block.insts[i] = inst;
            }
        }
    }
}

// Rebuilt constants can land in the same register repeatedly. Remember only
// physical register contents within a block, invalidating every GPR definition
// and every call. This does not extend any allocated live interval.
fn deduplicate_allocated_immediates(f: &mut IrFunc, registers: &HashMap<VReg, u8>) {
    let layout = f.rpo();
    let classes = &f.vreg_class;
    for block_id in layout {
        let block = &mut f.blocks[block_id];
        let mut known: HashMap<u8, (VReg, u16)> = HashMap::new();
        for inst in &mut block.insts {
            if matches!(
                inst,
                Instr::Call { .. } | Instr::CallPtr { .. } | Instr::StoreStatic { .. }
            ) {
                known.clear();
            }
            if let Instr::LoadImm { dst, value } = *inst {
                if let Some(&(src, old)) = known.get(&registers[&dst]) {
                    if old == value {
                        *inst = Instr::Mov { dst, src };
                        continue;
                    }
                }
            }
            if let Instr::Mov { dst, src } = *inst {
                if registers[&dst] == registers[&src] {
                    continue;
                }
            }
            for def in inst_defs(inst) {
                if classes[def as usize] == RegClass::Gpr {
                    known.remove(&registers[&def]);
                }
            }
            if let Instr::LoadImm { dst, value } = *inst {
                known.insert(registers[&dst], (dst, value));
            }
        }
    }
}

#[cfg(test)]
mod phi_cleanup_tests {
    use super::*;

    fn function() -> IrFunc {
        let (mut builder, _) = crate::FuncBuilder::new("phi_cleanup", 1, 1);
        builder.ret(&[0]);
        builder.finish()
    }

    #[test]
    fn dead_phi_trampolines_do_not_require_register_assignments() {
        let mut f = function();
        let dead = f.fresh_vreg(RegClass::Gpr);
        f.blocks.extend([
            Block {
                term: Some(Terminator::Jmp { target: 2 }),
                ..Block::default()
            },
            Block {
                preds: vec![1],
                term: Some(Terminator::Jmp { target: 3 }),
                ..Block::default()
            },
            Block {
                preds: vec![2],
                phis: vec![Phi {
                    dst: dead,
                    args: vec![(2, 0)],
                }],
                term: Some(Terminator::Ret { values: vec![dead] }),
                ..Block::default()
            },
        ]);
        let before = f.blocks.clone();
        collapse_redundant_phi_trampolines(&mut f, &HashMap::from([(0, 2)]));
        assert_eq!(f.blocks, before);
    }

    #[test]
    fn live_phi_trampolines_still_collapse_when_the_register_is_shared() {
        let mut f = function();
        let value = f.fresh_vreg(RegClass::Gpr);
        f.blocks[0].term = Some(Terminator::Jmp { target: 1 });
        f.blocks.extend([
            Block {
                preds: vec![0],
                term: Some(Terminator::Jmp { target: 2 }),
                ..Block::default()
            },
            Block {
                preds: vec![1],
                phis: vec![Phi {
                    dst: value,
                    args: vec![(1, 0)],
                }],
                term: Some(Terminator::Ret {
                    values: vec![value],
                }),
                ..Block::default()
            },
        ]);
        collapse_redundant_phi_trampolines(&mut f, &HashMap::from([(0, 2), (value, 2)]));
        assert_eq!(f.blocks[0].term, Some(Terminator::Jmp { target: 2 }));
        assert_eq!(f.blocks[2].preds, [0]);
        assert_eq!(f.blocks[2].phis[0].args, [(0, 0)]);
    }
}

fn validate_convention(convention: RegisterConvention) {
    let mut roles = HashSet::new();
    for &register in convention.allocatable_registers {
        assert!(
            register < 16,
            "allocatable register r{register} is outside GPR range"
        );
        assert!(
            roles.insert(register),
            "duplicate allocatable register r{register}"
        );
    }
    for (name, registers) in [
        ("return", convention.return_registers),
        ("argument", convention.argument_registers),
        ("caller-saved", convention.caller_saved),
        ("callee-saved", convention.callee_saved),
    ] {
        for &register in registers {
            assert!(
                convention.allocatable_registers.contains(&register),
                "{name} register r{register} is not allocatable"
            );
        }
    }
    for &register in convention.caller_saved {
        assert!(
            !convention.callee_saved.contains(&register),
            "r{register} is both caller- and callee-saved"
        );
    }
    let reserved = [
        convention.link_register,
        convention.stack_register,
        convention.temporary_register,
    ];
    assert!(reserved.iter().all(|register| *register < 16));
    assert_eq!(reserved.iter().copied().collect::<HashSet<_>>().len(), 3);
    for register in reserved {
        assert!(
            !convention.allocatable_registers.contains(&register),
            "reserved register r{register} is also allocatable"
        );
    }
    if let Some(fpu) = convention.fpu {
        // ABI registers (returns f0-f3, arguments f4-f27) are pinned only, not
        // generally allocatable; every FPU register is caller-saved. The FPU v2
        // file is F0..F63.
        let mut roles = HashSet::new();
        for (name, registers) in [
            ("fpu return", fpu.return_registers),
            ("fpu argument", fpu.argument_registers),
            ("fpu allocatable", fpu.allocatable_registers),
        ] {
            for &register in registers {
                assert!(register < 64, "{name} register f{register} is out of range");
                assert!(
                    roles.insert(register),
                    "duplicate {name} register f{register}"
                );
            }
        }
        assert!(
            roles.insert(fpu.scratch_register),
            "fpu scratch register f{} collides with an ABI role",
            fpu.scratch_register
        );
    }
}

/// After allocation, a split critical edge is unnecessary when every phi
/// value already occupies its destination register. Remove those empty
/// trampolines so codegen can use the real successor as a branch target and
/// fall through normally.
fn collapse_redundant_phi_trampolines(f: &mut IrFunc, registers: &HashMap<VReg, u8>) {
    // Constant folding can leave dead phi edges. Their values have no assigned
    // registers; only reachable edges participate in allocation and cleanup.
    for trampoline in f.rpo() {
        let [source] = f.blocks[trampoline].preds.as_slice() else {
            continue;
        };
        let source = *source;
        let Some(Terminator::Jmp { target }) = f.blocks[trampoline].term else {
            continue;
        };
        if !f.blocks[trampoline].phis.is_empty() || !f.blocks[trampoline].insts.is_empty() {
            continue;
        }
        let redundant = f.blocks[target].phis.iter().all(|phi| {
            phi.args
                .iter()
                .find(|(pred, _)| *pred == trampoline)
                .is_some_and(|(_, value)| registers[value] == registers[&phi.dst])
        });
        if !redundant || f.blocks[target].phis.is_empty() {
            continue;
        }
        match &mut f.blocks[source].term {
            Some(Terminator::Jmp { target: edge }) => {
                if *edge == trampoline {
                    *edge = target;
                }
            }
            Some(Terminator::Br {
                if_true, if_false, ..
            }) => {
                if *if_true == trampoline {
                    *if_true = target;
                }
                if *if_false == trampoline {
                    *if_false = target;
                }
            }
            _ => continue,
        }
        for pred in &mut f.blocks[target].preds {
            if *pred == trampoline {
                *pred = source;
            }
        }
        for phi in &mut f.blocks[target].phis {
            for (pred, _) in &mut phi.args {
                if *pred == trampoline {
                    *pred = source;
                }
            }
        }
        f.blocks[trampoline].preds.clear();
    }
}

// ---------------------------------------------------------------------------
// instruction use/def helpers
// ---------------------------------------------------------------------------

pub(crate) fn inst_uses(inst: &Instr) -> Vec<VReg> {
    let mut uses = vec![];
    inst.for_each_use(&mut |v| uses.push(v));
    uses
}
pub(crate) fn inst_defs(inst: &Instr) -> Vec<VReg> {
    match inst {
        Instr::Bin { dst, .. }
        | Instr::Mul { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Shift { dst, .. }
        | Instr::Mov { dst, .. }
        | Instr::LoadImm { dst, .. }
        | Instr::LoadMem { dst, .. }
        | Instr::DevRecv { dst, .. }
        | Instr::LoadFuncAddr { dst, .. }
        | Instr::LoadLocal { dst, .. }
        | Instr::AddrOfLocal { dst, .. }
        | Instr::Mfsr { dst, .. }
        | Instr::Bool { dst, .. }
        | Instr::CMov { dst, .. }
        | Instr::LoadSp { dst, .. }
        | Instr::FpuBin { dst, .. }
        | Instr::FpuUn { dst, .. }
        | Instr::FpuSpecial { dst, .. }
        | Instr::FpuMov { dst, .. }
        | Instr::FpuFromInt { dst, .. }
        | Instr::FpuFromLo { dst, .. }
        | Instr::FpuFromHi { dst, .. }
        | Instr::FpuVecBin { dst, .. }
        | Instr::FpuVecMulS { dst, .. }
        | Instr::FpuVecUn { dst, .. }
        | Instr::FpuVecMove { dst, .. }
        | Instr::FpuVecConstruct { dst, .. }
        | Instr::FpuVecLane { dst, .. }
        | Instr::FpuDotStore { dst, .. }
        | Instr::FpuLoad { dst, .. }
        | Instr::FpuVecLoad { dst, .. } => vec![*dst],
        Instr::FpuToInt { dst_gpr, .. }
        | Instr::FpuToLo { dst_gpr, .. }
        | Instr::FpuToHi { dst_gpr, .. }
        | Instr::AddrOfFpuSpill { dst: dst_gpr, .. } => vec![*dst_gpr],
        Instr::StoreMem { .. }
        | Instr::StoreStatic { .. }
        | Instr::DevSend { .. }
        | Instr::DcacheInvalidateAll
        | Instr::DcacheLineCopy { .. }
        | Instr::DcacheCleanLine { .. }
        | Instr::DcacheWait
        | Instr::MtsrDseg { .. }
        | Instr::Jseg { .. }
        | Instr::Signal { .. }
        | Instr::StoreSp { .. }
        | Instr::StoreLocal { .. }
        | Instr::FpuStore { .. }
        | Instr::FpuVecStore { .. } => {
            vec![]
        }
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
        Terminator::IcacheInvalidateDelayedAndJump { cseg, target } => vec![*cseg, *target],
    }
}

fn replace_all_uses(f: &mut IrFunc, from: VReg, to: VReg) {
    let mut subst = |v: &mut VReg| {
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
                Terminator::Ret { values } => values.iter_mut().for_each(&mut subst),
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
// step 1: ABI shims
// ---------------------------------------------------------------------------

struct AbiInfo {
    /// vregs forced into a fixed register (short intervals by construction)
    pinned: HashMap<VReg, u8>,
}

fn insert_abi_shims(f: &mut IrFunc, convention: RegisterConvention) -> AbiInfo {
    let mut pinned = HashMap::new();
    let fresh = |f: &mut IrFunc, v: VReg| {
        // the shim copy inherits the class and lane count of the value it carries
        f.fresh_vreg_lanes(f.class_of(v), f.lanes(v))
    };

    let count_class = |class: RegClass, values: &[VReg]| {
        values.iter().filter(|&&v| f.class_of(v) == class).count()
    };
    let lane_sum = |class: RegClass, values: &[VReg]| -> usize {
        values
            .iter()
            .filter(|&&v| f.class_of(v) == class)
            .map(|&v| usize::from(f.lanes(v)))
            .sum()
    };
    let gpr_params = count_class(RegClass::Gpr, &f.params);
    let fpu_lanes = lane_sum(RegClass::Fpu, &f.params);
    assert!(
        gpr_params <= convention.argument_registers.len(),
        "function {} has {} GPR params, max {}",
        f.name,
        gpr_params,
        convention.argument_registers.len()
    );
    if fpu_lanes > 0 {
        let fpu = convention
            .fpu
            .expect("FPU parameters need an FPU register convention");
        assert!(
            fpu_lanes <= fpu.argument_registers.len(),
            "function {} has {} FPU argument lanes, max {}",
            f.name,
            fpu_lanes,
            fpu.argument_registers.len()
        );
    }
    assert!(
        f.n_rets <= convention.return_registers.len(),
        "function {} has too many return values",
        f.name
    );

    // params: pin to their per-class ABI registers; copy to a fresh vreg for
    // all real uses. FPU values advance the argument offset by their lane
    // count, so vector parameters pack compactly from F4.
    let params = f.params.clone();
    let mut next_param = [0usize; 2];
    for p in params {
        let class = f.class_of(p);
        let lanes = usize::from(f.lanes(p));
        let index = next_param[class as usize];
        next_param[class as usize] += lanes;
        pinned.insert(p, abi_register(convention, class, index, AbiRole::Argument));
        let used = f.blocks.iter().any(|b| {
            b.phis
                .iter()
                .any(|phi| phi.args.iter().any(|(_, v)| *v == p))
                || b.insts.iter().any(|inst| inst_uses(inst).contains(&p))
                || b.term.as_ref().is_some_and(|t| term_uses(t).contains(&p))
        });
        if used {
            let p2 = fresh(f, p);
            replace_all_uses(f, p, p2);
            f.blocks[f.entry]
                .insts
                .insert(0, shim_mov(p2, p, class, lanes as u8));
            f.blocks[f.entry].lines.insert(0, None);
        }
    }

    // calls and rets
    for b in 0..f.blocks.len() {
        let mut new_insts = Vec::with_capacity(f.blocks[b].insts.len());
        let mut new_lines = Vec::with_capacity(f.blocks[b].lines.len());
        let old_lines = std::mem::take(&mut f.blocks[b].lines);
        let mut old_lines = old_lines.into_iter();
        for inst in std::mem::take(&mut f.blocks[b].insts) {
            let line = old_lines.next().unwrap_or(None);
            if let Instr::Call { func, args, rets } = inst {
                assert!(
                    abi_lane_count(f, &args, RegClass::Gpr) <= convention.argument_registers.len()
                        && abi_lane_count(f, &args, RegClass::Fpu)
                            <= convention.fpu.map_or(0, |c| c.argument_registers.len())
                        && abi_lane_count(f, &rets, RegClass::Gpr)
                            <= convention.return_registers.len()
                        && abi_lane_count(f, &rets, RegClass::Fpu)
                            <= convention.fpu.map_or(0, |c| c.return_registers.len()),
                    "call {func} exceeds ABI register count"
                );
                let mut pinned_args = Vec::with_capacity(args.len());
                let mut next_arg = [0usize; 2];
                for a in &args {
                    let class = f.class_of(*a);
                    let alpha = fresh(f, *a);
                    pinned.insert(
                        alpha,
                        abi_register(
                            convention,
                            class,
                            next_arg[class as usize],
                            AbiRole::Argument,
                        ),
                    );
                    next_arg[class as usize] += usize::from(f.lanes(*a));
                    new_insts.push(shim_mov(alpha, *a, class, f.lanes(*a)));
                    new_lines.push(line);
                    pinned_args.push(alpha);
                }
                let mut pinned_rets = Vec::with_capacity(rets.len());
                let mut result_movs = Vec::with_capacity(rets.len());
                let mut next_ret = [0usize; 2];
                for r in &rets {
                    let class = f.class_of(*r);
                    let rho = fresh(f, *r);
                    pinned.insert(
                        rho,
                        abi_register(convention, class, next_ret[class as usize], AbiRole::Return),
                    );
                    next_ret[class as usize] += usize::from(f.lanes(*r));
                    pinned_rets.push(rho);
                    result_movs.push(shim_mov(*r, rho, class, f.lanes(*r)));
                }
                new_insts.push(Instr::Call {
                    func,
                    args: pinned_args,
                    rets: pinned_rets,
                });
                new_lines.push(line);
                let n_movs = result_movs.len();
                new_insts.extend(result_movs);
                new_lines.extend(std::iter::repeat_n(line, n_movs));
            } else if let Instr::CallPtr { addr, args, rets } = inst {
                assert!(
                    abi_lane_count(f, &args, RegClass::Gpr) <= convention.argument_registers.len()
                        && abi_lane_count(f, &args, RegClass::Fpu)
                            <= convention.fpu.map_or(0, |c| c.argument_registers.len())
                        && abi_lane_count(f, &rets, RegClass::Gpr)
                            <= convention.return_registers.len()
                        && abi_lane_count(f, &rets, RegClass::Fpu)
                            <= convention.fpu.map_or(0, |c| c.return_registers.len()),
                    "indirect call exceeds ABI register count"
                );
                // The target address goes to the backend's temporary register;
                // it must survive the argument parallel move.
                let alpha_addr = fresh(f, addr);
                pinned.insert(alpha_addr, convention.temporary_register);
                new_insts.push(Instr::Mov {
                    dst: alpha_addr,
                    src: addr,
                });
                new_lines.push(line);
                let mut pinned_args = Vec::with_capacity(args.len());
                let mut next_arg = [0usize; 2];
                for a in &args {
                    let class = f.class_of(*a);
                    let alpha = fresh(f, *a);
                    pinned.insert(
                        alpha,
                        abi_register(
                            convention,
                            class,
                            next_arg[class as usize],
                            AbiRole::Argument,
                        ),
                    );
                    next_arg[class as usize] += usize::from(f.lanes(*a));
                    new_insts.push(shim_mov(alpha, *a, class, f.lanes(*a)));
                    new_lines.push(line);
                    pinned_args.push(alpha);
                }
                let mut pinned_rets = Vec::with_capacity(rets.len());
                let mut result_movs = Vec::with_capacity(rets.len());
                let mut next_ret = [0usize; 2];
                for r in &rets {
                    let class = f.class_of(*r);
                    let rho = fresh(f, *r);
                    pinned.insert(
                        rho,
                        abi_register(convention, class, next_ret[class as usize], AbiRole::Return),
                    );
                    next_ret[class as usize] += usize::from(f.lanes(*r));
                    pinned_rets.push(rho);
                    result_movs.push(shim_mov(*r, rho, class, f.lanes(*r)));
                }
                new_insts.push(Instr::CallPtr {
                    addr: alpha_addr,
                    args: pinned_args,
                    rets: pinned_rets,
                });
                new_lines.push(line);
                let n_movs = result_movs.len();
                new_insts.extend(result_movs);
                new_lines.extend(std::iter::repeat_n(line, n_movs));
            } else {
                new_insts.push(inst);
                new_lines.push(line);
            }
        }
        debug_assert_eq!(new_insts.len(), new_lines.len());
        f.blocks[b].insts = new_insts;
        f.blocks[b].lines = new_lines;

        if let Some(Terminator::Ret { values }) = f.blocks[b].term.clone() {
            let line = f.blocks[b].term_line;
            let mut pinned_values = Vec::with_capacity(values.len());
            let mut next_ret = [0usize; 2];
            for v in &values {
                let class = f.class_of(*v);
                let lanes = f.lanes(*v);
                let beta = fresh(f, *v);
                pinned.insert(
                    beta,
                    abi_register(convention, class, next_ret[class as usize], AbiRole::Return),
                );
                next_ret[class as usize] += usize::from(lanes);
                f.blocks[b].insts.push(shim_mov(beta, *v, class, lanes));
                f.blocks[b].lines.push(line);
                pinned_values.push(beta);
            }
            f.blocks[b].term = Some(Terminator::Ret {
                values: pinned_values,
            });
        }
    }

    AbiInfo { pinned }
}

/// which ABI register list a pinned value comes from
#[derive(Clone, Copy)]
enum AbiRole {
    Argument,
    Return,
}

/// ABI register for the `index`-th value of `class` in `role`
fn abi_register(
    convention: RegisterConvention,
    class: RegClass,
    index: usize,
    role: AbiRole,
) -> u8 {
    match (class, role) {
        (RegClass::Gpr, AbiRole::Argument) => convention.argument_registers[index],
        (RegClass::Gpr, AbiRole::Return) => convention.return_registers[index],
        (RegClass::Fpu, AbiRole::Argument) => {
            convention.fpu.expect("FPU ABI").argument_registers[index]
        }
        (RegClass::Fpu, AbiRole::Return) => {
            convention.fpu.expect("FPU ABI").return_registers[index]
        }
    }
}

/// shim copy between same-class vregs; a multi-lane FPU value uses the
/// cycle-safe vector move so any source/destination overlap is handled
fn shim_mov(dst: VReg, src: VReg, class: RegClass, lanes: u8) -> Instr {
    match class {
        RegClass::Gpr => Instr::Mov { dst, src },
        RegClass::Fpu if lanes > 1 => Instr::FpuVecMove { dst, src },
        RegClass::Fpu => Instr::FpuMov { dst, src },
    }
}

/// total ABI registers (lanes) `values` of `class` occupy
fn abi_lane_count(f: &IrFunc, values: &[VReg], class: RegClass) -> usize {
    values
        .iter()
        .filter(|&&v| f.class_of(v) == class)
        .map(|&v| usize::from(f.lanes(v)))
        .sum()
}

// ---------------------------------------------------------------------------
// step 2: critical edge splitting
// ---------------------------------------------------------------------------

/// split every edge from a multi-successor pred to a multi-pred succ by a
/// trampoline block, so phi moves can always be placed at the end of a
/// single-successor pred.
fn split_critical_edges(f: &mut IrFunc) {
    for b in 0..f.blocks.len() {
        let source_line = f.blocks[b].term_line;
        let (if_true, if_false) = match &f.blocks[b].term {
            Some(Terminator::Br {
                if_true, if_false, ..
            }) => (*if_true, *if_false),
            _ => continue,
        };
        let mut new_term = f.blocks[b].term.clone();
        for target in [if_true, if_false] {
            // A critical edge only needs its own block when the successor has
            // phi moves to perform. Splitting every multi-predecessor target
            // creates empty jump-only blocks and prevents useful fallthrough.
            if f.blocks[target].preds.len() > 1 && !f.blocks[target].phis.is_empty() {
                let tramp = f.blocks.len();
                f.blocks.push(Block {
                    phis: vec![],
                    insts: vec![],
                    lines: vec![],
                    term: Some(Terminator::Jmp { target }),
                    term_line: source_line,
                    preds: vec![b],
                });
                f.block_notes.push(None);
                f.block_lines.push(None);
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
                    Some(Terminator::Br {
                        if_true: t,
                        if_false: fl,
                        ..
                    }) => {
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
            if let Instr::CMov { cmp, src, .. } = inst {
                // Lowering initializes dst from fallback before comparing.
                // Keep the compare operands and true arm live through dst's
                // definition, so that initialization cannot overwrite them.
                end[cmp.lhs as usize] = end[cmp.lhs as usize].max(pos + 1);
                if let CmpRhs::Reg(rhs) = cmp.rhs {
                    end[rhs as usize] = end[rhs as usize].max(pos + 1);
                }
                end[*src as usize] = end[*src as usize].max(pos + 1);
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
fn compute_affinity(f: &IrFunc) -> HashMap<VReg, Vec<VReg>> {
    let mut aff: HashMap<VReg, Vec<VReg>> = HashMap::new();
    let mut pair = |a: VReg, b: VReg| {
        if !aff.entry(a).or_default().contains(&b) {
            aff.entry(a).or_default().push(b);
        }
        if !aff.entry(b).or_default().contains(&a) {
            aff.entry(b).or_default().push(a);
        }
    };
    for b in &f.blocks {
        for inst in &b.insts {
            match inst {
                Instr::Mov { dst, src } | Instr::FpuMov { dst, src } => pair(*src, *dst),
                Instr::CMov { dst, fallback, .. } => pair(*fallback, *dst),
                // Destructive two-address forms: the backend copies the first
                // operand into the destination before the operation whenever
                // the allocator cannot place them together. Prefer coalescing
                // `dst` with whichever operand becomes the destination so that
                // copy disappears.
                Instr::Bin {
                    dst,
                    lhs,
                    rhs: IntOperand::Imm(_),
                    ..
                }
                | Instr::Mul {
                    dst,
                    lhs,
                    rhs: IntOperand::Imm(_),
                    ..
                }
                | Instr::Shift {
                    dst,
                    src: lhs,
                    amount: IntOperand::Imm(_),
                    ..
                } => pair(*lhs, *dst),
                // MUL is commutative: the backend swaps operands when `dst`
                // already aliases the source, so either side may coalesce.
                Instr::Mul {
                    dst,
                    lhs,
                    rhs: IntOperand::Reg(rhs),
                    ..
                } => {
                    pair(*lhs, *dst);
                    pair(*rhs, *dst);
                }
                // Register-count shifts copy the shifted value into `dst`
                // (never the amount register: aliasing that takes the scratch
                // path in the backend).
                Instr::Shift {
                    dst,
                    src,
                    amount: IntOperand::Reg(_),
                    ..
                } => pair(*src, *dst),
                // Boolean materialization copies the first comparison operand
                // into `dst`, except for the swapped conditions (`>`/`<=`),
                // where the register right-hand side is the first operand.
                Instr::Bool { dst, cmp } => {
                    let swapped = matches!(cmp.cond, CompareOp::Greater | CompareOp::LessEqual);
                    match (swapped, cmp.rhs) {
                        (true, CmpRhs::Reg(rhs)) => pair(rhs, *dst),
                        (true, CmpRhs::Imm(_)) => {}
                        (false, _) => pair(cmp.lhs, *dst),
                    }
                }
                _ => {}
            }
        }
        for phi in &b.phis {
            for &(_, v) in &phi.args {
                pair(v, phi.dst);
            }
        }
    }
    aff
}

struct ScanResult {
    reg: HashMap<VReg, u8>,
    spilled: Vec<VReg>,
}

/// one linear scan per register class; the two files are independent
fn linear_scan(
    f: &IrFunc,
    intervals: &[Interval],
    abi: &AbiInfo,
    affinity: &HashMap<VReg, Vec<VReg>>,
    convention: RegisterConvention,
) -> ScanResult {
    let mut result = ScanResult {
        reg: HashMap::new(),
        spilled: vec![],
    };
    for class in [RegClass::Gpr, RegClass::Fpu] {
        let (allocatable, callee_saved): (&[u8], &[u8]) = match class {
            RegClass::Gpr => (convention.allocatable_registers, convention.callee_saved),
            // all FPU registers are caller-saved: intervals crossing a call
            // have no register to live in and always spill
            RegClass::Fpu => match convention.fpu {
                Some(fpu) => (fpu.allocatable_registers, &[]),
                None => continue,
            },
        };
        let scan = scan_class(
            f,
            intervals,
            abi,
            affinity,
            class,
            allocatable,
            callee_saved,
        );
        result.reg.extend(scan.reg);
        result.spilled.extend(scan.spilled);
    }
    result
}

/// Extra register constraint on the destination of a hardware VECTOR
/// instruction: the design's section 3.2 partial-overlap rule. The destination
/// range must be disjoint from every vector source, or share a source's base
/// exactly (in-place), and must not contain the scalar source of `VMULS`.
#[derive(Default)]
struct VecDefConstraint {
    vectors: Vec<VReg>,
    scalars: Vec<VReg>,
}

/// per-definition vector overlap constraints (only for the hardware VECTOR
/// subops; `FpuVecMove`/`FpuVecConstruct` lower to cycle-safe parallel moves)
fn vector_def_constraints(f: &IrFunc) -> HashMap<VReg, VecDefConstraint> {
    let mut map: HashMap<VReg, VecDefConstraint> = HashMap::new();
    for b in &f.blocks {
        for inst in &b.insts {
            match inst {
                Instr::FpuVecBin { dst, lhs, rhs, .. } => {
                    let e = map.entry(*dst).or_default();
                    e.vectors.push(*lhs);
                    e.vectors.push(*rhs);
                }
                Instr::FpuVecUn { dst, src, .. } => {
                    map.entry(*dst).or_default().vectors.push(*src);
                }
                Instr::FpuVecMulS { dst, lhs, scalar } => {
                    let e = map.entry(*dst).or_default();
                    e.vectors.push(*lhs);
                    e.scalars.push(*scalar);
                }
                _ => {}
            }
        }
    }
    map
}

/// `DOTSTORE` writes scalar `Fd` but the encoding still validates
/// `Fd + lanes <= 64`, so a dot destination cannot use the top few registers.
fn dot_destination_limits(f: &IrFunc) -> HashMap<VReg, u8> {
    let mut map = HashMap::new();
    for b in &f.blocks {
        for inst in &b.insts {
            if let Instr::FpuDotStore { dst, lhs, .. } = inst {
                map.insert(*dst, 64u8.saturating_sub(f.lanes(*lhs)));
            }
        }
    }
    map
}

fn ranges_overlap(a: usize, a_len: usize, b: usize, b_len: usize) -> bool {
    a < b + b_len && b < a + a_len
}

#[allow(clippy::too_many_arguments)]
fn scan_class(
    f: &IrFunc,
    intervals: &[Interval],
    abi: &AbiInfo,
    affinity: &HashMap<VReg, Vec<VReg>>,
    class: RegClass,
    allocatable: &'static [u8],
    callee_saved: &'static [u8],
) -> ScanResult {
    // fixed ranges per register (from pinned vregs), known upfront. A pinned
    // vector occupies one fixed range on each of its lane registers.
    let mut fixed_ranges: HashMap<u8, Vec<(u32, u32)>> = HashMap::new();
    for (&v, &r) in &abi.pinned {
        if f.class_of(v) != class {
            continue;
        }
        let range = (intervals[v as usize].start, intervals[v as usize].end);
        for lane in 0..f.lanes(v) {
            fixed_ranges.entry(r + lane).or_default().push(range);
        }
    }
    for (r, ranges) in &fixed_ranges {
        let mut sorted = ranges.clone();
        sorted.sort();
        for w in sorted.windows(2) {
            assert!(w[1].0 > w[0].1, "pinned intervals on r{r} overlap: {w:?}");
        }
    }
    let overlaps = |a: (u32, u32), b: (u32, u32)| !(a.1 < b.0 || b.1 < a.0);

    let constraints = if class == RegClass::Fpu {
        vector_def_constraints(f)
    } else {
        HashMap::new()
    };
    let dot_limits = if class == RegClass::Fpu {
        dot_destination_limits(f)
    } else {
        HashMap::new()
    };

    let mut order: Vec<VReg> = (0..intervals.len() as VReg)
        .filter(|&v| intervals[v as usize].start != u32::MAX) // skip dead vregs
        .filter(|&v| f.class_of(v) == class)
        .collect();
    order.sort_by_key(|&v| intervals[v as usize].start);

    let mut reg: HashMap<VReg, u8> = HashMap::new();
    let mut spilled = vec![];
    // (end, vreg, base register, lane count); kept sorted by end
    let mut active: Vec<(u32, VReg, u8, u8)> = vec![];

    for &v in &order {
        let iv = &intervals[v as usize];
        let lanes = f.lanes(v);
        // expire intervals that no longer overlap
        active.retain(|&(e, av, _, _)| {
            if e < iv.start {
                debug_assert!(reg.contains_key(&av) || spilled.contains(&av));
                false
            } else {
                true
            }
        });

        if let Some(&r) = abi.pinned.get(&v) {
            reg.insert(v, r);
            active.push((iv.end, v, r, lanes));
            active.sort_by_key(|&(e, _, _, _)| e);
            continue;
        }

        let prefs: &[u8] = if iv.crosses_call {
            callee_saved
        } else {
            allocatable
        };
        // The destination of a hardware VECTOR op must keep its whole range
        // clear of F63 and satisfy the partial-overlap rule; a dot destination
        // additionally cannot sit too close to F63.
        let mut limit = if lanes > 1 { 63 - lanes } else { 62 };
        if let Some(&dot) = dot_limits.get(&v) {
            limit = limit.min(dot);
        }
        // These closures take the live/assigned state as arguments so the
        // allocation loop can mutate `active`/`reg` while checking candidates.
        let free = |active: &[(u32, VReg, u8, u8)], base: u8, len: u8| {
            if base > limit {
                return false;
            }
            let lo = usize::from(base);
            let hi = lo + usize::from(len);
            (lo..hi).all(|r| allocatable.contains(&(r as u8)))
                && !active.iter().any(|&(_, _, ab, al)| {
                    ranges_overlap(lo, hi - lo, usize::from(ab), usize::from(al))
                })
                && !(lo..hi).any(|r| {
                    fixed_ranges
                        .get(&(r as u8))
                        .is_some_and(|rs| rs.iter().any(|&fr| overlaps(fr, (iv.start, iv.end))))
                })
        };
        let constraint_ok = |reg: &HashMap<VReg, u8>, base: u8| {
            let Some(c) = constraints.get(&v) else {
                return true;
            };
            for &s in &c.vectors {
                let Some(sb) = abi.pinned.get(&s).copied().or_else(|| reg.get(&s).copied()) else {
                    continue;
                };
                if base != sb
                    && ranges_overlap(
                        usize::from(base),
                        usize::from(lanes),
                        usize::from(sb),
                        usize::from(f.lanes(s)),
                    )
                {
                    return false;
                }
            }
            for &s in &c.scalars {
                let Some(sr) = abi.pinned.get(&s).copied().or_else(|| reg.get(&s).copied()) else {
                    continue;
                };
                let lo = usize::from(base);
                let hi = lo + usize::from(lanes);
                if (lo..hi).contains(&usize::from(sr)) {
                    return false;
                }
            }
            true
        };
        let usable = |active: &[(u32, VReg, u8, u8)], reg: &HashMap<VReg, u8>, base: u8| {
            free(active, base, lanes) && constraint_ok(reg, base)
        };
        // coalescing hint: prefer the affinity target's register
        let preferred = affinity.get(&v).and_then(|neighbors| {
            neighbors.iter().find_map(|a| {
                if f.class_of(*a) != class {
                    return None;
                }
                if let Some(&r) = abi.pinned.get(a) {
                    Some(r)
                } else {
                    reg.get(a).copied()
                }
            })
        });
        let mut chosen = preferred.filter(|&r| prefs.contains(&r) && usable(&active, &reg, r));
        if chosen.is_none() {
            for &r in prefs {
                if usable(&active, &reg, r) {
                    chosen = Some(r);
                    break;
                }
            }
        }

        if let Some(r) = chosen {
            reg.insert(v, r);
            active.push((iv.end, v, r, lanes));
            active.sort_by_key(|&(e, _, _, _)| e);
            continue;
        }

        // No free run: evict the active (non-fixed) interval with the latest
        // end if it outlives v and its run can hold v; otherwise spill v.
        let victim = active
            .iter()
            .enumerate()
            .filter(|(_, &(e, av, _, al))| {
                !abi.pinned.contains_key(&av) && e > iv.end && al >= lanes
            })
            .max_by_key(|(_, &(e, _, _, _))| e)
            .map(|(i, _)| i);
        if let Some(i) = victim {
            let (_, av, ar, al) = active.remove(i);
            if usable(&active, &reg, ar) {
                reg.remove(&av);
                spilled.push(av);
                reg.insert(v, ar);
                active.push((iv.end, v, ar, lanes));
                active.sort_by_key(|&(e, _, _, _)| e);
            } else {
                // the victim's base does not satisfy v's constraints: put the
                // victim back and spill v instead
                active.push((intervals[av as usize].end, av, ar, al));
                active.sort_by_key(|&(e, _, _, _)| e);
                spilled.push(v);
            }
        } else {
            spilled.push(v);
        }
    }

    ScanResult { reg, spilled }
}

// ---------------------------------------------------------------------------
// step 4: spill rewriting (returns number of frame slots used)
// ---------------------------------------------------------------------------

/// Stack homes and input-free recipes for one spill-rewrite round.
struct SpillLocations {
    gpr: HashMap<VReg, u8>,
    fpu: HashMap<VReg, u8>,
    rematerialized: HashMap<VReg, Instr>,
}
impl SpillLocations {
    fn new(f: &IrFunc, spilled: &[VReg]) -> Self {
        let spilled: HashSet<_> = spilled.iter().copied().collect();
        let mut seen: HashSet<_> = f.params.iter().copied().collect();
        let mut multiple = HashSet::new();
        let mut recipes = HashMap::new();
        for block in &f.blocks {
            for def in block
                .phis
                .iter()
                .map(|p| p.dst)
                .chain(block.insts.iter().flat_map(inst_defs))
            {
                if !seen.insert(def) {
                    multiple.insert(def);
                }
            }
            for inst in &block.insts {
                if let Instr::LoadImm { dst, .. } = inst {
                    if spilled.contains(dst) {
                        recipes.insert(*dst, inst.clone());
                    }
                }
            }
        }
        recipes.retain(|value, _| !multiple.contains(value));
        Self {
            gpr: HashMap::new(),
            fpu: HashMap::new(),
            rematerialized: recipes,
        }
    }
}

/// Rebuild single-definition GPR constants at their uses. Other GPR values get
/// explicit stack homes; FPU values use aligned FLD/FST slots. Slot counters are
/// monotonic across rewrite rounds so earlier homes cannot be clobbered.
/// TODO(M5): pack slots of non-overlapping spills.
fn rewrite_spills(
    f: &mut IrFunc,
    spilled: &[VReg],
    next_slot: &mut u8,
    next_fpu_slot: &mut u8,
) -> Result<(), AllocationError> {
    let mut locations = SpillLocations::new(f, spilled);
    for &v in spilled {
        if locations.rematerialized.contains_key(&v) {
            continue;
        }
        match f.class_of(v) {
            RegClass::Gpr => {
                locations.gpr.insert(v, *next_slot);
                *next_slot = next_slot
                    .checked_add(1)
                    .ok_or(AllocationError::SpillCapacity {
                        function: f.name,
                        class: RegClass::Gpr,
                    })?;
            }
            RegClass::Fpu => {
                locations.fpu.insert(v, *next_fpu_slot);
                // a spill slot is a 4-word unit; a vector needs 2 words per
                // lane, so a vec2 uses one unit, a vec3/vec4 two
                *next_fpu_slot = next_fpu_slot.checked_add(f.lanes(v).div_ceil(2)).ok_or(
                    AllocationError::SpillCapacity {
                        function: f.name,
                        class: RegClass::Fpu,
                    },
                )?;
            }
        }
    }

    let spilled: HashSet<VReg> = spilled.iter().copied().collect();

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
                    let mut lines = vec![];
                    let mut v = v;
                    reload(&mut v, &locations, &spilled, f, app, &mut lines, None);
                    spill_store(v, phi.dst, &locations, f, app, &mut lines, None);
                }
            } else {
                for (p, v) in &mut phi.args {
                    if spilled.contains(v) {
                        let mut lines = vec![];
                        reload(
                            v,
                            &locations,
                            &spilled,
                            f,
                            edge_appends.entry(*p).or_default(),
                            &mut lines,
                            None,
                        );
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
        let mut new_lines = Vec::with_capacity(f.blocks[b].lines.len());
        let old_lines = std::mem::take(&mut f.blocks[b].lines);
        let mut old_lines = old_lines.into_iter();
        for mut inst in std::mem::take(&mut f.blocks[b].insts) {
            let line = old_lines.next().unwrap_or(None);
            // Cheap single-definition values are reconstructed at their uses.
            if matches!(&inst, Instr::LoadImm { dst, .. } if locations.rematerialized.contains_key(dst))
            {
                continue;
            }
            // uses first
            inst.for_each_use_mut(&mut |v| {
                reload(
                    v,
                    &locations,
                    &spilled,
                    f,
                    &mut new_insts,
                    &mut new_lines,
                    line,
                )
            });
            new_insts.push(inst.clone());
            new_lines.push(line);
            // defs after
            for d in inst_defs(&inst) {
                if spilled.contains(&d) {
                    let t = f.fresh_vreg_lanes(f.class_of(d), f.lanes(d));
                    // replace the just-pushed inst's def with t, then store
                    let last = new_insts.last_mut().unwrap();
                    for dd in defs_mut(last) {
                        if *dd == d {
                            *dd = t;
                        }
                    }
                    spill_store(t, d, &locations, f, &mut new_insts, &mut new_lines, line);
                }
            }
        }
        // Reloads and stores introduced for an instruction retain its line so
        // source-to-disassembly highlighting covers the complete operation.
        debug_assert_eq!(new_insts.len(), new_lines.len());
        f.blocks[b].insts = new_insts;
        f.blocks[b].lines = new_lines;

        // terminator uses (take the terminator out to avoid double-borrow of f)
        let mut taken_term = f.blocks[b].term.take();
        let term_line = f.blocks[b].term_line;
        if let Some(term) = &mut taken_term {
            let mut pre = vec![];
            let mut pre_lines = vec![];
            match term {
                Terminator::Jmp { .. } => {}
                Terminator::Br { cmp, .. } => {
                    reload(
                        &mut cmp.lhs,
                        &locations,
                        &spilled,
                        f,
                        &mut pre,
                        &mut pre_lines,
                        term_line,
                    );
                    if let CmpRhs::Reg(r) = &mut cmp.rhs {
                        reload(
                            r,
                            &locations,
                            &spilled,
                            f,
                            &mut pre,
                            &mut pre_lines,
                            term_line,
                        );
                    }
                }
                Terminator::Ret { values } => {
                    for v in values {
                        reload(
                            v,
                            &locations,
                            &spilled,
                            f,
                            &mut pre,
                            &mut pre_lines,
                            term_line,
                        );
                    }
                }
                Terminator::Halt { signal } => reload(
                    signal,
                    &locations,
                    &spilled,
                    f,
                    &mut pre,
                    &mut pre_lines,
                    term_line,
                ),
                Terminator::IcacheInvalidateDelayedAndJump { cseg, target } => {
                    reload(
                        cseg,
                        &locations,
                        &spilled,
                        f,
                        &mut pre,
                        &mut pre_lines,
                        term_line,
                    );
                    reload(
                        target,
                        &locations,
                        &spilled,
                        f,
                        &mut pre,
                        &mut pre_lines,
                        term_line,
                    );
                }
            }
            f.blocks[b].insts.extend(pre);
            f.blocks[b].lines.extend(pre_lines);
        }
        f.blocks[b].term = taken_term;
    }

    // edge appends targeted at pred blocks (from phis of successors)
    for (b, app) in edge_appends {
        let n = app.len();
        f.blocks[b].insts.extend(app);
        let line = f.blocks[b].term_line;
        f.blocks[b].lines.extend(std::iter::repeat_n(line, n));
    }
    Ok(())
}

/// if `v` is spilled, load it from its frame slot into a fresh vreg (GPR:
/// LoadSp; FPU: FImport4 through a fresh GPR address temporary)
#[allow(clippy::too_many_arguments)]
fn reload(
    v: &mut VReg,
    locations: &SpillLocations,
    spilled: &HashSet<VReg>,
    f: &mut IrFunc,
    out: &mut Vec<Instr>,
    lines: &mut Vec<Option<u32>>,
    line: Option<u32>,
) {
    if !spilled.contains(v) {
        return;
    }
    if let Some(recipe) = locations.rematerialized.get(v) {
        let t = f.fresh_vreg(RegClass::Gpr);
        let mut inst = recipe.clone();
        for dst in defs_mut(&mut inst) {
            *dst = t;
        }
        out.push(inst);
        lines.push(line);
        *v = t;
        return;
    }
    match f.class_of(*v) {
        RegClass::Gpr => {
            let t = f.fresh_vreg(RegClass::Gpr);
            out.push(Instr::LoadSp {
                dst: t,
                slot: locations.gpr[v],
            });
            lines.push(line);
            *v = t;
        }
        RegClass::Fpu => {
            let lanes = f.lanes(*v);
            let addr = f.fresh_vreg(RegClass::Gpr);
            out.push(Instr::AddrOfFpuSpill {
                dst: addr,
                slot: locations.fpu[v],
            });
            lines.push(line);
            let t = f.fresh_vreg_lanes(RegClass::Fpu, lanes);
            if lanes > 1 {
                out.push(Instr::FpuVecLoad {
                    dst: t,
                    base_gpr: addr,
                    offset: 0,
                });
            } else {
                out.push(Instr::FpuLoad {
                    dst: t,
                    base_gpr: addr,
                    offset: 0,
                });
            }
            lines.push(line);
            *v = t;
        }
    }
}

/// store the fresh vreg `t` (holding the value of the spilled vreg `orig`)
/// into `orig`'s frame slot
#[allow(clippy::too_many_arguments)]
fn spill_store(
    t: VReg,
    orig: VReg,
    locations: &SpillLocations,
    f: &mut IrFunc,
    out: &mut Vec<Instr>,
    lines: &mut Vec<Option<u32>>,
    line: Option<u32>,
) {
    match f.class_of(orig) {
        RegClass::Gpr => {
            out.push(Instr::StoreSp {
                slot: locations.gpr[&orig],
                src: t,
            });
            lines.push(line);
        }
        RegClass::Fpu => {
            let lanes = f.lanes(orig);
            let addr = f.fresh_vreg(RegClass::Gpr);
            out.push(Instr::AddrOfFpuSpill {
                dst: addr,
                slot: locations.fpu[&orig],
            });
            lines.push(line);
            if lanes > 1 {
                out.push(Instr::FpuVecStore {
                    base_gpr: addr,
                    offset: 0,
                    src: t,
                });
            } else {
                out.push(Instr::FpuStore {
                    base_gpr: addr,
                    offset: 0,
                    src: t,
                });
            }
            lines.push(line);
        }
    }
}

pub(crate) fn defs_mut(inst: &mut Instr) -> Vec<&mut VReg> {
    match inst {
        Instr::Bin { dst, .. }
        | Instr::Mul { dst, .. }
        | Instr::Un { dst, .. }
        | Instr::Shift { dst, .. }
        | Instr::Mov { dst, .. }
        | Instr::LoadImm { dst, .. }
        | Instr::LoadMem { dst, .. }
        | Instr::DevRecv { dst, .. }
        | Instr::LoadFuncAddr { dst, .. }
        | Instr::LoadLocal { dst, .. }
        | Instr::AddrOfLocal { dst, .. }
        | Instr::Mfsr { dst, .. }
        | Instr::Bool { dst, .. }
        | Instr::CMov { dst, .. }
        | Instr::LoadSp { dst, .. }
        | Instr::FpuBin { dst, .. }
        | Instr::FpuUn { dst, .. }
        | Instr::FpuSpecial { dst, .. }
        | Instr::FpuMov { dst, .. }
        | Instr::FpuFromInt { dst, .. }
        | Instr::FpuFromLo { dst, .. }
        | Instr::FpuFromHi { dst, .. }
        | Instr::FpuVecBin { dst, .. }
        | Instr::FpuVecMulS { dst, .. }
        | Instr::FpuVecUn { dst, .. }
        | Instr::FpuVecMove { dst, .. }
        | Instr::FpuVecConstruct { dst, .. }
        | Instr::FpuVecLane { dst, .. }
        | Instr::FpuDotStore { dst, .. }
        | Instr::FpuLoad { dst, .. }
        | Instr::FpuVecLoad { dst, .. } => vec![dst],
        Instr::FpuToInt { dst_gpr, .. }
        | Instr::FpuToLo { dst_gpr, .. }
        | Instr::FpuToHi { dst_gpr, .. }
        | Instr::AddrOfFpuSpill { dst: dst_gpr, .. } => vec![dst_gpr],
        Instr::StoreMem { .. }
        | Instr::StoreStatic { .. }
        | Instr::DevSend { .. }
        | Instr::DcacheInvalidateAll
        | Instr::DcacheLineCopy { .. }
        | Instr::DcacheCleanLine { .. }
        | Instr::DcacheWait
        | Instr::MtsrDseg { .. }
        | Instr::Jseg { .. }
        | Instr::Signal { .. }
        | Instr::StoreSp { .. }
        | Instr::StoreLocal { .. }
        | Instr::FpuStore { .. }
        | Instr::FpuVecStore { .. } => {
            vec![]
        }
        Instr::Call { rets, .. } | Instr::CallPtr { rets, .. } => rets.iter_mut().collect(),
    }
}

#[cfg(test)]
mod spill_capacity_tests {
    use super::*;
    #[test]
    fn allocated_constants_are_reused_only_until_their_register_is_written_or_called() {
        let (mut b, _) = crate::FuncBuilder::new("constants", 0, 1);
        let a = b.load_imm(7);
        let c = b.load_imm(7);
        let sum = b.bin(crate::BinOp::Add, c, c);
        let d = b.load_imm(7);
        b.call("opaque", &[], 0);
        let e = b.load_imm(7);
        b.ret(&[e]);
        let mut f = b.finish();
        let registers = HashMap::from([(a, 0), (c, 0), (sum, 0), (d, 0), (e, 0)]);
        // Unreachable IR has no physical assignment and must remain untouched.
        let dead = f.fresh_vreg(RegClass::Gpr);
        f.blocks.push(Block {
            insts: vec![Instr::LoadImm {
                dst: dead,
                value: 7,
            }],
            ..Block::default()
        });
        deduplicate_allocated_immediates(&mut f, &registers);
        assert!(matches!(f.blocks[0].insts[1], Instr::Mov { dst, src } if dst == c && src == a));
        for i in [3, 5] {
            assert!(matches!(f.blocks[0].insts[i], Instr::LoadImm { .. }));
        }
    }

    #[test]
    fn stack_reload_cleanup_keeps_namespaces_slots_and_barriers_distinct() {
        let (mut b, args) = crate::FuncBuilder::new("cleanup", 2, 1);
        let a = b.get(args[0]);
        let value = b.get(args[1]);
        b.ret(&[value]);
        let mut f = b.finish();
        f.blocks[0].insts = vec![
            Instr::StoreSp { slot: 0, src: a },
            Instr::LoadSp {
                slot: 0,
                dst: value,
            },
            Instr::StoreSp { slot: 0, src: a },
            Instr::LoadSp {
                slot: 1,
                dst: value,
            },
            Instr::StoreLocal { slot: 0, src: a },
            Instr::LoadSp {
                slot: 0,
                dst: value,
            },
            Instr::StoreSp { slot: 0, src: a },
            Instr::Call {
                func: "opaque",
                args: vec![],
                rets: vec![],
            },
            Instr::LoadSp {
                slot: 0,
                dst: value,
            },
        ];
        forward_adjacent_stack_reloads(&mut f);
        assert!(
            matches!(f.blocks[0].insts[1], Instr::Mov { dst, src } if dst == value && src == a)
        );
        for i in [3, 5, 8] {
            assert!(matches!(f.blocks[0].insts[i], Instr::LoadSp { .. }));
        }
    }

    #[test]
    fn spilled_immediates_are_rebuilt_without_stack_homes() {
        let (mut b, _) = crate::FuncBuilder::new("constant", 0, 1);
        let value = b.load_imm(0x1234);
        b.call("opaque", &[value], 0);
        b.ret(&[value]);
        let mut f = b.finish();
        let (mut gpr, mut fpu) = (0, 0);
        rewrite_spills(&mut f, &[value], &mut gpr, &mut fpu).unwrap();
        assert_eq!((gpr, fpu), (0, 0));
        let instructions = f.blocks.iter().flat_map(|b| &b.insts).collect::<Vec<_>>();
        assert_eq!(
            instructions
                .iter()
                .filter(|i| matches!(i, Instr::LoadImm { value: 0x1234, .. }))
                .count(),
            2
        );
        assert!(!instructions
            .iter()
            .any(|i| matches!(i, Instr::LoadSp { .. } | Instr::StoreSp { .. })));
        assert!(f.blocks.iter().all(|b| b.insts.len() == b.lines.len()));
    }

    #[test]
    fn redefined_values_keep_a_real_spill_home() {
        let (mut b, _) = crate::FuncBuilder::new("mutable", 0, 1);
        let value = b.load_imm(2);
        b.call("opaque", &[value], 0);
        b.ret(&[value]);
        let mut f = b.finish();
        f.blocks[0].insts.insert(
            2,
            Instr::LoadImm {
                dst: value,
                value: 9,
            },
        );
        f.blocks[0].lines.insert(2, None);
        let (mut gpr, mut fpu) = (0, 0);
        rewrite_spills(&mut f, &[value], &mut gpr, &mut fpu).unwrap();
        assert_eq!((gpr, fpu), (1, 0));
        assert!(f
            .blocks
            .iter()
            .flat_map(|b| &b.insts)
            .any(|i| matches!(i, Instr::LoadSp { .. })));
    }

    #[test]
    fn spilled_slot_counters_cannot_wrap_before_frame_check() {
        for (class, lanes, start) in [(RegClass::Gpr, 1, 255), (RegClass::Fpu, 4, 254)] {
            let (mut builder, _) = crate::FuncBuilder::new("overflow", 0, 0);
            builder.ret(&[]);
            let mut f = builder.finish();
            let v = f.fresh_vreg_lanes(class, lanes);
            let mut gpr = if class == RegClass::Gpr { start } else { 0 };
            let mut fpu = if class == RegClass::Fpu { start } else { 0 };
            let error = rewrite_spills(&mut f, &[v], &mut gpr, &mut fpu).unwrap_err();
            assert_eq!(
                error,
                AllocationError::SpillCapacity {
                    function: "overflow",
                    class
                }
            );
        }
    }
}
