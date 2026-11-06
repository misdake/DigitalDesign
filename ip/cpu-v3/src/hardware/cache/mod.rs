//! CpuV3 two-way physical-address cache hardware.
//!
//! Cache lines use four ordered 64-bit memory beats. Two true-dual-port data
//! BSRAMs split each line by word parity and directly transfer all four words
//! of a beat. Tags and valid bits commit only after a complete error-free line. The
//! production I-cache exposes a read-only boundary around this refill
//! engine; the independent D-cache implements write-allocate, dirty eviction,
//! and blocking global clean/invalidate.

use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::{BsramBlocks, SsramBits},
    HardwareIdentity, Module, ModuleIo, TargetResourceRequest, VerilogDependency, VerilogIdentity,
};
use digital_design_hardware_gowin::{BsramImage, ZeroBsramImage};
use std::fmt::Write;
use std::marker::PhantomData;

pub const CPU_V3_CACHE_WAYS: usize = 2;
pub const CPU_V3_CACHE_WORDS_PER_WAY: usize = 1024;
pub const CPU_V3_CACHE_WORDS: usize = CPU_V3_CACHE_WAYS * CPU_V3_CACHE_WORDS_PER_WAY;
pub const CPU_V3_CACHE_LINE_WORDS: usize = 16;
pub const CPU_V3_CACHE_LINE_BEATS: usize = CPU_V3_CACHE_LINE_WORDS / 2;
pub const CPU_V3_CACHE_MEMORY_BEATS: usize = CPU_V3_CACHE_LINE_WORDS / 4;
pub const CPU_V3_CACHE_SETS: usize = CPU_V3_CACHE_WORDS_PER_WAY / CPU_V3_CACHE_LINE_WORDS;

mod data;
mod instruction;
mod storage;
pub use data::*;
pub use instruction::*;
pub use storage::*;

const fn line_base(address: u32) -> u32 {
    address & !((CPU_V3_CACHE_LINE_WORDS as u32) - 1) & 0x003f_ffff
}

const fn decode(address: u32) -> (usize, u16, usize) {
    let word = (address as usize) & (CPU_V3_CACHE_LINE_WORDS - 1);
    let set = ((address as usize) >> 4) & (CPU_V3_CACHE_SETS - 1);
    let tag = ((address >> 10) & 0x0fff) as u16;
    (set, tag, word)
}

const fn data_index(way: usize, set: usize, word: usize) -> usize {
    (way * CPU_V3_CACHE_SETS + set) * CPU_V3_CACHE_LINE_WORDS + word
}
