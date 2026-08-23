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

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum UnOp {
    Inv,
    Neg,
    Not0,
    Cnt1,
    Log2,
}

/// shifts are in-place on the ISA (`r0 = r0 << imm`), codegen inserts a mov
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ShiftOp {
    Lsl,
    Lsr,
    Asr,
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
        rhs: VReg,
    },
    Un {
        dst: VReg,
        op: UnOp,
        src: VReg,
    },
    Shift {
        dst: VReg,
        op: ShiftOp,
        src: VReg,
        amount: u8,
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
    /// G16-only: write the DSEG special register (MTSR DSEG)
    MtsrDseg {
        src: VReg,
    },
    /// G16-only: atomically switch CSEG and jump (JSEG); never returns
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
}

impl IrFunc {
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
                write!(f, "v{dst} = {op} v{lhs}, v{rhs}")
            }
            Instr::Un { dst, op, src } => {
                let op = match op {
                    UnOp::Inv => "inv",
                    UnOp::Neg => "neg",
                    UnOp::Not0 => "not0",
                    UnOp::Cnt1 => "cnt1",
                    UnOp::Log2 => "log2",
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
            Instr::MtsrDseg { src } => write!(f, "mtsr_dseg v{src}"),
            Instr::Jseg { cseg, target } => write!(f, "jseg v{cseg}, v{target}"),
            Instr::LoadSp { dst, slot } => write!(f, "v{dst} = load_sp #{slot}"),
            Instr::StoreSp { slot, src } => write!(f, "store_sp #{slot} = v{src}"),
            Instr::LoadLocal { dst, slot } => write!(f, "v{dst} = load_local #{slot}"),
            Instr::StoreLocal { slot, src } => write!(f, "store_local #{slot} = v{src}"),
            Instr::AddrOfLocal { dst, slot } => write!(f, "v{dst} = &local #{slot}"),
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
        }
    }
}
