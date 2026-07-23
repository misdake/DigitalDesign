//! FuncBuilder: builds `IrFunc` (CFG + SSA) directly from structured control
//! flow, without any global state. SSA construction follows Braun et al.,
//! "Simple and Efficient Construction of Static Single Assignment Form"
//! (CC'13): frontend variables are versioned per block; reading a variable
//! whose value is unknown in a block inserts a phi (lazily, with incomplete
//! phis for not-yet-sealed blocks such as loop headers).

use crate::compiler::ir::*;
use crate::compiler::FuncName;
use crate::isa::Cond;
use std::collections::HashMap;

/// a frontend variable handle (DSL-level mutable variable)
pub type VarId = usize;

/// boolean condition expression, lowered to short-circuit branch cascades
#[derive(Clone, Debug, PartialEq)]
pub enum BoolExpr {
    Cmp(Cmp),
    And(Box<BoolExpr>, Box<BoolExpr>),
    Or(Box<BoolExpr>, Box<BoolExpr>),
    Not(Box<BoolExpr>),
}
impl BoolExpr {
    pub fn and(self, other: BoolExpr) -> BoolExpr {
        BoolExpr::And(Box::new(self), Box::new(other))
    }
    pub fn or(self, other: BoolExpr) -> BoolExpr {
        BoolExpr::Or(Box::new(self), Box::new(other))
    }
}
impl std::ops::Not for BoolExpr {
    type Output = BoolExpr;
    fn not(self) -> BoolExpr {
        BoolExpr::Not(Box::new(self))
    }
}

struct LoopCtx {
    header: BlockId,
    exit: BlockId,
}

pub struct FuncBuilder {
    func: IrFunc,
    sealed: Vec<bool>,
    /// current SSA value of each variable, per block
    var_defs: Vec<HashMap<BlockId, VReg>>,
    /// phis awaiting their operands, per unsealed block: (phi dst, variable)
    incomplete: HashMap<BlockId, Vec<(VReg, VarId)>>,
    current: Option<BlockId>,
    loops: Vec<LoopCtx>,
    /// source line recorded for subsequently emitted instructions
    line_hint: Option<u32>,
}

impl FuncBuilder {
    /// creates a builder with the entry block as current; returns the
    /// builder plus the parameter variables (already bound to ABI vregs)
    pub fn new(name: FuncName, n_params: usize, n_rets: usize) -> (Self, Vec<VarId>) {
        let entry = 0;
        let mut b = Self {
            func: IrFunc {
                name,
                params: (0..n_params as VReg).collect(),
                n_rets,
                blocks: vec![Block {
                    phis: vec![],
                    insts: vec![],
                    lines: vec![],
                    term: None,
                    term_line: None,
                    preds: vec![],
                }],
                entry,
                vreg_count: n_params as u32,
                param_names: vec![],
                ret_names: vec![],
                block_notes: vec![None],
                block_lines: vec![None],
                local_slots: 0,
            },
            sealed: vec![false],
            var_defs: vec![],
            incomplete: HashMap::new(),
            current: Some(entry),
            loops: vec![],
            line_hint: None,
        };
        let params = (0..n_params).map(|_| b.new_var()).collect::<Vec<_>>();
        for (i, &var) in params.iter().enumerate() {
            b.write_var(var, entry, i as VReg);
        }
        b.sealed[entry] = true;
        (b, params)
    }

    pub fn new_var(&mut self) -> VarId {
        self.var_defs.push(HashMap::new());
        self.var_defs.len() - 1
    }

    fn fresh_vreg(&mut self) -> VReg {
        let v = self.func.vreg_count;
        self.func.vreg_count += 1;
        v
    }

    fn cur(&self) -> BlockId {
        self.current
            .expect("no current block (already terminated?)")
    }

    fn push(&mut self, inst: Instr) {
        let b = self.cur();
        assert!(
            self.func.blocks[b].term.is_none(),
            "block b{b} already terminated"
        );
        self.func.blocks[b].insts.push(inst);
        self.func.blocks[b].lines.push(self.line_hint);
    }

