//! rcc frontend: parse the Rust subset (see spec.md) with `syn`, validate it,
//! and lower it onto `FuncBuilder` to produce `IrFunc`s.
//!
//! anything outside the subset is a hard error with a source span.

use crate::CompareOp;
use crate::{BinOp, BlockId, Cmp, CmpRhs, Instr, IrFunc, ShiftOp, UnOp, VReg};
use crate::{BoolExpr, FuncBuilder, RccConfig, VarId};
use crate::{DebugVar, VarLoc};
use std::collections::{HashMap, HashSet};
use syn::{BinOp as SBinOp, Block, Expr, Item, ItemFn, Lit, Pat, Stmt, Type, UnOp as SUnOp};

mod diagnostics;
pub use diagnostics::CompileError;

#[cfg(doc)]
pub mod spec {}

/// a compiled program: functions plus the frontend half of the debug info
pub struct Program {
    pub funcs: Vec<IrFunc>,
    pub debug: FrontendDebug,
}

/// debug data collected by the frontend (the driver adds addresses and the
/// pc->line table to complete the picture)
#[derive(Default)]
pub struct FrontendDebug {
    pub files: Vec<String>,
    pub funcs: Vec<FnDebug>,
    pub globals: Vec<DebugVar>,
    pub consts: Vec<(String, String, u16)>,
    /// struct layouts, so a debugger can expand a value (spec §9b)
    pub types: Vec<crate::DebugType>,
}

pub struct FnDebug {
    pub name: String,
    pub file: u16,
    pub locals: Vec<DebugVar>,
}

/// parse rcc source text into a list of functions (IR), or report the first
/// subset violation with a span
pub fn parse_source(src: &str) -> Result<Program, CompileError> {
    parse_source_with(src, 0)
}

/// like `parse_source`, with the static data section starting at `data_base`
pub fn parse_source_with(src: &str, data_base: u16) -> Result<Program, CompileError> {
    let source_name = "<source>";
    let file =
        syn::parse_file(src).map_err(|error| CompileError::from_syn(source_name, src, error))?;
    let (funcs, debug) = parse_files(
        vec![file],
        data_base,
        1 << 16,
        "target memory",
        &["<source>".to_string()],
        false,
    )
    .map_err(|(_, error)| CompileError::from_syn(source_name, src, error))?;
    Ok(Program { funcs, debug })
}

/// the rcc standard library, embedded and appended to every program compiled
/// via `compile_program` (unused functions are dropped by the linker)
const STD_SOURCES: &[(&str, &str)] = &[
    ("rcc_std/div.rs", include_str!("../rcc_std/div.rs")),
    ("rcc_std/heap.rs", include_str!("../rcc_std/heap.rs")),
    ("rcc_std/mem.rs", include_str!("../rcc_std/mem.rs")),
    ("rcc_std/mul.rs", include_str!("../rcc_std/mul.rs")),
    ("rcc_std/vec.rs", include_str!("../rcc_std/vec.rs")),
];

/// The embedded standard library sources, exposed so tooling (such as the
/// debugger) can display library source text without reading it from disk.
pub fn std_sources() -> &'static [(&'static str, &'static str)] {
    STD_SOURCES
}

/// compile a full program: the user source, any `mod name;` files resolved
/// through `loader`, plus the rcc standard library, with automatic library
/// initialization driven by `opts`
pub fn compile_program(
    src: &str,
    opts: &impl RccConfig,
    loader: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Program, CompileError> {
    compile_program_named("<main>", src, opts, loader)
}

/// like `compile_program`, with the main source file named for debug output
pub fn compile_program_named(
    main_name: &str,
    src: &str,
    opts: &impl RccConfig,
    loader: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Program, CompileError> {
    // gather sources: main file + user modules (recursive) + std
    let mut srcs: Vec<(String, String)> = vec![(main_name.to_string(), src.to_string())];
    let mut seen: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < srcs.len() {
        let file_name = srcs[i].0.clone();
        let text = srcs[i].1.clone();
        let file = syn::parse_file(&text)
            .map_err(|error| CompileError::from_syn(&file_name, &text, error))?;
        for item in &file.items {
            if let Item::Mod(m) = item {
                if m.content.is_some() {
                    return Err(CompileError::from_syn(
                        &file_name,
                        &text,
                        err(
                            &m.ident,
                            "inline `mod name { }` is not supported; use `mod name;`",
                        ),
                    ));
                }
                let name = m.ident.to_string();
                if seen.insert(name.clone()) {
                    let text2 = loader(&name).map_err(|error| {
                        CompileError::from_syn(
                            &file_name,
                            &text,
                            err(&m.ident, format!("cannot load module `{name}`: {error}")),
                        )
                    })?;
                    srcs.push((format!("{name}.rs"), text2));
                }
            }
        }
        i += 1;
    }
    srcs.extend(
        STD_SOURCES
            .iter()
            .map(|(n, t)| (n.to_string(), t.to_string())),
    );

    let mut files = vec![];
    let mut names = vec![];
    for (name, text) in &srcs {
        names.push(name.clone());
        files.push(
            syn::parse_file(text).map_err(|error| CompileError::from_syn(name, text, error))?,
        );
    }
    let (mut funcs, debug) = parse_files(
        files,
        opts.data_base(),
        opts.static_data_limit(),
        opts.static_data_limit_name(),
        &names,
        opts.optimizations().is_disabled(),
    )
    .map_err(|(file, error)| {
        let (name, text) = &srcs[file.min(srcs.len() - 1)];
        CompileError::from_syn(name, text, error)
    })?;
    auto_init(&mut funcs, opts)
        .map_err(|error| CompileError::from_syn(&srcs[0].0, &srcs[0].1, error))?;
    Ok(Program { funcs, debug })
}

/// insert library initialization calls at the start of `main`, driven by
/// which library functions the program's call graph reaches
fn auto_init(out: &mut [IrFunc], opts: &impl RccConfig) -> Result<(), syn::Error> {
    // reachability over the call graph from main
    let mut reachable: HashSet<&str> = HashSet::new();
    let mut work = vec!["main"];
    reachable.insert("main");
    while let Some(n) = work.pop() {
        let Some(f) = out.iter().find(|f| f.name == n) else {
            continue;
        };
        for b in &f.blocks {
            for i in &b.insts {
                if let Instr::Call { func, .. } = i {
                    if reachable.insert(func) {
                        work.push(func);
                    }
                }
            }
        }
    }
    let heap_used = reachable.contains("malloc") || reachable.contains("free");
    let vec_used = reachable.contains("vec_new") || reachable.contains("init_vec");
    if !heap_used && !vec_used {
        return Ok(());
    }
    let Some(main) = out.iter_mut().find(|f| f.name == "main") else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "library initialization requires a `fn main`",
        ));
    };
    let mut heap_init = vec![];
    let mut vec_init = vec![];
    let mut next_v = main.vreg_count;
    // the init temporaries are GPRs
    main.vreg_class
        .resize(next_v as usize + 4, crate::RegClass::Gpr);
    if heap_used {
        heap_init.push(Instr::LoadImm {
            dst: next_v,
            value: opts.heap_begin(),
        });
        next_v += 1;
        heap_init.push(Instr::LoadImm {
            dst: next_v,
            value: opts.heap_size(),
        });
        next_v += 1;
        heap_init.push(Instr::Call {
            func: "init_heap",
            args: vec![next_v - 2, next_v - 1],
            rets: vec![],
        });
    }
    if vec_used {
        vec_init.push(Instr::LoadImm {
            dst: next_v,
            value: opts.vec_init_cap(),
        });
        next_v += 1;
        vec_init.push(Instr::Call {
            func: "init_vec",
            args: vec![next_v - 1],
            rets: vec![],
        });
    }
    main.vreg_count = next_v;
    // Prepend in reverse execution order: heap -> vec -> existing data init.
    if !vec_init.is_empty() {
        prepend_init_block(
            main,
            vec_init,
            format!("runtime vector: initial capacity {}", opts.vec_init_cap()),
        );
    }
    if !heap_init.is_empty() {
        prepend_init_block(
            main,
            heap_init,
            format!(
                "runtime heap: 0x{:04x}..0x{:04x}",
                opts.heap_begin(),
                opts.heap_begin().wrapping_add(opts.heap_size())
            ),
        );
    }
    Ok(())
}

fn prepend_init_block(main: &mut IrFunc, insts: Vec<Instr>, detail: String) {
    let old_entry = main.entry;
    let new_entry = main.blocks.len();
    let block = crate::Block {
        lines: vec![None; insts.len()],
        insts,
        term: Some(crate::Terminator::Jmp { target: old_entry }),
        ..Default::default()
    };
    main.blocks.push(block);
    main.block_notes
        .push(Some(intern(&format!("global init: {detail}"))));
    main.block_lines.push(None);
    main.blocks[old_entry].preds.push(new_entry);
    main.entry = new_entry;
}

fn parse_files(
    files: Vec<syn::File>,
    data_base: u16,
    static_data_limit: usize,
    static_data_limit_name: &'static str,
    file_names: &[String],
    materialize_debug_locals: bool,
) -> Result<(Vec<IrFunc>, FrontendDebug), (usize, syn::Error)> {
    let mut fns: Vec<(usize, &syn::ItemFn)> = vec![];
    let mut consts: HashMap<String, (u16, Ty)> = HashMap::new();
    let mut globals = Globals {
        next_addr: data_base,
        limit: static_data_limit,
        limit_name: static_data_limit_name,
        ..Globals::default()
    };
    // Struct definitions come first: a field may name a struct declared later, and
    // `ty_of` recognises a type path as a struct only through this name set.
    let mut raw_structs: std::collections::BTreeMap<String, (usize, &syn::ItemStruct)> =
        std::collections::BTreeMap::new();
    let mut raw_enums: std::collections::BTreeMap<String, (usize, &syn::ItemEnum)> =
        std::collections::BTreeMap::new();
    for (fi, file) in files.iter().enumerate() {
        for item in &file.items {
            if let Item::Enum(e) = item {
                let name = e.ident.to_string();
                if raw_enums.insert(name.clone(), (fi, e)).is_some() {
                    return Err((fi, err(&e.ident, format!("enum `{name}` defined twice"))));
                }
            }
        }
    }
    for (fi, file) in files.iter().enumerate() {
        for item in &file.items {
            if let Item::Struct(s) = item {
                if !s.generics.params.is_empty() {
                    return Err((fi, err(&s.generics, "generics are not supported")));
                }
                let name = s.ident.to_string();
                if raw_structs.insert(name.clone(), (fi, s)).is_some() {
                    return Err((fi, err(&s.ident, format!("struct `{name}` defined twice"))));
                }
            }
        }
    }
    let mut type_names: TypeNames = raw_structs
        .keys()
        .map(|name| (name.clone(), NominalKind::Struct))
        .collect();
    for (name, (fi, item)) in &raw_enums {
        let def = enum_def(item).map_err(|error| (*fi, error))?;
        type_names.insert(name.clone(), NominalKind::Enum);
        globals.enums.insert(name.clone(), def);
    }
    let mut deferred_statics: Vec<(usize, &syn::ItemStatic)> = vec![];
    for (fi, file) in files.iter().enumerate() {
        let result: Result<(), syn::Error> = (|| {
            for item in &file.items {
                match item {
                    Item::Fn(f) => {
                        for attr in &f.attrs {
                            if !attr.path.is_ident("allow") && !attr.path.is_ident("doc") {
                                return Err(err(
                                    attr,
                                    "attributes are not supported (except #[allow])",
                                ));
                            }
                        }
                        fns.push((fi, f))
                    }
                    Item::Use(_) => { /* ignored: for the IDE only */ }
                    Item::Const(c) => {
                        let ty = ty_of(&c.ty, &TypeNames::new())?;
                        if !matches!(ty, Ty::U16 | Ty::I16) {
                            return Err(err(&c.ty, "const must be u16 or i16"));
                        }
                        let v = const_eval(&c.expr, &consts)?;
                        let name = c.ident.to_string();
                        if consts.insert(name.clone(), (v, ty)).is_some() {
                            return Err(err(&c.ident, format!("const `{name}` defined twice")));
                        }
                    }
                    Item::Static(s) => {
                        // an aggregate static needs the resolved struct layouts, so it
                        // is laid out after the struct pass; scalars and buffers of
                        // scalars keep their original allocation order
                        if static_is_aggregate(s, &type_names) {
                            deferred_statics.push((fi, s));
                        } else {
                            add_static(s, &consts, &mut globals)?;
                        }
                    }
                    Item::Verbatim(_) => { /* attributes on use items land here */ }
                    Item::Mod(_) => { /* already resolved by compile_program */ }
                    Item::Struct(_) | Item::Enum(_) => { /* collected before this loop */ }
                    Item::Trait(_) => return Err(err(item, "traits are not supported")),
                    Item::Impl(_) => return Err(err(item, "impl blocks are not supported")),
                    Item::Macro(_) => return Err(err(item, "macros are not supported")),
                    _ => {
                        return Err(err(
                            item,
                            "item not supported (only fn/use/const/static are allowed)",
                        ))
                    }
                }
            }
            Ok(())
        })();
        result.map_err(|error| (fi, error))?;
    }
    // Resolve the layouts now that the consts (array lengths) are known.
    {
        let mut builder = LayoutBuilder {
            raw: &raw_structs,
            consts: &consts,
            names: &type_names,
            done: StructTable::new(),
            visiting: vec![],
        };
        for (name, (fi, item)) in &raw_structs {
            let def = builder.layout(name, item).map_err(|error| (*fi, error))?;
            builder.done.insert(name.clone(), def);
        }
        globals.structs = builder.done;
        globals.type_names = type_names.clone();
    }
    // aggregate statics now that the layouts exist (they land after the scalars)
    let layouts = globals.structs.clone();
    for (fi, s) in &deferred_statics {
        add_aggregate_static(s, &consts, &layouts, &type_names, &mut globals)
            .map_err(|error| (*fi, error))?;
    }
    // collect signatures first (functions can call each other regardless of order)
    let mut sigs: HashMap<String, Sig> = HashMap::new();
    let mut names = vec![];
    for (fi, f) in &fns {
        let name = f.sig.ident.to_string();
        let sig = signature(f, &consts, &globals.type_names).map_err(|error| (*fi, error))?;
        if sigs.insert(name.clone(), sig).is_some() {
            return Err((
                *fi,
                err(&f.sig.ident, format!("function `{name}` defined twice")),
            ));
        }
        names.push(intern(&name));
    }
    let mut out = vec![];
    let mut debug = FrontendDebug {
        files: file_names.to_vec(),
        ..FrontendDebug::default()
    };
    for (name, (fi, f)) in names.into_iter().zip(fns) {
        let (ir, fdbg) = lower_fn(
            name,
            f,
            &sigs,
            &consts,
            &globals,
            fi as u16,
            materialize_debug_locals,
        )
        .map_err(|error| (fi, error))?;
        out.push(ir);
        debug.funcs.push(fdbg);
    }
    emit_data_init(&mut out, &globals).map_err(|error| (0, error))?;

    // globals/consts for the debugger
    for (name, (addr, ty)) in &globals.scalars {
        debug.globals.push(DebugVar {
            name: name.clone(),
            ty: ty.display(),
            loc: VarLoc::Global(*addr),
            scope: None,
        });
    }
    for (name, (addr, elem, len)) in &globals.arrays {
        debug.globals.push(DebugVar {
            name: name.clone(),
            ty: Ty::Array(Box::new(elem.clone()), *len).display(),
            loc: VarLoc::Global(*addr),
            scope: None,
        });
    }
    for (name, (addr, ty)) in &globals.aggregates {
        debug.globals.push(DebugVar {
            name: name.clone(),
            ty: ty.display(),
            loc: VarLoc::Global(*addr),
            scope: None,
        });
    }
    for (name, def) in &globals.structs {
        debug.types.push(crate::DebugType {
            name: name.clone(),
            size: def.size,
            align: def.align,
            fields: def
                .fields
                .iter()
                .map(|f| crate::DebugTypeField {
                    name: f.name.clone(),
                    offset: f.offset,
                    ty: f.ty.display(),
                })
                .collect(),
        });
    }
    for (name, (v, ty)) in &consts {
        debug.consts.push((name.clone(), ty.display(), *v));
    }
    Ok((out, debug))
}

/// initialize non-zero static words at the start of main (__data_init)
fn emit_data_init(out: &mut [IrFunc], globals: &Globals) -> Result<(), syn::Error> {
    if globals.data_words.is_empty() {
        return Ok(());
    }
    let Some(main) = out.iter_mut().find(|f| f.name == "main") else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "static data requires a `fn main` to host __data_init",
        ));
    };
    let mut inits = vec![];
    for &(addr, value) in &globals.data_words {
        if value != 0 {
            inits.push(Instr::StoreStatic { addr, value });
        }
    }
    if !inits.is_empty() {
        let nonzero_words = globals
            .data_words
            .iter()
            .filter(|(_, value)| *value != 0)
            .count();
        let first = globals
            .data_words
            .iter()
            .map(|(addr, _)| *addr)
            .min()
            .unwrap();
        let end = globals
            .data_words
            .iter()
            .map(|(addr, _)| addr.saturating_add(1))
            .max()
            .unwrap();
        prepend_init_block(
            main,
            inits,
            format!("static data: {nonzero_words} nonzero words in 0x{first:04x}..0x{end:04x}"),
        );
    }
    Ok(())
}

fn intern(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// source line of a syntax node (for listing comments)
fn line_of(t: &impl syn::spanned::Spanned) -> u32 {
    t.span().start().line as u32
}

fn end_line_of(t: &impl syn::spanned::Spanned) -> u32 {
    t.span().end().line as u32
}

fn err(tokens: &impl syn::spanned::Spanned, msg: impl std::fmt::Display) -> syn::Error {
    syn::Error::new(tokens.span(), msg.to_string())
}

/// integer literal value (handles 0x/0o/0b prefixes, `_` separators, suffixes)
fn lit_int_value(i: &syn::LitInt) -> Result<u64, syn::Error> {
    let text = i.to_string().replace('_', "");
    let suffix = i.suffix();
    let digits = &text[..text.len() - suffix.len()];
    let v = if let Some(h) = digits.strip_prefix("0x") {
        u64::from_str_radix(h, 16)
    } else if let Some(o) = digits.strip_prefix("0o") {
        u64::from_str_radix(o, 8)
    } else if let Some(b) = digits.strip_prefix("0b") {
        u64::from_str_radix(b, 2)
    } else {
        digits.parse::<u64>()
    };
    v.map_err(|_| err(i, "invalid integer literal"))
}

// ---------------------------------------------------------------------------
// constants and globals (data section)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Globals {
    scalars: HashMap<String, (u16, Ty)>,
    arrays: HashMap<String, (u16, Ty, usize)>,
    /// aggregate statics (structs, tuples, buffers of structs): the symbol is its
    /// address, the type says how to read it (spec §9.4)
    aggregates: HashMap<String, (u16, Ty)>,
    /// (addr, value) words for __data_init
    data_words: Vec<(u16, u16)>,
    next_addr: u16,
    limit: usize,
    limit_name: &'static str,
    /// resolved struct layouts, by name
    structs: StructTable,
    /// C-style enums, by name (spec §9d)
    enums: EnumTable,
    /// every struct name in the program (ty_of recognises a type path through it)
    type_names: TypeNames,
}

/// a nominal type a `ty_of` path may name
#[derive(Clone, Copy, PartialEq, Debug)]
enum NominalKind {
    Struct,
    Enum,
}

/// every struct/enum name in the program, mapped to its kind; `ty_of` needs only
/// this, while the layout pass needs the resolved definitions
type TypeNames = std::collections::BTreeMap<String, NominalKind>;

/// one field of a struct: its name, type and word offset in the layout
#[derive(Clone, Debug)]
struct StructField {
    name: String,
    ty: Ty,
    offset: u16,
}

/// a resolved struct layout: fields in declaration order in 16-bit words, the
/// total size padded up to `align`, and the alignment itself
#[derive(Clone, Debug)]
struct StructDef {
    fields: Vec<StructField>,
    size: u16,
    align: u16,
}

/// resolved layouts by struct name (ordered, so listings and debug output are
/// deterministic)
type StructTable = std::collections::BTreeMap<String, StructDef>;

/// a C-style enum: variants in declaration order with their discriminants, plus
/// whether `#[derive(PartialEq)]` was written (spec §9d)
#[derive(Clone, Debug)]
struct EnumDef {
    variants: Vec<(String, u16)>,
    partial_eq: bool,
}

/// resolved enums by name
type EnumTable = std::collections::BTreeMap<String, EnumDef>;

/// storage size of a value in 16-bit words (spec §1: every scalar is one word)
fn word_size(
    ty: &Ty,
    structs: &StructTable,
    at: &impl syn::spanned::Spanned,
) -> Result<u16, syn::Error> {
    match ty {
        Ty::U16 | Ty::I16 | Ty::Ptr | Ty::Bool | Ty::Fix16 | Ty::ArrayRef(_) | Ty::Enum(_) => Ok(1),
        Ty::FnPtr { .. } => Ok(1),
        Ty::Tuple(elems) => u16::try_from(elems.len())
            .map_err(|_| err(at, "tuple is larger than the 16-bit address space")),
        Ty::Array(elem, n) => {
            let elem = word_size(elem, structs, at)?;
            u16::try_from(usize::from(elem) * n)
                .map_err(|_| err(at, "aggregate is larger than the 16-bit address space"))
        }
        Ty::Struct(name) => structs
            .get(name)
            .map(|def| def.size)
            .ok_or_else(|| err(at, format!("unknown struct `{name}`"))),
        _ => Err(err(
            at,
            format!("{} cannot be stored as a value here", ty.display()),
        )),
    }
}

/// a value that lives in memory as a word range: a struct, a tuple or a buffer
fn is_aggregate(ty: &Ty) -> bool {
    // an enum is a plain word, not an address (spec §9d)
    matches!(ty, Ty::Struct(_) | Ty::Tuple(_) | Ty::Array(..))
}

/// the word size of an aggregate
fn aggregate_size(
    ty: &Ty,
    structs: &StructTable,
    at: &impl syn::spanned::Spanned,
) -> Result<u16, syn::Error> {
    if !is_aggregate(ty) {
        return Err(err(
            at,
            format!(
                "{} is not an aggregate (struct, tuple or Buf)",
                ty.display()
            ),
        ));
    }
    word_size(ty, structs, at)
}

/// alignment of a value type in words: scalars are naturally aligned to one word,
/// an array takes its element's alignment, and a struct its own `#[repr(align)]`
fn type_align(ty: &Ty, structs: &StructTable) -> u16 {
    match ty {
        Ty::Array(elem, _) => type_align(elem, structs),
        Ty::Struct(name) => structs.get(name).map_or(1, |def| def.align),
        _ => 1,
    }
}

fn align_up(value: u16, align: u16) -> u16 {
    if align <= 1 {
        value
    } else {
        value.div_ceil(align) * align
    }
}

/// `#[repr(align(N))]` (N in bytes, like Rust) and `#[repr(C)]`; anything else
/// except `#[allow]`/`#[doc]` is an error. The target measures alignment in
/// 16-bit words, so the accepted byte alignments map to 1, 2, 4 and 8 words.
fn struct_align(s: &syn::ItemStruct) -> Result<u16, syn::Error> {
    let mut align = 1u16;
    for attr in &s.attrs {
        if attr.path.is_ident("allow") || attr.path.is_ident("doc") {
            continue;
        }
        if !attr.path.is_ident("repr") {
            return Err(err(
                attr,
                "attributes are not supported (only #[allow], #[doc] and #[repr])",
            ));
        }
        let syn::Meta::List(list) = attr.parse_meta()? else {
            return Err(err(attr, "#[repr] needs C or align(N)"));
        };
        let mut seen = false;
        for nested in &list.nested {
            match nested {
                syn::NestedMeta::Meta(syn::Meta::Path(path)) if path.is_ident("C") => {
                    seen = true;
                }
                syn::NestedMeta::Meta(syn::Meta::List(inner)) if inner.path.is_ident("align") => {
                    let Some(syn::NestedMeta::Lit(syn::Lit::Int(bytes))) = inner.nested.first()
                    else {
                        return Err(err(nested, "align needs a byte count, like align(4)"));
                    };
                    let bytes: u16 = bytes.base10_parse()?;
                    align = match bytes {
                        2 => 1,
                        4 => 2,
                        8 => 4,
                        16 => 8,
                        _ => {
                            return Err(err(
                                nested,
                                "align must be 2, 4, 8 or 16 bytes (1, 2, 4 or 8 words)",
                            ))
                        }
                    };
                    seen = true;
                }
                other => {
                    return Err(err(
                        other,
                        "only #[repr(C)] and #[repr(align(N))] are supported",
                    ))
                }
            }
        }
        if !seen {
            return Err(err(attr, "#[repr] needs C or align(N)"));
        }
    }
    Ok(align)
}

