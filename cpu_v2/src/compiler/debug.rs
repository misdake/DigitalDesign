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
    /// Inclusive lexical source-line range. Globals and legacy debug files
    /// have no scope restriction.
    pub scope: Option<(u32, u32)>,
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

#[derive(Clone, Debug, PartialEq)]
pub struct DebugInitSection {
    pub name: String,
    pub detail: String,
    pub addr: (usize, usize),
}

#[derive(Clone, Debug, Default)]
pub struct DebugInfo {
    pub files: Vec<String>,
    /// call_abs table entries as (u8 index, function name)
    pub function_table: Vec<(u8, String)>,
    /// compiler-generated startup ranges with no source-code ownership
    pub init_sections: Vec<DebugInitSection>,
    pub functions: Vec<DebugFunc>,
    pub globals: Vec<DebugVar>,
    pub consts: Vec<(String, String, u16)>,
    /// (address, file index, line)
    pub lines: Vec<(usize, u16, u32)>,
}

impl DebugInfo {
    pub fn render(&self) -> String {
        let mut out = String::from("# rcc debug info v2\n");
        for (i, f) in self.files.iter().enumerate() {
            let _ = writeln!(out, "file {i} {f}");
        }
        if !self.files.is_empty() {
            out.push('\n');
        }
        for (index, name) in &self.function_table {
            let _ = writeln!(out, "table {index} {name}");
        }
        if !self.function_table.is_empty() {
            out.push('\n');
        }
        for section in &self.init_sections {
            let _ = writeln!(
                out,
                "init {} 0x{:04x}..0x{:04x} {}",
                section.name, section.addr.0, section.addr.1, section.detail
            );
        }
        if !self.init_sections.is_empty() {
            out.push('\n');
        }
        for f in &self.functions {
            let file = self
                .files
                .get(f.file as usize)
                .map(|s| s.as_str())
                .unwrap_or("?");
            let _ = writeln!(
                out,
                "func {} 0x{:04x}..0x{:04x} {} frame {}",
                f.name, f.addr.0, f.addr.1, file, f.frame_size
            );
            for v in &f.locals {
                let scope = v
                    .scope
                    .map(|(start, end)| format!(" scope {start}..{end}"))
                    .unwrap_or_default();
                let _ = writeln!(out, "  {} {} {}{}", loc_kind(&v.loc), v.name, v.ty, scope);
            }
            out.push('\n');
        }
        for v in &self.globals {
            let _ = writeln!(
                out,
                "global {} {} 0x{:04x}",
                v.name,
                v.ty,
                unwrap_global(&v.loc)
            );
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

/// parse a `.dbg` file written by `DebugInfo::render` back into a DebugInfo
pub fn parse_debug(text: &str) -> Result<DebugInfo, String> {
    let mut info = DebugInfo::default();
    let mut cur_func: Option<DebugFunc> = None;
    for (ln, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = || format!("line {}: cannot parse `{line}`", ln + 1);
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("file") => {
                let idx: usize = parts.next().ok_or_else(err)?.parse().map_err(|_| err())?;
                let name = parts.next().ok_or_else(err)?;
                if idx != info.files.len() {
                    return Err(format!("line {}: file index out of order", ln + 1));
                }
                info.files.push(name.to_string());
            }
            Some("func") => {
                if let Some(f) = cur_func.take() {
                    info.functions.push(f);
                }
                let name = parts.next().ok_or_else(err)?.to_string();
                let range_s = parts.next().ok_or_else(err)?;
                let addr = parse_addr_range(range_s).ok_or_else(err)?;
                let file_name = parts.next().ok_or_else(err)?;
                let file = info
                    .files
                    .iter()
                    .position(|f| f == file_name)
                    .ok_or_else(|| format!("line {}: unknown file `{file_name}`", ln + 1))?
                    as u16;
                let _kw = parts.next(); // "frame"
                let frame_size: usize = parts.next().ok_or_else(err)?.parse().map_err(|_| err())?;
                cur_func = Some(DebugFunc {
                    name,
                    file,
                    addr,
                    frame_size,
                    locals: vec![],
                });
            }
            Some("table") => {
                let index: u8 = parts.next().ok_or_else(err)?.parse().map_err(|_| err())?;
                let name = parts.next().ok_or_else(err)?.to_string();
                info.function_table.push((index, name));
            }
            Some("init") => {
                let name = parts.next().ok_or_else(err)?.to_string();
                let addr = parse_addr_range(parts.next().ok_or_else(err)?).ok_or_else(err)?;
                let detail = parts.collect::<Vec<_>>().join(" ");
                info.init_sections
                    .push(DebugInitSection { name, detail, addr });
            }
            Some("global") => {
                // type may contain spaces (`[u16; 3]`): take addr from the end
                let toks: Vec<&str> = line.split_whitespace().collect();
                if toks.len() < 4 {
                    return Err(err());
                }
                let addr = parse_hex(toks[toks.len() - 1]).ok_or_else(err)?;
                let name = toks[1].to_string();
                let ty = toks[2..toks.len() - 1].join(" ");
                info.globals.push(DebugVar {
                    name,
                    ty,
                    loc: VarLoc::Global(addr),
                    scope: None,
                });
            }
            Some("const") => {
                let name = parts.next().ok_or_else(err)?.to_string();
                let ty = parts.next().ok_or_else(err)?.to_string();
                let value: u16 = parts.next().ok_or_else(err)?.parse().map_err(|_| err())?;
                info.consts.push((name, ty, value));
            }
            Some("line") => {
                let addr = parse_hex(parts.next().ok_or_else(err)?).ok_or_else(err)?;
                let file: u16 = parts.next().ok_or_else(err)?.parse().map_err(|_| err())?;
                let line_no: u32 = parts.next().ok_or_else(err)?.parse().map_err(|_| err())?;
                info.lines.push((addr as usize, file, line_no));
            }
            _ if line.starts_with(' ') => {
                // indented local variable line: `  <loc> <name> <ty>` (ty may contain spaces)
                let toks: Vec<&str> = line.split_whitespace().collect();
                if toks.len() < 3 {
                    return Err(err());
                }
                let loc_s = toks[0];
                let name = toks[1].to_string();
                let scope_pos = toks.iter().position(|t| *t == "scope");
                let ty_end = scope_pos.unwrap_or(toks.len());
                let ty = toks[2..ty_end].join(" ");
                let scope = scope_pos
                    .and_then(|i| toks.get(i + 1))
                    .and_then(|range| range.split_once(".."))
                    .and_then(|(start, end)| Some((start.parse().ok()?, end.parse().ok()?)));
                let loc = parse_loc(loc_s).ok_or_else(err)?;
                cur_func
                    .as_mut()
                    .ok_or_else(|| format!("line {}: local outside of a func", ln + 1))?
                    .locals
                    .push(DebugVar {
                        name,
                        ty,
                        loc,
                        scope,
                    });
            }
            _ => return Err(err()),
        }
    }
    if let Some(f) = cur_func.take() {
        info.functions.push(f);
    }
    Ok(info)
}

fn parse_hex(s: &str) -> Option<u16> {
    s.strip_prefix("0x")
        .and_then(|h| u16::from_str_radix(h, 16).ok())
}

fn parse_addr_range(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once("..")?;
    let start = parse_hex(a)?;
    let end = parse_hex(b)?;
    Some((start as usize, end as usize))
}

fn parse_loc(s: &str) -> Option<VarLoc> {
    if let Some(a) = s.strip_prefix("global@") {
        return Some(VarLoc::Global(parse_hex(a)?));
    }
    if let Some(n) = s.strip_prefix("frame+") {
        return Some(VarLoc::Frame(n.parse().ok()?));
    }
    if let Some(n) = s.strip_prefix('r') {
        return Some(VarLoc::Param(n.parse().ok()?));
    }
    if s == "ssa" {
        return Some(VarLoc::Ssa);
    }
    None
}