    /// record this source line for subsequently emitted instructions
    pub fn set_line_hint(&mut self, line: u32) {
        self.line_hint = Some(line);
    }

    fn terminate(&mut self, term: Terminator) {
        let b = self.cur();
        assert!(
            self.func.blocks[b].term.is_none(),
            "block b{b} already terminated"
        );
        self.func.blocks[b].term = Some(term);
        self.func.blocks[b].term_line = self.line_hint;
        self.current = None;
    }

    fn new_block(&mut self, preds: &[BlockId]) -> BlockId {
        let id = self.func.blocks.len();
        self.func.blocks.push(Block {
            phis: vec![],
            insts: vec![],
            lines: vec![],
            term: None,
            term_line: None,
            preds: preds.to_vec(),
        });
        self.func.block_notes.push(None);
        self.func.block_lines.push(None);
        self.sealed.push(false);
        id
    }

    /// tag a block with a source line (shown in the disassembly listing)
    pub fn set_block_line(&mut self, b: BlockId, line: u32) {
        self.func.block_lines[b] = Some(line);
    }

    /// the entry block id
    pub fn entry_block(&self) -> BlockId {
        self.func.entry
    }

    /// attach source-level parameter/return names (shown in the listing)
    pub fn set_names(&mut self, params: &[&'static str], rets: &[&'static str]) {
        self.func.param_names = params.to_vec();
        self.func.ret_names = rets.to_vec();
    }

    /// allocate `n` frame-local slots (address-taken locals / local arrays),
    /// returning the base slot index
    pub fn alloc_local_slots(&mut self, n: u8) -> u8 {
        let base = self.func.local_slots;
        self.func.local_slots += n;
        base
    }