/// resolve every struct definition into a layout. Definitions may appear in any
/// order and may nest, so this recurses on demand and reports a cycle instead of
/// looping forever.
struct LayoutBuilder<'a> {
    raw: &'a std::collections::BTreeMap<String, (usize, &'a syn::ItemStruct)>,
    consts: &'a HashMap<String, (u16, Ty)>,
    names: &'a TypeNames,
    done: StructTable,
    visiting: Vec<String>,
}

impl LayoutBuilder<'_> {
    fn layout(
        &mut self,
        name: &str,
        at: &impl syn::spanned::Spanned,
    ) -> Result<StructDef, syn::Error> {
        if let Some(def) = self.done.get(name) {
            return Ok(def.clone());
        }
        let Some((_, item)) = self.raw.get(name) else {
            return Err(err(at, format!("unknown struct `{name}`")));
        };
        if self.visiting.iter().any(|v| v == name) {
            return Err(err(
                item,
                format!("struct `{name}` contains itself; only sized, non-recursive layouts are supported"),
            ));
        }
        self.visiting.push(name.to_string());
        let align = struct_align(item)?;
        let mut fields = vec![];
        let mut offset: u16 = 0;
        match &item.fields {
            syn::Fields::Named(named) => {
                for field in &named.named {
                    let Some(ident) = &field.ident else { continue };
                    let fname = ident.to_string();
                    let fty = ty_of_maybe_array(&field.ty, self.consts, self.names)?;
                    if matches!(fty, Ty::Fix16 | Ty::Vec2 | Ty::Vec3 | Ty::Vec4) {
                        return Err(err(
                            &field.ty,
                            "fix16/vecN struct fields are not supported yet (spec §12)",
                        ));
                    }
                    if let Ty::Struct(inner) = &fty {
                        self.layout(inner, &field.ty)?;
                    }
                    let size = word_size(&fty, &self.done, &field.ty)?;
                    let falign = type_align(&fty, &self.done);
                    offset = align_up(offset, falign);
                    fields.push(StructField {
                        name: fname,
                        ty: fty,
                        offset,
                    });
                    offset = offset.checked_add(size).ok_or_else(|| {
                        err(&field.ty, "struct is larger than the 16-bit address space")
                    })?;
                }
            }
            syn::Fields::Unnamed(_) => {
                return Err(err(
                    item,
                    "tuple structs are not supported (use named fields)",
                ))
            }
            syn::Fields::Unit => {
                return Err(err(
                    item,
                    "unit structs are not supported (give it a field)",
                ))
            }
        }
        let def = StructDef {
            fields,
            size: align_up(offset, align),
            align,
        };
        self.visiting.pop();
        self.done.insert(name.to_string(), def.clone());
        Ok(def)
    }
}

/// evaluate a constant expression (literals, other consts, wrapping arithmetic)
fn const_eval(e: &Expr, consts: &HashMap<String, (u16, Ty)>) -> Result<u16, syn::Error> {
    match e {
        Expr::Paren(p) => const_eval(&p.expr, consts),
        // a braced const argument (`Buf<u16, {N + 1}>`) arrives as a block
        Expr::Block(b) => match b.block.stmts.as_slice() {
            [Stmt::Expr(inner)] => const_eval(inner, consts),
            _ => Err(err(e, "not a constant expression")),
        },
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(i) => {
                let v = lit_int_value(i)?;
                if v > u16::MAX as u64 {
                    return Err(err(&lit.lit, "literal out of 16-bit range"));
                }
                Ok(v as u16)
            }
            _ => Err(err(&lit.lit, "not a constant expression")),
        },
        Expr::Path(_) => {
            let name = path_ident(e)?;
            consts
                .get(&name)
                .map(|&(v, _)| v)
                .ok_or_else(|| err(e, format!("unknown const `{name}`")))
        }
        Expr::Unary(u) => match u.op {
            SUnOp::Neg(_) => Ok(const_eval(&u.expr, consts)?.wrapping_neg()),
            _ => Err(err(&u.op, "not a constant expression")),
        },
        Expr::Binary(b) => {
            let (a, c) = (const_eval(&b.left, consts)?, const_eval(&b.right, consts)?);
            match b.op {
                SBinOp::Add(_) => Ok(a.wrapping_add(c)),
                SBinOp::Sub(_) => Ok(a.wrapping_sub(c)),
                SBinOp::BitAnd(_) => Ok(a & c),
                SBinOp::BitOr(_) => Ok(a | c),
                SBinOp::BitXor(_) => Ok(a ^ c),
                SBinOp::Shl(_) => Ok(a.wrapping_shl(c as u32)),
                SBinOp::Shr(_) => Ok(a.wrapping_shr(c as u32)),
                SBinOp::Mul(_) => Ok(a.wrapping_mul(c)),
                SBinOp::Div(_) => {
                    if c == 0 {
                        return Err(err(&b.op, "division by zero in const expression"));
                    }
                    Ok(a / c)
                }
                SBinOp::Rem(_) => {
                    if c == 0 {
                        return Err(err(&b.op, "remainder by zero in const expression"));
                    }
                    Ok(a % c)
                }
                _ => Err(err(&b.op, "not a constant expression")),
            }
        }
        Expr::Cast(c) => {
            // u16/i16 casts are free
            let _ = ty_of(&c.ty, &TypeNames::new())?;
            const_eval(&c.expr, consts)
        }
        _ => Err(err(e, "not a constant expression")),
    }
}

/// whether a static's type is an aggregate, which needs the resolved layouts and
/// is therefore laid out after the struct pass (spec §9.4). Scalar buffers stay on
/// the original path so their addresses do not move.
/// `enum E { A, B }`: fieldless variants only, no generics and no explicit
/// discriminants; `#[derive(PartialEq)]`, `#[allow]` and `#[doc]` are accepted and
/// anything else is an error (spec §9d)
fn enum_def(item: &syn::ItemEnum) -> Result<EnumDef, syn::Error> {
    if !item.generics.params.is_empty() {
        return Err(err(&item.generics, "generics are not supported"));
    }
    let mut partial_eq = false;
    for attr in &item.attrs {
        if attr.path.is_ident("allow") || attr.path.is_ident("doc") {
            continue;
        }
        if attr.path.is_ident("derive") {
            let syn::Meta::List(list) = attr.parse_meta()? else {
                return Err(err(attr, "expected #[derive(PartialEq)]"));
            };
            for nested in &list.nested {
                match nested {
                    syn::NestedMeta::Meta(syn::Meta::Path(path)) if path.is_ident("PartialEq") => {
                        partial_eq = true;
                    }
                    other => {
                        return Err(err(
                            other,
                            "only #[derive(PartialEq)] is supported on an enum",
                        ))
                    }
                }
            }
            continue;
        }
        return Err(err(
            attr,
            "attributes are not supported on an enum (except #[derive(PartialEq)])",
        ));
    }
    let mut variants: Vec<(String, u16)> = vec![];
    for (index, variant) in item.variants.iter().enumerate() {
        if !matches!(variant.fields, syn::Fields::Unit) {
            return Err(err(
                variant,
                "only fieldless variants are supported (a C-style enum, spec §9d)",
            ));
        }
        if let Some((_, value)) = &variant.discriminant {
            return Err(err(value, "explicit discriminants are not supported"));
        }
        let name = variant.ident.to_string();
        if variants.iter().any(|(existing, _)| *existing == name) {
            return Err(err(
                &variant.ident,
                format!("variant `{name}` defined twice"),
            ));
        }
        variants.push((name, index as u16));
    }
    if variants.is_empty() {
        return Err(err(item, "an enum needs at least one variant"));
    }
    Ok(EnumDef {
        variants,
        partial_eq,
    })
}

