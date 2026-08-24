mod control_flow;
mod data_memory;
mod decoder;
mod execute;
mod inst_memory;
mod register_read;
mod writeback;

pub use control_flow::*;
pub use data_memory::*;
pub use decoder::*;
pub use execute::*;
pub use inst_memory::*;
pub use register_read::*;
pub use writeback::*;

use super::{FLAGS_WIDTH, REGISTER_COUNT, WORD_WIDTH};
use digital_design_code::{CircuitWires, Wire};

pub const REG_INDEX_WIDTH: usize = 4;
pub const EXEC_OP_WIDTH: usize = 5;
pub const WB_SRC_WIDTH: usize = 2;
pub const PC_SRC_WIDTH: usize = 2;
pub const MEMORY_WORDS: usize = 1 << WORD_WIDTH;

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ExecOp {
    Idle = 0,
    PassA,
    Inv,
    Neg,
    NotZero,
    CountOnes,
    Log2,
    Lsl,
    Lsr,
    Asr,
    And,
    Or,
    Xor,
    Add,
    Sub,
    AddImmediate,
    LoadHi,
    LoadLo,
    PcAdd,
    CompareUnsigned,
    CompareUnsignedImmediate,
    CompareSigned,
    CompareSignedImmediate,
    CallRelative,
    CallAbsolute,
    CallRegister,
    Max,
}

impl ExecOp {
    pub fn from_raw(raw: u8) -> Self {
        assert!(
            raw < Self::Max as u8,
            "invalid cpu_v2 execute operation: {raw}"
        );
        // SAFETY: ExecOp is repr(u8), starts at zero, and has no gaps before Max.
        unsafe { std::mem::transmute(raw) }
    }
}

const _: () = assert!(ExecOp::Max as usize <= (1usize << EXEC_OP_WIDTH));

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WbSrc {
    Execute,
    Memory,
    Device,
}

impl WbSrc {
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            value if value == Self::Execute as u8 => Self::Execute,
            value if value == Self::Memory as u8 => Self::Memory,
            value if value == Self::Device as u8 => Self::Device,
            _ => panic!("invalid cpu_v2 writeback source: {raw}"),
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PcSrc {
    Next,
    Execute,
    Memory,
}

impl PcSrc {
    pub fn from_raw(raw: u8) -> Self {
        match raw {
            value if value == Self::Next as u8 => Self::Next,
            value if value == Self::Execute as u8 => Self::Execute,
            value if value == Self::Memory as u8 => Self::Memory,
            _ => panic!("invalid cpu_v2 pc source: {raw}"),
        }
    }
}

fn set_wire(circuit: &mut CircuitWires, wire: Wire, value: bool) {
    wire.set(circuit, u8::from(value));
}
