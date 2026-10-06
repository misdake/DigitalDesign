//! Synthesizable framebuffer RTL sources for independent Icarus execution.
//!
//! `ROP_LEAF` is the exact oracle-free leaf datapath; `CACHE` is the bounded
//! four-bank cache with row-stream ingress, demand refill, dirty writeback,
//! flush and terminal fault. Both are plain Verilog-2001 modules with no delays
//! or initial blocks; the tests build a small memory model around them.
//!
//! Four 1024x16 banks use synchronous registered reads and synchronous writes.
//! The memory ports are inferred portable RTL; Gowin primitive mapping and fitted
//! area/timing remain separate synthesis checks.

pub const ROP_LEAF: &str = include_str!("rtl/rop_leaf.v");
pub const CACHE: &str = include_str!("rtl/framebuffer_cache.v");