fn static_is_aggregate(s: &syn::ItemStatic, structs: &TypeNames) -> bool {
    match s.ty.as_ref() {
        syn::Type::Tuple(_) => true,
        syn::Type::Path(tp) => {
            if tp.path.segments.len() != 1 {
                return false;
            }
            let seg = &tp.path.segments[0];
            if structs.get(&seg.ident.to_string()) == Some(&NominalKind::Struct) {
                return true;
            }
            if seg.ident == "Buf" {
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    if let Some(syn::GenericArgument::Type(syn::Type::Path(inner))) =
                        args.args.first()
                    {
                        return inner.path.segments.len() == 1
                            && structs.get(&inner.path.segments[0].ident.to_string())
                                == Some(&NominalKind::Struct);
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// `static NAME: Point = Point { .. };`, `static T: (u16, u16) = (1, 2);`,
/// `static TBL: Buf<Point, 2> = Buf::new([..]);` — constants only (spec §9.4)
fn add_aggregate_static(
    s: &syn::ItemStatic,
    consts: &HashMap<String, (u16, Ty)>,
    structs: &StructTable,
    names: &TypeNames,
    g: &mut Globals,
) -> Result<(), syn::Error> {
    if s.mutability.is_some() {
        return Err(err(
            s,
            "static mut is not supported; write via addr_of(&X) (see spec §9.2)",
        ));
    }
    let ty = ty_of_maybe_array(s.ty.as_ref(), consts, names)?;
    let size = word_size(&ty, structs, s)?;
    let addr = reserve_static(g, size as usize, &s.ident)?;
    let mut words: Vec<(u16, u16)> = vec![];
    const_words(&ty, s.expr.as_ref(), consts, structs, addr, &mut words)?;
    g.data_words.extend(words);
    let name = s.ident.to_string();
    let duplicate = g.aggregates.contains_key(&name)
        || g.scalars.contains_key(&name)
        || g.arrays.contains_key(&name);
    if duplicate || g.aggregates.insert(name.clone(), (addr, ty)).is_some() {
        return Err(err(&s.ident, format!("static `{name}` defined twice")));
    }
    Ok(())
}

/// fold a constant aggregate initializer into `(address, value)` words
fn const_words(
    ty: &Ty,
    expr: &Expr,
    consts: &HashMap<String, (u16, Ty)>,
    structs: &StructTable,
    base: u16,
    out: &mut Vec<(u16, u16)>,
) -> Result<(), syn::Error> {
    match ty {
        Ty::Struct(name) => {
            let def = structs
                .get(name)
                .cloned()
                .ok_or_else(|| err(expr, format!("unknown struct `{name}`")))?;
            let Expr::Struct(se) = expr else {
                return Err(err(
                    expr,
                    format!("a static {name} needs a {name} {{ .. }} literal"),
                ));
            };
            for value in &se.fields {
                let syn::Member::Named(ident) = &value.member else {
                    return Err(err(&value.member, "only named struct fields are supported"));
                };
                let fname = ident.to_string();
                let Some(field) = def.fields.iter().find(|f| f.name == fname) else {
                    return Err(err(
                        &value.member,
                        format!("struct `{name}` has no field `{fname}`"),
                    ));
                };
                const_words(
                    &field.ty,
                    &value.expr,
                    consts,
                    structs,
                    base + field.offset,
                    out,
                )?;
            }
            Ok(())
        }
        Ty::Tuple(elems) => {
            let Expr::Tuple(t) = expr else {
                return Err(err(expr, "a static tuple needs a tuple literal"));
            };
            if t.elems.len() != elems.len() {
                return Err(err(
                    expr,
                    format!(
                        "tuple has {} elements, expected {}",
                        t.elems.len(),
                        elems.len()
                    ),
                ));
            }
            for (i, (e, elem)) in t.elems.iter().zip(elems).enumerate() {
                const_words(elem, e, consts, structs, base + i as u16, out)?;
            }
            Ok(())
        }
        Ty::Array(elem, n) => {
            let inner = buf_initializer(expr)?;
            let size = word_size(elem, structs, inner)?;
            let elements: Vec<&Expr> = match inner {
                Expr::Repeat(r) => {
                    let m = const_eval(&r.len, consts)? as usize;
                    if m != *n {
                        return Err(err(
                            inner,
                            format!("Buf repeat count {m} does not match declared length {n}"),
                        ));
                    }
                    (0..*n).map(|_| r.expr.as_ref()).collect()
                }
                Expr::Array(arr) => {
                    if arr.elems.len() != *n {
                        return Err(err(
                            inner,
                            format!("initializer has {} elements, expected {n}", arr.elems.len()),
                        ));
                    }
                    arr.elems.iter().collect()
                }
                other => {
                    return Err(err(
                        other,
                        "a static Buf needs an initializer list or [v; N]",
                    ))
                }
            };
            for (i, e) in elements.into_iter().enumerate() {
                const_words(elem, e, consts, structs, base + (i as u16) * size, out)?;
            }
            Ok(())
        }
        scalar => {
            if !matches!(scalar, Ty::U16 | Ty::I16 | Ty::Ptr | Ty::Bool) {
                return Err(err(
                    expr,
                    format!("{} cannot be a static value", scalar.display()),
                ));
            }
            out.push((base, const_eval(expr, consts)?));
            Ok(())
        }
    }
}

fn reserve_static(
    g: &mut Globals,
    words: usize,
    at: &impl syn::spanned::Spanned,
) -> Result<u16, syn::Error> {
    let start = g.next_addr as usize;
    let end = start
        .checked_add(words)
        .ok_or_else(|| err(at, "static data address overflow"))?;
    if end > g.limit {
        return Err(err(
            at,
            format!(
                "static data reaches reserved {} at {:#06x}",
                g.limit_name, g.limit
            ),
        ));
    }
    g.next_addr = end as u16;
    Ok(start as u16)
}

/// `static NAME: Ty = expr;` or `static NAME: [Ty; N] = [..];`
fn add_static(
    s: &syn::ItemStatic,
    consts: &HashMap<String, (u16, Ty)>,
    g: &mut Globals,
) -> Result<(), syn::Error> {
    if s.mutability.is_some() {
        return Err(err(
            s,
            "static mut is not supported; write via addr_of(&X) (see spec §9.2)",
        ));
    }
    let name = s.ident.to_string();
    if let Some(owned) = buf_type(s.ty.as_ref(), consts, &TypeNames::new())? {
        let Ty::Array(elem, len) = owned else {
            unreachable!("buf_type returns an array")
        };
        if !matches!(*elem, Ty::U16 | Ty::I16) {
            return Err(err(
                &s.ty,
                "a static Buf element must be u16 or i16 (struct and enum statics are not supported yet)",
            ));
        }
        let elem = *elem;
        let init: Vec<u16> = match buf_initializer(s.expr.as_ref())? {
            Expr::Array(arr) => arr
                .elems
                .iter()
                .map(|e| const_eval(e, consts))
                .collect::<Result<_, _>>()?,
            Expr::Repeat(r) => {
                let m = const_eval(&r.len, consts)? as usize;
                if m != len {
                    return Err(err(
                        &s.expr,
                        format!("Buf repeat count {m} does not match declared length {len}"),
                    ));
                }
                let v = const_eval(&r.expr, consts)?;
                vec![v; len]
            }
            other => {
                return Err(err(
                    other,
                    "a static Buf needs an initializer list or [v; N]",
                ))
            }
        };
        if init.len() != len {
            return Err(err(
                &s.expr,
                format!("initializer has {} elements, expected {len}", init.len()),
            ));
        }
        let addr = reserve_static(g, len, &s.ident)?;
        for (i, w) in init.iter().enumerate() {
            g.data_words.push((addr + i as u16, *w));
        }
        if g.arrays.insert(name.clone(), (addr, elem, len)).is_some() {
            return Err(err(&s.ident, format!("static `{name}` defined twice")));
        }
        return Ok(());
    }
    match s.ty.as_ref() {
        Type::Array(_) => Err(err(
            &s.ty,
            "native arrays are not part of the subset; write Buf<T, N> (spec §10)",
        )),
        t => {
            let ty = ty_of(t, &TypeNames::new())?;
            if !matches!(ty, Ty::U16 | Ty::I16) {
                return Err(err(
                    t,
                    "static must be u16/i16 or a Buf of them (struct statics are not supported yet)",
                ));
            }
            let v = const_eval(&s.expr, consts)?;
            let addr = reserve_static(g, 1, &s.ident)?;
            g.data_words.push((addr, v));
            if g.scalars.insert(name.clone(), (addr, ty)).is_some() {
                return Err(err(&s.ident, format!("static `{name}` defined twice")));
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// types
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, Debug)]
enum Ty {
    U16,
    I16,
    /// unsuffixed integer literal; adopts the type it unifies with
    UntypedInt,
    Ptr,
    /// Typed, one-word unchecked array view (same target representation as Ptr).
    ArrayRef(Box<Ty>),
    Bool,
    FnPtr {
        params: Vec<Ty>,
        ret: Box<Ty>,
    },
    /// Buf<u16, N> / Buf<i16, N>, memory-resident (data section or stack frame)
    Array(Box<Ty>, usize),
    /// a struct: the value *is* its word address, like an array (spec §9b). The
    /// name keys `Globals::structs`, which owns the layout.
    Struct(String),
    /// a tuple of scalars: memory-resident like a struct, element `i` at offset
    /// `i` (spec §9c)
    Tuple(Vec<Ty>),
    /// a C-style enum: one word holding the variant's discriminant (spec §9d)
    Enum(String),
    /// CpuV3 FPU types: one F register per value. fix16 is a vector whose
    /// only meaningful lane is x; vec2/vec3 carry meaning in the first N
    /// lanes and keep the upper lanes zero.
    Fix16,
    Vec2,
    Vec3,
    Vec4,
    Unit,
    Never,
}
impl Ty {
    fn is_int(&self) -> bool {
        matches!(self, Ty::U16 | Ty::I16 | Ty::UntypedInt)
    }
    fn is_fpu(&self) -> bool {
        matches!(self, Ty::Fix16 | Ty::Vec2 | Ty::Vec3 | Ty::Vec4)
    }
    /// number of meaningful lanes (fix16 counts as a 1-lane vector)
    fn fpu_lanes(&self) -> usize {
        match self {
            Ty::Fix16 => 1,
            Ty::Vec2 => 2,
            Ty::Vec3 => 3,
            Ty::Vec4 => 4,
            _ => 0,
        }
    }
    fn display(&self) -> String {
        match self {
            Ty::U16 => "u16".into(),
            Ty::I16 => "i16".into(),
            Ty::UntypedInt => "integer literal".into(),
            Ty::Ptr => "Ptr".into(),
            Ty::ArrayRef(elem) => format!("Array<{}>", elem.display()),
            Ty::Bool => "bool".into(),
            Ty::FnPtr { params, ret } => format!(
                "fn({}) -> {}",
                params
                    .iter()
                    .map(|t| t.display())
                    .collect::<Vec<_>>()
                    .join(", "),
                ret.display()
            ),
            Ty::Array(elem, n) => format!("Buf<{}, {n}>", elem.display()),
            Ty::Struct(name) => name.clone(),
            Ty::Enum(name) => name.clone(),
            Ty::Tuple(elems) => format!(
                "({})",
                elems
                    .iter()
                    .map(|t| t.display())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Fix16 => "fix16".into(),
            Ty::Vec2 => "vec2".into(),
            Ty::Vec3 => "vec3".into(),
            Ty::Vec4 => "vec4".into(),
            Ty::Unit => "()".into(),
            Ty::Never => "!".into(),
        }
    }
}

#[derive(Clone)]
struct Sig {
    params: Vec<Ty>,
    ret: Ty,
}

/// `structs` is the nominal-type table (structs *and* enums): `ty_of` only needs to
/// know which kind a name is
fn ty_of(ty: &Type, structs: &TypeNames) -> Result<Ty, syn::Error> {
    match ty {
        Type::Path(tp) => {
            if tp.path.segments.len() != 1 {
                return Err(err(
                    ty,
                    "unsupported type (expected u16/i16/Ptr/Array<T>/fn pointer/struct)",
                ));
            }
            let seg = &tp.path.segments[0];
            if seg.ident == "Array" {
                let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
                    return Err(err(
                        ty,
                        "Array needs one element type: Array<u16> or Array<i16>",
                    ));
                };
                if args.args.len() != 1 {
                    return Err(err(
                        ty,
                        "Array needs one element type: Array<u16> or Array<i16>",
                    ));
                }
                let Some(syn::GenericArgument::Type(elem)) = args.args.first() else {
                    return Err(err(
                        ty,
                        "Array needs one element type: Array<u16> or Array<i16>",
                    ));
                };
                let elem = ty_of(elem, structs)?;
                // one list for both array spellings: this set must stay equal to
                // `buf_type`'s (see the whitelist test in rcc_errors.rs)
                if !matches!(elem, Ty::U16 | Ty::I16 | Ty::Struct(_) | Ty::Enum(_)) {
                    return Err(err(
                        ty,
                        "Array element type must be u16, i16, a struct or an enum",
                    ));
                }
                return Ok(Ty::ArrayRef(Box::new(elem)));
            }
            if seg.ident == "Buf" {
                return Err(err(
                    ty,
                    "a Buf cannot be a parameter or a cast target; pass Array<T> (a view) or Ptr \
                     (returning one is fine, spec §10/§14)",
                ));
            }
            if !seg.arguments.is_empty() {
                return Err(err(ty, "generics are only supported for Array<u16/i16>"));
            }
            let name = seg.ident.to_string();
            match name.as_str() {
                "u16" => Ok(Ty::U16),
                "i16" => Ok(Ty::I16),
                "Ptr" => Ok(Ty::Ptr),
                "bool" => Ok(Ty::Bool),
                "fix16" => Ok(Ty::Fix16),
                "vec2" => Ok(Ty::Vec2),
                "vec3" => Ok(Ty::Vec3),
                "vec4" => Ok(Ty::Vec4),
                "usize" => Err(err(
                    ty,
                    "rcc has no `usize`: one word is `u16`, so write `const N: u16 = 6;`. A named \\
                     length only builds for the target though (Rust const generics take `usize`), so \\
                     spell the literal (`Buf<u16, 6>`) when the source must build on the host too",
                )),
                _ => match structs.get(&name) {
                    Some(NominalKind::Struct) => Ok(Ty::Struct(name)),
                    Some(NominalKind::Enum) => Ok(Ty::Enum(name)),
                    None => Err(err(
                        ty,
                        "type not supported (only u16/i16/Ptr/Array<T>/fn pointer/fix16/vecN/struct/enum)",
                    )),
                },
            }
        }
        Type::BareFn(bf) => {
            let params = bf
                .inputs
                .iter()
                .map(|a| ty_of(&a.ty, structs))
                .collect::<Result<Vec<_>, _>>()?;
            let ret = match &bf.output {
                syn::ReturnType::Default => Ty::Unit,
                syn::ReturnType::Type(_, t) => ty_of(t, structs)?,
            };
            if is_aggregate(&ret) {
                return Err(err(
                    ty,
                    "a fn pointer cannot return an aggregate; call the function directly (spec §14)",
                ));
            }
            Ok(Ty::FnPtr {
                params,
                ret: Box::new(ret),
            })
        }
        Type::Tuple(t) => {
            if t.elems.is_empty() {
                return Ok(Ty::Unit); // `()` is the unit type
            }
            if t.elems.len() > 4 {
                return Err(err(ty, "tuples hold at most 4 elements"));
            }
            let mut elems = vec![];
            for e in &t.elems {
                let elem = ty_of(e, structs)?;
                if !matches!(elem, Ty::U16 | Ty::I16 | Ty::Ptr | Ty::Bool) {
                    return Err(err(e, "tuple elements must be u16, i16, Ptr or bool"));
                }
                elems.push(elem);
            }
            Ok(Ty::Tuple(elems))
        }
        Type::Never(_) => Ok(Ty::Unit),
        Type::Paren(p) => ty_of(&p.elem, structs),
        Type::Array(_) => Err(err(
            ty,
            "native arrays are not part of the subset; write Buf<T, N> (spec §10)",
        )),
        Type::Reference(_) => Err(err(
            ty,
            "reference types `&T` are not supported (pass Array<T> or Ptr)",
        )),
        Type::Slice(_) => Err(err(
            ty,
            "slice types are not supported (pass Array<T> or Ptr)",
        )),
        _ => Err(err(ty, "unsupported type")),
    }
}

/// type annotation that may be an owned array type (valid only in let/static
/// positions and struct fields)
fn ty_of_maybe_array(
    ty: &Type,
    consts: &HashMap<String, (u16, Ty)>,
    structs: &TypeNames,
) -> Result<Ty, syn::Error> {
    if let Some(owned) = buf_type(ty, consts, structs)? {
        return Ok(owned);
    }
    match ty {
        Type::Array(_) => Err(err(
            ty,
            "native arrays are not part of the subset; write Buf<T, N> (spec §10)",
        )),
        _ => ty_of(ty, structs),
    }
}

/// `Buf<T, N>` — the owned array type (spec §10). Native `[T; N]` is **not** part of
/// the subset: Rust arrays index by `usize`, so a word-sized `u16` index would not
/// type-check on the host, and the orphan rule forbids adding `Index<u16>` to them.
fn buf_type(
    ty: &Type,
    consts: &HashMap<String, (u16, Ty)>,
    structs: &TypeNames,
) -> Result<Option<Ty>, syn::Error> {
    let Type::Path(tp) = ty else {
        return Ok(None);
    };
    if tp.path.segments.len() != 1 || tp.path.segments[0].ident != "Buf" {
        return Ok(None);
    }
    let syn::PathArguments::AngleBracketed(args) = &tp.path.segments[0].arguments else {
        return Err(err(ty, "Buf needs two parameters, like Buf<u16, 8>"));
    };
    let mut args = args.args.iter();
    let Some(syn::GenericArgument::Type(elem_ty)) = args.next() else {
        return Err(err(ty, "Buf needs two parameters, like Buf<u16, 8>"));
    };
    let len = match args.next() {
        Some(syn::GenericArgument::Const(len)) => const_eval(len, consts)?,
        // syn parses a bare identifier as a *type* argument, but `Buf<u16, N>` names
        // the const `N` (Rust requires braces around anything more complex)
        Some(syn::GenericArgument::Type(Type::Path(tp)))
            if tp.path.segments.len() == 1 && tp.path.segments[0].arguments.is_empty() =>
        {
            let name = tp.path.segments[0].ident.to_string();
            consts.get(&name).map(|&(v, _)| v).ok_or_else(|| {
                err(
                    ty,
                    format!("the Buf length must be a constant: unknown const `{name}`"),
                )
            })?
        }
        _ => return Err(err(ty, "Buf needs two parameters, like Buf<u16, 8>")),
    };
    if args.next().is_some() {
        return Err(err(ty, "Buf needs two parameters, like Buf<u16, 8>"));
    }
    let elem = ty_of(elem_ty, structs)?;
    if !matches!(elem, Ty::U16 | Ty::I16 | Ty::Struct(_) | Ty::Enum(_)) {
        return Err(err(
            elem_ty,
            "Buf element type must be u16, i16, a struct or an enum",
        ));
    }
    let n = len as usize;
    Ok(Some(Ty::Array(Box::new(elem), n)))
}

/// the `[v; N]` / `[e0, e1, ...]` inside `Buf::new(...)` (or `Buf(...)`)
fn buf_initializer(init: &Expr) -> Result<&Expr, syn::Error> {
    let Expr::Call(call) = init else {
        return Err(err(
            init,
            "a Buf is initialized with Buf::new([v; N]) or Buf::new([e0, e1, ...])",
        ));
    };
    let Expr::Path(func) = call.func.as_ref() else {
        return Err(err(init, "a Buf is initialized with Buf::new([v; N])"));
    };
    let segs: Vec<String> = func
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    if segs.as_slice() != ["Buf", "new"] && segs.as_slice() != ["Buf"] {
        return Err(err(
            init,
            "a Buf is initialized with Buf::new([v; N]) or Buf::new([e0, e1, ...])",
        ));
    }
    if call.args.len() != 1 {
        return Err(err(init, "Buf::new takes one array literal"));
    }
    Ok(&call.args[0])
}

/// initialize an array at `base` (a frame slot, a struct field, ...). The
/// initializer is `Buf::new([v; N])` or `Buf::new([e0, e1, ...])`.
fn init_array_at(
    l: &mut FnLower,
    base: VReg,
    elem: &Ty,
    n: usize,
    init: &Expr,
) -> Result<(), syn::Error> {
    // an aggregate-returning call writes straight into the destination
    if sret_call_into(l, init, base, &Ty::Array(Box::new(elem.clone()), n))? {
        return Ok(());
    }
    let init = buf_initializer(init)?;
    // a struct element spans several words, so element `i` starts at `i * size`
    if let Ty::Struct(name) = elem {
        let name = name.clone();
        let def = l
            .globals
            .structs
            .get(&name)
            .cloned()
            .ok_or_else(|| err(init, format!("unknown struct `{name}`")))?;
        let stride = i16::try_from(def.size).map_err(|_| {
            err(
                init,
                "struct array element is larger than the address space",
            )
        })?;
        let element_at = |l: &mut FnLower, index: usize| {
            let offset = stride
                .checked_mul(index as i16)
                .expect("struct array fits the address space");
            place_addr(l, base, offset)
        };
        match init {
            Expr::Repeat(r) => {
                let m = const_eval(&r.len, l.consts)? as usize;
                if m != n {
                    return Err(err(
                        init,
                        format!("array repeat count {m} does not match declared length {n}"),
                    ));
                }
                let first = element_at(l, 0);
                init_struct_at(l, first, &name, &r.expr)?;
                // copy the initialized element over the rest
                for index in 1..n {
                    let dst = element_at(l, index);
                    let len = l.b.load_imm(def.size);
                    l.b.call("mem_copy", &[dst, first, len], 0);
                }
                return Ok(());
            }
            Expr::Array(arr) => {
                if arr.elems.len() != n {
                    return Err(err(
                        init,
                        format!(
                            "array initializer has {} elements, expected {n}",
                            arr.elems.len()
                        ),
                    ));
                }
                for (index, e) in arr.elems.iter().enumerate() {
                    let addr = element_at(l, index);
                    init_struct_at(l, addr, &name, e)?;
                }
                return Ok(());
            }
            _ => {
                return Err(err(
                    init,
                    "a struct Buf needs Buf::new([v; N]) or a list of struct literals",
                ))
            }
        }
    }
    match init {
        Expr::Repeat(r) => {
            let m = const_eval(&r.len, l.consts)? as usize;
            if m != n {
                return Err(err(
                    init,
                    format!("array repeat count {m} does not match declared length {n}"),
                ));
            }
            let (v, _) = expr(l, &r.expr)?.reg(l, &r.expr, "array initializer")?;
            let (v, _) = coerce(l, v, elem, elem, &r.expr)?;
            for i in 0..n as i16 {
                l.b.store_mem(base, i, v);
            }
            Ok(())
        }
        Expr::Array(arr) => {
            if arr.elems.len() != n {
                return Err(err(
                    init,
                    format!(
                        "array initializer has {} elements, expected {n}",
                        arr.elems.len()
                    ),
                ));
            }
            for (i, e) in arr.elems.iter().enumerate() {
                let (v, _) = expr(l, e)?.reg(l, e, "array initializer")?;
                let (v, _) = coerce(l, v, elem, elem, e)?;
                l.b.store_mem(base, i as i16, v);
            }
            Ok(())
        }
        _ => Err(err(
            init,
            "array initializer must be [v; N] or [e0, e1, ...]",
        )),
    }
}

/// initialize a struct value at `base`: a struct literal, or a copy of another
/// value of the same struct type (a struct value is its address, spec §9b)
fn init_struct_at(l: &mut FnLower, base: VReg, name: &str, init: &Expr) -> Result<(), syn::Error> {
    let def = l
        .globals
        .structs
        .get(name)
        .cloned()
        .ok_or_else(|| err(init, format!("unknown struct `{name}`")))?;
    let Expr::Struct(se) = init else {
        // an aggregate-returning call writes straight into the destination
        if sret_call_into(l, init, base, &Ty::Struct(name.to_string()))? {
            return Ok(());
        }
        // a copy from another value of the same struct type
        let (src_base, src_offset, src_ty, _) = place_addr_of(l, init)?;
        if src_ty != Ty::Struct(name.to_string()) {
            return Err(err(
                init,
                format!("expected {name}, got {}", src_ty.display()),
            ));
        }
        let src = place_addr(l, src_base, src_offset);
        let len = l.b.load_imm(def.size);
        l.b.call("mem_copy", &[base, src, len], 0);
        return Ok(());
    };
    if se.path.segments.len() != 1 {
        return Err(err(&se.path, "unsupported struct literal path"));
    }
    let literal = se.path.segments[0].ident.to_string();
    if literal != name {
        return Err(err(&se.path, format!("expected {name}, got {literal}")));
    }
    if let Some(rest) = &se.rest {
        return Err(err(
            rest,
            "struct update syntax (`..base`) is not supported",
        ));
    }
    let mut seen: Vec<String> = vec![];
    for value in &se.fields {
        let fname = match &value.member {
            syn::Member::Named(ident) => ident.to_string(),
            syn::Member::Unnamed(index) => {
                return Err(err(index, "only named struct fields are supported"))
            }
        };
        let Some(field) = def.fields.iter().find(|field| field.name == fname) else {
            let known = def
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(err(
                &value.member,
                format!("struct `{name}` has no field `{fname}` (fields: {known})"),
            ));
        };
        if seen.contains(&fname) {
            return Err(err(
                &value.member,
                format!("field `{fname}` is initialized twice"),
            ));
        }
        seen.push(fname);
        let field_ty = field.ty.clone();
        let field_offset = field.offset as i16;
        match &field_ty {
            Ty::Struct(inner) => {
                let inner = inner.clone();
                let addr = place_addr(l, base, field_offset);
                init_struct_at(l, addr, &inner, &value.expr)?;
            }
            Ty::Array(elem, n) => {
                let (elem, n) = ((**elem).clone(), *n);
                let addr = place_addr(l, base, field_offset);
                init_array_at(l, addr, &elem, n, &value.expr)?;
            }
            _ => {
                let (v, from) = expr(l, &value.expr)?.reg(l, &value.expr, "struct field")?;
                let (v, _) = coerce(l, v, &from, &field_ty, &value.expr)?;
                l.b.store_mem(base, field_offset, v);
            }
        }
    }
    if seen.len() != def.fields.len() {
        let missing = def
            .fields
            .iter()
            .filter(|field| !seen.contains(&field.name))
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(err(
            init,
            format!("struct `{name}` is missing field(s): {missing}"),
        ));
    }
    Ok(())
}

/// the type a direct call returns when that type is an aggregate (sret, spec §14)
fn sret_call_return(l: &FnLower, e: &Expr) -> Option<Ty> {
    let Expr::Call(call) = e else {
        return None;
    };
    let Expr::Path(p) = call.func.as_ref() else {
        return None;
    };
    let name = p.path.get_ident()?.to_string();
    let sig = l.sigs.get(&name)?;
    is_aggregate(&sig.ret).then(|| sig.ret.clone())
}

/// A direct call to a function that returns an aggregate writes into `dst` — the
/// hidden first parameter the caller supplies (spec §14). `Ok(false)` means `e`
/// is not such a call, so the caller falls back to its usual path.
fn sret_call_into(l: &mut FnLower, e: &Expr, dst: VReg, expected: &Ty) -> Result<bool, syn::Error> {
    let Expr::Call(call) = e else {
        return Ok(false);
    };
    let Expr::Path(p) = call.func.as_ref() else {
        return Ok(false);
    };
    let Some(name) = p.path.get_ident().map(|i| i.to_string()) else {
        return Ok(false);
    };
    let Some(sig) = l.sigs.get(&name).cloned() else {
        return Ok(false);
    };
    if !is_aggregate(&sig.ret) {
        return Ok(false);
    }
    if sig.ret != *expected {
        return Err(err(
            e,
            format!("expected {}, got {}", expected.display(), sig.ret.display()),
        ));
    }
    let args = call_arg_values(l, call)?;
    let arg_tys: Vec<Ty> = args.iter().map(|(_, t)| t.clone()).collect();
    check_call_args(call, &sig.params, &arg_tys, &name)?;
    let mut all = vec![dst];
    all.extend(args.iter().map(|(v, _)| *v));
    l.b.call(intern(&name), &all, 0);
    Ok(true)
}

/// initialize the aggregate `ty` at `dst` from `e`: a literal, a copy from
/// another value, or an aggregate-returning call (spec §14)
fn init_aggregate_at(l: &mut FnLower, dst: VReg, ty: &Ty, e: &Expr) -> Result<(), syn::Error> {
    match ty {
        Ty::Struct(name) => {
            let name = name.clone();
            init_struct_at(l, dst, &name, e)
        }
        Ty::Tuple(elems) => {
            let elems = elems.clone();
            init_tuple_at(l, dst, &elems, e)
        }
        Ty::Array(elem, n) => {
            let (elem, n) = ((**elem).clone(), *n);
            init_array_at(l, dst, &elem, n, e)
        }
        _ => Err(err(
            e,
            format!(
                "{} is not an aggregate (struct, tuple or Buf)",
                ty.display()
            ),
        )),
    }
}

/// initialize a tuple at `base`: a tuple literal, a copy from another tuple, or
/// an aggregate-returning call. Element `i` sits at offset `i`.
fn init_tuple_at(l: &mut FnLower, base: VReg, elems: &[Ty], init: &Expr) -> Result<(), syn::Error> {
    if let Expr::Tuple(t) = init {
        if t.elems.len() != elems.len() {
            return Err(err(
                init,
                format!(
                    "tuple has {} elements, expected {}",
                    t.elems.len(),
                    elems.len()
                ),
            ));
        }
        for (i, (e, ty)) in t.elems.iter().zip(elems).enumerate() {
            let (v, from) = expr(l, e)?.reg(l, e, "tuple element")?;
            let (v, _) = coerce(l, v, &from, ty, e)?;
            l.b.store_mem(base, i as i16, v);
        }
        return Ok(());
    }
    let tuple_ty = Ty::Tuple(elems.to_vec());
    if sret_call_into(l, init, base, &tuple_ty)? {
        return Ok(());
    }
    let (src_base, src_offset, src_ty, _) = place_addr_of(l, init)?;
    if src_ty != tuple_ty {
        return Err(err(
            init,
            format!("expected {}, got {}", tuple_ty.display(), src_ty.display()),
        ));
    }
    let src = place_addr(l, src_base, src_offset);
    let len = l.b.load_imm(elems.len() as u16);
    l.b.call("mem_copy", &[base, src, len], 0);
    Ok(())
}

/// `let (a, b) = expr;` — the value materializes in a frame slot and each name is
/// bound to its word (spec §9c)
fn let_tuple(
    l: &mut FnLower,
    pat: &syn::PatTuple,
    annotated: Option<Ty>,
    init: &Expr,
    at: &Stmt,
) -> Result<(), syn::Error> {
    // a tuple literal binds its elements directly, with no temporary
    if annotated.is_none() {
        if let Expr::Tuple(t) = init {
            if t.elems.len() != pat.elems.len() {
                return Err(err(
                    pat,
                    format!(
                        "pattern has {} names, the value has {} elements",
                        pat.elems.len(),
                        t.elems.len()
                    ),
                ));
            }
            let mut values = vec![];
            for e in &t.elems {
                let (v, ty) = expr(l, e)?.reg(l, e, "tuple element")?;
                let ty = if ty == Ty::UntypedInt { Ty::U16 } else { ty };
                if !matches!(ty, Ty::U16 | Ty::I16 | Ty::Ptr | Ty::Bool) {
                    return Err(err(e, "tuple elements must be u16, i16, Ptr or bool"));
                }
                values.push((v, ty));
            }
            return bind_tuple_names(l, pat, &values, at);
        }
    }
    let init_ty = match annotated {
        Some(ty) => ty,
        None => match sret_call_return(l, init) {
            Some(ty) => ty,
            None => peek_type(l, init).ok_or_else(|| {
                err(
                    init,
                    "cannot infer the tuple type here; annotate the binding",
                )
            })?,
        },
    };
    let Ty::Tuple(elems) = init_ty else {
        return Err(err(
            pat,
            format!("expected a tuple, got {}", init_ty.display()),
        ));
    };
    if elems.len() != pat.elems.len() {
        return Err(err(
            pat,
            format!(
                "pattern has {} names, the value has {} elements",
                pat.elems.len(),
                elems.len()
            ),
        ));
    }
    let slot = l.b.alloc_local_slots(elems.len() as u8);
    let base = l.b.addr_of_local(slot);
    init_tuple_at(l, base, &elems, init)?;
    let values: Vec<(VReg, Ty)> = elems
        .iter()
        .enumerate()
        .map(|(i, ty)| (l.b.load_mem(base, i as i16), ty.clone()))
        .collect();
    bind_tuple_names(l, pat, &values, at)
}

/// bind each name of a tuple pattern to its value
fn bind_tuple_names(
    l: &mut FnLower,
    pat: &syn::PatTuple,
    values: &[(VReg, Ty)],
    at: &Stmt,
) -> Result<(), syn::Error> {
    for (p, (v, ty)) in pat.elems.iter().zip(values) {
        match p {
            Pat::Wild(_) => {}
            Pat::Ident(pi) => {
                if pi.mutability.is_some() {
                    return Err(err(
                        &pi.ident,
                        "a tuple binding is immutable; copy it into a `let mut`",
                    ));
                }
                let name = pi.ident.to_string();
                // an address-taken name still gets its own frame slot
                let kind = if matches!(l.residents.remove(&name), Some(ResidentKind::Scalar)) {
                    let slot = l.b.alloc_local_slots(1);
                    l.b.store_local(slot, *v);
                    VarKind::Local { slot }
                } else {
                    let var = l.b.new_var_typed(if ty.is_fpu() {
                        crate::RegClass::Fpu
                    } else {
                        crate::RegClass::Gpr
                    });
                    l.b.set(var, *v);
                    VarKind::Ssa { var }
                };
                l.declare(
                    name,
                    VarInfo {
                        kind,
                        ty: ty.clone(),
                        mutable: false,
                    },
                    line_of(at),
                );
            }
            other => return Err(err(other, "unsupported tuple pattern element")),
        }
    }
    Ok(())
}

fn signature(
    f: &ItemFn,
    consts: &HashMap<String, (u16, Ty)>,
    structs: &TypeNames,
) -> Result<Sig, syn::Error> {
    if !f.sig.generics.params.is_empty() {
        return Err(err(&f.sig.generics, "generics are not supported"));
    }
    if f.sig.constness.is_some() || f.sig.asyncness.is_some() || f.sig.unsafety.is_some() {
        return Err(err(
            &f.sig,
            "const/async/unsafe functions are not supported",
        ));
    }
    if f.sig.abi.is_some() || f.sig.variadic.is_some() {
        return Err(err(&f.sig, "extern/variadic functions are not supported"));
    }
    let mut params = vec![];
    for arg in &f.sig.inputs {
        match arg {
            syn::FnArg::Typed(pt) => params.push(ty_of(&pt.ty, structs)?),
            syn::FnArg::Receiver(r) => return Err(err(r, "methods are not supported")),
        }
    }
    if params.len() > 6 {
        return Err(err(&f.sig, "too many parameters (max 6)"));
    }
    let ret = match &f.sig.output {
        syn::ReturnType::Default => Ty::Unit,
        // a returned Buf is spelled like any other owned array type
        syn::ReturnType::Type(_, t) => ty_of_maybe_array(t, consts, structs)?,
    };
    if params.iter().any(is_aggregate) {
        return Err(err(
            &f.sig,
            "a struct, tuple or Buf cannot be a parameter; pass a view (`Array<Point>`) and index \
             it, pass the fields, or return it instead (spec §9b/§9c/§14)",
        ));
    }
    // an aggregate return is written through a hidden first parameter (sret)
    if is_aggregate(&ret) && params.len() > 5 {
        return Err(err(
            &f.sig,
            "a function returning an aggregate takes at most 5 parameters: the destination \
             pointer is a hidden first parameter (spec §14)",
        ));
    }
    Ok(Sig { params, ret })
}

// ---------------------------------------------------------------------------
// per-function lowering
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum VarKind {
    /// SSA value (register allocated)
    Ssa { var: VarId },
    /// memory-resident in the stack frame (address-taken locals and arrays)
    Local { slot: u8 },
}

struct VarInfo {
    kind: VarKind,
    ty: Ty,
    mutable: bool,
}

struct FnLower<'a> {
    b: FuncBuilder,
    sigs: &'a HashMap<String, Sig>,
    consts: &'a HashMap<String, (u16, Ty)>,
    globals: &'a Globals,
    residents: HashMap<String, ResidentKind>,
    scopes: Vec<HashMap<String, VarInfo>>,
    /// Inclusive end line for each lexical scope in `scopes`.
    scope_ends: Vec<u32>,
    debug_locals: Vec<DebugVar>,
    ret_ty: Ty,
    /// the hidden destination pointer of an aggregate return (sret, spec §14)
    sret_dst: Option<VarId>,
    /// true once the current block has ended (return/halt)
    dead: bool,
    /// In no-opt builds, scalar locals use stable frame slots so the debugger
    /// can read them throughout their lexical lifetime.
    materialize_debug_locals: bool,
}

/// a lowered expression: a machine value, a boolean condition, or a function item
#[derive(Clone)]
enum Val {
    V(VReg, Ty),
    Bool(BoolExpr),
    FnItem(&'static str),
    Unit,
    /// the never type `!` (diverging expressions like halt)
    Never,
}
impl Val {
    fn reg(
        self,
        l: &mut FnLower,
        at: &impl syn::spanned::Spanned,
        what: &str,
    ) -> Result<(VReg, Ty), syn::Error> {
        match self {
            Val::V(v, ty) => Ok((v, ty)),
            // a condition used where a value is needed is materialized as a
            // stored 0/1 bool (spec §1.1)
            Val::Bool(c) => Ok((bool_value(l, c), Ty::Bool)),
            Val::FnItem(name) => Err(err(
                at,
                format!(
                "function `{name}` used as {what}; assign it to a fn pointer variable or call it"
            ),
            )),
            Val::Unit => Err(err(at, format!("unit value used as {what}"))),
            Val::Never => Ok((0, Ty::Never)),
        }
    }
}

impl FnLower<'_> {
    fn lookup(&self, name: &str) -> Option<&VarInfo> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    /// the destination register of an aggregate return (sret, spec §14)
    fn sret_dst_reg(&mut self) -> VReg {
        let var = self
            .sret_dst
            .expect("an aggregate-returning function has a destination");
        self.b.get(var)
    }
    fn declare(&mut self, name: String, info: VarInfo, start_line: u32) {
        let loc = match info.kind {
            VarKind::Ssa { .. } => VarLoc::Ssa,
            VarKind::Local { slot } => VarLoc::Frame(slot),
        };
        self.debug_locals.push(DebugVar {
            name: name.clone(),
            ty: info.ty.display(),
            loc,
            scope: Some((start_line, *self.scope_ends.last().unwrap())),
        });
        self.scopes.last_mut().unwrap().insert(name, info);
    }
    fn read_var(&mut self, kind: &VarKind) -> VReg {
        match kind {
            VarKind::Ssa { var } => self.b.get(*var),
            VarKind::Local { slot } => self.b.load_local(*slot),
        }
    }
    fn write_var(&mut self, kind: &VarKind, v: VReg) {
        match kind {
            VarKind::Ssa { var } => self.b.set(*var, v),
            VarKind::Local { slot } => self.b.store_local(*slot, v),
        }
    }
}

/// why a variable must live in the stack frame instead of a register
enum ResidentKind {
    Scalar,
    Array,
}

/// the root variable name of a place expression (`x`, `x.f`, `x[i].f` all give `x`)
fn place_base_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Paren(p) => place_base_name(&p.expr),
        Expr::Path(_) => path_ident(e).ok(),
        Expr::Field(f) => place_base_name(&f.base),
        Expr::Index(i) => place_base_name(&i.expr),
        _ => None,
    }
}

/// prescan a function body for names that must be memory-resident:
/// variables whose address is taken (addr_of) and array-typed lets
fn scan_residents(
    blk: &Block,
    consts: &HashMap<String, (u16, Ty)>,
    structs: &TypeNames,
    out: &mut HashMap<String, ResidentKind>,
) -> Result<(), syn::Error> {
    for s in &blk.stmts {
        scan_stmt(s, consts, structs, out)?;
    }
    Ok(())
}
fn scan_stmt(
    s: &Stmt,
    consts: &HashMap<String, (u16, Ty)>,
    structs: &TypeNames,
    out: &mut HashMap<String, ResidentKind>,
) -> Result<(), syn::Error> {
    match s {
        Stmt::Local(local) => {
            if let Pat::Type(pt) = &local.pat {
                if matches!(ty_of_maybe_array(&pt.ty, consts, structs)?, Ty::Array(..)) {
                    if let Pat::Ident(p) = pt.pat.as_ref() {
                        out.insert(p.ident.to_string(), ResidentKind::Array);
                    }
                }
            }
            if let Some((_, init)) = &local.init {
                scan_expr(init, consts, structs, out)?;
            }
        }
        Stmt::Expr(e) | Stmt::Semi(e, _) => scan_expr(e, consts, structs, out)?,
        Stmt::Item(_) => {}
    }
    Ok(())
}
fn scan_expr(
    e: &Expr,
    consts: &HashMap<String, (u16, Ty)>,
    structs: &TypeNames,
    out: &mut HashMap<String, ResidentKind>,
) -> Result<(), syn::Error> {
    match e {
        Expr::Paren(x) => scan_expr(&x.expr, consts, structs, out),
        Expr::Binary(x) => {
            scan_expr(&x.left, consts, structs, out)?;
            scan_expr(&x.right, consts, structs, out)
        }
        Expr::Unary(x) => scan_expr(&x.expr, consts, structs, out),
        Expr::Cast(x) => scan_expr(&x.expr, consts, structs, out),
        Expr::Call(x) => {
            if let Expr::Path(p) = x.func.as_ref() {
                // taking the address of (or a view of) a scalar makes it
                // memory-resident; structs and arrays already are
                if p.path.is_ident("addr_of") || p.path.is_ident("view_of") {
                    if let Some(Expr::Reference(r)) = x.args.first() {
                        if let Some(name) = place_base_name(&r.expr) {
                            out.entry(name).or_insert(ResidentKind::Scalar);
                        }
                    }
                }
            }
            for a in &x.args {
                scan_expr(a, consts, structs, out)?;
            }
            Ok(())
        }
        Expr::MethodCall(x) => {
            scan_expr(&x.receiver, consts, structs, out)?;
            for a in &x.args {
                scan_expr(a, consts, structs, out)?;
            }
            Ok(())
        }
        Expr::Index(x) => {
            scan_expr(&x.expr, consts, structs, out)?;
            scan_expr(&x.index, consts, structs, out)
        }
        Expr::Assign(x) => {
            scan_expr(&x.left, consts, structs, out)?;
            scan_expr(&x.right, consts, structs, out)
        }
        Expr::AssignOp(x) => {
            scan_expr(&x.left, consts, structs, out)?;
            scan_expr(&x.right, consts, structs, out)
        }
        Expr::If(x) => {
            scan_expr(&x.cond, consts, structs, out)?;
            scan_residents(&x.then_branch, consts, structs, out)?;
            if let Some((_, e)) = &x.else_branch {
                scan_expr(e, consts, structs, out)?;
            }
            Ok(())
        }
        Expr::While(x) => {
            scan_expr(&x.cond, consts, structs, out)?;
            scan_residents(&x.body, consts, structs, out)
        }
        Expr::Loop(x) => scan_residents(&x.body, consts, structs, out),
        Expr::ForLoop(x) => {
            scan_expr(&x.expr, consts, structs, out)?;
            scan_residents(&x.body, consts, structs, out)
        }
        Expr::Block(x) => scan_residents(&x.block, consts, structs, out),
        Expr::Return(x) => {
            if let Some(e) = &x.expr {
                scan_expr(e, consts, structs, out)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn lower_fn(
    name: &'static str,
    f: &ItemFn,
    sigs: &HashMap<String, Sig>,
    consts: &HashMap<String, (u16, Ty)>,
    globals: &Globals,
    file: u16,
    materialize_debug_locals: bool,
) -> Result<(IrFunc, FnDebug), syn::Error> {
    let sig = sigs.get(&f.sig.ident.to_string()).unwrap().clone();
    // An aggregate return is written through a hidden first parameter: the callee
    // takes one extra GPR argument and returns nothing in registers (spec §14).
    let sret = is_aggregate(&sig.ret);
    let n_rets = if sig.ret == Ty::Unit || sret { 0 } else { 1 };
    let mut param_classes: Vec<crate::RegClass> = if sret {
        vec![crate::RegClass::Gpr]
    } else {
        vec![]
    };
    param_classes.extend(sig.params.iter().map(|ty| {
        if ty.is_fpu() {
            crate::RegClass::Fpu
        } else {
            crate::RegClass::Gpr
        }
    }));
    let (b, param_vars) = FuncBuilder::new_typed(name, &param_classes, n_rets);
    let sret_dst = if sret { Some(param_vars[0]) } else { None };

    // prescan: which names must be memory-resident
    let mut residents: HashMap<String, ResidentKind> = HashMap::new();
    scan_residents(&f.block, consts, &globals.type_names, &mut residents)?;

    let mut param_names = vec![];
    let mut l = FnLower {
        b,
        sigs,
        consts,
        globals,
        residents,
        scopes: vec![HashMap::new()],
        scope_ends: vec![end_line_of(&f.block)],
        debug_locals: vec![],
        ret_ty: sig.ret.clone(),
        sret_dst,
        dead: false,
        materialize_debug_locals,
    };
    // with sret the hidden destination occupies parameter slot 0
    let declared_start = if sret { 1 } else { 0 };
    for (arg, (var, ty)) in f
        .sig
        .inputs
        .iter()
        .zip(param_vars[declared_start..].iter().zip(sig.params.iter()))
    {
        let syn::FnArg::Typed(pt) = arg else {
            unreachable!()
        };
        let (ident, mutable) = match pt.pat.as_ref() {
            Pat::Ident(p) => {
                let mutable = p.mutability.is_some();
                if mutable && !matches!(ty, Ty::ArrayRef(_)) {
                    return Err(err(
                        &p.ident,
                        "only Array<T> parameters may be declared mut",
                    ));
                }
                (p.ident.to_string(), mutable)
            }
            _ => return Err(err(&pt.pat, "unsupported parameter pattern")),
        };
        param_names.push(intern(&ident));
        // an address-taken param is copied into a frame slot at entry
        let kind = match l.residents.remove(&ident) {
            Some(ResidentKind::Scalar) => {
                if ty.is_fpu() {
                    return Err(err(
                        &pt.pat,
                        "FPU values live in F registers and cannot be address-taken",
                    ));
                }
                let slot = l.b.alloc_local_slots(1);
                let pv = l.b.get(*var);
                l.b.store_local(slot, pv);
                VarKind::Local { slot }
            }
            Some(ResidentKind::Array) => {
                return Err(err(
                    &pt.pat,
                    "owned arrays cannot be parameters; pass Array<T> or Ptr",
                ))
            }
            None => VarKind::Ssa { var: *var },
        };
        l.declare(
            ident,
            VarInfo {
                kind,
                ty: ty.clone(),
                mutable,
            },
            line_of(arg),
        );
    }
    // params occupy the first entries of debug_locals (declaration order);
    // rewrite their locations to ABI registers (frame slot when address-taken)
    for (i, dv) in l.debug_locals.iter_mut().enumerate() {
        if let VarLoc::Ssa = dv.loc {
            dv.loc = VarLoc::ParamIndex((i + declared_start) as u8);
        }
    }
    let ret_names: Vec<&'static str> = if sig.ret == Ty::Unit {
        vec![]
    } else {
        vec!["r"]
    };
    l.b.set_names(&param_names, &ret_names);
    l.b.set_block_line(l.b.entry_block(), line_of(&f.sig.ident));

    // function body; a trailing tail-expression (no semicolon) is the return value
    let stmts = &f.block.stmts;
    let (head, tail) = match stmts.split_last() {
        Some((Stmt::Expr(t), head)) if sig.ret != Ty::Unit => (head, Some(t)),
        _ => (&stmts[..], None),
    };
    for s in head {
        if l.dead {
            return Err(err(s, "unreachable code (after return/halt)"));
        }
        stmt(&mut l, s)?;
    }
    // procedures without explicit return fall through to a plain ret
    if sig.ret == Ty::Unit && !l.dead {
        l.b.ret(&[]);
        l.dead = true;
    }
    if let Some(t) = tail {
        if l.dead {
            return Err(err(t, "unreachable code (after return/halt)"));
        }
        // Unlike ordinary statements, a trailing return expression does not
        // pass through `stmt`, so establish its source line explicitly.
        l.b.set_line_hint(line_of(t));
        // an aggregate tail is written into the caller's destination, not a register
        if is_aggregate(&sig.ret) {
            let dst = l.sret_dst_reg();
            let ret_ty = sig.ret.clone();
            init_aggregate_at(&mut l, dst, &ret_ty, t)
                .map_err(|e| syn::Error::new(e.span(), format!("in fn `{name}`: {e}")))?;
            if !l.dead {
                l.b.ret(&[]);
                l.dead = true;
            }
            return finish_fn(l, name, file);
        }
        let (v, from) = expr(&mut l, t)?
            .reg(&mut l, t, "tail expression")
            .map_err(|e| syn::Error::new(e.span(), format!("in fn `{name}`: {e}")))?;
        // a diverging tail (halt) terminates the block itself
        if !l.dead {
            let expected = sig.ret.clone();
            let (v, _) = coerce(&mut l, v, &from, &expected, t)?;
            l.b.ret(&[v]);
            l.dead = true;
        }
    }
    if sig.ret != Ty::Unit && !l.dead {
        return Err(err(
            &f.block,
            format!("function `{name}` may reach its end without returning a value"),
        ));
    }
    finish_fn(l, name, file)
}

fn finish_fn(l: FnLower, name: &str, file: u16) -> Result<(IrFunc, FnDebug), syn::Error> {
    let fdbg = FnDebug {
        name: name.to_string(),
        file,
        locals: l.debug_locals,
    };
    Ok((l.b.finish(), fdbg))
}

fn block(l: &mut FnLower, blk: &Block) -> Result<(), syn::Error> {
    l.scopes.push(HashMap::new());
    l.scope_ends.push(end_line_of(blk));
    for s in &blk.stmts {
        if l.dead {
            return Err(err(s, "unreachable code (after return/halt)"));
        }
        stmt(l, s)?;
    }
    l.scopes.pop();
    l.scope_ends.pop();
    Ok(())
}

fn stmt(l: &mut FnLower, s: &Stmt) -> Result<(), syn::Error> {
    l.b.set_line_hint(line_of(s));
    match s {
        Stmt::Local(local) => {
            let (inner_pat, annotated) = match &local.pat {
                Pat::Type(pt) => {
                    let ty = ty_of_maybe_array(&pt.ty, l.consts, &l.globals.type_names)?;
                    (pt.pat.as_ref(), Some(ty))
                }
                p => (p, None),
            };
            let init = local
                .init
                .as_ref()
                .ok_or_else(|| err(s, "let without initializer is not supported"))?;

            // `let (q, r) = divmod_pair(a, b);` — tuples destructure by value
            if let Pat::Tuple(pat) = inner_pat {
                return let_tuple(l, pat, annotated, &init.1, s);
            }

            let (ident, mutable) = match inner_pat {
                Pat::Ident(p) => (p.ident.to_string(), p.mutability.is_some()),
                _ => {
                    return Err(err(
                        &local.pat,
                        "unsupported pattern (only plain identifiers and tuples)",
                    ))
                }
            };

            // an aggregate binding: `let mut p: Point = Point { .. };`,
            // `let t = divmod_pair(a, b);` (the type then comes from the callee)
            let aggregate = match (&annotated, sret_call_return(l, &init.1)) {
                (Some(ty), _) if is_aggregate(ty) => Some(ty.clone()),
                (None, Some(ret)) => Some(ret),
                _ => None,
            };
            if let Some(ty) = aggregate {
                let size = aggregate_size(&ty, &l.globals.structs, &init.1)?;
                let slot = l.b.alloc_local_slots(size as u8);
                let base = l.b.addr_of_local(slot);
                init_aggregate_at(l, base, &ty, &init.1)?;
                l.declare(
                    ident,
                    VarInfo {
                        kind: VarKind::Local { slot },
                        ty,
                        mutable,
                    },
                    line_of(s),
                );
                return Ok(());
            }
            if matches!(l.residents.get(&ident), Some(ResidentKind::Array)) {
                return Err(err(
                    &local.pat,
                    "a Buf needs a type annotation like `let mut buf: Buf<u16, N> = Buf::new([0; N]);`",
                ));
            }
            if matches!(init.1.as_ref(), Expr::Struct(_)) {
                return Err(err(
                    &init.1,
                    "a struct literal needs a type annotation: `let p: Point = Point { .. };`",
                ));
            }

            let val = expr(l, &init.1)?;
            // fn pointer binding: `let f: fn(...) = some_fn;`
            let (v, ty) = match (val, &annotated) {
                (Val::FnItem(name), Some(Ty::FnPtr { .. })) => {
                    check_fn_sig(l, name, annotated.as_ref().unwrap(), &init.1)?;
                    let v = l.b.load_func_addr(name);
                    (v, annotated.clone().unwrap())
                }
                (Val::FnItem(name), None) => {
                    return Err(err(&init.1, format!(
                        "function `{name}` needs an explicit fn pointer type: `let f: fn(..) -> .. = {name};`"
                    )))
                }
                (Val::FnItem(name), Some(_)) => {
                    return Err(err(&init.1, format!(
                        "cannot assign function `{name}` to a non-fn-pointer variable"
                    )))
                }
                (val, _) => {
                    let (v, from) = val.reg(l, &init.1, "let initializer")?;
                    let to = annotated.clone().unwrap_or_else(|| from.clone());
                    let (v, _) = coerce(l, v, &from, &to, &init.1)?;
                    (v, to)
                }
            };
            if ty.is_fpu() && matches!(l.residents.get(&ident), Some(ResidentKind::Scalar)) {
                return Err(err(
                    &local.pat,
                    "FPU values live in F registers and cannot be address-taken",
                ));
            }
            // a scalar whose address is taken anywhere is memory-resident
            // (FPU values always stay in SSA: they live in F registers)
            let kind = if !ty.is_fpu()
                && (l.materialize_debug_locals
                    || matches!(l.residents.get(&ident), Some(ResidentKind::Scalar)))
            {
                let slot = l.b.alloc_local_slots(1);
                l.b.store_local(slot, v);
                VarKind::Local { slot }
            } else {
                let var = l.b.new_var_typed(if ty.is_fpu() {
                    crate::RegClass::Fpu
                } else {
                    crate::RegClass::Gpr
                });
                l.b.set(var, v);
                VarKind::Ssa { var }
            };
            l.declare(ident, VarInfo { kind, ty, mutable }, line_of(s));
            Ok(())
        }
        Stmt::Expr(e) | Stmt::Semi(e, _) => stmt_expr(l, e),
        Stmt::Item(item) => Err(err(item, "items inside functions are not supported")),
    }
}

/// statements that are also expressions (control flow, assignment, calls)
fn stmt_expr(l: &mut FnLower, e: &Expr) -> Result<(), syn::Error> {
    match e {
        Expr::Assign(a) => {
            // whole-aggregate assignment: `p = q;` / `p = make();` (spec §14)
            if let Some(ty) = peek_type(l, &a.left) {
                if is_aggregate(&ty) {
                    let (base, offset, _, mutable) = place_addr_of(l, &a.left)?;
                    if !mutable {
                        return Err(err(
                            &a.left,
                            "the aggregate is not mutable (declare it with `let mut`)",
                        ));
                    }
                    let dst = place_addr(l, base, offset);
                    init_aggregate_at(l, dst, &ty, &a.right)?;
                    return Ok(());
                }
            }
            if let Expr::Field(f) = a.left.as_ref() {
                let (base, offset, ty, mutable) = struct_field_place(l, f)?;
                if !mutable {
                    return Err(err(
                        &a.left,
                        "the struct is not mutable (declare it with `let mut`)",
                    ));
                }
                let (v, from) = expr(l, &a.right)?.reg(l, &a.right, "field assignment")?;
                let (v, _) = coerce(l, v, &from, &ty, &a.right)?;
                l.b.store_mem(base, offset, v);
                return Ok(());
            }
            if let Expr::Index(index) = a.left.as_ref() {
                ensure_mutable_array_view(l, &index.expr)?;
                let (base, off, elem, mutable) = array_index_addr(l, index)?;
                if !mutable {
                    return Err(err(
                        &a.left,
                        "the array field is not mutable (declare the struct with `let mut`)",
                    ));
                }
                let (v, from) = expr(l, &a.right)?.reg(l, &a.right, "array assignment")?;
                let (v, _) = coerce(l, v, &from, &elem, &a.right)?;
                l.b.store_mem(base, off, v);
                return Ok(());
            }
            let name = path_ident(&a.left)?;
            let info = l
                .lookup(&name)
                .ok_or_else(|| err(&a.left, format!("undefined variable `{name}`")))?;
            if !info.mutable {
                return Err(err(
                    &a.left,
                    format!("`{name}` is not mutable (declare with `let mut`)"),
                ));
            }
            let (kind, ty) = (info.kind.clone(), info.ty.clone());
            let val = expr(l, &a.right)?;
            // fn pointer reassignment: `f = other_fn;`
            if let (Val::FnItem(fname), Ty::FnPtr { .. }) = (&val, &ty) {
                check_fn_sig(l, fname, &ty, &a.right)?;
                let v = l.b.load_func_addr(fname);
                l.write_var(&kind, v);
                return Ok(());
            }
            let (v, _) = val.reg(l, &a.right, "assignment")?;
            let (v, _) = coerce(l, v, &ty, &ty, &a.right)?;
            l.write_var(&kind, v);
            Ok(())
        }
        Expr::AssignOp(a) => {
            if let Expr::Field(f) = a.left.as_ref() {
                let (base, offset, ty, mutable) = struct_field_place(l, f)?;
                if !mutable {
                    return Err(err(
                        &a.left,
                        "the struct is not mutable (declare it with `let mut`)",
                    ));
                }
                if !ty.is_int() {
                    return Err(err(&a.left, "compound assignment only works on integers"));
                }
                let cur = l.b.load_mem(base, offset);
                let (rhs, rhs_ty) = expr(l, &a.right)?.reg(l, &a.right, "compound assignment")?;
                let (rhs, _) = coerce(l, rhs, &rhs_ty, &ty, &a.right)?;
                let value = match a.op {
                    SBinOp::AddEq(_) => l.b.bin(BinOp::Add, cur, rhs),
                    SBinOp::SubEq(_) => l.b.bin(BinOp::Sub, cur, rhs),
                    SBinOp::BitAndEq(_) => l.b.bin(BinOp::And, cur, rhs),
                    SBinOp::BitOrEq(_) => l.b.bin(BinOp::Or, cur, rhs),
                    SBinOp::BitXorEq(_) => l.b.bin(BinOp::Xor, cur, rhs),
                    SBinOp::MulEq(_) => {
                        l.b.mul(crate::MulWindow::Low, cur, crate::IntOperand::Reg(rhs))
                    }
                    SBinOp::ShlEq(_) | SBinOp::ShrEq(_) => {
                        let amount = shift_operand(l, &a.right)?;
                        l.b.shift_op(shift_op(&a.op, &ty), cur, amount)
                    }
                    _ => return Err(err(&a.op, "unsupported compound assignment operator")),
                };
                l.b.store_mem(base, offset, value);
                return Ok(());
            }
            if let Expr::Index(index) = a.left.as_ref() {
                ensure_mutable_array_view(l, &index.expr)?;
                let (base, off, elem, mutable) = array_index_addr(l, index)?;
                if !mutable {
                    return Err(err(
                        &a.left,
                        "the array field is not mutable (declare the struct with `let mut`)",
                    ));
                }
                let cur = l.b.load_mem(base, off);
                let (rhs, rhs_ty) = expr(l, &a.right)?.reg(l, &a.right, "compound assignment")?;
                let (rhs, _) = coerce(l, rhs, &rhs_ty, &elem, &a.right)?;
                let value = match a.op {
                    SBinOp::AddEq(_) => l.b.bin(BinOp::Add, cur, rhs),
                    SBinOp::SubEq(_) => l.b.bin(BinOp::Sub, cur, rhs),
                    SBinOp::BitAndEq(_) => l.b.bin(BinOp::And, cur, rhs),
                    SBinOp::BitOrEq(_) => l.b.bin(BinOp::Or, cur, rhs),
                    SBinOp::BitXorEq(_) => l.b.bin(BinOp::Xor, cur, rhs),
                    SBinOp::MulEq(_) => {
                        l.b.mul(crate::MulWindow::Low, cur, crate::IntOperand::Reg(rhs))
                    }
                    SBinOp::ShlEq(_) | SBinOp::ShrEq(_) => {
                        let amount = shift_operand(l, &a.right)?;
                        l.b.shift_op(shift_op(&a.op, &elem), cur, amount)
                    }
                    _ => return Err(err(&a.op, "unsupported compound assignment operator")),
                };
                l.b.store_mem(base, off, value);
                return Ok(());
            }
            let name = path_ident(&a.left)?;
            let info = l
                .lookup(&name)
                .ok_or_else(|| err(&a.left, format!("undefined variable `{name}`")))?;
            if !info.mutable {
                return Err(err(
                    &a.left,
                    format!("`{name}` is not mutable (declare with `let mut`)"),
                ));
            }
            let (kind, ty) = (info.kind.clone(), info.ty.clone());
            if !ty.is_int() {
                return Err(err(&a.left, "compound assignment only works on integers"));
            }
            let cur = l.read_var(&kind);
            let (rhs, _) = expr(l, &a.right)?.reg(l, &a.right, "compound assignment")?;
            let op = match a.op {
                SBinOp::AddEq(_) => BinOp::Add,
                SBinOp::SubEq(_) => BinOp::Sub,
                SBinOp::BitAndEq(_) => BinOp::And,
                SBinOp::BitOrEq(_) => BinOp::Or,
                SBinOp::BitXorEq(_) => BinOp::Xor,
                SBinOp::MulEq(_) => {
                    let v =
                        l.b.mul(crate::MulWindow::Low, cur, crate::IntOperand::Reg(rhs));
                    l.write_var(&kind, v);
                    return Ok(());
                }
                SBinOp::ShlEq(_) | SBinOp::ShrEq(_) => {
                    let amount = shift_operand(l, &a.right)?;
                    let sop = shift_op(&a.op, &ty);
                    let v = l.b.shift_op(sop, cur, amount);
                    l.write_var(&kind, v);
                    return Ok(());
                }
                _ => return Err(err(&a.op, "unsupported compound assignment operator")),
            };
            let v = l.b.bin(op, cur, rhs);
            l.write_var(&kind, v);
            Ok(())
        }
        Expr::Return(r) => {
            let ret_ty = l.ret_ty.clone();
            if is_aggregate(&ret_ty) {
                let Some(e) = &r.expr else {
                    return Err(err(
                        r,
                        format!("missing return value (expected {})", ret_ty.display()),
                    ));
                };
                let dst = l.sret_dst_reg();
                init_aggregate_at(l, dst, &ret_ty, e)?;
                l.b.ret(&[]);
                l.dead = true;
                return Ok(());
            }
            match (&ret_ty, &r.expr) {
                (Ty::Unit, None) => l.b.ret(&[]),
                (Ty::Unit, Some(e)) => {
                    return Err(err(
                        e,
                        "returning a value from a function without return type",
                    ))
                }
                (expected, Some(e)) => {
                    let (v, from) = expr(l, e)?.reg(l, e, "return value")?;
                    let (v, _) = coerce(l, v, &from, expected, e)?;
                    l.b.ret(&[v]);
                }
                (expected, None) => {
                    return Err(err(
                        r,
                        format!("missing return value (expected {})", expected.display()),
                    ))
                }
            }
            l.dead = true;
            Ok(())
        }
        Expr::If(_) | Expr::While(_) | Expr::Loop(_) | Expr::ForLoop(_) => control_flow(l, e),
        Expr::Match(m) => lower_match(l, m),
        Expr::Break(b) => {
            if b.expr.is_some() {
                return Err(err(&b.break_token, "`break` with a value is not supported"));
            }
            let label = jump_label(&b.label);
            if !l.b.break_labeled(label) {
                return Err(err(
                    e,
                    match label {
                        Some(name) => format!("no enclosing loop labeled '{name}"),
                        None => "`break` outside of a loop".to_string(),
                    },
                ));
            }
            l.dead = true;
            Ok(())
        }
        Expr::Continue(c) => {
            let label = jump_label(&c.label);
            if !l.b.continue_labeled(label) {
                return Err(err(
                    e,
                    match label {
                        Some(name) => format!("no enclosing loop labeled '{name}"),
                        None => "`continue` outside of a loop".to_string(),
                    },
                ));
            }
            l.dead = true;
            Ok(())
        }
        _ => {
            // expression statement for side effects (calls, stores, intrinsics)
            let val = expr(l, e)?;
            match val {
                Val::Unit | Val::V(_, _) | Val::Bool(_) | Val::Never => Ok(()),
                Val::FnItem(name) => Err(err(
                    e,
                    format!("function `{name}` as a statement; did you mean to call it?"),
                )),
            }
        }
    }
}

/// the `'name` of a labeled loop, for the builder's loop stack
fn loop_label(label: &Option<syn::Label>) -> Option<&'static str> {
    label.as_ref().map(|l| intern(&l.name.ident.to_string()))
}

/// the `'name` of a `break 'name` / `continue 'name` target
fn jump_label(label: &Option<syn::Lifetime>) -> Option<&'static str> {
    label.as_ref().map(|l| intern(&l.ident.to_string()))
}

/// `match` lowered to a chain of `Br`s (the IR has no jump-table terminator).
/// Patterns are integer literals, enum variants and `_` — no bindings, guards,
/// ranges or nesting. Exhaustiveness follows Rust: an enum match must list every
/// variant or have a `_` arm, and an integer match needs `_`.
fn lower_match(l: &mut FnLower, m: &syn::ExprMatch) -> Result<(), syn::Error> {
    let (scrut, scrut_ty) = expr(l, &m.expr)?.reg(l, &m.expr, "match scrutinee")?;
    if !matches!(scrut_ty, Ty::U16 | Ty::I16 | Ty::Ptr | Ty::Enum(_)) {
        return Err(err(
            &m.expr,
            format!(
                "match needs an integer or enum value, got {}",
                scrut_ty.display()
            ),
        ));
    }
    let mut arms: Vec<(Option<u16>, &syn::Arm)> = vec![];
    let mut seen: Vec<u16> = vec![];
    let mut wild = false;
    for arm in &m.arms {
        if let Some((_, guard)) = &arm.guard {
            return Err(err(guard, "match guards are not supported"));
        }
        let value = match &arm.pat {
            syn::Pat::Wild(_) => {
                wild = true;
                None
            }
            syn::Pat::Lit(lit) => Some(literal_pattern(lit)?),
            syn::Pat::Path(path) => Some(variant_pattern(l, &scrut_ty, path)?),
            syn::Pat::Ident(ident) => {
                return Err(err(
                    &ident.ident,
                    "match bindings are not supported; use `_` or a constant",
                ))
            }
            other => {
                return Err(err(
                    other,
                    "unsupported pattern (an integer literal, an enum variant or `_`)",
                ))
            }
        };
        if let Some(v) = value {
            if seen.contains(&v) {
                return Err(err(&arm.pat, format!("duplicate match arm for {v}")));
            }
            seen.push(v);
        }
        arms.push((value, arm));
    }
    if !wild {
        match &scrut_ty {
            Ty::Enum(name) => {
                let def = l
                    .globals
                    .enums
                    .get(name)
                    .cloned()
                    .ok_or_else(|| err(&m.expr, format!("unknown enum `{name}`")))?;
                let missing: Vec<&str> = def
                    .variants
                    .iter()
                    .filter(|(_, value)| !seen.contains(value))
                    .map(|(variant, _)| variant.as_str())
                    .collect();
                if !missing.is_empty() {
                    return Err(err(
                        m,
                        format!(
                            "match on `{name}` is not exhaustive: missing {} (add the arm or `_`)",
                            missing.join(", ")
                        ),
                    ));
                }
            }
            _ => return Err(err(m, "a match on integers needs a `_` arm")),
        }
    }
    let join = l.b.raw_block(&[]);
    for (value, arm) in arms {
        match value {
            Some(pattern) => {
                let body = l.b.raw_block(&[]);
                let next = l.b.raw_block(&[]);
                let cmp = l.b.cmp(scrut, CmpRhs::Imm(pattern), CompareOp::Equal);
                l.b.br(cmp, body, next);
                l.b.enter_block(body);
                lower_match_arm(l, arm)?;
                // wire this arm's body to the join and continue in `next`
                l.b.mid_if_else(next, join);
                l.dead = false;
            }
            None => {
                lower_match_arm(l, arm)?;
                l.b.end_if(join);
                l.dead = false;
                return Ok(());
            }
        }
    }
    l.b.end_if(join);
    l.dead = false;
    Ok(())
}

/// the body of one `match` arm (a block, or any other statement expression)
fn lower_match_arm(l: &mut FnLower, arm: &syn::Arm) -> Result<(), syn::Error> {
    match arm.body.as_ref() {
        Expr::Block(b) => block(l, &b.block),
        other => stmt_expr(l, other),
    }
}

/// an integer pattern literal (`3u16`, `-1i16`)
fn literal_pattern(lit: &syn::PatLit) -> Result<u16, syn::Error> {
    let negative = matches!(
        lit.expr.as_ref(),
        Expr::Unary(u) if matches!(u.op, SUnOp::Neg(_))
    );
    let inner = match lit.expr.as_ref() {
        Expr::Unary(u) => u.expr.as_ref(),
        other => other,
    };
    let Expr::Lit(value) = inner else {
        return Err(err(lit, "a match pattern must be an integer literal"));
    };
    let Lit::Int(int) = &value.lit else {
        return Err(err(lit, "a match pattern must be an integer literal"));
    };
    let v = lit_int_value(int)?;
    if v > u16::MAX as u64 {
        return Err(err(lit, "literal out of 16-bit range"));
    }
    let v = v as u16;
    Ok(if negative { v.wrapping_neg() } else { v })
}

/// an enum variant pattern (`Trace::Run`), checked against the scrutinee's enum
fn variant_pattern(l: &FnLower, scrut_ty: &Ty, path: &syn::PatPath) -> Result<u16, syn::Error> {
    if path.qself.is_some() || path.path.segments.len() != 2 {
        return Err(err(
            path,
            "a match pattern must be an integer literal, an enum variant or `_`",
        ));
    }
    let enum_name = path.path.segments[0].ident.to_string();
    let variant = path.path.segments[1].ident.to_string();
    let Some(def) = l.globals.enums.get(&enum_name) else {
        return Err(err(path, format!("unknown enum `{enum_name}`")));
    };
    let Some((_, value)) = def.variants.iter().find(|(name, _)| *name == variant) else {
        let known = def
            .variants
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(err(
            path,
            format!("enum `{enum_name}` has no variant `{variant}` (variants: {known})"),
        ));
    };
    if scrut_ty != &Ty::Enum(enum_name.clone()) {
        return Err(err(
            path,
            format!(
                "pattern `{enum_name}::{variant}` does not match {}",
                scrut_ty.display()
            ),
        ));
    }
    Ok(*value)
}

fn control_flow(l: &mut FnLower, e: &Expr) -> Result<(), syn::Error> {
    match e {
        Expr::If(i) => {
            match &i.else_branch {
                None => {
                    let then_b = l.b.raw_block(&[]);
                    let join = l.b.raw_block(&[]);
                    l.b.set_block_line(then_b, line_of(&i.if_token));
                    cond_lazy(l, &i.cond, then_b, join)?;
                    l.b.enter_block(then_b);
                    block(l, &i.then_branch)?;
                    l.b.end_if(join);
                    l.dead = false;
                }
                Some((_, else_e)) => {
                    let then_b = l.b.raw_block(&[]);
                    let else_b = l.b.raw_block(&[]);
                    let join = l.b.raw_block(&[]);
                    l.b.set_block_line(then_b, line_of(&i.if_token));
                    l.b.set_block_line(else_b, line_of(&i.if_token));
                    cond_lazy(l, &i.cond, then_b, else_b)?;
                    l.b.enter_block(then_b);
                    block(l, &i.then_branch)?;
                    let then_dead = l.dead;
                    l.dead = false;
                    l.b.mid_if_else(else_b, join);
                    match else_e.as_ref() {
                        Expr::Block(b) => block(l, &b.block)?,
                        Expr::If(nested) => control_flow(l, &Expr::If(nested.clone()))?,
                        _ => return Err(err(else_e, "expected block or else-if")),
                    }
                    let else_dead = l.dead;
                    l.b.end_if_else(join);
                    l.dead = then_dead && else_dead;
                }
            }
            Ok(())
        }
        Expr::While(w) => {
            let (header, body_b, exit) = l.b.begin_while();
            l.b.set_block_line(header, line_of(&w.while_token));
            l.b.set_block_line(body_b, line_of(&w.while_token));
            cond_lazy(l, &w.cond, body_b, exit)?;
            l.b.begin_loop_body(header, body_b, exit, loop_label(&w.label));
            block(l, &w.body)?;
            l.b.end_while(header, exit);
            l.dead = false;
            Ok(())
        }
        Expr::Loop(lp) => {
            let (header, body_b, exit) = l.b.begin_while();
            l.b.jmp(body_b);
            l.b.begin_loop_body(header, body_b, exit, loop_label(&lp.label));
            block(l, &lp.body)?;
            l.b.end_while(header, exit);
            l.dead = false;
            Ok(())
        }
        Expr::ForLoop(fl) => {
            let ident = match &fl.pat {
                Pat::Ident(p) => {
                    if p.mutability.is_some() {
                        return Err(err(&p.ident, "loop variable must not be declared mut"));
                    }
                    p.ident.to_string()
                }
                _ => return Err(err(&fl.pat, "unsupported loop pattern")),
            };
            let (from, to, inclusive) = match fl.expr.as_ref() {
                Expr::Range(r) => {
                    let from = r
                        .from
                        .as_deref()
                        .ok_or_else(|| err(&fl.expr, "range needs a start"))?;
                    let to =
                        r.to.as_deref()
                            .ok_or_else(|| err(&fl.expr, "range needs an end"))?;
                    let inclusive = matches!(r.limits, syn::RangeLimits::Closed(_));
                    (from, to, inclusive)
                }
                _ => return Err(err(&fl.expr, "for loops need a range (a..b or a..=b)")),
            };
            let (from_v, from_ty) = expr(l, from)?.reg(l, from, "range start")?;
            let (to_v, to_ty) = expr(l, to)?.reg(l, to, "range end")?;
            let ty = match unify_int(from_ty.clone(), to_ty.clone()) {
                Some(t) => t,
                None => {
                    return Err(err(
                        &fl.expr,
                        format!(
                            "range type mismatch: {} vs {}",
                            from_ty.display(),
                            to_ty.display()
                        ),
                    ))
                }
            };

            l.scopes.push(HashMap::new());
            l.scope_ends.push(end_line_of(&fl.body));
            let ivar = l.b.new_var();
            l.b.set(ivar, from_v);
            l.declare(
                ident,
                VarInfo {
                    kind: VarKind::Ssa { var: ivar },
                    ty: ty.clone(),
                    mutable: true, // incremented by the loop itself
                },
                line_of(fl),
            );

            let (header, body_b, exit) = l.b.begin_while();
            {
                let i = l.b.get(ivar);
                l.b.br(
                    Cmp {
                        lhs: i,
                        rhs: CmpRhs::Reg(to_v),
                        cond: if inclusive {
                            CompareOp::LessEqual
                        } else {
                            CompareOp::Less
                        },
                        signed: ty == Ty::I16,
                    },
                    body_b,
                    exit,
                );
            }
            l.b.set_block_line(body_b, line_of(&fl.for_token));
            l.b.begin_loop_body(header, body_b, exit, loop_label(&fl.label));
            // continue must hit the increment block, not the header
            let incr = l.b.begin_continue_block();
            l.b.set_block_line(incr, line_of(&fl.for_token));
            block(l, &fl.body)?;
            l.dead = false;
            l.b.end_continue_block(incr);
            // inclusive ranges: stop before the increment wraps the end value
            if inclusive {
                let i = l.b.get(ivar);
                let stop = l.b.raw_block(&[]);
                let go = l.b.raw_block(&[]);
                l.b.br(
                    Cmp {
                        lhs: i,
                        rhs: CmpRhs::Reg(to_v),
                        cond: CompareOp::Equal,
                        signed: ty == Ty::I16,
                    },
                    stop,
                    go,
                );
                l.b.enter_block(stop);
                l.b.jmp(exit);
                l.b.enter_block(go);
            }
            {
                let i = l.b.get(ivar);
                let one = l.b.load_imm(1);
                let i = l.b.bin(BinOp::Add, i, one);
                l.b.set(ivar, i);
            }
            l.b.end_while(header, exit);
            l.scopes.pop();
            l.scope_ends.pop();
            l.dead = false;
            Ok(())
        }
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

fn expr(l: &mut FnLower, e: &Expr) -> Result<Val, syn::Error> {
    match e {
        Expr::Paren(p) => expr(l, &p.expr),
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(i) => {
                let v = lit_int_value(i)?;
                let suffix = i.suffix();
                let ty = match suffix {
                    "" => Ty::UntypedInt,
                    "u16" => Ty::U16,
                    "i16" => Ty::I16,
                    _ => {
                        return Err(err(
                            &lit.lit,
                            format!("unsupported literal suffix `{suffix}`"),
                        ))
                    }
                };
                if v > u16::MAX as u64 {
                    return Err(err(&lit.lit, "literal out of 16-bit range"));
                }
                Ok(Val::V(l.b.load_imm(v as u16), ty))
            }
            Lit::Bool(b) => Ok(Val::Bool(if b.value {
                true_cond(l)
            } else {
                false_cond(l)
            })),
            Lit::Str(_) => Err(err(&lit.lit, "string literals are not supported")),
            Lit::Float(_) => Err(err(&lit.lit, "float literals are not supported")),
            _ => Err(err(
                &lit.lit,
                "unsupported literal (only integers and bools)",
            )),
        },
        Expr::Path(p) => {
            // `E::A` names a variant constant (spec §9d)
            if p.path.segments.len() == 2 && p.qself.is_none() {
                let enum_name = p.path.segments[0].ident.to_string();
                let variant = p.path.segments[1].ident.to_string();
                if let Some(def) = l.globals.enums.get(&enum_name) {
                    let Some((_, value)) = def
                        .variants
                        .iter()
                        .find(|(name, _)| *name == variant)
                        .map(|(name, value)| (name.clone(), *value))
                    else {
                        let known = def
                            .variants
                            .iter()
                            .map(|(name, _)| name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        return Err(err(
                            &p.path,
                            format!("enum `{enum_name}` has no variant `{variant}` (variants: {known})"),
                        ));
                    };
                    return Ok(Val::V(l.b.load_imm(value), Ty::Enum(enum_name)));
                }
            }
            let name = path_ident(e)?;
            if let Some(info) = l.lookup(&name) {
                let (kind, ty) = (info.kind.clone(), info.ty.clone());
                if matches!(ty, Ty::Array(..)) {
                    return Err(err(&p, format!(
                        "array `{name}` used as a value; use {name}.as_array(), {name}.as_ptr(), or {name}.read(i)"
                    )));
                }
                if matches!(ty, Ty::Struct(_)) {
                    let shown = ty.display();
                    return Err(err(
                        &p,
                        format!(
                            "struct `{name}` used as a value; read a field (`{name}.x`), copy it \
                             with a typed `let` (e.g. `let q: {shown} = {name};`), or take its address"
                        ),
                    ));
                }
                let v = l.read_var(&kind);
                return Ok(Val::V(v, ty));
            }
            if let Some((v, ty)) = l.consts.get(&name) {
                let (v, ty) = (*v, ty.clone());
                return Ok(Val::V(l.b.load_imm(v), ty));
            }
            if let Some((addr, ty)) = l.globals.scalars.get(&name) {
                let (addr, ty) = (*addr, ty.clone());
                let base = l.b.load_imm(addr);
                let v = l.b.load_mem(base, 0);
                return Ok(Val::V(v, ty));
            }
            if let Some(&(addr, _, _)) = l.globals.arrays.get(&name) {
                // a global array used as a value decays to its address (like C)
                return Ok(Val::V(l.b.load_imm(addr), Ty::Ptr));
            }
            if let Some((addr, ty)) = l.globals.aggregates.get(&name) {
                // an aggregate static *is* its address, like a local aggregate
                let (addr, ty) = (*addr, ty.clone());
                return Ok(Val::V(l.b.load_imm(addr), ty));
            }
            if l.sigs.contains_key(&name) {
                return Ok(Val::FnItem(intern(&name)));
            }
            Err(err(&p, format!("undefined name `{name}`")))
        }
        Expr::Unary(u) => match u.op {
            SUnOp::Neg(_) => {
                let (v, ty) = expr(l, &u.expr)?.reg(l, &u.expr, "negation")?;
                if ty.is_fpu() {
                    return Ok(Val::V(l.b.funary(crate::FUnOp::Neg, v), ty));
                }
                if ty == Ty::U16 {
                    return Err(err(&u.op, "unary `-` is only allowed on i16"));
                }
                Ok(Val::V(l.b.un(UnOp::Neg, v), Ty::I16))
            }
            SUnOp::Not(_) => {
                let val = expr(l, &u.expr)?;
                match val {
                    Val::V(v, ty) if ty.is_int() => Ok(Val::V(l.b.un(UnOp::Inv, v), ty)),
                    // stored bool: `!b` is `b ^ 1`
                    Val::V(v, Ty::Bool) => {
                        let one = l.b.load_imm(1);
                        Ok(Val::V(l.b.bin(crate::BinOp::Xor, v, one), Ty::Bool))
                    }
                    Val::V(_, ty) => Err(err(
                        &u.op,
                        format!("`!` does not apply to {}", ty.display()),
                    )),
                    Val::Bool(c) => Ok(Val::Bool(BoolExpr::Not(Box::new(c)))),
                    Val::FnItem(_) => Err(err(&u.op, "`!` does not apply to functions")),
                    Val::Unit => Err(err(&u.op, "`!` does not apply to unit")),
                    Val::Never => Err(err(&u.op, "`!` does not apply to never")),
                }
            }
            _ => Err(err(&u.op, "unsupported unary operator")),
        },
        Expr::Binary(b) => {
            use SBinOp::*;
            match b.op {
                Add(_) | Sub(_) | BitAnd(_) | BitOr(_) | BitXor(_) => {
                    // a lone constant moves to the rhs of a commutative
                    // operation so it can select an immediate encoding
                    let commutative = !matches!(b.op, Sub(_));
                    let swap = commutative
                        && immediate_operand(l, &b.left).is_some()
                        && immediate_operand(l, &b.right).is_none();
                    let (left_e, right_e) = if swap {
                        (&b.right, &b.left)
                    } else {
                        (&b.left, &b.right)
                    };
                    let (lhs, lt) = expr(l, left_e)?.reg(l, left_e, "binary operand")?;
                    let literal_rhs = immediate_operand(l, right_e);
                    let (rhs, rt) = match literal_rhs {
                        Some((_, ref ty)) => (None, ty.clone()),
                        None => {
                            let (v, t) = expr(l, right_e)?.reg(l, right_e, "binary operand")?;
                            (Some(v), t)
                        }
                    };
                    if lt.is_fpu() || rt.is_fpu() {
                        // per-lane FADD/FSUB on matching FPU types
                        let fop = match b.op {
                            Add(_) => crate::FBinOp::Add,
                            Sub(_) => crate::FBinOp::Sub,
                            _ => {
                                return Err(err(
                                    &b.op,
                                    "bitwise operators do not apply to fix16/vecN",
                                ))
                            }
                        };
                        if lt != rt {
                            return Err(err(
                                e,
                                format!(
                                    "type mismatch: {} vs {} (no implicit conversions)",
                                    lt.display(),
                                    rt.display()
                                ),
                            ));
                        }
                        let rhs = match rhs {
                            Some(v) => v,
                            None => l.b.load_imm(literal_rhs.unwrap().0),
                        };
                        return Ok(Val::V(l.b.fbin(fop, lhs, rhs), lt));
                    }
                    let ty = unify_int(lt.clone(), rt.clone()).ok_or_else(|| {
                        err(
                            e,
                            format!(
                                "type mismatch: {} vs {} (cast with `as`)",
                                lt.display(),
                                rt.display()
                            ),
                        )
                    })?;
                    let op = match b.op {
                        Add(_) => BinOp::Add,
                        Sub(_) => BinOp::Sub,
                        BitAnd(_) => BinOp::And,
                        BitOr(_) => BinOp::Or,
                        BitXor(_) => BinOp::Xor,
                        _ => unreachable!(),
                    };
                    let rhs_op = match rhs {
                        Some(v) => crate::IntOperand::Reg(v),
                        None => crate::IntOperand::Imm(literal_rhs.unwrap().0),
                    };
                    Ok(Val::V(l.b.bin_op(op, lhs, rhs_op), ty))
                }
                Shl(_) | Shr(_) => {
                    let (lhs, lt) = expr(l, &b.left)?.reg(l, &b.left, "shift value")?;
                    if !lt.is_int() {
                        return Err(err(&b.left, "shifts only work on integers"));
                    }
                    let amount = shift_operand(l, &b.right)?;
                    Ok(Val::V(l.b.shift_op(shift_op(&b.op, &lt), lhs, amount), lt))
                }
                Mul(_) => {
                    // a lone constant moves to the rhs to select MULI
                    let swap = immediate_operand(l, &b.left).is_some()
                        && immediate_operand(l, &b.right).is_none();
                    let (left_e, right_e) = if swap {
                        (&b.right, &b.left)
                    } else {
                        (&b.left, &b.right)
                    };
                    let (lhs, lt) = expr(l, left_e)?.reg(l, left_e, "binary operand")?;
                    let literal_rhs = immediate_operand(l, right_e);
                    let (rhs, rt) = match literal_rhs {
                        Some((_, ref ty)) => (None, ty.clone()),
                        None => {
                            let (v, t) = expr(l, right_e)?.reg(l, right_e, "binary operand")?;
                            (Some(v), t)
                        }
                    };
                    if lt.is_fpu() || rt.is_fpu() {
                        // same-type: per-lane FMUL; vecN scaled by fix16: an
                        // explicit ACC splat of the scalar followed by FMUL
                        // (the ISA has no scalar-by-vector instruction)
                        let rhs = match rhs {
                            Some(v) => v,
                            None => l.b.load_imm(literal_rhs.unwrap().0),
                        };
                        if lt == rt {
                            return Ok(Val::V(l.b.fbin(crate::FBinOp::Mul, lhs, rhs), lt));
                        }
                        let (vector, scalar, ty) = if lt != Ty::Fix16 && rt == Ty::Fix16 {
                            (lhs, rhs, lt)
                        } else if lt == Ty::Fix16 && rt != Ty::Fix16 && rt.is_fpu() {
                            (rhs, lhs, rt)
                        } else {
                            return Err(err(
                                e,
                                format!(
                                    "cannot multiply {} by {} (want vecN * fix16 or same types)",
                                    lt.display(),
                                    rt.display()
                                ),
                            ));
                        };
                        l.b.facc_load(scalar, 0);
                        let splat = l.b.facc_store(0b1111);
                        return Ok(Val::V(l.b.fbin(crate::FBinOp::Mul, vector, splat), ty));
                    }
                    // integers: hardware MUL on CpuV3, library call on CpuV2
                    let ty = unify_int(lt.clone(), rt.clone()).ok_or_else(|| {
                        err(
                            e,
                            format!(
                                "type mismatch: {} vs {} (cast with `as`)",
                                lt.display(),
                                rt.display()
                            ),
                        )
                    })?;
                    let rhs_op = match rhs {
                        Some(v) => crate::IntOperand::Reg(v),
                        None => crate::IntOperand::Imm(literal_rhs.unwrap().0),
                    };
                    Ok(Val::V(l.b.mul(crate::MulWindow::Low, lhs, rhs_op), ty))
                }
                Div(_) | Rem(_) => {
                    // Neither ISA has a divide, so `/` and `%` lower to the
                    // rcc_std software routine (the `mul_16x16` precedent). A
                    // constant divisor on unsigned values is cheaper: a power of
                    // two becomes a shift or a mask, and any other constant that
                    // has a 16-bit round-up magic becomes one `MUL16` plus a shift.
                    let (lhs, lt) = expr(l, &b.left)?.reg(l, &b.left, "binary operand")?;
                    let (rhs, rt) = expr(l, &b.right)?.reg(l, &b.right, "binary operand")?;
                    let ty = unify_int(lt.clone(), rt.clone()).ok_or_else(|| {
                        err(
                            e,
                            format!(
                                "type mismatch: {} vs {} (cast with `as`)",
                                lt.display(),
                                rt.display()
                            ),
                        )
                    })?;
                    let remainder = matches!(b.op, Rem(_));
                    if ty == Ty::U16 {
                        if let Some((value, _)) = immediate_operand(l, &b.right) {
                            if value != 0 && value.is_power_of_two() {
                                let lowered = if remainder {
                                    let mask = l.b.load_imm(value - 1);
                                    l.b.bin(crate::BinOp::And, lhs, mask)
                                } else {
                                    l.b.shift_op(
                                        ShiftOp::Lsr,
                                        lhs,
                                        crate::IntOperand::Imm(value.trailing_zeros() as u16),
                                    )
                                };
                                return Ok(Val::V(lowered, ty));
                            }
                            if let Some(magic) = magic_divide_u16(value) {
                                let magic_constant = match magic {
                                    MagicDivide::High { m, .. } | MagicDivide::Add { m, .. } => m,
                                };
                                let high = l.b.mul(
                                    crate::MulWindow::Shift16,
                                    lhs,
                                    crate::IntOperand::Imm(magic_constant),
                                );
                                let quotient = match magic {
                                    MagicDivide::High { s, .. } => {
                                        if s == 0 {
                                            high
                                        } else {
                                            l.b.shift(ShiftOp::Lsr, high, s)
                                        }
                                    }
                                    MagicDivide::Add { s, .. } => {
                                        // (x & t) + ((x ^ t) >> 1) is (x + t) >> 1
                                        // without letting the 17-bit sum overflow
                                        let both = l.b.bin(crate::BinOp::And, lhs, high);
                                        let diff = l.b.bin(crate::BinOp::Xor, lhs, high);
                                        let half = l.b.shift(ShiftOp::Lsr, diff, 1);
                                        let average = l.b.bin(crate::BinOp::Add, both, half);
                                        l.b.shift(ShiftOp::Lsr, average, s - 1)
                                    }
                                };
                                if !remainder {
                                    return Ok(Val::V(quotient, ty));
                                }
                                // x % d == x - (x / d) * d
                                let product = l.b.mul(
                                    crate::MulWindow::Low,
                                    quotient,
                                    crate::IntOperand::Imm(value),
                                );
                                return Ok(Val::V(l.b.bin(crate::BinOp::Sub, lhs, product), ty));
                            }
                        }
                    }
                    let name: crate::FuncName = match (ty == Ty::I16, remainder) {
                        (false, false) => "div_u16",
                        (false, true) => "rem_u16",
                        (true, false) => "div_i16",
                        (true, true) => "rem_i16",
                    };
                    let ret = l.b.call(name, &[lhs, rhs], 1);
                    Ok(Val::V(ret[0], ty))
                }
                Lt(_) | Le(_) | Gt(_) | Ge(_) | Eq(_) | Ne(_) => {
                    // Put a constant operand on the rhs so the compare can
                    // select an immediate encoding. Swapping an ordered
                    // comparison also inverts its condition (`k < x` becomes
                    // `x > k`); equality is symmetric and needs no inversion.
                    let ordered = matches!(b.op, Lt(_) | Le(_) | Gt(_) | Ge(_));
                    let swap_operands = immediate_operand(l, &b.left).is_some()
                        && immediate_operand(l, &b.right).is_none();
                    let (left_e, right_e) = if swap_operands {
                        (&b.right, &b.left)
                    } else {
                        (&b.left, &b.right)
                    };
                    let (lhs, lt) = expr(l, left_e)?.reg(l, left_e, "comparison")?;
                    let (rhs, rt) = match immediate_operand(l, right_e) {
                        Some((value, ty)) => (CmpRhs::Imm(value), ty),
                        None => {
                            let (v, t) = expr(l, right_e)?.reg(l, right_e, "comparison")?;
                            (CmpRhs::Reg(v), t)
                        }
                    };
                    if matches!(lt, Ty::Enum(_)) {
                        if lt != rt {
                            return Err(err(
                                e,
                                format!(
                                    "cannot compare {} with {}",
                                    lt.display(),
                                    rt.display()
                                ),
                            ));
                        }
                        if !matches!(b.op, SBinOp::Eq(_) | SBinOp::Ne(_)) {
                            return Err(err(&b.op, "enums compare with `==`/`!=` only"));
                        }
                        let Ty::Enum(name) = &lt else { unreachable!() };
                        let partial_eq =
                            l.globals.enums.get(name).is_some_and(|def| def.partial_eq);
                        if !partial_eq {
                            return Err(err(
                                e,
                                format!(
                                    "enum `{name}` needs #[derive(PartialEq)] to be compared (spec §9d)"
                                ),
                            ));
                        }
                        return compare(e, b.op, lhs, lt, rhs, rt, swap_operands && ordered)
                            .map(Val::Bool);
                    }
                    compare(e, b.op, lhs, lt, rhs, rt, swap_operands && ordered).map(Val::Bool)
                }
                And(_) | Or(_) => {
                    let lhs = cond(l, &b.left)?;
                    let rhs = cond(l, &b.right)?;
                    Ok(Val::Bool(match b.op {
                        And(_) => BoolExpr::And(Box::new(lhs), Box::new(rhs)),
                        Or(_) => BoolExpr::Or(Box::new(lhs), Box::new(rhs)),
                        _ => unreachable!(),
                    }))
                }
                _ => Err(err(&b.op, "unsupported binary operator")),
            }
        }
        Expr::Cast(c) => {
            let (v, from) = expr(l, &c.expr)?.reg(l, &c.expr, "cast")?;
            let to = ty_of(&c.ty, &l.globals.type_names)?;
            cast(e, v, from, to).map(|(v, t)| Val::V(v, t))
        }
        Expr::Call(call) => call_expr(l, call),
        Expr::MethodCall(m) => method_call(l, m),
        Expr::Field(f) => {
            let (base, offset, ty, _) = struct_field_place(l, f)?;
            Ok(Val::V(l.b.load_mem(base, offset), ty))
        }
        Expr::Index(index) => {
            let (base, off, elem, _) = array_index_addr(l, index)?;
            if matches!(elem, Ty::Struct(_)) {
                // a struct element *is* its address, like the struct value itself
                return Ok(Val::V(place_addr(l, base, off), elem));
            }
            Ok(Val::V(l.b.load_mem(base, off), elem))
        }
        Expr::If(_) => {
            // if used as a value: `let x = if c { a } else { b };`
            control_flow_value(l, e)
        }
        Expr::Block(b) => Err(err(&b, "blocks as expressions are not supported")),
        Expr::Tuple(_) => Err(err(
            e,
            "a tuple is memory-resident; bind it with `let (a, b) = ...`, a typed `let`, or return \
             it (spec §9c)",
        )),
        Expr::Match(_) => Err(err(
            e,
            "match is a statement in this version; assign inside its arms (or use if/else)",
        )),
        Expr::Closure(_) => Err(err(e, "closures are not supported")),
        Expr::Macro(_) => Err(err(e, "macros are not supported")),
        Expr::Reference(_) => Err(err(
            e,
            "references `&` are not supported (take addresses with addr_of(&x))",
        )),
        _ => Err(err(e, "expression not supported in this subset (see spec)")),
    }
}

/// address, constant word offset, type and mutability of a **place**: a
/// frame-resident variable (a struct, an address-taken local), or a field chain
/// into one. The address of the place itself is `base + offset`.
fn place_addr_of(l: &mut FnLower, e: &Expr) -> Result<(VReg, i16, Ty, bool), syn::Error> {
    match e {
        Expr::Paren(p) => place_addr_of(l, &p.expr),
        Expr::Path(_) => {
            let name = path_ident(e)?;
            if let Some(info) = l.lookup(&name) {
                let (kind, ty, mutable) = (info.kind.clone(), info.ty.clone(), info.mutable);
                return match kind {
                    VarKind::Local { slot } => Ok((l.b.addr_of_local(slot), 0, ty, mutable)),
                    VarKind::Ssa { .. } => Err(err(
                        e,
                        format!(
                            "`{name}` is not memory-resident; declare it as an array or struct so \
                             it gets a frame slot, or take its address with addr_of"
                        ),
                    )),
                };
            }
            if let Some((addr, ty)) = l.globals.scalars.get(&name) {
                let (addr, ty) = (*addr, ty.clone());
                return Ok((l.b.load_imm(addr), 0, ty, true));
            }
            if let Some((addr, elem, n)) = l.globals.arrays.get(&name) {
                let (addr, elem, n) = (*addr, elem.clone(), *n);
                return Ok((l.b.load_imm(addr), 0, Ty::Array(Box::new(elem), n), true));
            }
            if let Some((addr, ty)) = l.globals.aggregates.get(&name) {
                let (addr, ty) = (*addr, ty.clone());
                return Ok((l.b.load_imm(addr), 0, ty, true));
            }
            Err(err(e, format!("undefined name `{name}`")))
        }
        Expr::Field(f) => struct_field_place(l, f),
        Expr::Index(index) => {
            // the element address of an array or view (`p[i].x` reaches here)
            let (base, offset, elem, mutable) = array_index_addr(l, index)?;
            Ok((place_addr(l, base, offset), 0, elem, mutable))
        }
        _ => {
            if sret_call_return(l, e).is_some() {
                return Err(err(
                    e,
                    "a function returning an aggregate writes through a hidden destination pointer; \
                     bind it with `let` first (spec §14)",
                ));
            }
            Err(err(
                e,
                "expected a variable or a field of one (struct values live in memory)",
            ))
        }
    }
}

/// a place whose offset is folded in: `base + offset` when there is an offset
fn place_addr(l: &mut FnLower, base: VReg, offset: i16) -> VReg {
    if offset == 0 {
        base
    } else {
        let off = l.b.load_imm(offset as u16);
        l.b.bin(BinOp::Add, base, off)
    }
}

/// `a.b`: the address of the base place plus the field's offset, the field's type
/// and the base's mutability
fn struct_field_place(
    l: &mut FnLower,
    f: &syn::ExprField,
) -> Result<(VReg, i16, Ty, bool), syn::Error> {
    let (base, offset, base_ty, mutable) = place_addr_of(l, &f.base)?;
    // a tuple field is positional: element `i` sits at offset `i` (spec §9c)
    if let Ty::Tuple(elems) = &base_ty {
        let syn::Member::Unnamed(index) = &f.member else {
            return Err(err(
                &f.member,
                "a tuple has only positional fields (`.0`, `.1`, ...)",
            ));
        };
        let i = index.index as usize;
        let Some(elem) = elems.get(i) else {
            return Err(err(
                &index,
                format!(
                    "tuple has {} elements; index {i} is out of range",
                    elems.len()
                ),
            ));
        };
        return Ok((base, offset + i as i16, elem.clone(), mutable));
    }
    let Ty::Struct(name) = &base_ty else {
        return Err(err(
            &f.base,
            format!(
                "{} has no fields (only structs and tuples do)",
                base_ty.display()
            ),
        ));
    };
    let member = match &f.member {
        syn::Member::Named(ident) => ident.to_string(),
        syn::Member::Unnamed(index) => {
            return Err(err(
                &index,
                "only named struct fields are supported (use `name` not `.0`)",
            ))
        }
    };
    let def = l
        .globals
        .structs
        .get(name)
        .cloned()
        .ok_or_else(|| err(&f.base, format!("unknown struct `{name}`")))?;
    let Some(field) = def.fields.iter().find(|field| field.name == member) else {
        let known = def
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(err(
            &f.member,
            format!("struct `{name}` has no field `{member}` (fields: {known})"),
        ));
    };
    Ok((
        base,
        offset + field.offset as i16,
        field.ty.clone(),
        mutable,
    ))
}

/// the type a place expression denotes, without lowering it: choosing between the
/// value path (an `Array<T>` view *is* a value) and the place path (a raw array is
/// storage) needs the type before any instructions are emitted
fn peek_type(l: &FnLower, e: &Expr) -> Option<Ty> {
    match e {
        Expr::Paren(p) => peek_type(l, &p.expr),
        Expr::Path(_) => {
            let Expr::Path(p) = e else { unreachable!() };
            if p.path.segments.len() == 2 {
                let enum_name = p.path.segments[0].ident.to_string();
                if l.globals.enums.contains_key(&enum_name) {
                    return Some(Ty::Enum(enum_name));
                }
            }
            let name = path_ident(e).ok()?;
            if let Some(info) = l.lookup(&name) {
                return Some(info.ty.clone());
            }
            if let Some((_, ty)) = l.globals.scalars.get(&name) {
                return Some(ty.clone());
            }
            if let Some((_, ty)) = l.globals.aggregates.get(&name) {
                return Some(ty.clone());
            }
            l.globals
                .arrays
                .get(&name)
                .map(|(_, elem, n)| Ty::Array(Box::new(elem.clone()), *n))
        }
        Expr::Field(f) => {
            let Ty::Struct(name) = peek_type(l, &f.base)? else {
                return None;
            };
            let syn::Member::Named(ident) = &f.member else {
                return None;
            };
            let def = l.globals.structs.get(&name)?;
            let member = ident.to_string();
            def.fields
                .iter()
                .find(|field| field.name == member)
                .map(|field| field.ty.clone())
        }
        Expr::Index(i) => match peek_type(l, &i.expr)? {
            Ty::Array(elem, _) | Ty::ArrayRef(elem) => Some(*elem),
            _ => None,
        },
        _ => None,
    }
}

/// a struct element spans several words, so its index is scaled by the size
fn scaled_index(
    l: &mut FnLower,
    index: &Expr,
    size: u16,
    at: &impl syn::spanned::Spanned,
) -> Result<VReg, syn::Error> {
    let (off, off_ty) = expr(l, index)?.reg(l, index, "array index")?;
    if !matches!(off_ty, Ty::U16 | Ty::I16) {
        return Err(err(at, "array index must have type u16 or i16"));
    }
    if size == 1 {
        return Ok(off);
    }
    Ok(l.b.mul(crate::MulWindow::Low, off, crate::IntOperand::Imm(size)))
}

fn array_index_addr(
    l: &mut FnLower,
    index: &syn::ExprIndex,
) -> Result<(VReg, i16, Ty, bool), syn::Error> {
    // A plain `Array<T>` view is a value; an array *field* and a raw struct array
    // are places whose address is the base. The returned flag is the place's
    // mutability: a read ignores it, an assignment checks it.
    let place_base = matches!(index.expr.as_ref(), Expr::Field(_))
        || matches!(peek_type(l, &index.expr), Some(Ty::Array(elem, _)) if matches!(*elem, Ty::Struct(_)));
    let (base, ty, mutable) = if place_base {
        let (b, off, ty, mutable) = place_addr_of(l, &index.expr)?;
        (place_addr(l, b, off), ty, mutable)
    } else {
        let (base, ty) = expr(l, &index.expr)?.reg(l, &index.expr, "array index base")?;
        (base, ty, true)
    };
    let elem = match ty {
        Ty::ArrayRef(elem) => *elem,
        Ty::Array(elem, _) => *elem,
        _ => {
            return Err(err(
                &index.expr,
                "indexing requires Array<T>, an array field or a struct array",
            ))
        }
    };
    if let Expr::Lit(lit) = index.index.as_ref() {
        if let Lit::Int(value) = &lit.lit {
            if value.suffix().is_empty() {
                return Err(err(
                    &index.index,
                    "array index literals need an explicit u16 or i16 suffix",
                ));
            }
        }
    }
    if let Expr::Unary(unary) = index.index.as_ref() {
        if let Expr::Lit(lit) = unary.expr.as_ref() {
            if let Lit::Int(value) = &lit.lit {
                if value.suffix().is_empty() {
                    return Err(err(
                        &index.index,
                        "array index literals need an explicit u16 or i16 suffix",
                    ));
                }
            }
        }
    }
    let size = word_size(&elem, &l.globals.structs, &index.index)?;
    if let Some(off) = literal_mem_offset(&index.index)? {
        let scaled = i32::from(off)
            .checked_mul(i32::from(size))
            .and_then(|value| i16::try_from(value).ok())
            .ok_or_else(|| err(&index.index, "array offset out of range"))?;
        return Ok((base, scaled, elem, mutable));
    }
    let off = scaled_index(l, &index.index, size, &index.index)?;
    Ok((l.b.bin(BinOp::Add, base, off), 0, elem, mutable))
}

fn ensure_mutable_array_view(l: &FnLower, receiver: &Expr) -> Result<(), syn::Error> {
    if let Ok(name) = path_ident(receiver) {
        if let Some(info) = l.lookup(&name) {
            if !info.mutable {
                return Err(err(
                    receiver,
                    format!("`{name}` is not mutable (declare with `let mut`)"),
                ));
            }
        }
    }
    Ok(())
}

/// if as an expression: `if c { a } else { b }`
fn control_flow_value(l: &mut FnLower, e: &Expr) -> Result<Val, syn::Error> {
    let Expr::If(i) = e else { unreachable!() };
    let Some((_, else_e)) = &i.else_branch else {
        return Err(err(e, "if-expression needs an else branch"));
    };
    let then_b = l.b.raw_block(&[]);
    let else_b = l.b.raw_block(&[]);
    let join = l.b.raw_block(&[]);
    cond_lazy(l, &i.cond, then_b, else_b)?;
    l.b.enter_block(then_b);

    // then branch value
    let (tv, tt) = if_expr_branch(l, &i.then_branch)?;
    let r = l.b.new_var_typed(if tt.is_fpu() {
        crate::RegClass::Fpu
    } else {
        crate::RegClass::Gpr
    });
    l.b.set(r, tv);
    l.b.mid_if_else(else_b, join);

    // else branch value
    let else_blk = match else_e.as_ref() {
        Expr::Block(b) => &b.block,
        _ => return Err(err(else_e, "if-expression branches must be blocks")),
    };
    let (ev, et) = if_expr_branch(l, else_blk)?;
    // integer/FPU branches unify through unify_int; identical non-integer types
    // (bool, Ptr) unify with themselves
    let ty = unify_int(tt.clone(), et.clone())
        .or_else(|| (tt == et).then(|| tt.clone()))
        .ok_or_else(|| {
            err(
                e,
                format!(
                    "if-expression branches have different types: {} vs {}",
                    tt.display(),
                    et.display()
                ),
            )
        })?;
    l.b.set(r, ev);
    l.b.end_if_else(join);

    let v = l.b.get(r);
    l.b.set_line_hint(line_of(e));
    Ok(Val::V(v, ty))
}

fn if_expr_branch(l: &mut FnLower, blk: &Block) -> Result<(VReg, Ty), syn::Error> {
    if blk.stmts.len() != 1 {
        return Err(err(
            blk,
            "if-expression branches must be single expressions",
        ));
    }
    match &blk.stmts[0] {
        Stmt::Expr(e) | Stmt::Semi(e, _) => {
            l.b.set_line_hint(line_of(e));
            expr(l, e)?.reg(l, e, "if-expression")
        }
        s => Err(err(s, "if-expression branches must be expressions")),
    }
}

// ---------------------------------------------------------------------------
// conditions
// ---------------------------------------------------------------------------

fn cond(l: &mut FnLower, e: &Expr) -> Result<BoolExpr, syn::Error> {
    match expr(l, e)? {
        Val::Bool(c) => Ok(c),
        // a stored bool is true when it is nonzero
        Val::V(v, Ty::Bool) => {
            let cmp = l.b.cmp(v, CmpRhs::Imm(0), CompareOp::NotEqual);
            Ok(BoolExpr::Cmp(cmp))
        }
        Val::V(_, ty) => Err(err(
            e,
            format!(
                "condition must be a boolean expression, got {} (compare something)",
                ty.display()
            ),
        )),
        Val::FnItem(_) => Err(err(e, "function used as a condition")),
        Val::Unit => Err(err(e, "unit used as a condition")),
        Val::Never => Err(err(e, "never used as a condition")),
    }
}

/// materialize a condition as a stored 0/1 bool: one comparison uses the ISA's
/// Boolean-producing form, `!` flips the low bit, and a compound condition becomes
/// a two-block diamond (which the diamond-conversion pass folds back into a
/// Boolean comparison when the shape allows).
fn bool_value(l: &mut FnLower, c: BoolExpr) -> VReg {
    match c {
        BoolExpr::Cmp(cmp) => match cmp.cond {
            CompareOp::Always => l.b.load_imm(1),
            CompareOp::Never => l.b.load_imm(0),
            _ => l.b.bool_value(cmp),
        },
        BoolExpr::Not(inner) => {
            let v = bool_value(l, *inner);
            let one = l.b.load_imm(1);
            l.b.bin(crate::BinOp::Xor, v, one)
        }
        c => {
            let r = l.b.new_var_typed(crate::RegClass::Gpr);
            let (else_b, join) = l.b.begin_if_else(c);
            let one = l.b.load_imm(1);
            l.b.set(r, one);
            l.b.mid_if_else(else_b, join);
            let zero = l.b.load_imm(0);
            l.b.set(r, zero);
            l.b.end_if_else(join);
            l.b.get(r)
        }
    }
}

fn compare(
    e: &Expr,
    op: SBinOp,
    lhs: VReg,
    lt: Ty,
    rhs: CmpRhs,
    rt: Ty,
    swapped: bool,
) -> Result<BoolExpr, syn::Error> {
    // fix16 comparisons use FCMP (signed lane-x ordering); vecN values have
    // no per-lane compare in this version
    if lt == Ty::Fix16 && rt == Ty::Fix16 {
        let CmpRhs::Reg(rhs) = rhs else {
            return Err(err(e, "fix16 comparisons do not take an immediate operand"));
        };
        let mut cond = compare_cond(&op, e)?;
        if swapped {
            cond = swapped_cond(cond);
        }
        return Ok(BoolExpr::Cmp(Cmp {
            lhs,
            rhs: CmpRhs::Reg(rhs),
            cond,
            signed: true,
        }));
    }
    if lt.is_fpu() || rt.is_fpu() {
        return Err(err(
            e,
            format!(
                "cannot compare {} with {} (only fix16 comparisons are supported)",
                lt.display(),
                rt.display()
            ),
        ));
    }
    // a C-style enum compares by discriminant, equality only (spec §9d)
    if let (Ty::Enum(_), Ty::Enum(_)) = (&lt, &rt) {
        return Ok(BoolExpr::Cmp(Cmp {
            lhs,
            rhs,
            cond: compare_cond(&op, e)?,
            signed: false,
        }));
    }
    let signed = match unify_int(lt.clone(), rt.clone()) {
        Some(Ty::I16) => true,
        Some(t) if t.is_int() => false,
        _ if lt == Ty::Ptr && rt == Ty::Ptr => false,
        _ => {
            return Err(err(
                e,
                format!("cannot compare {} with {}", lt.display(), rt.display()),
            ))
        }
    };
    let mut cond = compare_cond(&op, e)?;
    if swapped {
        cond = swapped_cond(cond);
    }
    if matches!(
        op,
        SBinOp::Lt(_) | SBinOp::Le(_) | SBinOp::Gt(_) | SBinOp::Ge(_)
    ) && (lt == Ty::Ptr || rt == Ty::Ptr)
    {
        return Err(err(e, "ordered comparisons on pointers are not supported"));
    }
    Ok(BoolExpr::Cmp(Cmp {
        lhs,
        rhs,
        cond,
        signed,
    }))
}

/// the condition code for `rhs cond' lhs` when `lhs cond rhs` was written:
/// `a < b` becomes `b > a`, `a <= b` becomes `b >= a`, and so on. This is the
/// operand-swap mapping, not the logical negation (`CompareOp::invert`).
fn swapped_cond(cond: CompareOp) -> CompareOp {
    match cond {
        CompareOp::Equal => CompareOp::Equal,
        CompareOp::NotEqual => CompareOp::NotEqual,
        CompareOp::Less => CompareOp::Greater,
        CompareOp::LessEqual => CompareOp::GreaterEqual,
        CompareOp::Greater => CompareOp::Less,
        CompareOp::GreaterEqual => CompareOp::LessEqual,
        CompareOp::Never | CompareOp::Always => cond,
    }
}

fn compare_cond(op: &SBinOp, e: &Expr) -> Result<CompareOp, syn::Error> {
    Ok(match op {
        SBinOp::Lt(_) => CompareOp::Less,
        SBinOp::Le(_) => CompareOp::LessEqual,
        SBinOp::Gt(_) => CompareOp::Greater,
        SBinOp::Ge(_) => CompareOp::GreaterEqual,
        SBinOp::Eq(_) => CompareOp::Equal,
        SBinOp::Ne(_) => CompareOp::NotEqual,
        _ => return Err(err(e, "unsupported comparison operator")),
    })
}

/// lower a condition into a branch cascade to `t`/`f`, evaluating each
/// comparison exactly where the cascade reaches it — true short-circuit for
/// side effects (calls inside conditions run only when reached)
fn cond_lazy(l: &mut FnLower, e: &Expr, t: BlockId, f: BlockId) -> Result<(), syn::Error> {
    match e {
        Expr::Paren(p) => cond_lazy(l, &p.expr, t, f),
        Expr::Binary(b) => match b.op {
            SBinOp::And(_) => {
                let m = l.b.raw_block(&[]);
                l.b.set_block_line(m, line_of(&b.op));
                cond_lazy(l, &b.left, m, f)?;
                l.b.enter_block(m);
                cond_lazy(l, &b.right, t, f)
            }
            SBinOp::Or(_) => {
                let m = l.b.raw_block(&[]);
                l.b.set_block_line(m, line_of(&b.op));
                cond_lazy(l, &b.left, t, m)?;
                l.b.enter_block(m);
                cond_lazy(l, &b.right, t, f)
            }
            _ => {
                // a comparison: evaluate its operands right here, then branch
                let c = cond(l, e)?;
                let BoolExpr::Cmp(cmp) = c else {
                    return Err(err(e, "condition must be a boolean expression"));
                };
                l.b.br(cmp, t, f);
                Ok(())
            }
        },
        Expr::Unary(u) if matches!(u.op, SUnOp::Not(_)) => cond_lazy(l, &u.expr, f, t),
        Expr::Lit(lit) => match &lit.lit {
            Lit::Bool(b) => {
                if b.value {
                    l.b.jmp(t);
                } else {
                    l.b.jmp(f);
                }
                Ok(())
            }
            _ => Err(err(
                e,
                "condition must be a boolean expression (compare something)",
            )),
        },
        _ => {
            // a plain expression: a stored bool branches on "nonzero", everything
            // else gets the eager checker's precise message
            match expr(l, e)? {
                Val::V(v, Ty::Bool) => {
                    let cmp = l.b.cmp(v, CmpRhs::Imm(0), CompareOp::NotEqual);
                    l.b.br(cmp, t, f);
                    Ok(())
                }
                Val::Bool(c) => {
                    let BoolExpr::Cmp(cmp) = c else {
                        return Err(err(e, "condition must be a boolean expression"));
                    };
                    l.b.br(cmp, t, f);
                    Ok(())
                }
                Val::V(_, ty) => Err(err(
                    e,
                    format!(
                        "condition must be a boolean expression, got {} (compare something)",
                        ty.display()
                    ),
                )),
                Val::FnItem(_) => Err(err(e, "function used as a condition")),
                Val::Unit => Err(err(e, "unit used as a condition")),
                Val::Never => Err(err(e, "never used as a condition")),
            }
        }
    }
}

fn true_cond(l: &mut FnLower) -> BoolExpr {
    let zero = l.b.load_imm(0);
    BoolExpr::Cmp(Cmp {
        lhs: zero,
        rhs: CmpRhs::Imm(0),
        cond: CompareOp::Equal,
        signed: false,
    })
}
fn false_cond(l: &mut FnLower) -> BoolExpr {
    let zero = l.b.load_imm(0);
    BoolExpr::Cmp(Cmp {
        lhs: zero,
        rhs: CmpRhs::Imm(0),
        cond: CompareOp::NotEqual,
        signed: false,
    })
}

// ---------------------------------------------------------------------------
// calls and intrinsics
// ---------------------------------------------------------------------------

/// lower a call's arguments to (register, type) pairs, materializing function
/// items passed as fn pointers
fn call_arg_values(l: &mut FnLower, call: &syn::ExprCall) -> Result<Vec<(VReg, Ty)>, syn::Error> {
    call.args
        .iter()
        .map(|a| match expr(l, a)? {
            Val::FnItem(fname) => {
                let sig = l
                    .sigs
                    .get(fname)
                    .cloned()
                    .ok_or_else(|| err(a, format!("undefined function `{fname}`")))?;
                let v = l.b.load_func_addr(fname);
                Ok((
                    v,
                    Ty::FnPtr {
                        params: sig.params,
                        ret: Box::new(sig.ret),
                    },
                ))
            }
            val => val.reg(l, a, "argument"),
        })
        .collect::<Result<_, _>>()
}

fn call_expr(l: &mut FnLower, call: &syn::ExprCall) -> Result<Val, syn::Error> {
    let Expr::Path(p) = call.func.as_ref() else {
        return Err(err(&call.func, "unsupported callee"));
    };
    // intrinsic or fn-item path (possibly qualified like Ptr::from_addr)
    let segs: Vec<String> = p
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    if segs == ["Ptr", "from_addr"] {
        let (v, _) = exactly_args(l, &call.args, call, 1, "Ptr::from_addr")?[0]
            .clone()
            .reg(l, &call.args[0], "Ptr::from_addr")?;
        let (v, _) = coerce(l, v, &Ty::U16, &Ty::U16, &call.args[0])?;
        return Ok(Val::V(v, Ty::Ptr));
    }
    if segs.len() == 2 && matches!(segs[0].as_str(), "fix16" | "vec2" | "vec3" | "vec4") {
        return fpu_associated_call(l, &segs[0], &segs[1], call);
    }
    if segs.len() != 1 {
        return Err(err(&p, "unsupported path"));
    }
    let name = &segs[0];

    // intrinsics with non-value arguments must be handled before generic arg
    // evaluation (assert takes a condition, addr_of takes a reference)
    match name.as_str() {
        "addr_of" => {
            if call.args.len() != 1 {
                return Err(err(call, "addr_of(&x) takes 1 argument"));
            }
            let Expr::Reference(r) = &call.args[0] else {
                return Err(err(
                    &call.args[0],
                    "addr_of expects a reference: addr_of(&x)",
                ));
            };
            let (base, offset, _, _) = place_addr_of(l, &r.expr)?;
            return Ok(Val::V(place_addr(l, base, offset), Ty::Ptr));
        }
        // the address of one value as a typed view: `view_of(&p)` for a struct or
        // scalar place (a Buf uses `.as_array()`)
        "view_of" => {
            if call.args.len() != 1 {
                return Err(err(call, "view_of(&x) takes 1 argument"));
            }
            let Expr::Reference(r) = &call.args[0] else {
                return Err(err(
                    &call.args[0],
                    "view_of expects a reference: view_of(&x)",
                ));
            };
            let (base, offset, ty, _) = place_addr_of(l, &r.expr)?;
            match ty {
                Ty::Array(..) => {
                    return Err(err(
                        &r.expr,
                        "use `buf.as_array()` for a Buf; view_of takes one value",
                    ))
                }
                Ty::ArrayRef(_) => return Err(err(&r.expr, "that expression is already a view")),
                _ => {}
            }
            let addr = place_addr(l, base, offset);
            return Ok(Val::V(addr, Ty::ArrayRef(Box::new(ty))));
        }
        "assert" => {
            if call.args.len() != 2 {
                return Err(err(call, "assert(cond, sig) takes 2 arguments"));
            }
            let (sig_v, _) = expr(l, &call.args[1])?.reg(l, &call.args[1], "assert signal")?;
            let fail = l.b.raw_block(&[]);
            let join = l.b.raw_block(&[]);
            cond_lazy(l, &call.args[0], join, fail)?;
            l.b.enter_block(fail);
            l.b.halt(sig_v);
            l.b.end_if(join);
            return Ok(Val::Unit);
        }
        "dev_recv" => {
            if call.args.len() != 2 {
                return Err(err(call, "dev_recv(dev, ch) takes 2 arguments"));
            }
            let device = constant_below(&call.args[0], "device index", 8, l.consts)?;
            let channel = constant_below(&call.args[1], "channel", 16, l.consts)?;
            return Ok(Val::V(l.b.dev_recv(device, channel), Ty::U16));
        }
        "dev_send" => {
            if call.args.len() != 3 {
                return Err(err(call, "dev_send(dev, ch, value) takes 3 arguments"));
            }
            let device = constant_below(&call.args[0], "device index", 8, l.consts)?;
            let channel = constant_below(&call.args[1], "channel", 16, l.consts)?;
            let (value, from) = expr(l, &call.args[2])?.reg(l, &call.args[2], "device value")?;
            let (value, _) = coerce(l, value, &from, &Ty::U16, &call.args[2])?;
            l.b.dev_send(device, channel, value);
            return Ok(Val::Unit);
        }
        "signal" => {
            if call.args.len() != 2 {
                return Err(err(call, "signal(type, value) takes 2 arguments"));
            }
            let signal_type = constant_u8(&call.args[0], "signal type", l.consts)?;
            if !(1..=15).contains(&signal_type) {
                return Err(err(
                    &call.args[0],
                    "signal type must be a compile-time constant in 1..=15",
                ));
            }
            let (value, from) = expr(l, &call.args[1])?.reg(l, &call.args[1], "signal value")?;
            let (value, _) = coerce(l, value, &from, &Ty::U16, &call.args[1])?;
            l.b.signal(signal_type, value);
            return Ok(Val::Unit);
        }
        _ => {}
    }

    // remaining calls take plain value arguments; function items passed as fn
    // pointer arguments are materialized to their address inline
    let args: Vec<(VReg, Ty)> = call_arg_values(l, call)?;
    if name.as_str() == "halt" {
        if args.len() != 1 {
            return Err(err(call, "halt(x) takes 1 argument"));
        }
        let v = args[0].0;
        l.b.halt(v);
        l.dead = true;
        return Ok(Val::Never);
    }
    if name.as_str() == "mtsr_dseg" {
        if args.len() != 1 {
            return Err(err(call, "mtsr_dseg(v) takes 1 argument"));
        }
        let (v, from) = &args[0];
        let (v, _) = coerce(l, *v, from, &Ty::U16, &call.args[0])?;
        l.b.mtsr_dseg(v);
        return Ok(Val::Unit);
    }
    if name.as_str() == "dcache_invalidate_all" {
        if !args.is_empty() {
            return Err(err(call, "dcache_invalidate_all() takes no arguments"));
        }
        l.b.dcache_invalidate_all();
        return Ok(Val::V(l.b.dev_recv(0, 5), Ty::U16));
    }
    if name.as_str() == "dcache_clean_all" {
        if !args.is_empty() {
            return Err(err(call, "dcache_clean_all() takes no arguments"));
        }
        let zero = l.b.load_imm(0);
        l.b.dev_send(0, 4, zero);
        return Ok(Val::V(l.b.dev_recv(0, 5), Ty::U16));
    }
    if name.as_str() == "icache_invalidate_delayed_and_jump" {
        if args.len() != 2 {
            return Err(err(
                call,
                "icache_invalidate_delayed_and_jump(cseg, target) takes 2 arguments",
            ));
        }
        let (cseg, from) = &args[0];
        let (cseg, _) = coerce(l, *cseg, from, &Ty::U16, &call.args[0])?;
        let (target, from) = &args[1];
        let (target, _) = coerce(l, *target, from, &Ty::U16, &call.args[1])?;
        l.b.icache_invalidate_delayed_and_jump(cseg, target);
        l.dead = true;
        return Ok(Val::Never);
    }
    if name.as_str() == "jseg" {
        if args.len() != 2 {
            return Err(err(call, "jseg(cseg, target) takes 2 arguments"));
        }
        let (cseg, from) = &args[0];
        let (cseg, _) = coerce(l, *cseg, from, &Ty::U16, &call.args[0])?;
        let (target, from) = &args[1];
        let (target, _) = coerce(l, *target, from, &Ty::U16, &call.args[1])?;
        l.b.jseg(cseg, target);
        // control never returns; terminate the block with an unreachable halt
        let zero = l.b.load_imm(0);
        l.b.halt(zero);
        l.dead = true;
        return Ok(Val::Never);
    }
    if name.as_str() == "fdot" {
        // atomic ACC sequence: FDOT4ACC immediately followed by FACCSTORE
        // lane 0, so ACC is zero before and after (the FPU ABI invariant)
        if args.len() != 2 {
            return Err(err(call, "fdot(a, b) takes 2 arguments"));
        }
        let (a, at) = &args[0];
        let (b, bt) = &args[1];
        if !at.is_fpu() || at != bt {
            return Err(err(
                call,
                format!(
                    "fdot needs two values of the same FPU type, got {} and {}",
                    at.display(),
                    bt.display()
                ),
            ));
        }
        l.b.fdot4acc(*a, *b);
        return Ok(Val::V(l.b.facc_store(1), Ty::Fix16));
    }
    if matches!(name.as_str(), "frcp" | "frsqrt" | "fsincos") {
        if args.len() != 1 {
            return Err(err(call, format!("{name}(x) takes 1 argument")));
        }
        let (v, from) = &args[0];
        if *from != Ty::Fix16 {
            return Err(err(
                &call.args[0],
                format!("{name} takes a fix16, got {}", from.display()),
            ));
        }
        let (op, ty) = match name.as_str() {
            "frcp" => (crate::FUnOp::Rcp, Ty::Fix16),
            "frsqrt" => (crate::FUnOp::Rsqrt, Ty::Fix16),
            _ => (crate::FUnOp::SinCos, Ty::Vec2),
        };
        return Ok(Val::V(l.b.funary(op, *v), ty));
    }
    if matches!(name.as_str(), "cnt1" | "log2") {
        if args.len() != 1 {
            return Err(err(call, format!("{name}(x) takes 1 argument")));
        }
        let (v, from) = &args[0];
        let (v, _) = coerce(l, *v, from, &Ty::U16, &call.args[0])?;
        let op = if name == "cnt1" {
            UnOp::Cnt1
        } else {
            UnOp::Log2
        };
        return Ok(Val::V(l.b.un(op, v), Ty::U16));
    }
    if matches!(name.as_str(), "mul8" | "mul16") {
        if args.len() != 2 {
            return Err(err(call, format!("{name}(a, b) takes 2 arguments")));
        }
        let (a, from) = &args[0];
        let (a, _) = coerce(l, *a, from, &Ty::U16, &call.args[0])?;
        let (b, from) = &args[1];
        let (b, _) = coerce(l, *b, from, &Ty::U16, &call.args[1])?;
        let window = if name == "mul8" {
            crate::MulWindow::Shift8
        } else {
            crate::MulWindow::Shift16
        };
        return Ok(Val::V(
            l.b.mul(window, a, crate::IntOperand::Reg(b)),
            Ty::U16,
        ));
    }
    if matches!(name.as_str(), "sextb" | "clz") {
        if args.len() != 1 {
            return Err(err(call, format!("{name}(x) takes 1 argument")));
        }
        let (v, from) = &args[0];
        let (v, _) = coerce(l, *v, from, &Ty::U16, &call.args[0])?;
        return Ok(match name.as_str() {
            "sextb" => Val::V(l.b.un(UnOp::Sextb, v), Ty::I16),
            _ => Val::V(l.b.un(UnOp::Clz, v), Ty::U16),
        });
    }
    if matches!(name.as_str(), "read_cseg" | "read_dseg") {
        if !args.is_empty() {
            return Err(err(call, format!("{name}() takes no arguments")));
        }
        let sr = if name == "read_cseg" {
            crate::SpecialReg::Cseg
        } else {
            crate::SpecialReg::Dseg
        };
        return Ok(Val::V(l.b.mfsr(sr), Ty::U16));
    }

    // direct or indirect call
    let arg_vregs: Vec<VReg> = args.iter().map(|(v, _)| *v).collect();
    let arg_tys: Vec<Ty> = args.iter().map(|(_, t)| t.clone()).collect();
    if let Some(sig) = l.sigs.get(name).cloned() {
        check_call_args(call, &sig.params, &arg_tys, name)?;
        if is_aggregate(&sig.ret) {
            return Err(err(
                call,
                "a function returning an aggregate writes through a hidden destination pointer, so \
                 it must be bound (`let x = f(..)`), assigned, or returned directly (spec §14)",
            ));
        }
        let n_rets = if sig.ret == Ty::Unit { 0 } else { 1 };
        let rets = l.b.call(intern(name), &arg_vregs, n_rets);
        if sig.ret.is_fpu() {
            l.b.set_vreg_class(rets[0], crate::RegClass::Fpu);
        }
        return Ok(match n_rets {
            0 => Val::Unit,
            _ => Val::V(rets[0], sig.ret),
        });
    }
    if let Some(info) = l.lookup(name) {
        let Ty::FnPtr { params, ret } = info.ty.clone() else {
            return Err(err(call, format!("`{name}` is not callable")));
        };
        check_call_args(call, &params, &arg_tys, name)?;
        let addr = {
            let kind = info.kind.clone();
            l.read_var(&kind)
        };
        let n_rets = if *ret == Ty::Unit { 0 } else { 1 };
        let rets = l.b.call_ptr(addr, &arg_vregs, n_rets);
        if ret.is_fpu() {
            l.b.set_vreg_class(rets[0], crate::RegClass::Fpu);
        }
        return Ok(match n_rets {
            0 => Val::Unit,
            _ => Val::V(rets[0], *ret),
        });
    }
    Err(err(&p, format!("undefined function `{name}`")))
}

fn constant_u8(
    expression: &Expr,
    name: &str,
    consts: &HashMap<String, (u16, Ty)>,
) -> Result<u8, syn::Error> {
    u8::try_from(const_eval(expression, consts)?)
        .map_err(|_| err(expression, format!("{name} must be from 0 through 255")))
}

/// a compile-time constant that must be strictly below `limit`, so an invalid
/// device/channel/type argument is a source diagnostic rather than a lowering
/// panic
fn constant_below(
    expression: &Expr,
    name: &str,
    limit: u8,
    consts: &HashMap<String, (u16, Ty)>,
) -> Result<u8, syn::Error> {
    let value = constant_u8(expression, name, consts)?;
    if value < limit {
        Ok(value)
    } else {
        Err(err(
            expression,
            format!("{name} {value} is outside 0..={}", limit - 1),
        ))
    }
}

// ---------------------------------------------------------------------------
// FPU (fix16/vec2/vec3/vec4) lowering helpers
// ---------------------------------------------------------------------------

/// a 4-word aligned 4-word scratch window in the stack frame (8 words are
/// reserved so the aligned window always fits; alignment is computed at run
/// time because nothing guarantees sp mod 4 == 0)
fn aligned_scratch4(l: &mut FnLower) -> VReg {
    let slot = l.b.alloc_local_slots(8);
    let base = l.b.addr_of_local(slot);
    let three = l.b.load_imm(3);
    let up = l.b.bin(BinOp::Add, base, three);
    let mask = l.b.load_imm(0xfffc);
    l.b.bin(BinOp::And, up, mask)
}

/// extract lane `lane` as a fix16 through the frame scratch (FEXPORT4 +
/// LOAD + FLOAD); documented as expensive compared to the free `.x()`
fn fpu_lane(l: &mut FnLower, v: VReg, lane: i16) -> VReg {
    let addr = aligned_scratch4(l);
    l.b.fexport4(v, addr);
    let word = l.b.load_mem(addr, lane);
    l.b.fload(word)
}

/// fix16::/vec2::/vec3::/vec4:: associated functions
fn fpu_associated_call(
    l: &mut FnLower,
    ty_name: &str,
    method: &str,
    call: &syn::ExprCall,
) -> Result<Val, syn::Error> {
    let ty = match ty_name {
        "fix16" => Ty::Fix16,
        "vec2" => Ty::Vec2,
        "vec3" => Ty::Vec3,
        _ => Ty::Vec4,
    };
    match (ty_name, method) {
        ("fix16", "from_bits") => {
            let (v, from) = exactly_args(l, &call.args, call, 1, "fix16::from_bits")?[0]
                .clone()
                .reg(l, &call.args[0], "fix16::from_bits")?;
            let (v, _) = coerce(l, v, &from, &Ty::U16, &call.args[0])?;
            Ok(Val::V(l.b.fload(v), Ty::Fix16))
        }
        ("fix16", "from_int") => {
            let (v, from) = exactly_args(l, &call.args, call, 1, "fix16::from_int")?[0]
                .clone()
                .reg(l, &call.args[0], "fix16::from_int")?;
            let (v, _) = coerce(l, v, &from, &Ty::I16, &call.args[0])?;
            let shifted = l.b.shift(ShiftOp::Lsl, v, 8);
            Ok(Val::V(l.b.fload(shifted), Ty::Fix16))
        }
        (_, "zero") => {
            if !call.args.is_empty() {
                return Err(err(call, format!("{ty_name}::zero() takes no arguments")));
            }
            Ok(Val::V(l.b.fzero(), ty))
        }
        ("vec2" | "vec3" | "vec4", "new") => {
            let lanes = ty.fpu_lanes();
            if call.args.len() != lanes {
                return Err(err(
                    call,
                    format!(
                        "{ty_name}::new takes {lanes} fix16 arguments, got {}",
                        call.args.len()
                    ),
                ));
            }
            let addr = aligned_scratch4(l);
            for i in 0..4usize {
                let word = if i < lanes {
                    let (v, from) =
                        expr(l, &call.args[i])?.reg(l, &call.args[i], "vec constructor lane")?;
                    let (v, _) = coerce(l, v, &from, &Ty::Fix16, &call.args[i])?;
                    l.b.fstore(v)
                } else {
                    // upper lanes carry no meaning and stay zero
                    l.b.load_imm(0)
                };
                l.b.store_mem(addr, i as i16, word);
            }
            Ok(Val::V(l.b.fimport4(addr), ty))
        }
        ("vec4", "import") => {
            let (v, from) = exactly_args(l, &call.args, call, 1, "vec4::import")?[0]
                .clone()
                .reg(l, &call.args[0], "vec4::import")?;
            if from != Ty::Ptr {
                return Err(err(
                    &call.args[0],
                    format!("vec4::import takes a Ptr, got {}", from.display()),
                ));
            }
            // hardware faults when the address is not 4-aligned
            Ok(Val::V(l.b.fimport4(v), Ty::Vec4))
        }
        ("vec4", "export") => {
            if call.args.len() != 2 {
                return Err(err(call, "vec4::export(v, ptr) takes 2 arguments"));
            }
            let (v, from) = expr(l, &call.args[0])?.reg(l, &call.args[0], "vec4::export")?;
            let (v, _) = coerce(l, v, &from, &Ty::Vec4, &call.args[0])?;
            let (p, from) = expr(l, &call.args[1])?.reg(l, &call.args[1], "vec4::export")?;
            if from != Ty::Ptr {
                return Err(err(
                    &call.args[1],
                    format!("vec4::export takes a Ptr, got {}", from.display()),
                ));
            }
            l.b.fexport4(v, p);
            Ok(Val::Unit)
        }
        _ => Err(err(
            &call.func,
            format!("unknown {ty_name} function `{method}`"),
        )),
    }
}

/// methods on fix16/vec2/vec3/vec4 values (lane access, bit bridge, unary ops)
fn fpu_method(
    l: &mut FnLower,
    base: VReg,
    base_ty: &Ty,
    m: &syn::ExprMethodCall,
) -> Result<Val, syn::Error> {
    let method = m.method.to_string();
    match method.as_str() {
        "x" | "y" | "z" | "w" => {
            if !m.args.is_empty() {
                return Err(err(&m.method, format!("{method}() takes no arguments")));
            }
            let lane = match method.as_str() {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                _ => 3,
            };
            if lane >= base_ty.fpu_lanes() {
                return Err(err(
                    &m.method,
                    format!("{} has no lane {method}", base_ty.display()),
                ));
            }
            // lane x is a free retype; the others go through the frame scratch
            let v = if lane == 0 {
                base
            } else {
                fpu_lane(l, base, lane as i16)
            };
            Ok(Val::V(v, Ty::Fix16))
        }
        "to_bits" => {
            if !m.args.is_empty() || *base_ty != Ty::Fix16 {
                return Err(err(
                    &m.method,
                    "to_bits() is a fix16 method without arguments",
                ));
            }
            Ok(Val::V(l.b.fstore(base), Ty::U16))
        }
        "to_int" => {
            if !m.args.is_empty() || *base_ty != Ty::Fix16 {
                return Err(err(
                    &m.method,
                    "to_int() is a fix16 method without arguments",
                ));
            }
            let bits = l.b.fstore(base);
            Ok(Val::V(l.b.shift(ShiftOp::Asr, bits, 8), Ty::I16))
        }
        "abs" | "floor" | "ceil" | "round" | "sat01" | "sign" => {
            if !m.args.is_empty() {
                return Err(err(&m.method, format!("{method}() takes no arguments")));
            }
            let op = match method.as_str() {
                "abs" => crate::FUnOp::Abs,
                "floor" => crate::FUnOp::Floor,
                "ceil" => crate::FUnOp::Ceil,
                "round" => crate::FUnOp::Round,
                "sat01" => crate::FUnOp::Sat01,
                _ => crate::FUnOp::Sign,
            };
            Ok(Val::V(l.b.funary(op, base), base_ty.clone()))
        }
        _ => Err(err(
            &m.method,
            format!("unknown {} method `{method}`", base_ty.display()),
        )),
    }
}

/// Buf methods (spec §10): read/write/as_ptr/as_array/len
fn array_method(
    l: &mut FnLower,
    base: VReg,
    elem: &Ty,
    n: usize,
    mutable: bool,
    m: &syn::ExprMethodCall,
) -> Result<Val, syn::Error> {
    let method = m.method.to_string();
    match method.as_str() {
        "len" => {
            if !m.args.is_empty() {
                return Err(err(&m.method, "len() takes no arguments"));
            }
            Ok(Val::V(l.b.load_imm(n as u16), Ty::U16))
        }
        "as_ptr" => {
            if !m.args.is_empty() {
                return Err(err(&m.method, "as_ptr() takes no arguments"));
            }
            Ok(Val::V(base, Ty::Ptr))
        }
        "as_array" => {
            if !m.args.is_empty() {
                return Err(err(&m.method, "as_array() takes no arguments"));
            }
            Ok(Val::V(base, Ty::ArrayRef(Box::new(elem.clone()))))
        }
        "read" => {
            if matches!(elem, Ty::Struct(_)) {
                return Err(err(
                    &m.method,
                    "read() needs a scalar element; index a struct array instead (`arr[i].field`)",
                ));
            }
            if m.args.len() != 1 {
                return Err(err(&m.method, "read(off) takes 1 argument"));
            }
            let (base2, off) = ptr_with_offset(l, base, &m.args[0])?;
            Ok(Val::V(l.b.load_mem(base2, off), Ty::U16))
        }
        "write" => {
            if matches!(elem, Ty::Struct(_)) {
                return Err(err(
                    &m.method,
                    "write() needs a scalar element; assign a field instead (`arr[i].field = v`)",
                ));
            }
            if !mutable {
                return Err(err(
                    &m.method,
                    "the Buf is not mutable (declare it with `let mut`)",
                ));
            }
            if m.args.len() != 2 {
                return Err(err(&m.method, "write(off, v) takes 2 arguments"));
            }
            let (base2, off) = ptr_with_offset(l, base, &m.args[0])?;
            let (v, _) = expr(l, &m.args[1])?.reg(l, &m.args[1], "write value")?;
            let (v, _) = coerce(l, v, &Ty::U16, &Ty::U16, &m.args[1])?;
            l.b.store_mem(base2, off, v);
            Ok(Val::V(v, Ty::Unit))
        }
        _ => Err(err(&m.method, format!("unknown array method `{method}`"))),
    }
}

fn method_call(l: &mut FnLower, m: &syn::ExprMethodCall) -> Result<Val, syn::Error> {
    // Buf methods on local buffers / global buffers
    if let Ok(name) = path_ident(&m.receiver) {
        if let Some(info) = l.lookup(&name) {
            if let Ty::Array(elem, n) = &info.ty {
                let (kind, elem, n, mutable) =
                    (info.kind.clone(), elem.as_ref().clone(), *n, info.mutable);
                let VarKind::Local { slot } = kind else {
                    unreachable!("buffers are always memory-resident")
                };
                let base = l.b.addr_of_local(slot);
                return array_method(l, base, &elem, n, mutable, m);
            }
        }
        if let Some((addr, elem, n)) = l.globals.arrays.get(&name).cloned() {
            let base = l.b.load_imm(addr);
            return array_method(l, base, &elem, n, true, m);
        }
        if let Some((Ty::Array(elem, n), addr)) = l
            .globals
            .aggregates
            .get(&name)
            .map(|(addr, ty)| (ty.clone(), *addr))
        {
            let base = l.b.load_imm(addr);
            return array_method(l, base, &elem, n, true, m);
        }
    }
    // ... and on a Buf *place* (`p.flags.as_array()`, a buffer field)
    if let Some(Ty::Array(elem, n)) = peek_type(l, &m.receiver) {
        let (elem, n) = ((*elem).clone(), n);
        let (base, offset, _, mutable) = place_addr_of(l, &m.receiver)?;
        let base = place_addr(l, base, offset);
        return array_method(l, base, &elem, n, mutable, m);
    }

    let (base, base_ty) = expr(l, &m.receiver)?.reg(l, &m.receiver, "method receiver")?;
    let method = m.method.to_string();
    if base_ty.is_fpu() {
        return fpu_method(l, base, &base_ty, m);
    }
    if let Ty::ArrayRef(_) = base_ty {
        return match method.as_str() {
            "as_ptr" => {
                if !m.args.is_empty() {
                    return Err(err(&m.method, "as_ptr() takes no arguments"));
                }
                Ok(Val::V(base, Ty::Ptr))
            }
            _ => Err(err(&m.method, format!("unknown Array method `{method}`"))),
        };
    }
    if base_ty != Ty::Ptr {
        return Err(err(
            &m.receiver,
            format!(
                "methods only exist on Ptr and arrays (got {})",
                base_ty.display()
            ),
        ));
    }
    match method.as_str() {
        "addr" => {
            if !m.args.is_empty() {
                return Err(err(&m.method, "addr() takes no arguments"));
            }
            Ok(Val::V(base, Ty::U16))
        }
        "add" => {
            let (off, off_ty) = exactly_args(l, &m.args, m, 1, "add")?[0].clone().reg(
                l,
                &m.args[0],
                "add offset",
            )?;
            if !off_ty.is_int() {
                return Err(err(&m.args[0], "pointer offset must be an integer"));
            }
            Ok(Val::V(l.b.bin(BinOp::Add, base, off), Ty::Ptr))
        }
        "read" => {
            if m.args.len() != 1 {
                return Err(err(&m.method, "read(off) takes 1 argument"));
            }
            let (base2, off) = ptr_with_offset(l, base, &m.args[0])?;
            Ok(Val::V(l.b.load_mem(base2, off), Ty::U16))
        }
        "write" => {
            if m.args.len() != 2 {
                return Err(err(&m.method, "write(off, v) takes 2 arguments"));
            }
            let (base2, off) = ptr_with_offset(l, base, &m.args[0])?;
            let (v, _) = expr(l, &m.args[1])?.reg(l, &m.args[1], "write value")?;
            let (v, _) = coerce(l, v, &Ty::U16, &Ty::U16, &m.args[1])?;
            l.b.store_mem(base2, off, v);
            Ok(Val::V(v, Ty::Unit))
        }
        "as_u16_array" | "as_i16_array" => {
            if !m.args.is_empty() {
                return Err(err(&m.method, format!("{method}() takes no arguments")));
            }
            let elem = if method == "as_u16_array" {
                Ty::U16
            } else {
                Ty::I16
            };
            Ok(Val::V(base, Ty::ArrayRef(Box::new(elem))))
        }
        _ => Err(err(&m.method, format!("unknown Ptr method `{method}`"))),
    }
}

/// compute the effective address for a memory access with offset `off`:
/// a literal becomes the ISA addressing offset; an expression is added to the
/// base (offset 0 remains).
fn ptr_with_offset(l: &mut FnLower, base: VReg, off: &Expr) -> Result<(VReg, i16), syn::Error> {
    // literal offset: use the ISA's addressing offset directly
    if let Some(offset) = literal_mem_offset(off)? {
        return Ok((base, offset));
    }
    let (off_v, off_ty) = expr(l, off)?.reg(l, off, "pointer offset")?;
    if !off_ty.is_int() {
        return Err(err(off, "pointer offset must be an integer"));
    }
    Ok((l.b.bin(BinOp::Add, base, off_v), 0))
}

fn literal_mem_offset(off: &Expr) -> Result<Option<i16>, syn::Error> {
    if let Expr::Lit(lit) = off {
        if let Lit::Int(i) = &lit.lit {
            if matches!(i.suffix(), "" | "u16" | "i16") {
                let v = lit_int_value(i)? as i64;
                let max = if i.suffix() == "u16" {
                    u16::MAX as i64
                } else {
                    i16::MAX as i64
                };
                if (0..=max).contains(&v) {
                    return Ok(Some(v as i16));
                }
            }
        }
    }
    if let Expr::Unary(unary) = off {
        if matches!(unary.op, SUnOp::Neg(_)) {
            if let Expr::Lit(lit) = unary.expr.as_ref() {
                if let Lit::Int(i) = &lit.lit {
                    if i.suffix() == "i16" {
                        let magnitude = lit_int_value(i)?;
                        if magnitude <= 32768 {
                            return Ok(Some((-(magnitude as i32)) as i16));
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn path_ident(e: &Expr) -> Result<String, syn::Error> {
    if let Expr::Path(p) = e {
        if let Some(seg) = p.path.segments.first() {
            if p.path.segments.len() == 1 {
                return Ok(seg.ident.to_string());
            }
        }
    }
    Err(err(e, "expected a plain identifier"))
}

fn unify_int(a: Ty, b: Ty) -> Option<Ty> {
    match (a, b) {
        (Ty::U16, Ty::U16) => Some(Ty::U16),
        (Ty::I16, Ty::I16) => Some(Ty::I16),
        (Ty::UntypedInt, t) | (t, Ty::UntypedInt) if t.is_int() => Some(t),
        // identical FPU types unify (if-expression branches, phis)
        (a, b) if a == b && a.is_fpu() => Some(a),
        _ => None,
    }
}

/// implicit conversion at assignment/argument/return positions: only the
/// same type, or an untyped literal adopting the target type. anything else
/// needs an explicit `as` cast (see cast()).
fn coerce(
    _l: &mut FnLower,
    v: VReg,
    from: &Ty,
    to: &Ty,
    at: &Expr,
) -> Result<(VReg, Ty), syn::Error> {
    if from == to || *from == Ty::Never || (*from == Ty::UntypedInt && to.is_int()) {
        Ok((v, to.clone()))
    } else {
        Err(err(
            at,
            format!(
                "type mismatch: expected {}, got {} (cast with `as`)",
                to.display(),
                from.display()
            ),
        ))
    }
}

fn cast(e: &Expr, v: VReg, from: Ty, to: Ty) -> Result<(VReg, Ty), syn::Error> {
    let ok = matches!(
        (&from, &to),
        (Ty::U16, Ty::I16)
            | (Ty::I16, Ty::U16)
            | (Ty::U16, Ty::Ptr)
            | (Ty::Ptr, Ty::U16)
            | (Ty::Bool, Ty::U16)
            | (Ty::Bool, Ty::I16)
            | (Ty::Enum(_), Ty::U16)
            | (Ty::Enum(_), Ty::I16)
    ) || from == to
        || (from == Ty::UntypedInt && to.is_int());
    if ok {
        Ok((v, to))
    } else {
        Err(err(
            e,
            format!("cannot cast {} to {}", from.display(), to.display()),
        ))
    }
}

/// a plain integer literal (optionally negated) usable directly as an
/// immediate operand; other expressions keep the normal register lowering
fn literal_int(e: &Expr) -> Option<(u16, Ty)> {
    fn plain(i: &syn::LitInt) -> Option<(u16, Ty)> {
        let ty = match i.suffix() {
            "" => Ty::UntypedInt,
            "u16" => Ty::U16,
            "i16" => Ty::I16,
            _ => return None,
        };
        // `lit_int_value` understands hex/octal/binary and `_` separators
        u16::try_from(lit_int_value(i).ok()?).ok().map(|v| (v, ty))
    }
    match e {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(i) => plain(i),
            _ => None,
        },
        Expr::Unary(u) if matches!(u.op, SUnOp::Neg(_)) => match u.expr.as_ref() {
            Expr::Lit(lit) => match &lit.lit {
                Lit::Int(i) => plain(i).map(|(v, ty)| (v.wrapping_neg(), ty)),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// an integer operand usable directly as an immediate: a literal or a named
/// integer constant, resolved to its value and declared type. Variables,
/// pointers, and aggregate/FPU constants are not eligible.
fn immediate_operand(l: &FnLower, e: &Expr) -> Option<(u16, Ty)> {
    if let Some(value) = literal_int(e) {
        return Some(value);
    }
    if let Expr::Path(_) = e {
        if let Ok(name) = path_ident(e) {
            // a local or parameter shadows a constant of the same name
            if l.lookup(&name).is_none() {
                if let Some((value, ty)) = l.consts.get(&name) {
                    if ty.is_int() {
                        return Some((*value, ty.clone()));
                    }
                }
            }
        }
    }
    None
}

/// shift amount: a constant selects the immediate encoding, any other integer
/// expression selects the register-count encoding (the hardware masks the
/// amount to the low four bits)
fn shift_operand(l: &mut FnLower, e: &Expr) -> Result<crate::IntOperand, syn::Error> {
    if let Some((value, _)) = immediate_operand(l, e) {
        if value > 15 {
            return Err(err(e, "shift amount must be a literal constant in 0..=15"));
        }
        return Ok(crate::IntOperand::Imm(value));
    }
    let (v, ty) = expr(l, e)?.reg(l, e, "shift amount")?;
    if !ty.is_int() {
        return Err(err(e, "shift amount must be an integer"));
    }
    Ok(crate::IntOperand::Reg(v))
}

/// How a constant unsigned divisor becomes a multiply plus shifts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MagicDivide {
    /// `q = mulhi(x, m) >> s`, with a magic that fits 16 bits
    High { m: u16, s: u8 },
    /// A 17-bit magic `M = 2^16 + m`: `q = (x + mulhi(x, m)) >> s`. That sum needs
    /// 17 bits, so the sequence averages without overflowing:
    /// `avg = (x & t) + ((x ^ t) >> 1) == (x + t) >> 1`, then `q = avg >> (s - 1)`.
    /// The rounding-up magic only needs its extra bit when `s >= 1`.
    Add { m: u16, s: u8 },
}

impl MagicDivide {
    /// the exact sequence the backend emits, in u16 arithmetic
    fn quotient(self, x: u16) -> u16 {
        match self {
            MagicDivide::High { m, s } => (((u32::from(x) * u32::from(m)) >> 16) as u16) >> s,
            MagicDivide::Add { m, s } => {
                let t = ((u32::from(x) * u32::from(m)) >> 16) as u16;
                let avg = (x & t).wrapping_add((x ^ t) >> 1);
                avg >> (s - 1)
            }
        }
    }
}

/// round-up magic for a constant unsigned divisor: `x / d` as one `MUL16` plus
/// shifts, exact for every 16-bit `x`.
///
/// Returns `None` when no such form exists (`d` is 1 or a power of two, or the shift
/// search runs out); the caller then falls back to the `div_u16` routine. Candidates
/// are checked against **every** 16-bit numerator rather than a closed-form bound: a
/// rejected candidate stops at its first mismatch, an accepted one costs 65 536
/// iterations of a multiply and a shift, and the compiler never has to trust a
/// rounding bound it might have subtly wrong.
fn magic_divide_u16(d: u16) -> Option<MagicDivide> {
    if d < 2 || d.is_power_of_two() {
        return None;
    }
    for s in 0..=16u8 {
        let wide = (1u64 << (16 + u32::from(s))).div_ceil(u64::from(d));
        let candidate = if wide <= u64::from(u16::MAX) && s <= 15 {
            // the shift is the final `>> s`, which must fit the immediate field
            MagicDivide::High { m: wide as u16, s }
        } else if wide <= 0x1_ffff && s >= 1 {
            // here the final shift is `s - 1`, so s may reach 16
            MagicDivide::Add {
                m: (wide - 0x1_0000) as u16,
                s,
            }
        } else {
            continue;
        };
        if (0..=u16::MAX).all(|x| candidate.quotient(x) == x / d) {
            return Some(candidate);
        }
    }
    None
}

fn shift_op(op: &SBinOp, ty: &Ty) -> ShiftOp {
    use SBinOp::*;
    match (op, ty) {
        (Shl(_) | ShlEq(_), _) => ShiftOp::Lsl,
        (Shr(_) | ShrEq(_), Ty::I16) => ShiftOp::Asr,
        (Shr(_) | ShrEq(_), _) => ShiftOp::Lsr,
        _ => unreachable!("shift_op"),
    }
}

fn exactly_args(
    l: &mut FnLower,
    args: &syn::punctuated::Punctuated<Expr, syn::Token![,]>,
    at: &impl syn::spanned::Spanned,
    n: usize,
    what: &str,
) -> Result<Vec<Val>, syn::Error> {
    if args.len() != n {
        return Err(err(
            at,
            format!("{what} takes {n} arguments, got {}", args.len()),
        ));
    }
    args.iter().map(|a| expr(l, a)).collect()
}

fn check_call_args(
    call: &syn::ExprCall,
    params: &[Ty],
    args: &[Ty],
    name: &str,
) -> Result<(), syn::Error> {
    if params.len() != args.len() {
        return Err(err(
            call,
            format!(
                "`{name}` takes {} arguments, got {}",
                params.len(),
                args.len()
            ),
        ));
    }
    for (i, (p, a)) in params.iter().zip(args).enumerate() {
        let ok = p == a || (*a == Ty::UntypedInt && p.is_int());
        if !ok {
            return Err(err(
                call,
                format!(
                    "argument {} of `{name}`: expected {}, got {}",
                    i + 1,
                    p.display(),
                    a.display()
                ),
            ));
        }
    }
    Ok(())
}

fn check_fn_sig(
    l: &mut FnLower,
    name: &'static str,
    expected: &Ty,
    at: &impl syn::spanned::Spanned,
) -> Result<(), syn::Error> {
    let sig = l
        .sigs
        .get(name)
        .ok_or_else(|| err(at, format!("undefined function `{name}`")))?;
    let Ty::FnPtr { params, ret } = expected else {
        unreachable!()
    };
    if *params != sig.params || **ret != sig.ret {
        return Err(err(
            at,
            format!(
                "fn pointer type mismatch for `{name}`: expected fn({}) -> {}",
                params
                    .iter()
                    .map(|t| t.display())
                    .collect::<Vec<_>>()
                    .join(", "),
                ret.display()
            ),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::parse_source_with;

    #[test]
    fn fix16_rejects_int_operands_without_conversion() {
        let src = "fn main() { let a = fix16::zero(); let b: u16 = 1; let c = a + b; halt(c.to_bits()); }";
        assert!(parse_source_with(src, 0).is_err());
    }

    #[test]
    fn vec4_has_no_comparison_operator() {
        let src = "fn main() { let a = vec4::zero(); if a == a { halt(1); } }";
        assert!(parse_source_with(src, 0).is_err());
    }

    #[test]
    fn fix16_comparison_is_allowed() {
        let src = "fn main() { let a = fix16::zero(); if a == a { halt(1); } }";
        assert!(parse_source_with(src, 0).is_ok());
    }

    #[test]
    fn magic_divisor_is_exact_for_every_numerator() {
        // 0/1 and powers of two have no magic form: the caller uses shift/mask there
        for d in [0u16, 1, 2, 4, 4096, 32768] {
            assert!(
                super::magic_divide_u16(d).is_none(),
                "d={d} must not have one"
            );
        }
        for d in [
            3u16, 5, 6, 7, 9, 10, 11, 12, 13, 17, 100, 255, 257, 1000, 1001, 4095, 32767, 65534,
            65535,
        ] {
            let magic =
                super::magic_divide_u16(d).unwrap_or_else(|| panic!("no magic form for d={d}"));
            for x in 0..=u16::MAX {
                assert_eq!(magic.quotient(x), x / d, "d={d} x={x} magic={magic:?}");
            }
        }
    }

    #[test]
    fn struct_layout_pads_fields_to_alignment() {
        let file = syn::parse_file(
            r#"
            struct Inner { a: u16, b: u16 }
            #[repr(align(4))]
            struct Aligned { v: u16 }
            struct Mixed { a: u16, b: Aligned, c: u16 }
            #[repr(C)]
            struct WithArray { head: u16, data: Buf<u16, 3>, tail: u16 }
            "#,
        )
        .unwrap();
        let mut raw: std::collections::BTreeMap<String, (usize, &syn::ItemStruct)> =
            std::collections::BTreeMap::new();
        for item in &file.items {
            if let syn::Item::Struct(s) = item {
                raw.insert(s.ident.to_string(), (0usize, s));
            }
        }
        let names: super::TypeNames = raw
            .keys()
            .map(|name| (name.clone(), super::NominalKind::Struct))
            .collect();
        let consts = std::collections::HashMap::new();
        let mut builder = super::LayoutBuilder {
            raw: &raw,
            consts: &consts,
            names: &names,
            done: super::StructTable::new(),
            visiting: vec![],
        };
        let inner = builder.layout("Inner", raw["Inner"].1).unwrap();
        assert_eq!((inner.size, inner.align), (2, 1));

        // align(4) is two 16-bit words: one field pads the size up to two
        let aligned = builder.layout("Aligned", raw["Aligned"].1).unwrap();
        assert_eq!((aligned.size, aligned.align), (2, 2));

        // `b` is padded to offset 2, `c` follows it, and the total stays unpadded
        let mixed = builder.layout("Mixed", raw["Mixed"].1).unwrap();
        assert_eq!(
            mixed
                .fields
                .iter()
                .map(|f| (f.name.as_str(), f.offset))
                .collect::<Vec<_>>(),
            vec![("a", 0), ("b", 2), ("c", 4)]
        );
        assert_eq!(mixed.size, 5);

        let with_array = builder.layout("WithArray", raw["WithArray"].1).unwrap();
        assert_eq!(with_array.size, 5);
    }

    #[test]
    fn struct_layout_rejects_cycles_and_bad_align() {
        let file = syn::parse_file(
            r#"
            struct Loop { next: Loop }
            #[repr(align(3))]
            struct Odd { v: u16 }
            "#,
        )
        .unwrap();
        let mut raw: std::collections::BTreeMap<String, (usize, &syn::ItemStruct)> =
            std::collections::BTreeMap::new();
        for item in &file.items {
            if let syn::Item::Struct(s) = item {
                raw.insert(s.ident.to_string(), (0usize, s));
            }
        }
        let names: super::TypeNames = raw
            .keys()
            .map(|name| (name.clone(), super::NominalKind::Struct))
            .collect();
        let consts = std::collections::HashMap::new();
        let mut builder = super::LayoutBuilder {
            raw: &raw,
            consts: &consts,
            names: &names,
            done: super::StructTable::new(),
            visiting: vec![],
        };
        let error = builder
            .layout("Loop", raw["Loop"].1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("contains itself"), "{error}");
        let error = builder.layout("Odd", raw["Odd"].1).unwrap_err().to_string();
        assert!(error.contains("align must be"), "{error}");
    }
}
