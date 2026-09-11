//! SSA-based CFG IR for the new compiler pipeline (compiler).
//!
//! A function is a CFG of basic blocks. Each block holds a list of phi nodes,
//! a list of instructions, and a terminator. Values are SSA virtual registers
//! (`VReg`), produced at most once. Branch conditions are always a single
//! comparison (`Cmp`) matching the ISA's cmp + j_cc model.

use crate::FuncName;
use std::fmt;

pub type VReg = u32;
pub type BlockId = usize;

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum CompareOp {
    Never = 0,
    Greater = 1,
    Equal = 2,
    Less = 4,
    GreaterEqual = 3,
    NotEqual = 5,
    LessEqual = 6,
    Always = 7,
}

impl CompareOp {
    pub fn invert(self) -> Self {
        match self {
            Self::Never => Self::Always,
            Self::Greater => Self::LessEqual,
            Self::Equal => Self::NotEqual,
            Self::Less => Self::GreaterEqual,
            Self::GreaterEqual => Self::Less,
            Self::NotEqual => Self::Equal,
            Self::LessEqual => Self::Greater,
            Self::Always => Self::Never,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    And,
    Or,
    Xor,
}

/// right-hand side of an integer operation: a register or a literal immediate
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum IntOperand {
    Reg(VReg),
    Imm(u16),
}

/// post-multiply window kept from the full unsigned 32-bit product
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum MulWindow {
    /// bits [15:0] (MUL0 / MULI)
    Low,
    /// bits [23:8] (MUL8)
    Shift8,
    /// bits [31:16] (MUL16)
    Shift16,
}

/// architectural special register read by `Instr::Mfsr`
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum SpecialReg {
    Cseg,
    Dseg,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum UnOp {
    Inv,
    Neg,
    Not0,
    Cnt1,
    Log2,
    /// sign-extend the low byte (CpuV3 SEXTB; CpuV2 expands through shifts)
    Sextb,
    /// count leading zeros (CpuV3 CLZ; rejected for CpuV2)
    Clz,
}

/// shifts are destructive two-operand forms on the ISA (`rd = rd << n`),
/// so codegen inserts a mov when the SSA destination differs from the source
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ShiftOp {
    Lsl,
    Lsr,
    Asr,
}

/// register file a virtual register belongs to
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum RegClass {
    Gpr,
    /// CpuV3 FPU: one 4-lane fix16 vector register per value
    Fpu,
}

/// per-lane fixed-point binary operations (FADD/FSUB/FMUL)
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum FBinOp {
    Add,
    Sub,
    Mul,
}

/// fixed-point unary operations (FUNARY)
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum FUnOp {
    /// lane x only; domain fault on zero
    Rcp,
    /// lane x only; domain fault on non-positive input
    Rsqrt,
    /// dst = [sin, cos, 0, 0]
    SinCos,
    Abs,
    Neg,
    Floor,
    Ceil,
    Round,
    Sat01,
    Sign,
}

/// right-hand side of a comparison; immediates are legalized (u4/i4) in codegen
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CmpRhs {
    Reg(VReg),
    Imm(u16),
}

/// semantics: `lhs cond rhs`
#[derive(Copy, Clone, Debug)]
pub struct Cmp {
    pub lhs: VReg,
    pub rhs: CmpRhs,
    pub cond: CompareOp,
    pub signed: bool,
}

impl PartialEq for Cmp {
    fn eq(&self, other: &Self) -> bool {
        self.lhs == other.lhs
            && self.rhs == other.rhs
            && self.cond as u8 == other.cond as u8
            && self.signed == other.signed
    }
}
impl Eq for Cmp {}

#[derive(Clone, Debug, PartialEq)]
pub enum Instr {
    Bin {
        dst: VReg,
        op: BinOp,
        lhs: VReg,
        rhs: IntOperand,
    },
    /// unsigned multiply keeping one 16-bit window of the 32-bit product;
    /// CpuV3 lowers to MUL0/MUL8/MUL16 (or MULI for a Low immediate), CpuV2
    /// rewrites a Low register product to the rcc_std mul_16x16 library call
    Mul {
        dst: VReg,
        window: MulWindow,
        lhs: VReg,
        rhs: IntOperand,
    },
    Un {
        dst: VReg,
        op: UnOp,
        src: VReg,
    },
    /// destructive two-operand shift; a register amount is masked to the low
    /// four bits by the hardware (`rs & 15`)
    Shift {
        dst: VReg,
        op: ShiftOp,
        src: VReg,
        amount: IntOperand,
    },
    /// CpuV3 SIGNAL with a nonzero type (1..=15): a simulator-side event that
    /// retires as a NOP in hardware. This is an observable compiler barrier:
    /// it is never deleted or merged, and no instruction or memory operation
    /// moves across it. The host (CpuV2) implementation is a NOP.
    Signal {
        signal_type: u8,
        value: VReg,
    },
    /// CpuV3-only: read an architectural special register (MFSR)
    Mfsr {
        dst: VReg,
        sr: SpecialReg,
    },
    /// dst = (cmp.lhs cond cmp.rhs) as 0 or 1 (Boolean-producing comparison;
    /// only generated by the diamond-conversion pass, CpuV3 lowering)
    Bool {
        dst: VReg,
        cmp: Cmp,
    },
    /// dst = src when `cmp` holds, otherwise dst keeps its incoming value
    /// (conditional move; only generated by the diamond-conversion pass,
    /// CpuV3 lowering). Consumes the pending test like a conditional branch.
    CMov {
        dst: VReg,
        cmp: Cmp,
        src: VReg,
    },
    Mov {
        dst: VReg,
        src: VReg,
    },
    LoadImm {
        dst: VReg,
        value: u16,
    },
    LoadMem {
        dst: VReg,
        base: VReg,
        offset: i16,
    },
    StoreMem {
        base: VReg,
        offset: i16,
        src: VReg,
    },
    /// Compiler-generated static data initialization. Codegen groups these by
    /// 256-word page and uses sp + u8 offset stores.
    StoreStatic {
        addr: u16,
        value: u16,
    },
    /// rets are SSA defs (filled by the callee per the calling convention)
    Call {
        func: FuncName,
        args: Vec<VReg>,
        rets: Vec<VReg>,
    },
    /// load the absolute address of a function (relocation slot)
    LoadFuncAddr {
        dst: VReg,
        func: FuncName,
    },
    /// indirect call through a function pointer (Harvard: distinct from data ptr)
    CallPtr {
        addr: VReg,
        args: Vec<VReg>,
        rets: Vec<VReg>,
    },
    DevRecv {
        dst: VReg,
        device: u8,
        channel: u8,
    },
    DevSend {
        device: u8,
        channel: u8,
        src: VReg,
    },
    /// CpuV3-only: blocking clean-plus-invalidate of the complete write-back
    /// data cache. This is a compiler memory and control barrier.
    DcacheInvalidateAll,
    /// CpuV3-only: write the DSEG special register (MTSR DSEG)
    MtsrDseg {
        src: VReg,
    },
    /// CpuV3-only: atomically switch CSEG and jump (JSEG); never returns
    Jseg {
        cseg: VReg,
        target: VReg,
    },
    /// frame slot access (register allocator spills only; offset is a frame
    /// slot index, resolved to load_sp/store_sp in codegen)
    LoadSp {
        dst: VReg,
        slot: u8,
    },
    StoreSp {
        slot: u8,
        src: VReg,
    },
    /// frame-local slot access for address-taken locals and local arrays;
    /// slots are assigned by the frontend (distinct from spill slots)
    LoadLocal {
        dst: VReg,
        slot: u8,
    },
    StoreLocal {
        slot: u8,
        src: VReg,
    },
    /// dst = sp + slot (address of a frame-local variable)
    AddrOfLocal {
        dst: VReg,
        slot: u8,
    },
    // ----- CpuV3 FPU instructions (Fpu-class vregs unless noted) -----
    /// per-lane fixed-point arithmetic: dst = lhs op rhs (FMULS: lane i of
    /// dst = lane i of lhs times lane x of rhs)
    FBin {
        dst: VReg,
        op: FBinOp,
        lhs: VReg,
        rhs: VReg,
    },
    FMov {
        dst: VReg,
        src: VReg,
    },
    /// GPR to FPU lane-x bridge: dst = [src_gpr, 0, 0, 0] (FLOAD)
    FLoad {
        dst: VReg,
        src_gpr: VReg,
    },
    /// FPU to GPR lane-x bridge: dst_gpr = src lane x (FSTORE)
    FStore {
        dst_gpr: VReg,
        src: VReg,
    },
    /// dst = four words at {DSEG, base_gpr} (FIMPORT4; base must be 4-aligned)
    FImport4 {
        dst: VReg,
        base_gpr: VReg,
    },
    /// four words at {DSEG, base_gpr} = src lanes (FEXPORT4; 4-aligned base)
    FExport4 {
        src: VReg,
        base_gpr: VReg,
    },
    /// dst = op(src) (FUNARY; Rcp/Rsqrt can raise a domain fault)
    FUnary {
        dst: VReg,
        op: FUnOp,
        src: VReg,
    },
    /// ACC += dot4(lhs, rhs) (FDOT4ACC; touches the ACC machine state)
    FDot4Acc {
        lhs: VReg,
        rhs: VReg,
    },
    /// dst lanes selected by `mask` = round(ACC); ACC = 0 (FACCSTORE)
    FAccStore {
        dst: VReg,
        mask: u8,
    },
    /// ACC = sign_extend(src lane `lane`) << 8 (FACCLOAD.*; ACC machine state)
    FAccLoad {
        src: VReg,
        lane: u8,
    },
    /// dst = [0, 0, 0, 0] (FUNARY ZERO)
    FZero {
        dst: VReg,
    },
    /// dst (Gpr) = 4-aligned address of FPU spill frame slot `slot`
    /// (register allocator spills only; resolved in codegen)
    AddrOfFpuSpill {
        dst: VReg,
        slot: u8,
    },
}

impl Instr {
    /// calls `f` on every vreg this instruction reads (uses, never defs)
    pub fn for_each_use(&self, f: &mut impl FnMut(VReg)) {
        match self {
            Instr::Bin { lhs, rhs, .. } => {
                f(*lhs);
                rhs.for_each_reg(f);
            }
            Instr::Mul { lhs, rhs, .. } => {
                f(*lhs);
                rhs.for_each_reg(f);
            }
            Instr::Un { src, .. } | Instr::Mov { src, .. } => f(*src),
            Instr::Shift { src, amount, .. } => {
                f(*src);
                amount.for_each_reg(f);
            }
            Instr::Signal { value, .. } => f(*value),
            Instr::Bool { cmp, .. } => cmp.for_each_use(f),
            Instr::CMov { cmp, src, .. } => {
                cmp.for_each_use(f);
                f(*src);
            }
            Instr::LoadMem { base, .. } => f(*base),
            Instr::StoreMem { base, src, .. } => {
                f(*base);
                f(*src);
            }
            Instr::Call { args, .. } => args.iter().copied().for_each(f),
            Instr::CallPtr { addr, args, .. } => {
                f(*addr);
                args.iter().copied().for_each(f);
            }
            Instr::DevSend { src, .. }
            | Instr::MtsrDseg { src }
            | Instr::StoreSp { src, .. }
            | Instr::StoreLocal { src, .. } => f(*src),
            Instr::Jseg { cseg, target } => {
                f(*cseg);
                f(*target);
            }
            Instr::FBin { lhs, rhs, .. } | Instr::FDot4Acc { lhs, rhs } => {
                f(*lhs);
                f(*rhs);
            }
            Instr::FAccLoad { src, .. } => f(*src),
            Instr::FMov { src, .. } | Instr::FUnary { src, .. } | Instr::FStore { src, .. } => {
                f(*src)
            }
            Instr::FLoad { src_gpr, .. }
            | Instr::FImport4 {
                base_gpr: src_gpr, ..
            } => f(*src_gpr),
            Instr::FExport4 { src, base_gpr } => {
                f(*src);
                f(*base_gpr);
            }
            Instr::LoadImm { .. }
            | Instr::StoreStatic { .. }
            | Instr::DevRecv { .. }
            | Instr::DcacheInvalidateAll
            | Instr::LoadSp { .. }
            | Instr::LoadLocal { .. }
            | Instr::AddrOfLocal { .. }
            | Instr::LoadFuncAddr { .. }
            | Instr::Mfsr { .. }
            | Instr::FAccStore { .. }
            | Instr::FZero { .. }
            | Instr::AddrOfFpuSpill { .. } => {}
        }
    }

    /// calls `f` on a mutable reference to every vreg this instruction reads
    pub fn for_each_use_mut(&mut self, f: &mut impl FnMut(&mut VReg)) {
        match self {
            Instr::Bin { lhs, rhs, .. } => {
                f(lhs);
                rhs.for_each_reg_mut(f);
            }
            Instr::Mul { lhs, rhs, .. } => {
                f(lhs);
                rhs.for_each_reg_mut(f);
            }
            Instr::Un { src, .. } | Instr::Mov { src, .. } => f(src),
            Instr::Shift { src, amount, .. } => {
                f(src);
                amount.for_each_reg_mut(f);
            }
            Instr::Signal { value, .. } => f(value),
            Instr::Bool { cmp, .. } => cmp.for_each_use_mut(f),
            Instr::CMov { cmp, src, .. } => {
                cmp.for_each_use_mut(f);
                f(src);
            }
            Instr::LoadMem { base, .. } => f(base),
            Instr::StoreMem { base, src, .. } => {
                f(base);
                f(src);
            }
            Instr::Call { args, .. } => args.iter_mut().for_each(f),
            Instr::CallPtr { addr, args, .. } => {
                f(addr);
                args.iter_mut().for_each(f);
            }
            Instr::DevSend { src, .. }
            | Instr::MtsrDseg { src }
            | Instr::StoreSp { src, .. }
            | Instr::StoreLocal { src, .. } => f(src),
            Instr::Jseg { cseg, target } => {
                f(cseg);
                f(target);
            }
            Instr::FBin { lhs, rhs, .. } | Instr::FDot4Acc { lhs, rhs } => {
                f(lhs);
                f(rhs);
            }
            Instr::FAccLoad { src, .. } => f(src),
            Instr::FMov { src, .. } | Instr::FUnary { src, .. } | Instr::FStore { src, .. } => {
                f(src)
            }
            Instr::FLoad { src_gpr, .. }
            | Instr::FImport4 {
                base_gpr: src_gpr, ..
            } => f(src_gpr),
            Instr::FExport4 { src, base_gpr } => {
                f(src);
                f(base_gpr);
            }
            Instr::LoadImm { .. }
            | Instr::StoreStatic { .. }
            | Instr::DevRecv { .. }
            | Instr::DcacheInvalidateAll
            | Instr::LoadSp { .. }
            | Instr::LoadLocal { .. }
            | Instr::AddrOfLocal { .. }
            | Instr::LoadFuncAddr { .. }
            | Instr::Mfsr { .. }
            | Instr::FAccStore { .. }
            | Instr::FZero { .. }
            | Instr::AddrOfFpuSpill { .. } => {}
        }
    }
}

impl IntOperand {
    pub fn for_each_reg(&self, f: &mut impl FnMut(VReg)) {
        if let IntOperand::Reg(v) = self {
            f(*v);
        }
    }
    pub fn for_each_reg_mut(&mut self, f: &mut impl FnMut(&mut VReg)) {
        if let IntOperand::Reg(v) = self {
            f(v);
        }
    }
}

impl Cmp {
    fn for_each_use(&self, f: &mut impl FnMut(VReg)) {
        f(self.lhs);
        if let CmpRhs::Reg(r) = &self.rhs {
            f(*r);
        }
    }
    fn for_each_use_mut(&mut self, f: &mut impl FnMut(&mut VReg)) {
        f(&mut self.lhs);
        if let CmpRhs::Reg(r) = &mut self.rhs {
            f(r);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Phi {
    pub dst: VReg,
    pub args: Vec<(BlockId, VReg)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Terminator {
    Jmp {
        target: BlockId,
    },
    Br {
        cmp: Cmp,
        if_true: BlockId,
        if_false: BlockId,
    },
    Ret {
        values: Vec<VReg>,
    },
    /// halt with a signal value (main program exit)
    Halt {
        signal: VReg,
    },
    /// CpuV3-only terminal handoff. Codegen must emit the delayed complete
    /// instruction-cache invalidate command immediately followed by JSEG.
    IcacheInvalidateDelayedAndJump {
        cseg: VReg,
        target: VReg,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    pub phis: Vec<Phi>,
    pub insts: Vec<Instr>,
    /// source line per instruction, parallel to `insts` (debugger line table;
    /// None = no source line, e.g. compiler-generated)
    pub lines: Vec<Option<u32>>,
    /// None while the block is still being built (or unreachable)
    pub term: Option<Terminator>,
    /// source line of the terminator (debuggers map halts/rets/branches too)
    pub term_line: Option<u32>,
    pub preds: Vec<BlockId>,
}

#[derive(Clone, Debug)]
pub struct IrFunc {
    pub name: FuncName,
    /// vregs holding the parameters at function entry (defined by the ABI)
    pub params: Vec<VReg>,
    pub n_rets: usize,
    pub blocks: Vec<Block>,
    pub entry: BlockId,
    pub vreg_count: u32,
    /// source-level names for the disassembly listing (optional)
    pub param_names: Vec<&'static str>,
    pub ret_names: Vec<&'static str>,
    /// per-block role notes for the disassembly listing (loop header, then, ...)
    pub block_notes: Vec<Option<&'static str>>,
    /// per-block source line numbers for the disassembly listing (best effort)
    pub block_lines: Vec<Option<u32>>,
    /// number of frame-local slots assigned by the frontend (address-taken
    /// locals and local arrays); frame layout puts these after callee saves
    /// and before spill slots
    pub local_slots: u8,
    /// register class of every vreg (indexed by vreg id)
    pub vreg_class: Vec<RegClass>,
}

impl IrFunc {
    /// allocate a fresh vreg of the given class
    pub fn fresh_vreg(&mut self, class: RegClass) -> VReg {
        let v = self.vreg_count;
        self.vreg_count += 1;
        self.vreg_class.push(class);
        v
    }
    /// register class of vreg `v`
    pub fn class_of(&self, v: VReg) -> RegClass {
        self.vreg_class[v as usize]
    }

    /// successor blocks of `b` (terminator targets)
    pub fn successors(&self, b: BlockId) -> Vec<BlockId> {
        match &self.blocks[b].term {
            Some(Terminator::Jmp { target }) => vec![*target],
            Some(Terminator::Br {
                if_true, if_false, ..
            }) => vec![*if_true, *if_false],
            _ => vec![],
        }
    }

    /// blocks in reverse post-order starting from entry
    pub fn rpo(&self) -> Vec<BlockId> {
        let mut visited = vec![false; self.blocks.len()];
        let mut post = vec![];
        fn dfs(f: &IrFunc, b: BlockId, visited: &mut [bool], post: &mut Vec<BlockId>) {
            if visited[b] {
                return;
            }
            visited[b] = true;
            for s in f.successors(b) {
                dfs(f, s, visited, post);
            }
            post.push(b);
        }
        dfs(self, self.entry, &mut visited, &mut post);
        post.reverse();
        post
    }
}

fn fmt_vregs(v: &[VReg]) -> String {
    v.iter()
        .map(|r| format!("v{r}"))
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for IrFunc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(
            f,
            "fn {} params=({}) rets={}",
            self.name,
            fmt_vregs(&self.params),
            self.n_rets
        )?;
        for (i, b) in self.blocks.iter().enumerate() {
            let preds = b
                .preds
                .iter()
                .map(|p| format!("b{p}"))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(f, "b{i}: ; preds=[{preds}]")?;
            for phi in &b.phis {
                let args = phi
                    .args
                    .iter()
                    .map(|(b, v)| format!("(b{b}, v{v})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(f, "  v{} = phi [{args}]", phi.dst)?;
            }
            for inst in &b.insts {
                writeln!(f, "  {inst}")?;
            }
            match &b.term {
                Some(t) => writeln!(f, "  {t}")?,
                None => writeln!(f, "  <unterminated>")?,
            }
        }
        Ok(())
    }
}

impl fmt::Display for IntOperand {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            IntOperand::Reg(v) => write!(f, "v{v}"),
            IntOperand::Imm(value) => write!(f, "{value}"),
        }
    }
}

impl fmt::Display for MulWindow {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MulWindow::Low => write!(f, "mul0"),
            MulWindow::Shift8 => write!(f, "mul8"),
            MulWindow::Shift16 => write!(f, "mul16"),
        }
    }
}

fn fmt_cmp(cmp: &Cmp) -> String {
    let rhs = match &cmp.rhs {
        CmpRhs::Reg(r) => format!("v{r}"),
        CmpRhs::Imm(i) => format!("{i}"),
    };
    let sign = if cmp.signed { "s" } else { "" };
    format!("v{} {}{} {}", cmp.lhs, cond_symbol(cmp.cond), sign, rhs)
}

impl fmt::Display for Instr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Instr::Bin { dst, op, lhs, rhs } => {
                let op = match op {
                    BinOp::Add => "add",
                    BinOp::Sub => "sub",
                    BinOp::And => "and",
                    BinOp::Or => "or",
                    BinOp::Xor => "xor",
                };
                write!(f, "v{dst} = {op} v{lhs}, {rhs}")
            }
            Instr::Mul {
                dst,
                window,
                lhs,
                rhs,
            } => write!(f, "v{dst} = {window} v{lhs}, {rhs}"),
            Instr::Un { dst, op, src } => {
                let op = match op {
                    UnOp::Inv => "inv",
                    UnOp::Neg => "neg",
                    UnOp::Not0 => "not0",
                    UnOp::Cnt1 => "cnt1",
                    UnOp::Log2 => "log2",
                    UnOp::Sextb => "sextb",
                    UnOp::Clz => "clz",
                };
                write!(f, "v{dst} = {op} v{src}")
            }
            Instr::Shift {
                dst,
                op,
                src,
                amount,
            } => {
                let op = match op {
                    ShiftOp::Lsl => "lsl",
                    ShiftOp::Lsr => "lsr",
                    ShiftOp::Asr => "asr",
                };
                write!(f, "v{dst} = {op} v{src}, {amount}")
            }
            Instr::Signal { signal_type, value } => {
                write!(f, "signal {signal_type}, v{value}")
            }
            Instr::Mfsr { dst, sr } => {
                let sr = match sr {
                    SpecialReg::Cseg => "CSEG",
                    SpecialReg::Dseg => "DSEG",
                };
                write!(f, "v{dst} = mfsr {sr}")
            }
            Instr::Bool { dst, cmp } => write!(f, "v{dst} = bool {}", fmt_cmp(cmp)),
            Instr::CMov { dst, cmp, src } => {
                write!(f, "v{dst} = cmov v{src} if {}", fmt_cmp(cmp))
            }
            Instr::Mov { dst, src } => write!(f, "v{dst} = mov v{src}"),
            Instr::LoadImm { dst, value } => write!(f, "v{dst} = imm {value}"),
            Instr::LoadMem { dst, base, offset } => write!(f, "v{dst} = load [v{base} + {offset}]"),
            Instr::StoreMem { base, offset, src } => {
                write!(f, "store [v{base} + {offset}] = v{src}")
            }
            Instr::StoreStatic { addr, value } => write!(f, "static[{addr:#06x}] = {value:#06x}"),
            Instr::Call { func, args, rets } => {
                write!(
                    f,
                    "({}) = call {}({})",
                    fmt_vregs(rets),
                    func,
                    fmt_vregs(args)
                )
            }
            Instr::LoadFuncAddr { dst, func } => write!(f, "v{dst} = &{func}"),
            Instr::CallPtr { addr, args, rets } => {
                write!(
                    f,
                    "({}) = call_ptr v{}({})",
                    fmt_vregs(rets),
                    addr,
                    fmt_vregs(args)
                )
            }
            Instr::DevRecv {
                dst,
                device,
                channel,
            } => write!(f, "v{dst} = dev_recv {device}, {channel}"),
            Instr::DevSend {
                device,
                channel,
                src,
            } => write!(f, "dev_send {device}, {channel}, v{src}"),
            Instr::DcacheInvalidateAll => write!(f, "dcache_invalidate_all"),
            Instr::MtsrDseg { src } => write!(f, "mtsr_dseg v{src}"),
            Instr::Jseg { cseg, target } => write!(f, "jseg v{cseg}, v{target}"),
            Instr::LoadSp { dst, slot } => write!(f, "v{dst} = load_sp #{slot}"),
            Instr::StoreSp { slot, src } => write!(f, "store_sp #{slot} = v{src}"),
            Instr::LoadLocal { dst, slot } => write!(f, "v{dst} = load_local #{slot}"),
            Instr::StoreLocal { slot, src } => write!(f, "store_local #{slot} = v{src}"),
            Instr::AddrOfLocal { dst, slot } => write!(f, "v{dst} = &local #{slot}"),
            Instr::FBin { dst, op, lhs, rhs } => {
                let op = match op {
                    FBinOp::Add => "fadd",
                    FBinOp::Sub => "fsub",
                    FBinOp::Mul => "fmul",
                };
                write!(f, "v{dst} = {op} v{lhs}, v{rhs}")
            }
            Instr::FMov { dst, src } => write!(f, "v{dst} = fmov v{src}"),
            Instr::FLoad { dst, src_gpr } => write!(f, "v{dst} = fload v{src_gpr}"),
            Instr::FStore { dst_gpr, src } => write!(f, "v{dst_gpr} = fstore v{src}"),
            Instr::FImport4 { dst, base_gpr } => {
                write!(f, "v{dst} = fimport4 [v{base_gpr}]")
            }
            Instr::FExport4 { src, base_gpr } => {
                write!(f, "fexport4 [v{base_gpr}] = v{src}")
            }
            Instr::FUnary { dst, op, src } => {
                let op = match op {
                    FUnOp::Rcp => "frcp",
                    FUnOp::Rsqrt => "frsqrt",
                    FUnOp::SinCos => "fsincos",
                    FUnOp::Abs => "fabs",
                    FUnOp::Neg => "fneg",
                    FUnOp::Floor => "ffloor",
                    FUnOp::Ceil => "fceil",
                    FUnOp::Round => "fround",
                    FUnOp::Sat01 => "fsat01",
                    FUnOp::Sign => "fsign",
                };
                write!(f, "v{dst} = {op} v{src}")
            }
            Instr::FDot4Acc { lhs, rhs } => write!(f, "fdot4acc v{lhs}, v{rhs}"),
            Instr::FAccStore { dst, mask } => write!(f, "v{dst} = faccstore {mask:#06b}"),
            Instr::FAccLoad { src, lane } => write!(f, "faccload.{lane} v{src}"),
            Instr::FZero { dst } => write!(f, "v{dst} = fzero"),
            Instr::AddrOfFpuSpill { dst, slot } => write!(f, "v{dst} = &fpu_spill #{slot}"),
        }
    }
}

pub(crate) fn cond_symbol(cond: CompareOp) -> &'static str {
    match cond {
        CompareOp::Never => "never",
        CompareOp::Greater => ">",
        CompareOp::Equal => "==",
        CompareOp::Less => "<",
        CompareOp::GreaterEqual => ">=",
        CompareOp::NotEqual => "!=",
        CompareOp::LessEqual => "<=",
        CompareOp::Always => "always",
    }
}

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Terminator::Jmp { target } => write!(f, "jmp b{target}"),
            Terminator::Br {
                cmp,
                if_true,
                if_false,
            } => {
                let rhs = match &cmp.rhs {
                    CmpRhs::Reg(r) => format!("v{r}"),
                    CmpRhs::Imm(i) => format!("{i}"),
                };
                let sign = if cmp.signed { "s" } else { "" };
                write!(
                    f,
                    "br v{} {}{} {} -> b{}, b{}",
                    cmp.lhs,
                    cond_symbol(cmp.cond),
                    sign,
                    rhs,
                    if_true,
                    if_false
                )
            }
            Terminator::Ret { values } => write!(f, "ret [{}]", fmt_vregs(values)),
            Terminator::Halt { signal } => write!(f, "halt v{signal}"),
            Terminator::IcacheInvalidateDelayedAndJump { cseg, target } => {
                write!(f, "icache_invalidate_delayed_and_jump v{cseg}, v{target}")
            }
        }
    }
}
