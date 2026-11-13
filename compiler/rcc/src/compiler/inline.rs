//! Bounded automatic and source-requested CFG inlining before ABI lowering.
//! Original functions remain in the table; target reachability owns emission.

use super::regalloc::{defs_mut, inst_defs};
use super::*;
use crate::FuncName;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct InlineOptions {
    pub enabled: bool,
    /// Select small unannotated direct callees as well as explicit hints.
    pub automatic: bool,
    pub maximum_automatic_nodes: usize,
    pub maximum_automatic_blocks: usize,
    pub maximum_depth: usize,
    pub maximum_callee_nodes: usize,
    pub maximum_callee_blocks: usize,
    pub maximum_callee_vregs: usize,
    /// Optional compilation-work limit for cumulative clone/glue nodes.
    /// None leaves selection to each caller's growth limit; DCE never refunds costs.
    pub maximum_program_nodes: Option<usize>,
}
impl Default for InlineOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            automatic: true,
            maximum_automatic_nodes: 24,
            maximum_automatic_blocks: 4,
            maximum_depth: 2,
            maximum_callee_nodes: 32,
            maximum_callee_blocks: 8,
            maximum_callee_vregs: 48,
            maximum_program_nodes: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InlineDecision {
    Expanded,
    Never,
    Entry,
    Recursive,
    Indirect,
    SourceFrame,
    SpecialTransfer,
    NoReturn,
    EntryPhi,
    NonLeaf,
    DepthBudget,
    CalleeBudget,
    AutomaticBudget,
    AutomaticCycle,
    CallerBudget,
    ProgramBudget,
}
#[derive(Clone, Debug)]
pub struct InlineSite {
    pub caller: FuncName,
    pub callee: FuncName,
    /// Source function containing the original call, including nested sites.
    pub source_function: FuncName,
    pub line: Option<u32>,
    /// Physical caller line used by the current file-per-function debugger.
    pub callsite_line: Option<u32>,
    pub depth: usize,
    pub automatic: bool,
    pub decision: InlineDecision,
}
#[derive(Clone, Debug, Default)]
pub struct InlineReport {
    pub sites: Vec<InlineSite>,
    pub introduced_nodes: usize,
}
impl InlineReport {
    pub fn expanded_sites(&self) -> usize {
        self.sites
            .iter()
            .filter(|s| s.decision == InlineDecision::Expanded)
            .count()
    }
    pub fn automatic_expanded_sites(&self) -> usize {
        self.sites
            .iter()
            .filter(|s| s.automatic && s.decision == InlineDecision::Expanded)
            .count()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineError(pub String);
impl std::fmt::Display for InlineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for InlineError {}
fn invalid(message: impl Into<String>) -> InlineError {
    InlineError(message.into())
}

#[derive(Clone)]
struct Site {
    block: BlockId,
    index: usize,
    depth: usize,
    source: FuncName,
    line: Option<u32>,
}
struct CallerWork {
    name: FuncName,
    budget: usize,
    charged: usize,
    sites: Vec<Option<Site>>,
}
/// Deterministic global priority: explicit requests, then cheaper automatic
/// callees. Stable site indices survive block splitting; deterministic source
/// order breaks equal-cost ties without depending on HashMap iteration.
#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct PendingSite {
    automatic: bool,
    nodes: usize,
    sequence: usize,
    caller: usize,
    site: usize,
}
impl CallerWork {
    fn push(
        &mut self,
        site: Site,
        callee: &IrFunc,
        caller: usize,
        sequence: &mut usize,
    ) -> PendingSite {
        let index = self.sites.len();
        self.sites.push(Some(site));
        let automatic = callee.inline_hint == InlineHint::Default;
        let pending = PendingSite {
            automatic,
            nodes: if automatic { nodes(callee) } else { 0 },
            sequence: *sequence,
            caller,
            site: index,
        };
        *sequence += 1;
        pending
    }
}

fn nodes(f: &IrFunc) -> usize {
    f.rpo()
        .iter()
        .map(|&b| f.blocks[b].phis.len() + f.blocks[b].insts.len() + 1)
        .sum()
}
fn calls(f: &IrFunc) -> Vec<FuncName> {
    f.rpo()
        .iter()
        .flat_map(|&b| &f.blocks[b].insts)
        .filter_map(|i| match i {
            Instr::Call { func, .. } => Some(*func),
            _ => None,
        })
        .collect()
}
fn has_cycle(f: &IrFunc, order: &[BlockId]) -> bool {
    // Reverse postorder is topological exactly when all reachable edges go forward.
    let mut rank = vec![usize::MAX; f.blocks.len()];
    for (position, &block) in order.iter().enumerate() {
        rank[block] = position;
    }
    order.iter().any(|&block| {
        f.successors(block)
            .iter()
            .any(|&next| rank[next] <= rank[block])
    })
}
fn recursive(name: FuncName, originals: &HashMap<FuncName, IrFunc>) -> bool {
    let mut seen = HashSet::new();
    let mut work = calls(&originals[&name]);
    while let Some(next) = work.pop() {
        if next == name {
            return true;
        }
        if seen.insert(next) {
            if let Some(f) = originals.get(next) {
                work.extend(calls(f));
            }
        }
    }
    false
}

/// Transform only callers selected by the target's original live graph.
/// `entries` cannot be cloned as callees; it does not replace emission roots.
pub fn inline_calls(
    functions: &mut HashMap<FuncName, IrFunc>,
    callers: &HashSet<FuncName>,
    entries: &HashSet<FuncName>,
    options: &InlineOptions,
) -> Result<InlineReport, InlineError> {
    let mut report = InlineReport::default();
    if !options.enabled {
        return Ok(report);
    }
    let originals = functions.clone();
    let mut order: Vec<_> = callers.iter().copied().collect();
    order.sort_unstable();
    let recursive: HashSet<_> = order
        .iter()
        .copied()
        .filter(|n| recursive(n, &originals))
        .collect();
    let mut states = Vec::new();
    let mut pending = BTreeSet::new();
    let mut sequence = 0;
    for name in order {
        let original = originals
            .get(&name)
            .ok_or_else(|| invalid(format!("missing inline caller {name}")))?;
        let mut state = CallerWork {
            name,
            budget: 32.max(nodes(original).div_ceil(4)),
            charged: 0,
            sites: Vec::new(),
        };
        for block in original.rpo() {
            for (index, inst) in original.blocks[block].insts.iter().enumerate() {
                if let Instr::Call { func, .. } = inst {
                    let callee = originals
                        .get(func)
                        .ok_or_else(|| invalid(format!("unknown inline callee {func}")))?;
                    if callee.inline_hint == InlineHint::Default && !options.automatic {
                        continue;
                    }
                    pending.insert(state.push(
                        Site {
                            block,
                            index,
                            depth: 1,
                            source: name,
                            line: original.blocks[block].lines.get(index).copied().flatten(),
                        },
                        callee,
                        states.len(),
                        &mut sequence,
                    ));
                }
            }
        }
        states.push(state);
    }
    while let Some(queued) = pending.pop_first() {
        let state = &mut states[queued.caller];
        let name = state.name;
        let site = state.sites[queued.site].take().unwrap();
        let caller = functions.get_mut(&name).unwrap();
        let Instr::Call { func, args, rets } = caller.blocks[site.block].insts[site.index].clone()
        else {
            return Err(invalid("inline work item no longer names a call"));
        };
        let callee = originals
            .get(&func)
            .ok_or_else(|| invalid(format!("unknown inline callee {func}")))?;
        let automatic = callee.inline_hint == InlineHint::Default;
        if automatic && !options.automatic {
            continue;
        }
        let line = caller.blocks[site.block]
            .lines
            .get(site.index)
            .copied()
            .flatten();
        let mut record = InlineSite {
            caller: name,
            callee: func,
            source_function: site.source,
            line: site.line,
            callsite_line: line,
            depth: site.depth,
            automatic,
            decision: InlineDecision::Expanded,
        };
        let blocks = callee.rpo();
        // Spill IR belongs to a later pipeline stage, not to source eligibility.
        if blocks
            .iter()
            .flat_map(|&b| &callee.blocks[b].insts)
            .any(|i| {
                matches!(
                    i,
                    Instr::LoadSp { .. } | Instr::StoreSp { .. } | Instr::AddrOfFpuSpill { .. }
                )
            })
        {
            return Err(invalid(format!(
                "post-allocation IR supplied to inliner: {func}"
            )));
        }
        let count = nodes(callee);
        let cost = count + rets.len() + 1;
        let decision = if callee.inline_hint == InlineHint::Never {
            Some(InlineDecision::Never)
        } else if entries.contains(&func) {
            Some(InlineDecision::Entry)
        } else if recursive.contains(&func) {
            Some(InlineDecision::Recursive)
        } else if callee.local_slots != 0
            || blocks
                .iter()
                .flat_map(|&b| &callee.blocks[b].insts)
                .any(|i| {
                    matches!(
                        i,
                        Instr::LoadLocal { .. }
                            | Instr::StoreLocal { .. }
                            | Instr::AddrOfLocal { .. }
                    )
                })
        {
            Some(InlineDecision::SourceFrame)
        } else if blocks
            .iter()
            .flat_map(|&b| &callee.blocks[b].insts)
            .any(|i| matches!(i, Instr::CallPtr { .. }))
        {
            Some(InlineDecision::Indirect)
        } else if blocks.iter().any(|&b| {
            matches!(
                callee.blocks[b].term,
                Some(Terminator::IcacheInvalidateDelayedAndJump { .. })
            )
        }) || blocks
            .iter()
            .flat_map(|&b| &callee.blocks[b].insts)
            .any(|i| matches!(i, Instr::Jseg { .. }))
        {
            Some(InlineDecision::SpecialTransfer)
        } else if !blocks
            .iter()
            .any(|&b| matches!(callee.blocks[b].term, Some(Terminator::Ret { .. })))
        {
            Some(InlineDecision::NoReturn)
        } else if !callee.blocks[callee.entry].phis.is_empty()
            || !callee.blocks[callee.entry].preds.is_empty()
        {
            Some(InlineDecision::EntryPhi)
        } else if site.depth > options.maximum_depth {
            Some(InlineDecision::DepthBudget)
        } else if options.maximum_depth == 1 && !calls(callee).is_empty() {
            Some(InlineDecision::NonLeaf)
        } else if count > options.maximum_callee_nodes
            || blocks.len() > options.maximum_callee_blocks
            || callee.vreg_count as usize > options.maximum_callee_vregs
        {
            Some(InlineDecision::CalleeBudget)
        } else if automatic
            && (count > options.maximum_automatic_nodes
                || blocks.len() > options.maximum_automatic_blocks)
        {
            Some(InlineDecision::AutomaticBudget)
        } else if automatic && has_cycle(callee, &blocks) {
            Some(InlineDecision::AutomaticCycle)
        } else if state.charged + cost > state.budget {
            Some(InlineDecision::CallerBudget)
        } else if options
            .maximum_program_nodes
            .is_some_and(|limit| report.introduced_nodes + cost > limit)
        {
            Some(InlineDecision::ProgramBudget)
        } else {
            None
        };
        if let Some(reason) = decision {
            record.decision = reason;
            report.sites.push(record);
            continue;
        }
        validate_inline_ir(callee)?;
        if state.charged == 0 {
            validate_inline_ir(caller)?;
        }
        let (continuation, suffix_offset, nested) =
            clone_call(caller, &site, callee, &args, &rets, line)?;
        for pending in state.sites.iter_mut().flatten() {
            if pending.block == site.block && pending.index > site.index {
                pending.block = continuation;
                pending.index = pending.index - site.index - 1 + suffix_offset;
            }
        }
        for site in nested {
            let Instr::Call { func, .. } = &caller.blocks[site.block].insts[site.index] else {
                return Err(invalid("nested inline work item no longer names a call"));
            };
            let callee = originals
                .get(func)
                .ok_or_else(|| invalid(format!("unknown inline callee {func}")))?;
            if callee.inline_hint == InlineHint::Default && !options.automatic {
                continue;
            }
            pending.insert(state.push(site, callee, queued.caller, &mut sequence));
        }
        state.charged += cost;
        report.introduced_nodes += cost;
        report.sites.push(record);
    }
    for state in states {
        if state.charged != 0 {
            validate_inline_ir(&functions[&state.name])?;
        }
    }
    Ok(report)
}

fn move_value(f: &IrFunc, dst: VReg, src: VReg) -> Instr {
    match (f.class_of(dst), f.lanes(dst)) {
        (RegClass::Gpr, _) => Instr::Mov { dst, src },
        (RegClass::Fpu, 1) => Instr::FpuMov { dst, src },
        (RegClass::Fpu, _) => Instr::FpuVecMove { dst, src },
    }
}
fn term_values(term: &Terminator) -> Vec<VReg> {
    match term {
        Terminator::Br { cmp, .. } => {
            let mut v = vec![cmp.lhs];
            if let CmpRhs::Reg(r) = cmp.rhs {
                v.push(r);
            }
            v
        }
        Terminator::Ret { values } => values.clone(),
        Terminator::Halt { signal } => vec![*signal],
        Terminator::IcacheInvalidateDelayedAndJump { cseg, target } => vec![*cseg, *target],
        Terminator::Jmp { .. } => vec![],
    }
}
fn clone_call(
    caller: &mut IrFunc,
    site: &Site,
    callee: &IrFunc,
    args: &[VReg],
    rets: &[VReg],
    line: Option<u32>,
) -> Result<(BlockId, usize, Vec<Site>), InlineError> {
    if args.len() != callee.params.len() || rets.len() != callee.n_rets {
        return Err(invalid(format!("invalid call shape for {}", callee.name)));
    }
    let blocks = callee.rpo();
    let returns: Vec<_> = blocks
        .iter()
        .filter_map(|&b| match &callee.blocks[b].term {
            Some(Terminator::Ret { values }) => Some((b, values.clone())),
            _ => None,
        })
        .collect();
    for (&p, &a) in callee.params.iter().zip(args) {
        if callee.class_of(p) != caller.class_of(a) || callee.lanes(p) != caller.lanes(a) {
            return Err(invalid("inline parameter type mismatch"));
        }
    }
    for (_, values) in &returns {
        if values.len() != rets.len() {
            return Err(invalid("inline return arity mismatch"));
        }
        for (&v, &r) in values.iter().zip(rets) {
            if callee.class_of(v) != caller.class_of(r) || callee.lanes(v) != caller.lanes(r) {
                return Err(invalid("inline return type mismatch"));
            }
        }
    }
    // auto_init reserves unused metadata tail entries. Fresh SSA allocation
    // appends at vreg_count, so canonicalize only this owned candidate copy;
    // all occupied ids and the independent baseline remain untouched.
    caller.vreg_class.truncate(caller.vreg_count as usize);
    caller.vreg_lanes.truncate(caller.vreg_count as usize);
    let continuation = caller.blocks.len();
    let mut block_map = vec![usize::MAX; callee.blocks.len()];
    for (i, &b) in blocks.iter().enumerate() {
        block_map[b] = continuation + 1 + i;
    }
    let mut values = vec![None; callee.vreg_count as usize];
    for (&p, &a) in callee.params.iter().zip(args) {
        values[p as usize] = Some(a);
    }
    for v in 0..callee.vreg_count {
        if values[v as usize].is_none() {
            values[v as usize] = Some(caller.fresh_vreg_lanes(callee.class_of(v), callee.lanes(v)));
        }
    }
    let value = |v: VReg| values[v as usize].unwrap();
    let old_successors = caller.successors(site.block);
    let block = &mut caller.blocks[site.block];
    block.lines.resize(block.insts.len(), None);
    let tail = block.insts.split_off(site.index + 1);
    block.insts.pop();
    let tail_lines = block.lines.split_off(site.index + 1);
    block.lines.pop();
    let old_term = block.term.take();
    let old_term_line = block.term_line;
    block.term = Some(Terminator::Jmp {
        target: block_map[callee.entry],
    });
    block.term_line = line;
    caller.blocks.push(Block {
        insts: tail,
        lines: tail_lines,
        term: old_term,
        term_line: old_term_line,
        ..Block::default()
    });
    caller.block_notes.push(Some("inline continuation"));
    caller.block_lines.push(line);
    for successor in old_successors {
        for phi in &mut caller.blocks[successor].phis {
            for (pred, _) in &mut phi.args {
                if *pred == site.block {
                    *pred = continuation;
                }
            }
        }
    }
    let mut nested = Vec::new();
    for &old in &blocks {
        let mut block = callee.blocks[old].clone();
        for phi in &mut block.phis {
            phi.dst = value(phi.dst);
            phi.args.retain(|(b, _)| block_map[*b] != usize::MAX);
            for (b, v) in &mut phi.args {
                *b = block_map[*b];
                *v = value(*v);
            }
        }
        for (index, inst) in block.insts.iter_mut().enumerate() {
            if matches!(inst, Instr::Call { .. }) {
                nested.push(Site {
                    block: block_map[old],
                    index,
                    depth: site.depth + 1,
                    source: callee.name,
                    line: block.lines.get(index).copied().flatten(),
                });
            }
            inst.for_each_use_mut(&mut |v| *v = value(*v));
            for def in defs_mut(inst) {
                *def = value(*def);
            }
        }
        block.lines = vec![line; block.insts.len()];
        block.term_line = line;
        block.preds.clear();
        let term = block
            .term
            .as_mut()
            .ok_or_else(|| invalid("unterminated inline block"))?;
        match term {
            Terminator::Jmp { target } => *target = block_map[*target],
            Terminator::Br {
                cmp,
                if_true,
                if_false,
            } => {
                cmp.lhs = value(cmp.lhs);
                if let CmpRhs::Reg(v) = &mut cmp.rhs {
                    *v = value(*v);
                }
                *if_true = block_map[*if_true];
                *if_false = block_map[*if_false];
            }
            Terminator::Ret { .. } => {
                *term = Terminator::Jmp {
                    target: continuation,
                }
            }
            Terminator::Halt { signal } => *signal = value(*signal),
            Terminator::IcacheInvalidateDelayedAndJump { .. } => {
                return Err(invalid("special transfer reached clone"))
            }
        }
        caller.blocks.push(block);
        caller.block_notes.push(Some("inlined body"));
        caller.block_lines.push(line);
    }
    if returns.len() == 1 {
        for (&dst, &src) in rets.iter().zip(&returns[0].1) {
            let inst = move_value(caller, dst, value(src));
            caller.blocks[continuation].insts.insert(0, inst);
            caller.blocks[continuation].lines.insert(0, line);
        }
        // Existing calls in the moved suffix shift past these return moves.
    } else {
        for (i, &dst) in rets.iter().enumerate() {
            caller.blocks[continuation].phis.push(Phi {
                dst,
                args: returns
                    .iter()
                    .map(|(b, vs)| (block_map[*b], value(vs[i])))
                    .collect(),
            });
        }
    }
    // Caller successor phis were repaired above. Rebuild all reciprocal edges.
    for block in &mut caller.blocks {
        block.preds.clear();
    }
    for b in 0..caller.blocks.len() {
        for target in caller.successors(b) {
            if !caller.blocks[target].preds.contains(&b) {
                caller.blocks[target].preds.push(b);
            }
        }
    }
    // Return moves precede the old suffix; expose their offset to the queue.
    Ok((
        continuation,
        if returns.len() == 1 { rets.len() } else { 0 },
        nested,
    ))
}

/// Verify pre-ABI reachable SSA, including loop-phi edge dominance.
/// This reports transformation errors; it is never an optimization fallback.
pub fn validate_inline_ir(f: &IrFunc) -> Result<(), InlineError> {
    let fail = || invalid(format!("invalid pre-ABI inline IR in {}", f.name));
    if f.entry >= f.blocks.len()
        || f.vreg_class.len() < f.vreg_count as usize
        || f.vreg_lanes.len() < f.vreg_count as usize
        || f.block_notes.len() != f.blocks.len()
        || f.block_lines.len() != f.blocks.len()
    {
        return Err(fail());
    }
    for b in 0..f.blocks.len() {
        if f.successors(b).iter().any(|&t| t >= f.blocks.len()) {
            return Err(fail());
        }
    }
    let order = f.rpo();
    let live: HashSet<_> = order.iter().copied().collect();
    let mut preds = vec![BTreeSet::new(); f.blocks.len()];
    for &b in &order {
        for next in f.successors(b) {
            preds[next].insert(b);
        }
    }
    let mut defs = vec![None; f.vreg_count as usize];
    let mut define = |v: VReg, b: usize, pos: usize| -> Result<(), InlineError> {
        if v as usize >= defs.len() || defs[v as usize].replace((b, pos)).is_some() {
            return Err(fail());
        }
        Ok(())
    };
    for &p in &f.params {
        define(p, f.entry, 0)?;
    }
    for &b in &order {
        let block = &f.blocks[b];
        if block.term.is_none() || block.insts.len() != block.lines.len() {
            return Err(fail());
        }
        let actual: BTreeSet<_> = block
            .preds
            .iter()
            .filter(|p| live.contains(p))
            .copied()
            .collect();
        if actual != preds[b] {
            return Err(fail());
        }
        for phi in &block.phis {
            define(phi.dst, b, 0)?;
            let incoming: BTreeSet<_> = phi
                .args
                .iter()
                .filter(|(p, _)| live.contains(p))
                .map(|(p, _)| *p)
                .collect();
            if incoming != preds[b]
                || incoming.len() != phi.args.iter().filter(|(p, _)| live.contains(p)).count()
            {
                return Err(fail());
            }
        }
        for (i, inst) in block.insts.iter().enumerate() {
            for v in inst_defs(inst) {
                define(v, b, i + 1)?;
            }
        }
    }
    let mut dom = vec![live.clone(); f.blocks.len()];
    dom[f.entry] = HashSet::from([f.entry]);
    loop {
        let mut changed = false;
        for &b in &order {
            if b != f.entry {
                let mut next = live.clone();
                for p in &preds[b] {
                    next.retain(|v| dom[*p].contains(v));
                }
                next.insert(b);
                if next != dom[b] {
                    dom[b] = next;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let check = |v: VReg, b: usize, pos: usize| -> Result<(), InlineError> {
        let Some(Some((owner, defpos))) = defs.get(v as usize) else {
            return Err(fail());
        };
        if !dom[b].contains(owner) || (*owner == b && *defpos >= pos) {
            return Err(fail());
        }
        let lanes = f.lanes(v);
        if !(1..=4).contains(&lanes) || (f.class_of(v) == RegClass::Gpr && lanes != 1) {
            return Err(fail());
        }
        Ok(())
    };
    for &b in &order {
        let block = &f.blocks[b];
        for phi in &block.phis {
            for &(pred, v) in &phi.args {
                if live.contains(&pred) {
                    check(v, pred, usize::MAX)?;
                    if f.class_of(v) != f.class_of(phi.dst) || f.lanes(v) != f.lanes(phi.dst) {
                        return Err(fail());
                    }
                }
            }
        }
        for (i, inst) in block.insts.iter().enumerate() {
            let mut error = None;
            inst.for_each_use(&mut |v| {
                if let Err(e) = check(v, b, i + 1) {
                    error = Some(e);
                }
            });
            if let Some(e) = error {
                return Err(e);
            }
        }
        for v in term_values(block.term.as_ref().unwrap()) {
            check(v, b, usize::MAX)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn functions(source: &str) -> HashMap<FuncName, IrFunc> {
        crate::frontend::parse_source_with_config(source, &crate::RccOptions::default())
            .unwrap()
            .funcs
            .into_iter()
            .map(|f| (f.name, f))
            .collect()
    }
    fn apply(fs: &mut HashMap<FuncName, IrFunc>) -> InlineReport {
        let callers = fs.keys().copied().collect();
        inline_calls(
            fs,
            &callers,
            &HashSet::from(["main"]),
            &InlineOptions {
                maximum_depth: 1,
                ..InlineOptions::default()
            },
        )
        .unwrap()
    }
    #[test]
    fn attributes_are_explicit_and_precise() {
        let fs=functions("#[inline] fn a(x:u16)->u16{x} #[inline(never)] fn b(x:u16)->u16{x} fn main()->u16{b(a(2))}");
        assert_eq!(fs["a"].inline_hint, InlineHint::Hint);
        assert_eq!(fs["b"].inline_hint, InlineHint::Never);
        assert_eq!(fs["main"].inline_hint, InlineHint::Default);
        for attr in [
            "#[inline(always)]",
            "#[inline(foo)]",
            "#[inline()]",
            "#[inline = 1]",
            "#[inline] #[inline(never)]",
        ] {
            let error = crate::frontend::parse_source_with_config(
                &format!("{attr} fn f()->u16{{1}}"),
                &crate::RccOptions::default(),
            )
            .err()
            .unwrap()
            .to_string();
            assert!(error.contains("inline"), "{error}");
        }
    }
    #[test]
    fn automatic_policy_accepts_short_cfgs_and_respects_configuration() {
        let source = "#[inline] fn hinted(x:u16)->u16{x+1} #[inline(never)] fn barrier(x:u16)->u16{x} fn choose(x:u16)->u16{if x>7{x*2+1}else{x+3}} fn main()->u16{barrier(hinted(choose(4)))}";
        let run = |options: InlineOptions| {
            let mut fs = functions(source);
            let live = fs.keys().copied().collect();
            let report = inline_calls(&mut fs, &live, &HashSet::from(["main"]), &options).unwrap();
            validate_inline_ir(&fs["main"]).unwrap();
            report
        };
        let default = run(InlineOptions::default());
        assert!(default.sites.iter().any(|s| s.callee == "choose"
            && s.automatic
            && s.decision == InlineDecision::Expanded));
        assert!(default
            .sites
            .iter()
            .any(|s| s.callee == "barrier" && s.decision == InlineDecision::Never));
        for options in [
            InlineOptions {
                automatic: false,
                ..InlineOptions::default()
            },
            InlineOptions {
                maximum_automatic_nodes: 1,
                ..InlineOptions::default()
            },
        ] {
            let report = run(options);
            assert_eq!(report.automatic_expanded_sites(), 0);
            assert!(report
                .sites
                .iter()
                .any(|s| s.callee == "hinted" && s.decision == InlineDecision::Expanded));
        }
    }
    #[test]
    fn automatic_cycles_and_source_frames_are_kept_as_calls() {
        let mut fs = functions("fn spin(input:u16)->u16{let mut x=input;while x<4{x+=1;}x} fn local(x:u16)->u16{x} fn main()->u16{local(spin(1))}");
        fs.get_mut("local").unwrap().local_slots = 1;
        let live = fs.keys().copied().collect();
        let report = inline_calls(
            &mut fs,
            &live,
            &HashSet::from(["main"]),
            &InlineOptions::default(),
        )
        .unwrap();
        assert!(report
            .sites
            .iter()
            .any(|s| s.callee == "spin" && s.decision == InlineDecision::AutomaticCycle));
        assert!(report
            .sites
            .iter()
            .any(|s| s.callee == "local" && s.decision == InlineDecision::SourceFrame));
        assert_eq!(report.expanded_sites(), 0);
    }
    #[test]
    fn explicit_requests_precede_automatic_sites_without_resetting_clone_depth() {
        let mut fs=functions("fn automatic(x:u16)->u16{x+2} fn a(x:u16)->u16{automatic(x)} #[inline] fn requested(x:u16)->u16{x+1} fn z(x:u16)->u16{requested(x)} fn main()->u16{a(1)+z(2)}");
        let live = fs.keys().copied().collect();
        let report = inline_calls(
            &mut fs,
            &live,
            &HashSet::from(["main"]),
            &InlineOptions {
                maximum_program_nodes: Some(6),
                ..InlineOptions::default()
            },
        )
        .unwrap();
        assert!(report
            .sites
            .iter()
            .any(|s| s.callee == "requested" && s.decision == InlineDecision::Expanded));
        assert_eq!(report.automatic_expanded_sites(), 0);
        let mut fs=functions("#[inline] fn leaf(x:u16)->u16{x+1} fn middle(x:u16)->u16{leaf(x)+2} #[inline] fn outer(x:u16)->u16{middle(x)+3} fn main()->u16{outer(4)}");
        let live = fs.keys().copied().collect();
        let report = inline_calls(
            &mut fs,
            &live,
            &HashSet::from(["main"]),
            &InlineOptions::default(),
        )
        .unwrap();
        assert!(report.sites.iter().any(|s| s.caller == "main"
            && s.callee == "leaf"
            && s.depth == 3
            && s.decision == InlineDecision::DepthBudget));
        assert!(report
            .sites
            .iter()
            .filter(|s| s.decision == InlineDecision::Expanded)
            .all(|s| s.depth <= 2));
    }
    #[test]
    fn cheaper_automatic_sites_receive_budget_before_earlier_large_calls() {
        let mut fs=functions("fn cheap(x:u16)->u16{x} fn larger(x:u16)->u16{x+2} fn a(x:u16)->u16{larger(x)} fn z(x:u16)->u16{cheap(x)} fn main()->u16{a(1)+z(2)}");
        let live = fs.keys().copied().collect();
        let report = inline_calls(
            &mut fs,
            &live,
            &HashSet::from(["main"]),
            &InlineOptions {
                maximum_program_nodes: Some(5),
                ..InlineOptions::default()
            },
        )
        .unwrap();
        assert!(report.sites.iter().any(|s| s.caller == "z"
            && s.callee == "cheap"
            && s.decision == InlineDecision::Expanded));
        assert!(!report
            .sites
            .iter()
            .any(|s| s.callee == "larger" && s.decision == InlineDecision::Expanded));
        assert!(report.introduced_nodes <= 5);
    }
    #[test]
    fn unrelated_explicit_expansions_do_not_displace_default_caller_decisions() {
        let shared = "fn automatic(x:u16)->u16{x+1} #[inline] fn hinted(x:u16)->u16{x+2} #[inline(never)] fn focus(x:u16)->u16{automatic(x)}";
        let mut small = functions(&format!("{shared} fn main()->u16{{focus(1)}}"));
        let mut source = shared.to_owned();
        for index in 0..600 {
            source.push_str(&format!(
                "#[inline(never)] fn extra_{index}(x:u16)->u16{{hinted(x)}}"
            ));
        }
        source.push_str("fn main()->u16{let mut sum=focus(1);");
        for index in 0..600 {
            source.push_str(&format!("sum+=extra_{index}(1);"));
        }
        source.push_str("sum}");
        let large = functions(&source);
        let mut grown = large.clone();
        let options = InlineOptions::default();
        let apply = |fs: &mut HashMap<FuncName, IrFunc>, options: &InlineOptions| {
            let live = fs.keys().copied().collect();
            inline_calls(fs, &live, &HashSet::from(["main"]), options).unwrap()
        };
        let before = apply(&mut small, &options);
        let after = apply(&mut grown, &options);
        let focus_expanded = |r: &InlineReport| {
            r.sites.iter().any(|s| {
                s.caller == "focus"
                    && s.callee == "automatic"
                    && s.decision == InlineDecision::Expanded
            })
        };
        assert!(focus_expanded(&before));
        assert!(focus_expanded(&after));
        assert!(
            after.introduced_nodes > 2048,
            "fixture must exceed the old global quota"
        );
        assert!(!after
            .sites
            .iter()
            .any(|s| s.decision == InlineDecision::ProgramBudget));
        assert_eq!(small["focus"].blocks, grown["focus"].blocks);
        validate_inline_ir(&grown["focus"]).unwrap();

        // An explicitly requested work limit retains the old cumulative policy.
        let limited = apply(
            &mut large.clone(),
            &InlineOptions {
                maximum_program_nodes: Some(2048),
                ..options
            },
        );
        assert!(limited.introduced_nodes <= 2048);
        assert!(limited.sites.iter().any(|s| s.caller == "focus"
            && s.callee == "automatic"
            && s.decision == InlineDecision::ProgramBudget));
    }
    #[test]
    fn method_lowering_preserves_hint() {
        let source="struct S {x:u16} impl S { #[inline] fn get(&self)->u16{self.x} } fn main()->u16{ let s:S=S{x:7}; s.get() }";
        let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/rcc-inline-rustc/method");
        let checked = crate::frontend::rustc_check::RustcCheck::new(directory);
        let program = crate::frontend::compile_program_named_checked(
            "method.rs",
            source,
            &crate::RccOptions::default(),
            &checked,
            &mut |n| Err(format!("unexpected module {n}")),
        )
        .unwrap();
        let fs: HashMap<_, _> = program.funcs.into_iter().map(|f| (f.name, f)).collect();
        assert_eq!(
            fs.values()
                .filter(|f| f.inline_hint == InlineHint::Hint)
                .count(),
            1
        );
    }
    #[test]
    fn multiple_calls_keep_suffix_indices_and_single_return_defs() {
        let mut fs=functions("#[inline] fn add(x:u16)->u16{x+1} fn main()->u16{ let a=add(3); let b=add(a); b+add(2) }");
        let before = fs["add"].blocks.clone();
        let report = apply(&mut fs);
        assert_eq!(report.expanded_sites(), 3);
        assert!(calls(&fs["main"]).is_empty());
        assert_eq!(fs["add"].blocks, before);
        validate_inline_ir(&fs["main"]).unwrap();
    }
    #[test]
    fn loop_backedge_phi_and_multiple_returns_are_remapped() {
        let mut fs=functions("#[inline] fn choose(input:u16)->u16 { let mut x=input; while x<4 {x=x+1;} if x>9 {return x+2;} x+3 } fn main()->u16{choose(2)}");
        let report = apply(&mut fs);
        assert_eq!(report.expanded_sites(), 1, "{:?}", report.sites);
        validate_inline_ir(&fs["main"]).unwrap();
        assert!(calls(&fs["main"]).is_empty());
        assert!(fs["main"].blocks.iter().any(|b| !b.phis.is_empty()));
    }
    #[test]
    fn never_recursion_frames_and_nonleaf_are_not_expanded() {
        let mut fs=functions("#[inline(never)] fn a(x:u16)->u16{x} #[inline] fn rec(x:u16)->u16{rec(x)} #[inline] fn wrapped(x:u16)->u16{a(x)} fn main()->u16{a(rec(wrapped(1)))}");
        let r = apply(&mut fs);
        assert!(r.sites.iter().any(|s| s.decision == InlineDecision::Never));
        assert!(r
            .sites
            .iter()
            .any(|s| s.decision == InlineDecision::Recursive));
        assert!(r
            .sites
            .iter()
            .any(|s| s.decision == InlineDecision::NonLeaf));
        assert_eq!(r.expanded_sites(), 0);
        let mut fs = functions("#[inline] fn f(x:u16)->u16{x} fn main()->u16{f(2)}");
        fs.get_mut("f").unwrap().local_slots = 1;
        assert_eq!(
            apply(&mut fs).sites[0].decision,
            InlineDecision::SourceFrame
        );
    }
    #[test]
    fn no_return_is_skipped_and_no_empty_phi_created() {
        let mut fs = functions("#[inline] fn f(x:u16)->u16{x} fn main()->u16{f(2)}");
        fs.get_mut("f").unwrap().blocks[0].term = Some(Terminator::Halt { signal: 0 });
        let r = apply(&mut fs);
        assert_eq!(r.sites[0].decision, InlineDecision::NoReturn);
        assert!(fs["main"].blocks.iter().all(|b| b.phis.is_empty()));
    }
    #[test]
    fn cumulative_budget_is_not_refunded_and_pass_is_deterministic() {
        let source = "#[inline] fn f(x:u16)->u16{x+1} fn main()->u16{f(f(f(2)))}";
        let mut a = functions(source);
        let mut b = a.clone();
        let opts = InlineOptions {
            maximum_program_nodes: Some(6),
            ..InlineOptions::default()
        };
        let live = a.keys().copied().collect();
        let entries = HashSet::from(["main"]);
        let ra = inline_calls(&mut a, &live, &entries, &opts).unwrap();
        let rb = inline_calls(&mut b, &live, &entries, &opts).unwrap();
        assert_eq!(ra.expanded_sites(), 1);
        assert_eq!(ra.introduced_nodes, rb.introduced_nodes);
        assert_eq!(a["main"].blocks, b["main"].blocks);
        assert!(ra
            .sites
            .iter()
            .any(|s| s.decision == InlineDecision::ProgramBudget));
    }
    #[test]
    fn invalid_ir_is_an_error_not_a_skip() {
        let mut fs = functions("#[inline] fn f(x:u16)->u16{x+1} fn main()->u16{f(2)}");
        fs.get_mut("f").unwrap().blocks[0].term = Some(Terminator::Ret { values: vec![1000] });
        let live = fs.keys().copied().collect();
        assert!(inline_calls(
            &mut fs,
            &live,
            &HashSet::from(["main"]),
            &InlineOptions::default()
        )
        .is_err());
    }
}
