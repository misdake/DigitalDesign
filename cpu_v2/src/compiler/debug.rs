//! debug info model: what a hypothetical debugger needs — functions with
//! address ranges and frames, variables with locations, and a pc->line table.

use std::fmt::Write as _;

/// where a variable lives
#[derive(Clone, Debug, PartialEq)]
pub enum VarLoc {
    /// global/static: absolute data address
    Global(u16),
    /// frame-local: frame slot index (callee-save area included)
    Frame(u8),
    /// ABI register (params)
    Param(u8),
    /// register/SSA (versioned; no stable address — "optimized out")
    Ssa,
}

#[derive(Clone, Debug)]
pub struct DebugVar {
    pub name: String,
    pub ty: String,
    pub loc: VarLoc,
}

#[derive(Clone, Debug)]
pub struct DebugFunc {
    pub name: String,
    /// index into DebugInfo::files
    pub file: u16,
    pub addr: (usize, usize),
    pub frame_size: usize,
    pub locals: Vec<DebugVar>,
}

#[derive(Clone, Debug, Default)]
pub struct DebugInfo {
    pub files: Vec<String>,
    pub functions: Vec<DebugFunc>,
    pub globals: Vec<DebugVar>,
    pub consts: Vec<(String, String, u16)>,
    /// (address, file index, line)
    pub lines: Vec<(usize, u16, u32)>,
}

impl DebugInfo {
    pub fn render(&self) -> String {
        let mut out = String::from("# rcc debug info v1\n");
        for (i, f) in self.files.iter().enumerate() {
            let _ = writeln!(out, "file {i} {f}");
        }
        if !self.files.is_empty() {
            out.push('\n');
        }
        for f in &self.functions {
            let file = self.files.get(f.file as usize).map(|s| s.as_str()).unwrap_or("?");
            let _ = writeln!(
                out,
                "func {} 0x{:04x}..0x{:04x} {} frame {}",
                f.name, f.addr.0, f.addr.1, file, f.frame_size
            );
            for v in &f.locals {
                let _ = writeln!(out, "  {} {} {}", loc_kind(&v.loc), v.name, v.ty);
            }
            out.push('\n');
        }
        for v in &self.globals {
            let _ = writeln!(out, "global {} {} 0x{:04x}", v.name, v.ty, unwrap_global(&v.loc));
        }
        if !self.globals.is_empty() {
            out.push('\n');
        }
        for (name, ty, value) in &self.consts {
            let _ = writeln!(out, "const {name} {ty} {value}");
        }
        if !self.consts.is_empty() {
            out.push('\n');
        }
        for (addr, file, line) in &self.lines {
            let _ = writeln!(out, "line 0x{addr:04x} {file} {line}");
        }
        out
    }
}

fn loc_kind(loc: &VarLoc) -> String {
    match loc {
        VarLoc::Global(a) => format!("global@{a:#06x}"),
        VarLoc::Frame(s) => format!("frame+{s}"),
        VarLoc::Param(r) => format!("r{r}"),
        VarLoc::Ssa => "ssa".to_string(),
    }
}
fn unwrap_global(loc: &VarLoc) -> u16 {
    match loc {
        VarLoc::Global(a) => *a,
        _ => 0,
    }
}