    pub fn load_local(&mut self, slot: u8) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::LoadLocal { dst, slot });
        dst
    }
    pub fn store_local(&mut self, slot: u8, src: VReg) {
        self.push(Instr::StoreLocal { slot, src });
    }
    /// dst = sp + slot (address of a frame-local variable)
    pub fn addr_of_local(&mut self, slot: u8) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::AddrOfLocal { dst, slot });
        dst
    }

    // ----- instruction emitters (SSA value producers) -----

    pub fn load_imm(&mut self, value: u16) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::LoadImm { dst, value });
        dst
    }
    pub fn bin(&mut self, op: BinOp, lhs: VReg, rhs: VReg) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::Bin { dst, op, lhs, rhs });
        dst
    }
    pub fn un(&mut self, op: UnOp, src: VReg) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::Un { dst, op, src });
        dst
    }
    pub fn shift(&mut self, op: ShiftOp, src: VReg, amount: u8) -> VReg {
        assert!(amount <= 15, "shift amount {amount} does not fit u4");
        let dst = self.fresh_vreg();
        self.push(Instr::Shift {
            dst,
            op,
            src,
            amount,
        });
        dst
    }
    pub fn mov(&mut self, src: VReg) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::Mov { dst, src });
        dst
    }
    pub fn load_mem(&mut self, base: VReg, offset: i16) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::LoadMem { dst, base, offset });
        dst
    }
    pub fn store_mem(&mut self, base: VReg, offset: i16, src: VReg) {
        self.push(Instr::StoreMem { base, offset, src });
    }
    pub fn call(&mut self, func: FuncName, args: &[VReg], n_rets: usize) -> Vec<VReg> {
        let rets = (0..n_rets).map(|_| self.fresh_vreg()).collect::<Vec<_>>();
        self.push(Instr::Call {
            func,
            args: args.to_vec(),
            rets: rets.clone(),
        });
        rets
    }
    /// load the address of a function (as a function pointer value)
    pub fn load_func_addr(&mut self, func: FuncName) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::LoadFuncAddr { dst, func });
        dst
    }
    /// indirect call through a function pointer
    pub fn call_ptr(&mut self, addr: VReg, args: &[VReg], n_rets: usize) -> Vec<VReg> {
        let rets = (0..n_rets).map(|_| self.fresh_vreg()).collect::<Vec<_>>();
        self.push(Instr::CallPtr {
            addr,
            args: args.to_vec(),
            rets: rets.clone(),
        });
        rets
    }
    pub fn dev_recv(&mut self, device: u8, channel: u8) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::DevRecv {
            dst,
            device,
            channel,
        });
        dst
    }
    pub fn dev_send(&mut self, device: u8, channel: u8, src: VReg) {
        self.push(Instr::DevSend {
            device,
            channel,
            src,
        });
    }

    // ----- variables (versioned SSA views) -----

    /// assign `value` to `var` in the current block
    pub fn set(&mut self, var: VarId, value: VReg) {
        let b = self.cur();
        self.write_var(var, b, value);
    }
    /// read the current SSA value of `var` in the current block
    pub fn get(&mut self, var: VarId) -> VReg {
        let b = self.cur();
        self.read_var(var, b)
    }

    fn write_var(&mut self, var: VarId, block: BlockId, value: VReg) {
        self.var_defs[var].insert(block, value);
    }

    fn read_var(&mut self, var: VarId, block: BlockId) -> VReg {
        if let Some(&v) = self.var_defs[var].get(&block) {
            return v;
        }
        if !self.sealed[block] {
            // block not sealed (loop header): phi with operands filled at seal time
            let dst = self.fresh_vreg();
            self.func.blocks[block].phis.push(Phi { dst, args: vec![] });
            self.incomplete.entry(block).or_default().push((dst, var));
            self.write_var(var, block, dst);
            return dst;
        }
        match self.func.blocks[block].preds.as_slice() {
            [] => panic!(
                "use of undefined variable {var} in function {}",
                self.func.name
            ),
            [pred] => {
                let pred = *pred;
                let v = self.read_var(var, pred);
                self.write_var(var, block, v);
                v
            }
            preds => {
                let preds = preds.to_vec();
                let dst = self.fresh_vreg();
                // write before recursing, to break cycles through this phi
                self.write_var(var, block, dst);
                let args = preds.iter().map(|&p| (p, self.read_var(var, p))).collect();
                self.func.blocks[block].phis.push(Phi { dst, args });
                dst
            }
        }
    }

    /// all predecessors of `block` are known: fill incomplete phis
    fn seal(&mut self, block: BlockId) {
        assert!(!self.sealed[block], "block b{block} sealed twice");
        if let Some(list) = self.incomplete.remove(&block) {
            for (dst, var) in list {
                let preds = self.func.blocks[block].preds.clone();
                let args = preds.iter().map(|&p| (p, self.read_var(var, p))).collect();
                let phi = self.func.blocks[block]
                    .phis
                    .iter_mut()
                    .find(|p| p.dst == dst)
                    .unwrap();
                phi.args = args;
            }
        }
        self.sealed[block] = true;
    }

    // ----- control flow -----

    pub fn if_then(&mut self, cmp: Cmp, f: impl FnOnce(&mut Self)) {
        self.if_bool(BoolExpr::Cmp(cmp), f);
    }

    pub fn if_else(
        &mut self,
        cmp: Cmp,
        then_f: impl FnOnce(&mut Self),
        else_f: impl FnOnce(&mut Self),
    ) {
        self.if_else_bool(BoolExpr::Cmp(cmp), then_f, else_f);
    }

    /// while loop with the condition evaluated at the loop header.
    /// `cond` runs in the (unsealed) header block, so variables it reads
    /// become loop-carried phis automatically.
    pub fn while_loop(
        &mut self,
        cond: impl FnOnce(&mut Self) -> Cmp,
        body: impl FnOnce(&mut Self),
    ) {
        self.while_bool(|b| BoolExpr::Cmp(cond(b)), body);
    }

    pub fn break_(&mut self) {
        let exit = self.loops.last().expect("break outside of loop").exit;
        let cur = self.cur();
        self.func.blocks[exit].preds.push(cur);
        self.terminate(Terminator::Jmp { target: exit });
    }

    pub fn continue_(&mut self) {
        let header = self.loops.last().expect("continue outside of loop").header;
        let cur = self.cur();
        self.func.blocks[header].preds.push(cur);
        self.terminate(Terminator::Jmp { target: header });
    }

    pub fn ret(&mut self, values: &[VReg]) {
        assert_eq!(
            values.len(),
            self.func.n_rets,
            "function {} expects {} return values, got {}",
            self.func.name,
            self.func.n_rets,
            values.len()
        );
        self.terminate(Terminator::Ret {
            values: values.to_vec(),
        });
    }

    pub fn halt(&mut self, signal: VReg) {
        self.terminate(Terminator::Halt { signal });
    }

    // ----- boolean condition combinators (short-circuit) -----

    /// lower a boolean expression into branches from the current block to
    /// `t` (cond holds) and `f` (cond does not hold); records preds on both.
    /// the current block must be unterminated; afterwards there is no
    /// current block.
    fn lower_cond(&mut self, cond: BoolExpr, t: BlockId, f: BlockId) {
        match cond {
            BoolExpr::Cmp(cmp) => {
                let cur = self.cur();
                self.func.blocks[t].preds.push(cur);
                self.func.blocks[f].preds.push(cur);
                self.terminate(Terminator::Br {
                    cmp,
                    if_true: t,
                    if_false: f,
                });
            }
            BoolExpr::And(a, b) => {
                let m = self.new_block(&[]);
                self.lower_cond(*a, m, f);
                self.current = Some(m);
                self.seal(m);
                self.lower_cond(*b, t, f);
            }
            BoolExpr::Or(a, b) => {
                let m = self.new_block(&[]);
                self.lower_cond(*a, t, m);
                self.current = Some(m);
                self.seal(m);
                self.lower_cond(*b, t, f);
            }
            BoolExpr::Not(a) => self.lower_cond(*a, f, t),
        }
    }

    // ----- structured control flow primitives (begin/end) -----
    //
    // these exist so a DSL layer can drive block construction step by step
    // (e.g. when the builder is behind a RefCell and closures cannot be
    // nested inside a single borrow). prefer the convenience wrappers
    // (if_bool/if_else_bool/while_bool) when driving FuncBuilder directly.

    /// begin an if-then: returns the join block; current = then block
    pub fn begin_if(&mut self, cond: BoolExpr) -> BlockId {
        let then_b = self.new_block(&[]);
        let join = self.new_block(&[]);
        self.func.block_notes[then_b] = Some("then");
        self.func.block_notes[join] = Some("if-end");
        self.lower_cond(cond, then_b, join);
        self.current = Some(then_b);
        self.seal(then_b);
        join
    }
    /// finish an if-then: wires the fall-through to the join
    pub fn end_if(&mut self, join: BlockId) {
        if let Some(end) = self.current {
            self.func.blocks[join].preds.push(end);
            self.terminate(Terminator::Jmp { target: join });
        }
        self.seal(join);
        self.current = Some(join);
    }

    /// begin an if-else: returns (else block, join block); current = then block
    pub fn begin_if_else(&mut self, cond: BoolExpr) -> (BlockId, BlockId) {
        let then_b = self.new_block(&[]);
        let else_b = self.new_block(&[]);
        let join = self.new_block(&[]);
        self.func.block_notes[then_b] = Some("then");
        self.func.block_notes[else_b] = Some("else");
        self.func.block_notes[join] = Some("if-end");
        self.lower_cond(cond, then_b, else_b);
        self.current = Some(then_b);
        self.seal(then_b);
        (else_b, join)
    }
    /// switch from then branch to else branch
    pub fn mid_if_else(&mut self, else_b: BlockId, join: BlockId) {
        if let Some(end) = self.current {
            self.func.blocks[join].preds.push(end);
            self.terminate(Terminator::Jmp { target: join });
        }
        self.current = Some(else_b);
        self.seal(else_b);
    }
    /// finish an if-else
    pub fn end_if_else(&mut self, join: BlockId) {
        if let Some(end) = self.current {
            self.func.blocks[join].preds.push(end);
            self.terminate(Terminator::Jmp { target: join });
        }
        self.seal(join);
        self.current = if self.func.blocks[join].preds.is_empty() {
            None
        } else {
            Some(join)
        };
    }

    /// begin a while loop: returns (header, body, exit); current = header
    pub fn begin_while(&mut self) -> (BlockId, BlockId, BlockId) {
        let cur = self.cur();
        let header = self.new_block(&[cur]);
        let body_b = self.new_block(&[]);
        let exit = self.new_block(&[]);
        self.func.block_notes[header] = Some("loop header");
        self.func.block_notes[body_b] = Some("loop body");
        self.func.block_notes[exit] = Some("loop end");
        self.terminate(Terminator::Jmp { target: header });
        self.current = Some(header);
        (header, body_b, exit)
    }

    /// create an unsealed block with the given preds (raw API for condition
    /// cascades and custom loop shapes)
    pub fn raw_block(&mut self, preds: &[BlockId]) -> BlockId {
        self.new_block(preds)
    }
    /// set the current block to `b` and seal it (raw API)
    pub fn enter_block(&mut self, b: BlockId) {
        self.current = Some(b);
        self.seal(b);
    }
    /// terminate the current block with a conditional branch, recording preds
    pub fn br(&mut self, cmp: Cmp, if_true: BlockId, if_false: BlockId) {
        let cur = self.cur();
        self.func.blocks[if_true].preds.push(cur);
        self.func.blocks[if_false].preds.push(cur);
        self.terminate(Terminator::Br {
            cmp,
            if_true,
            if_false,
        });
    }
    /// terminate the current block with an unconditional jump
    pub fn jmp(&mut self, target: BlockId) {
        let cur = self.cur();
        self.func.blocks[target].preds.push(cur);
        self.terminate(Terminator::Jmp { target });
    }

    /// push a loop context and enter the body block (sealing it)
    pub fn begin_loop_body(&mut self, header: BlockId, body_b: BlockId, exit: BlockId) {
        self.loops.push(LoopCtx { header, exit });
        self.current = Some(body_b);
        self.seal(body_b);
    }

    /// redirect the innermost loop's continue target to a fresh block
    /// (for-loop increment blocks); returns the new block, unsealed
    pub fn begin_continue_block(&mut self) -> BlockId {
        let incr = self.new_block(&[]);
        self.loops
            .last_mut()
            .expect("continue block outside of loop")
            .header = incr;
        incr
    }
    /// finish a continue block: wire the body's fall-through to it, seal it,
    /// and make it the current block (the caller emits the increment there
    /// and ends with the usual back edge via `end_while`)
    pub fn end_continue_block(&mut self, incr: BlockId) {
        if let Some(end) = self.current {
            self.func.blocks[incr].preds.push(end);
            self.terminate(Terminator::Jmp { target: incr });
        }
        self.seal(incr);
        self.current = Some(incr);
    }

    /// evaluate at the header: branch on `cond` into body/exit; current = body
    pub fn while_cond(&mut self, cond: BoolExpr, header: BlockId, body_b: BlockId, exit: BlockId) {
        self.lower_cond(cond, body_b, exit);
        self.loops.push(LoopCtx { header, exit });
        self.current = Some(body_b);
        self.seal(body_b);
    }
    /// finish a while loop (back edge, seal header/exit); current = exit
    pub fn end_while(&mut self, header: BlockId, exit: BlockId) {
        if let Some(end) = self.current {
            self.func.blocks[header].preds.push(end);
            self.terminate(Terminator::Jmp { target: header });
        }
        self.loops.pop();
        self.seal(header);
        self.seal(exit);
        self.current = Some(exit);
    }

    // ----- structured control flow convenience wrappers -----

    pub fn if_bool(&mut self, cond: BoolExpr, f: impl FnOnce(&mut Self)) {
        let join = self.begin_if(cond);
        f(self);
        self.end_if(join);
    }

    pub fn if_else_bool(
        &mut self,
        cond: BoolExpr,
        then_f: impl FnOnce(&mut Self),
        else_f: impl FnOnce(&mut Self),
    ) {
        let (else_b, join) = self.begin_if_else(cond);
        then_f(self);
        self.mid_if_else(else_b, join);
        else_f(self);
        self.end_if_else(join);
    }

    /// while loop with a (possibly compound) condition evaluated at the header
    pub fn while_bool(
        &mut self,
        cond: impl FnOnce(&mut Self) -> BoolExpr,
        body: impl FnOnce(&mut Self),
    ) {
        let (header, body_b, exit) = self.begin_while();
        let cond = cond(self);
        self.while_cond(cond, header, body_b, exit);
        body(self);
        self.end_while(header, exit);
    }

    // ----- comparisons -----

    pub fn cmp(&self, lhs: VReg, rhs: CmpRhs, cond: Cond) -> Cmp {
        Cmp {
            lhs,
            rhs,
            cond,
            signed: false,
        }
    }
    pub fn cmp_signed(&self, lhs: VReg, rhs: CmpRhs, cond: Cond) -> Cmp {
        Cmp {
            lhs,
            rhs,
            cond,
            signed: true,
        }
    }

    // ----- finish -----

    pub fn finish(mut self) -> IrFunc {
        for (i, b) in self.func.blocks.iter().enumerate() {
            let reachable = i == self.func.entry || !b.preds.is_empty();
            assert!(
                b.term.is_some() || !reachable,
                "block b{i} of function {} is not terminated",
                self.func.name
            );
        }
        remove_trivial_phis(&mut self.func);
        self.func
    }
}

