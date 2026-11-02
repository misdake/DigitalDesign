//! Production compact input/bounds/Vertex subset. Triangle execution is explicit
//! Unsupported, before any new Vertex result is written for that meshlet.
mod core;
mod input;
mod meshlet;
use super::{require, ContractError};
pub use core::{Core, CoreInput, CoreOutput, Registers};
pub use input::{Bits, InputRam, RamPorts};
pub use meshlet::{Input, Meshlet, Output, Phase};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Contract(ContractError),
    Unknown(&'static str),
    UnsupportedTriangles { count: u8, slot: bool },
    UnsupportedStartT,
}
impl From<ContractError> for Error {
    fn from(e: ContractError) -> Self {
        Self::Contract(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract(e) => e.fmt(f),
            Self::UnsupportedStartT => f.write_str("unsupported explicit T launch"),
            Self::Unknown(s) => write!(f, "unknown production data: {s}"),
            Self::UnsupportedTriangles { count, slot } => write!(
                f,
                "unsupported T count {count}, input page {} before Vertex launch",
                u8::from(*slot)
            ),
        }
    }
}
impl std::error::Error for Error {}
fn check(ok: bool, rule: &'static str) -> Result<(), Error> {
    require(ok, "vertex", rule).map_err(Into::into)
}
fn mask(n: u32) -> u64 {
    if n == 64 {
        u64::MAX
    } else {
        (1u64 << n) - 1
    }
}
fn signed(v: u64, n: u32) -> i64 {
    ((v << (64 - n)) as i64) >> (64 - n)
}
