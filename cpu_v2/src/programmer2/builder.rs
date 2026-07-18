//! FuncBuilder: builds `IrFunc` (CFG + SSA) directly from structured control
//! flow, without any global state. SSA construction follows Braun et al.,
//! "Simple and Efficient Construction of Static Single Assignment Form"
//! (CC'13): frontend variables are versioned per block; reading a variable
//! whose value is unknown in a block inserts a phi (lazily, with incomplete
//! phis for not-yet-sealed blocks such as loop headers).

use crate::isa::Cond;
use crate::programmer::FuncName;
use crate::programmer2::ir::*;
use std::collections::HashMap;

/// a frontend variable handle (DSL-level mutable variable)
pub type VarId = usize;

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
                    term: None,
                    preds: vec![],
                }],
                entry,
                vreg_count: n_params as u32,
            },
            sealed: vec![false],
            var_defs: vec![],
            incomplete: HashMap::new(),
            current: Some(entry),
            loops: vec![],
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
        self.current.expect("no current block (already terminated?)")
    }

    fn push(&mut self, inst: Instr) {
        let b = self.cur();
        assert!(self.func.blocks[b].term.is_none(), "block b{b} already terminated");
        self.func.blocks[b].insts.push(inst);
    }

    fn terminate(&mut self, term: Terminator) {
        let b = self.cur();
        assert!(self.func.blocks[b].term.is_none(), "block b{b} already terminated");
        self.func.blocks[b].term = Some(term);
        self.current = None;
    }

    fn new_block(&mut self, preds: &[BlockId]) -> BlockId {
        let id = self.func.blocks.len();
        self.func.blocks.push(Block {
            phis: vec![],
            insts: vec![],
            term: None,
            preds: preds.to_vec(),
        });
        self.sealed.push(false);
        id
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
        self.push(Instr::Shift { dst, op, src, amount });
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
    pub fn dev_recv(&mut self, device: u8, channel: u8) -> VReg {
        let dst = self.fresh_vreg();
        self.push(Instr::DevRecv { dst, device, channel });
        dst
    }
    pub fn dev_send(&mut self, device: u8, channel: u8, src: VReg) {
        self.push(Instr::DevSend { device, channel, src });
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
            [] => panic!("use of undefined variable {var} in function {}", self.func.name),
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
        let cur = self.cur();
        let then_b = self.new_block(&[cur]);
        let join = self.new_block(&[cur]);
        self.terminate(Terminator::Br {
            cmp,
            if_true: then_b,
            if_false: join,
        });
        self.current = Some(then_b);
        self.seal(then_b);
        f(self);
        if let Some(end) = self.current {
            self.func.blocks[join].preds.push(end);
            self.terminate(Terminator::Jmp { target: join });
        }
        self.seal(join);
        self.current = Some(join);
    }

    pub fn if_else(
        &mut self,
        cmp: Cmp,
        then_f: impl FnOnce(&mut Self),
        else_f: impl FnOnce(&mut Self),
    ) {
        let cur = self.cur();
        let then_b = self.new_block(&[cur]);
        let else_b = self.new_block(&[cur]);
        let join = self.new_block(&[]);
        self.terminate(Terminator::Br {
            cmp,
            if_true: then_b,
            if_false: else_b,
        });

        self.current = Some(then_b);
        self.seal(then_b);
        then_f(self);
        if let Some(end) = self.current {
            self.func.blocks[join].preds.push(end);
            self.terminate(Terminator::Jmp { target: join });
        }

        self.current = Some(else_b);
        self.seal(else_b);
        else_f(self);
        if let Some(end) = self.current {
            self.func.blocks[join].preds.push(end);
            self.terminate(Terminator::Jmp { target: join });
        }

        self.seal(join);
        // both branches terminated: join is unreachable, no current block
        self.current = if self.func.blocks[join].preds.is_empty() {
            None
        } else {
            Some(join)
        };
    }

    /// while loop with the condition evaluated at the loop header.
    /// `cond` runs in the (unsealed) header block, so variables it reads
    /// become loop-carried phis automatically.
    pub fn while_loop(&mut self, cond: impl FnOnce(&mut Self) -> Cmp, body: impl FnOnce(&mut Self)) {
        let cur = self.cur();
        let header = self.new_block(&[cur]);
        let body_b = self.new_block(&[header]);
        let exit = self.new_block(&[header]);
        self.terminate(Terminator::Jmp { target: header });

        self.current = Some(header);
        let cmp = cond(self);
        self.terminate(Terminator::Br {
            cmp,
            if_true: body_b,
            if_false: exit,
        });

        self.loops.push(LoopCtx { header, exit });
        self.current = Some(body_b);
        self.seal(body_b);
        body(self);
        if let Some(end) = self.current {
            self.func.blocks[header].preds.push(end);
            self.terminate(Terminator::Jmp { target: header });
        }
        self.loops.pop();

        self.seal(header);
        self.seal(exit);
        self.current = Some(exit);
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
/// self-references) with that value, iteratively.
fn remove_trivial_phis(func: &mut IrFunc) {
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
                    Instr::LoadImm { .. } | Instr::DevRecv { .. } => {}
                    Instr::LoadMem { base, .. } => subst(base),
                    Instr::StoreMem { base, src, .. } => {
                        subst(base);
                        subst(src);
                    }
                    Instr::Call { args, .. } => args.iter_mut().for_each(|a| subst(a)),
                    Instr::DevSend { src, .. } => subst(src),
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
                    Terminator::Ret { values } => values.iter_mut().for_each(|v| subst(v)),
                    Terminator::Halt { signal } => subst(signal),
                }
            }
        }
    }
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