/// replace phis whose operands are all the same value (ignoring
/// self-references) with that value, iteratively. returns whether any phi
/// was removed.
pub(crate) fn remove_trivial_phis(func: &mut IrFunc) -> bool {
    let mut any = false;
    loop {
        let mut replace: HashMap<VReg, VReg> = HashMap::new();
        for b in &func.blocks {
            for phi in &b.phis {
                let mut operands = phi.args.iter().map(|&(_, v)| v).filter(|&v| v != phi.dst);
                if let Some(first) = operands.next() {
                    if operands.all(|v| v == first) {
                        replace.insert(phi.dst, first);
                    }
                }
            }
        }
        if replace.is_empty() {
            break;
        }
        any = true;
        let subst = |v: &mut VReg| {
            while let Some(&r) = replace.get(v) {
                *v = r;
            }
        };
        for b in &mut func.blocks {
            b.phis.retain(|p| !replace.contains_key(&p.dst));
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
                    Instr::LoadImm { .. }
                    | Instr::StoreStatic { .. }
                    | Instr::DevRecv { .. }
                    | Instr::LoadSp { .. }
                    | Instr::LoadLocal { .. }
                    | Instr::AddrOfLocal { .. } => {}
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
                    Instr::DevSend { src, .. } => subst(src),
                    Instr::StoreSp { src, .. } | Instr::StoreLocal { src, .. } => subst(src),
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
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_add() {
        let (mut b, params) = FuncBuilder::new("add", 2, 1);
        let [a, x] = [params[0], params[1]];
        let (a, x) = (b.get(a), b.get(x));
        let s = b.bin(BinOp::Add, a, x);
        b.ret(&[s]);
        let f = b.finish();
        let expected = "\
fn add params=(v0, v1) rets=1
b0: ; preds=[]
  v2 = add v0, v1
  ret [v2]
";
        assert_eq!(f.to_string(), expected);
    }

    #[test]
    fn test_build_if_else_phi() {
        let (mut b, params) = FuncBuilder::new("f", 1, 1);
        let c = params[0];
        let x = b.new_var();
        let ten = b.load_imm(10);
        b.set(x, ten);
        let c0 = b.get(c);
        let cmp = b.cmp(c0, CmpRhs::Imm(0), Cond::NotEqual);
        b.if_else(
            cmp,
            |b| {
                let one = b.load_imm(1);
                b.set(x, one);
            },
            |b| {
                let two = b.load_imm(2);
                b.set(x, two);
            },
        );
        let v = b.get(x);
        b.ret(&[v]);
        let f = b.finish();
        let expected = "\
fn f params=(v0) rets=1
b0: ; preds=[]
  v1 = imm 10
  br v0 != 0 -> b1, b2
b1: ; preds=[b0]
  v2 = imm 1
  jmp b3
b2: ; preds=[b0]
  v3 = imm 2
  jmp b3
b3: ; preds=[b1, b2]
  v4 = phi [(b1, v2), (b2, v3)]
  ret [v4]
";
        assert_eq!(f.to_string(), expected);
    }

    #[test]
    fn test_build_while_loop_phi() {
        let (mut b, params) = FuncBuilder::new("sum", 1, 1);
        let n = params[0];
        let sum = b.new_var();
        let i = b.new_var();
        let zero = b.load_imm(0);
        b.set(sum, zero);
        let one = b.load_imm(1);
        b.set(i, one);
        b.while_loop(
            |b| {
                let i = b.get(i);
                let n = b.get(n);
                b.cmp(i, CmpRhs::Reg(n), Cond::LessEqual)
            },
            |b| {
                let s = b.get(sum);
                let i0 = b.get(i);
                let s = b.bin(BinOp::Add, s, i0);
                b.set(sum, s);
                let one = b.load_imm(1);
                let i1 = b.bin(BinOp::Add, i0, one);
                b.set(i, i1);
            },
        );
        let s = b.get(sum);
        b.ret(&[s]);
        let f = b.finish();
        let text = f.to_string();
        // header has exactly two phis (i and sum); the phi for loop-invariant
        // n is trivial and must be removed by cleanup
        assert_eq!(f.blocks[1].phis.len(), 2);
        assert!(text.contains("v3 = phi [(b0, v2), (b2, v8)]"), "{text}");
        assert!(text.contains("v5 = phi [(b0, v1), (b2, v6)]"), "{text}");
        assert!(text.contains("br v3 <= v0 -> b2, b3"), "{text}");
        assert!(text.contains("ret [v5]"), "{text}");
    }

    #[test]
    fn test_build_break_continue() {
        let (mut b, params) = FuncBuilder::new("g", 1, 1);
        let n = params[0];
        let i = b.new_var();
        let zero = b.load_imm(0);
        b.set(i, zero);
        b.while_loop(
            |b| {
                let i = b.get(i);
                b.cmp(i, CmpRhs::Imm(10), Cond::Less)
            },
            |b| {
                let i0 = b.get(i);
                let one = b.load_imm(1);
                let i1 = b.bin(BinOp::Add, i0, one);
                b.set(i, i1);
                // if (i == 3) continue;
                let i1c = b.get(i);
                let cmp = b.cmp(i1c, CmpRhs::Imm(3), Cond::Equal);
                b.if_then(cmp, |b| b.continue_());
                // if (i == 8) break;
                let i1b = b.get(i);
                let cmp = b.cmp(i1b, CmpRhs::Imm(8), Cond::Equal);
                b.if_then(cmp, |b| b.break_());
                let n = b.get(n);
                let i2 = b.get(i);
                let s = b.bin(BinOp::Add, i2, n);
                b.set(i, s);
            },
        );
        let v = b.get(i);
        b.ret(&[v]);
        let f = b.finish();
        // header preds: preheader, continue-edge, fall-through-body-end
        let header = 1;
        assert_eq!(f.blocks[header].preds.len(), 3);
        // exit preds: header-false + break edge
        let exit = 3;
        assert_eq!(f.blocks[exit].preds.len(), 2);
        // every reachable block terminated
        for (i, blk) in f.blocks.iter().enumerate() {
            if i == f.entry || !blk.preds.is_empty() {
                assert!(blk.term.is_some(), "b{i}");
            }
        }
    }
}
