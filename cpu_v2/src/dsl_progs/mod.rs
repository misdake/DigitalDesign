//! rcc subset programs: these files are BOTH valid Rust (rust-analyzer reads
//! them like any module) and rcc compiler inputs (parsed by
//! `compiler::frontend`). file names end with `_dsl.rs` to mark them.

#[allow(dead_code)]
mod arrays_dsl;
#[allow(dead_code)]
mod fnptr_dsl;
#[allow(dead_code)]
mod sum_dsl;
