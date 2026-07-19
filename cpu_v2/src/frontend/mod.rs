//! rcc frontend: parse the Rust subset (see spec.md) with `syn`, validate it,
//! and lower it onto `FuncBuilder` to produce `IrFunc`s.
//!
//! anything outside the subset is a hard error with a source span.

use crate::{BoolExpr, CompilerOptions, FuncBuilder, VarId};
use crate::{BlockId, Cmp, CmpRhs, Instr, IrFunc, BinOp, ShiftOp, UnOp, VReg};
use crate::isa::Cond;
use std::collections::{HashMap, HashSet};
use syn::{BinOp as SBinOp, Block, Expr, Item, ItemFn, Lit, Pat, Stmt, Type, UnOp as SUnOp};

#[cfg(doc)]
pub mod spec {}

/// parse rcc source text into a list of functions (IR), or report the first
/// subset violation with a span
pub fn parse_source(src: &str) -> Result<Vec<IrFunc>, syn::Error> {
    parse_source_with(src, 0)
}

/// like `parse_source`, with the static data section starting at `data_base`
pub fn parse_source_with(src: &str, data_base: u16) -> Result<Vec<IrFunc>, syn::Error> {
    let file = syn::parse_file(src)?;
    parse_files(vec![file], data_base)
}

/// the rcc standard library, embedded and appended to every program compiled
/// via `compile_program` (unused functions are dropped by the linker)
const STD_SOURCES: &[&str] = &[
    include_str!("../rcc_std/heap.rs"),
    include_str!("../rcc_std/mem.rs"),
    include_str!("../rcc_std/mul.rs"),
    include_str!("../rcc_std/vec.rs"),
];

/// compile a full program: the user source, any `mod name;` files resolved
/// through `loader`, plus the rcc standard library, with automatic library
/// initialization driven by `opts`
pub fn compile_program(
    src: &str,
    opts: &CompilerOptions,
    loader: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Vec<IrFunc>, syn::Error> {
    // gather sources: main file + user modules (recursive) + std
    let mut srcs: Vec<String> = vec![src.to_string()];
    let mut seen: HashSet<String> = HashSet::new();
    let mut i = 0;
    while i < srcs.len() {
        let text = srcs[i].clone();
        let file = syn::parse_file(&text)?;
        for item in &file.items {
            if let Item::Mod(m) = item {
                if m.content.is_some() {
                    return Err(err(&m.ident, "inline `mod name { }` is not supported; use `mod name;`"));
                }
                let name = m.ident.to_string();
                if seen.insert(name.clone()) {
                    let text2 = loader(&name)
                        .map_err(|e| err(&m.ident, format!("cannot load module `{name}`: {e}")))?;
                    srcs.push(text2);
                }
            }
        }
        i += 1;
    }
    srcs.extend(STD_SOURCES.iter().map(|t| t.to_string()));

    let mut files = vec![];
    for text in &srcs {
        files.push(syn::parse_file(text)?);
    }
    let mut out = parse_files(files, opts.data_base)?;
    auto_init(&mut out, opts)?;
    Ok(out)
}

/// insert library initialization calls at the start of `main`, driven by
/// which library functions the program's call graph reaches
fn auto_init(out: &mut Vec<IrFunc>, opts: &CompilerOptions) -> Result<(), syn::Error> {
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
    let entry = main.entry;
    let mut pre = vec![];
    let mut next_v = main.vreg_count;
    if heap_used {
        pre.push(Instr::LoadImm {
            dst: next_v,
            value: opts.heap_begin,
        });
        next_v += 1;
        pre.push(Instr::LoadImm {
            dst: next_v,
            value: opts.heap_size,
        });
        next_v += 1;
        pre.push(Instr::Call {
            func: "init_heap",
            args: vec![next_v - 2, next_v - 1],
            rets: vec![],
        });
    }
    if vec_used {
        pre.push(Instr::LoadImm {
            dst: next_v,
            value: opts.vec_init_cap,
        });
        next_v += 1;
        pre.push(Instr::Call {
            func: "init_vec",
            args: vec![next_v - 1],
            rets: vec![],
        });
    }
    main.vreg_count = next_v;
    let old = std::mem::take(&mut main.blocks[entry].insts);
    main.blocks[entry].insts = pre;
    main.blocks[entry].insts.extend(old);
    Ok(())
}

fn parse_files(files: Vec<syn::File>, data_base: u16) -> Result<Vec<IrFunc>, syn::Error> {
    let mut fns = vec![];
    let mut consts: HashMap<String, (u16, Ty)> = HashMap::new();
    let mut globals = Globals {
        next_addr: data_base,
        ..Globals::default()
    };
    for file in &files {
        for item in &file.items {
            match item {
                Item::Fn(f) => {
                    for attr in &f.attrs {
                        if !attr.path.is_ident("allow") && !attr.path.is_ident("doc") {
                            return Err(err(attr, "attributes are not supported (except #[allow])"));
                        }
                    }
                    fns.push(f)
                }
                Item::Use(_) => { /* ignored: for the IDE only */ }
                Item::Const(c) => {
                    let ty = ty_of(&c.ty)?;
                    if !matches!(ty, Ty::U16 | Ty::I16) {
                        return Err(err(&c.ty, "const must be u16 or i16"));
                    }
                    let v = const_eval(&c.expr, &consts)?;
                    let name = c.ident.to_string();
                    if consts.insert(name.clone(), (v, ty)).is_some() {
                        return Err(err(&c.ident, format!("const `{name}` defined twice")));
                    }
                }
                Item::Static(s) => add_static(s, &consts, &mut globals)?,
                Item::Verbatim(_) => { /* attributes on use items land here */ }
                Item::Mod(_) => { /* already resolved by compile_program */ }
                Item::Struct(_) => return Err(err(item, "structs are not supported (see spec §12)")),
                Item::Trait(_) => return Err(err(item, "traits are not supported")),
                Item::Impl(_) => return Err(err(item, "impl blocks are not supported")),
                Item::Macro(_) => return Err(err(item, "macros are not supported")),
                _ => return Err(err(item, "item not supported (only fn/use/const/static are allowed)")),
            }
        }
    }
    // collect signatures first (functions can call each other regardless of order)
    let mut sigs: HashMap<String, Sig> = HashMap::new();
    let mut names = vec![];
    for f in &fns {
        let name = f.sig.ident.to_string();
        let sig = signature(f)?;
        if sigs.insert(name.clone(), sig).is_some() {
            return Err(err(&f.sig.ident, format!("function `{name}` defined twice")));
        }
        names.push(intern(&name));
    }
    let mut out = vec![];
    for (name, f) in names.into_iter().zip(fns) {
        out.push(lower_fn(name, f, &sigs, &consts, &globals)?);
    }
    emit_data_init(&mut out, &globals)?;
    Ok(out)
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
    let entry = main.entry;
    let mut inits = vec![];
    for &(addr, value) in &globals.data_words {
        if value != 0 {
            inits.push(Instr::LoadImm {
                dst: main.vreg_count,
                value: addr,
            });
            main.vreg_count += 1;
            inits.push(Instr::LoadImm {
                dst: main.vreg_count,
                value,
            });
            main.vreg_count += 1;
            let base = main.vreg_count - 2;
            let val = main.vreg_count - 1;
            inits.push(Instr::StoreMem {
                base,
                offset: 0,
                src: val,
            });
        }
    }
    let old = std::mem::take(&mut main.blocks[entry].insts);
    main.blocks[entry].insts = inits;
    main.blocks[entry].insts.extend(old);
    Ok(())
}

fn intern(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// source line of a syntax node (for listing comments)
fn line_of(t: &impl syn::spanned::Spanned) -> u32 {
    t.span().start().line as u32
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
    /// (addr, value) words for __data_init
    data_words: Vec<(u16, u16)>,
    next_addr: u16,
}

/// evaluate a constant expression (literals, other consts, wrapping arithmetic)
fn const_eval(e: &Expr, consts: &HashMap<String, (u16, Ty)>) -> Result<u16, syn::Error> {
    match e {
        Expr::Paren(p) => const_eval(&p.expr, consts),
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
            let _ = ty_of(&c.ty)?;
            const_eval(&c.expr, consts)
        }
        _ => Err(err(e, "not a constant expression")),
    }
}

/// `static NAME: Ty = expr;` or `static NAME: [Ty; N] = [..];`
fn add_static(
    s: &syn::ItemStatic,
    consts: &HashMap<String, (u16, Ty)>,
    g: &mut Globals,
) -> Result<(), syn::Error> {
    if s.mutability.is_some() {
        return Err(err(s, "static mut is not supported; write via addr_of(&X) (see spec §9.2)"));
    }
    let name = s.ident.to_string();
    match s.ty.as_ref() {
        Type::Array(a) => {
            let elem = ty_of(&a.elem)?;
            if !matches!(elem, Ty::U16 | Ty::I16) {
                return Err(err(&a.elem, "array element type must be u16 or i16"));
            }
            let len = const_eval(&a.len, consts)? as usize;
            let init: Vec<u16> = match s.expr.as_ref() {
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
                            format!("array repeat count {m} does not match declared length {len}"),
                        ));
                    }
                    let v = const_eval(&r.expr, consts)?;
                    vec![v; len]
                }
                _ => return Err(err(&s.expr, "array static needs an initializer list or [v; N]")),
            };
            if init.len() != len {
                return Err(err(
                    &s.expr,
                    format!("initializer has {} elements, expected {len}", init.len()),
                ));
            }
            let addr = g.next_addr;
            g.next_addr += len as u16;
            for (i, w) in init.iter().enumerate() {
                g.data_words.push((addr + i as u16, *w));
            }
            if g.arrays.insert(name.clone(), (addr, elem, len)).is_some() {
                return Err(err(&s.ident, format!("static `{name}` defined twice")));
            }
            Ok(())
        }
        t => {
            let ty = ty_of(t)?;
            if !matches!(ty, Ty::U16 | Ty::I16) {
                return Err(err(t, "static must be u16/i16 or an array of them"));
            }
            let v = const_eval(&s.expr, consts)?;
            let addr = g.next_addr;
            g.next_addr += 1;
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
    Bool,
    FnPtr { params: Vec<Ty>, ret: Box<Ty> },
    /// [u16; N] / [i16; N], memory-resident (data section or stack frame)
    Array(Box<Ty>, usize),
    Unit,
    Never,
}
impl Ty {
    fn is_int(&self) -> bool {
        matches!(self, Ty::U16 | Ty::I16 | Ty::UntypedInt)
    }
    fn display(&self) -> String {
        match self {
            Ty::U16 => "u16".into(),
            Ty::I16 => "i16".into(),
            Ty::UntypedInt => "integer literal".into(),
            Ty::Ptr => "Ptr".into(),
            Ty::Bool => "bool".into(),
            Ty::FnPtr { params, ret } => format!(
                "fn({}) -> {}",
                params.iter().map(|t| t.display()).collect::<Vec<_>>().join(", "),
                ret.display()
            ),
            Ty::Array(elem, n) => format!("[{}; {n}]", elem.display()),
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

fn ty_of(ty: &Type) -> Result<Ty, syn::Error> {
    match ty {
        Type::Path(tp) => {
            if tp.path.segments.len() != 1 {
                return Err(err(ty, "unsupported type (expected u16/i16/Ptr/fn pointer)"));
            }
            let seg = &tp.path.segments[0];
            if !seg.arguments.is_empty() {
                return Err(err(ty, "generics are not supported"));
            }
            match seg.ident.to_string().as_str() {
                "u16" => Ok(Ty::U16),
                "i16" => Ok(Ty::I16),
                "Ptr" => Ok(Ty::Ptr),
                "bool" => Ok(Ty::Bool),
                _ => Err(err(ty, "type not supported (only u16/i16/Ptr/fn pointer)")),
            }
        }
        Type::BareFn(bf) => {
            let params = bf
                .inputs
                .iter()
                .map(|a| ty_of(&a.ty))
                .collect::<Result<Vec<_>, _>>()?;
            let ret = match &bf.output {
                syn::ReturnType::Default => Ty::Unit,
                syn::ReturnType::Type(_, t) => ty_of(t)?,
            };
            Ok(Ty::FnPtr {
                params,
                ret: Box::new(ret),
            })
        }
        Type::Never(_) => Ok(Ty::Unit),
        Type::Paren(p) => ty_of(&p.elem),
        Type::Array(_) => Err(err(ty, "arrays are not allowed here (declare them in let/static, pass as Ptr)")),
        Type::Reference(_) => Err(err(ty, "reference types `&T` are not supported (pass arrays as Ptr)")),
        Type::Slice(_) => Err(err(ty, "slice types are not supported (pass arrays as Ptr)")),
        _ => Err(err(ty, "unsupported type")),
    }
}

/// type annotation that may be an array type (valid only in let/static positions)
fn ty_of_maybe_array(ty: &Type, consts: &HashMap<String, (u16, Ty)>) -> Result<Ty, syn::Error> {
    match ty {
        Type::Array(a) => {
            let elem = ty_of(&a.elem)?;
            if !matches!(elem, Ty::U16 | Ty::I16) {
                return Err(err(&a.elem, "array element type must be u16 or i16"));
            }
            let len = const_eval(&a.len, consts)? as usize;
            Ok(Ty::Array(Box::new(elem), len))
        }
        _ => ty_of(ty),
    }
}

/// initialize a local array in the stack frame
fn init_array(l: &mut FnLower, slot: u8, elem: &Ty, n: usize, init: &Expr) -> Result<(), syn::Error> {
    let base = l.b.addr_of_local(slot);
    match init {
        Expr::Repeat(r) => {
            let m = const_eval(&r.len, l.consts)? as usize;
            if m != n {
                return Err(err(
                    init,
                    format!("array repeat count {m} does not match declared length {n}"),
                ));
            }
            let (v, _) = expr(l, &r.expr)?.reg(l, "array initializer")?;
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
                    format!("array initializer has {} elements, expected {n}", arr.elems.len()),
                ));
            }
            for (i, e) in arr.elems.iter().enumerate() {
                let (v, _) = expr(l, e)?.reg(l, "array initializer")?;
                let (v, _) = coerce(l, v, elem, elem, e)?;
                l.b.store_mem(base, i as i16, v);
            }
            Ok(())
        }
        _ => Err(err(init, "array initializer must be [v; N] or [e0, e1, ...]")),
    }
}

fn signature(f: &ItemFn) -> Result<Sig, syn::Error> {
    if !f.sig.generics.params.is_empty() {
        return Err(err(&f.sig.generics, "generics are not supported"));
    }
    if f.sig.constness.is_some() || f.sig.asyncness.is_some() || f.sig.unsafety.is_some() {
        return Err(err(&f.sig, "const/async/unsafe functions are not supported"));
    }
    if f.sig.abi.is_some() || f.sig.variadic.is_some() {
        return Err(err(&f.sig, "extern/variadic functions are not supported"));
    }
    let mut params = vec![];
    for arg in &f.sig.inputs {
        match arg {
            syn::FnArg::Typed(pt) => params.push(ty_of(&pt.ty)?),
            syn::FnArg::Receiver(r) => return Err(err(r, "methods are not supported")),
        }
    }
    if params.len() > 6 {
        return Err(err(&f.sig, "too many parameters (max 6)"));
    }
    for (i, ty) in params.iter().enumerate() {
        if *ty == Ty::Bool {
            return Err(err(&f.sig, format!(
                "parameter {} is bool: bool only lives in conditions (use u16 0/1)",
                i + 1
            )));
        }
    }
    let ret = match &f.sig.output {
        syn::ReturnType::Default => Ty::Unit,
        syn::ReturnType::Type(_, t) => ty_of(t)?,
    };
    if ret == Ty::Bool {
        return Err(err(&f.sig, "bool return type is not supported (bool only lives in conditions)"));
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
    ret_ty: Ty,
    /// true once the current block has ended (return/halt)
    dead: bool,
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
    fn reg(self, l: &mut FnLower, what: &str) -> Result<(VReg, Ty), syn::Error> {
        match self {
            Val::V(v, ty) => Ok((v, ty)),
            Val::Bool(_) => Err(l.err_here(format!(
                "boolean value used as {what}; bool only lives in conditions (see spec §6)"
            ))),
            Val::FnItem(name) => Err(l.err_here(format!(
                "function `{name}` used as {what}; assign it to a fn pointer variable or call it"
            ))),
            Val::Unit => Err(l.err_here(format!("unit value used as {what}"))),
            Val::Never => Ok((0, Ty::Never)),
        }
    }
}

impl FnLower<'_> {
    fn err_here(&self, msg: impl std::fmt::Display) -> syn::Error {
        syn::Error::new(proc_macro2::Span::call_site(), msg.to_string())
    }
    fn lookup(&self, name: &str) -> Option<&VarInfo> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
    fn declare(&mut self, name: String, info: VarInfo) {
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
    /// the address of a variable as a Ptr (addr_of / as_ptr)
    fn addr_of_var(&mut self, name: &str, at: &Expr) -> Result<VReg, syn::Error> {
        if let Some(info) = self.lookup(name) {
            let kind = info.kind.clone();
            return match kind {
                VarKind::Local { slot } => Ok(self.b.addr_of_local(slot)),
                VarKind::Ssa { .. } => Err(err(
                    at,
                    format!("`{name}` is not memory-resident; it must be declared as an array"),
                )),
            };
        }
        if let Some(&(addr, _)) = self.globals.scalars.get(name) {
            return Ok(self.b.load_imm(addr));
        }
        if let Some(&(addr, _, _)) = self.globals.arrays.get(name) {
            return Ok(self.b.load_imm(addr));
        }
        Err(err(at, format!("undefined name `{name}`")))
    }
}

/// why a variable must live in the stack frame instead of a register
enum ResidentKind {
    Scalar,
    Array,
}

/// prescan a function body for names that must be memory-resident:
/// variables whose address is taken (addr_of) and array-typed lets
fn scan_residents(
    blk: &Block,
    consts: &HashMap<String, (u16, Ty)>,
    out: &mut HashMap<String, ResidentKind>,
) -> Result<(), syn::Error> {
    for s in &blk.stmts {
        scan_stmt(s, consts, out)?;
    }
    Ok(())
}
fn scan_stmt(s: &Stmt, consts: &HashMap<String, (u16, Ty)>, out: &mut HashMap<String, ResidentKind>) -> Result<(), syn::Error> {
    match s {
        Stmt::Local(local) => {
            if let Pat::Type(pt) = &local.pat {
                if let Type::Array(a) = pt.ty.as_ref() {
                    ty_of(&a.elem)?;
                    const_eval(&a.len, consts)?;
                    if let Pat::Ident(p) = pt.pat.as_ref() {
                        out.insert(p.ident.to_string(), ResidentKind::Array);
                    }
                }
            }
            if let Some((_, init)) = &local.init {
                scan_expr(init, consts, out)?;
            }
        }
        Stmt::Expr(e) | Stmt::Semi(e, _) => scan_expr(e, consts, out)?,
        Stmt::Item(_) => {}
    }
    Ok(())
}
fn scan_expr(e: &Expr, consts: &HashMap<String, (u16, Ty)>, out: &mut HashMap<String, ResidentKind>) -> Result<(), syn::Error> {
    match e {
        Expr::Paren(x) => scan_expr(&x.expr, consts, out),
        Expr::Binary(x) => {
            scan_expr(&x.left, consts, out)?;
            scan_expr(&x.right, consts, out)
        }
        Expr::Unary(x) => scan_expr(&x.expr, consts, out),
        Expr::Cast(x) => scan_expr(&x.expr, consts, out),
        Expr::Call(x) => {
            if let Expr::Path(p) = x.func.as_ref() {
                if p.path.is_ident("addr_of") {
                    if let Some(Expr::Reference(r)) = x.args.first() {
                        if let Ok(name) = path_ident(&r.expr) {
                            out.entry(name).or_insert(ResidentKind::Scalar);
                        }
                    }
                }
            }
            for a in &x.args {
                scan_expr(a, consts, out)?;
            }
            Ok(())
        }
        Expr::MethodCall(x) => {
            scan_expr(&x.receiver, consts, out)?;
            for a in &x.args {
                scan_expr(a, consts, out)?;
            }
            Ok(())
        }
        Expr::Assign(x) => {
            scan_expr(&x.left, consts, out)?;
            scan_expr(&x.right, consts, out)
        }
        Expr::AssignOp(x) => {
            scan_expr(&x.left, consts, out)?;
            scan_expr(&x.right, consts, out)
        }
        Expr::If(x) => {
            scan_expr(&x.cond, consts, out)?;
            scan_residents(&x.then_branch, consts, out)?;
            if let Some((_, e)) = &x.else_branch {
                scan_expr(e, consts, out)?;
            }
            Ok(())
        }
        Expr::While(x) => {
            scan_expr(&x.cond, consts, out)?;
            scan_residents(&x.body, consts, out)
        }
        Expr::Loop(x) => scan_residents(&x.body, consts, out),
        Expr::ForLoop(x) => {
            scan_expr(&x.expr, consts, out)?;
            scan_residents(&x.body, consts, out)
        }
        Expr::Block(x) => scan_residents(&x.block, consts, out),
        Expr::Return(x) => {
            if let Some(e) = &x.expr {
                scan_expr(e, consts, out)?;
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
) -> Result<IrFunc, syn::Error> {
    let sig = sigs.get(&f.sig.ident.to_string()).unwrap().clone();
    let n_rets = if sig.ret == Ty::Unit { 0 } else { 1 };
    let (b, param_vars) = FuncBuilder::new(name, sig.params.len(), n_rets);

    // prescan: which names must be memory-resident
    let mut residents: HashMap<String, ResidentKind> = HashMap::new();
    scan_residents(&f.block, consts, &mut residents)?;

    let mut param_names = vec![];
    let mut l = FnLower {
        b,
        sigs,
        consts,
        globals,
        residents,
        scopes: vec![HashMap::new()],
        ret_ty: sig.ret.clone(),
        dead: false,
    };
    for (arg, (var, ty)) in f.sig.inputs.iter().zip(param_vars.iter().zip(sig.params.iter())) {
        let syn::FnArg::Typed(pt) = arg else {
            unreachable!()
        };
        let ident = match pt.pat.as_ref() {
            Pat::Ident(p) => {
                if p.mutability.is_some() {
                    return Err(err(&p.ident, "params are always immutable; use a local copy"));
                }
                p.ident.to_string()
            }
            _ => return Err(err(&pt.pat, "unsupported parameter pattern")),
        };
        param_names.push(intern(&ident));
        // an address-taken param is copied into a frame slot at entry
        let kind = match l.residents.remove(&ident) {
            Some(ResidentKind::Scalar) => {
                let slot = l.b.alloc_local_slots(1);
                let pv = l.b.get(*var);
                l.b.store_local(slot, pv);
                VarKind::Local { slot }
            }
            Some(ResidentKind::Array) => {
                return Err(err(&pt.pat, "params cannot be arrays; pass a Ptr instead"))
            }
            None => VarKind::Ssa { var: *var },
        };
        l.declare(
            ident,
            VarInfo {
                kind,
                ty: ty.clone(),
                mutable: false,
            },
        );
    }
    let ret_names: Vec<&'static str> = if sig.ret == Ty::Unit { vec![] } else { vec!["r"] };
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
        let (v, from) = expr(&mut l, t)?.reg(&mut l, "tail expression").map_err(|e| syn::Error::new(e.span(), format!("in fn `{name}`: {e}")))?;
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
    Ok(l.b.finish())
}

fn block(l: &mut FnLower, blk: &Block) -> Result<(), syn::Error> {
    l.scopes.push(HashMap::new());
    for s in &blk.stmts {
        if l.dead {
            return Err(err(s, "unreachable code (after return/halt)"));
        }
        stmt(l, s)?;
    }
    l.scopes.pop();
    Ok(())
}

fn stmt(l: &mut FnLower, s: &Stmt) -> Result<(), syn::Error> {
    match s {
        Stmt::Local(local) => {
            let (inner_pat, annotated) = match &local.pat {
                Pat::Type(pt) => {
                    let ty = ty_of_maybe_array(&pt.ty, l.consts)?;
                    (pt.pat.as_ref(), Some(ty))
                }
                p => (p, None),
            };
            let (ident, mutable) = match inner_pat {
                Pat::Ident(p) => (p.ident.to_string(), p.mutability.is_some()),
                _ => return Err(err(&local.pat, "unsupported pattern (only plain identifiers)")),
            };
            let init = local
                .init
                .as_ref()
                .ok_or_else(|| err(s, "let without initializer is not supported"))?;

            // local array: `let mut buf: [u16; N] = [0; N];`
            if let Some(Ty::Array(elem, n)) = &annotated {
                let (elem, n) = (elem.as_ref().clone(), *n);
                let slot = l.b.alloc_local_slots(n as u8);
                init_array(l, slot, &elem, n, &init.1)?;
                l.declare(
                    ident,
                    VarInfo {
                        kind: VarKind::Local { slot },
                        ty: annotated.clone().unwrap(),
                        mutable,
                    },
                );
                return Ok(());
            }
            if matches!(l.residents.get(&ident), Some(ResidentKind::Array)) {
                return Err(err(&local.pat, "arrays need a type annotation like `let mut buf: [u16; N] = [0; N];`"));
            }

            let val = expr(l, &init.1)?;
            // fn pointer binding: `let f: fn(...) = some_fn;`
            let (v, ty) = match (val, &annotated) {
                (Val::FnItem(name), Some(Ty::FnPtr { .. })) => {
                    check_fn_sig(l, name, annotated.as_ref().unwrap())?;
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
                    let (v, from) = val.reg(l, "let initializer")?;
                    let to = annotated.clone().unwrap_or_else(|| from.clone());
                    let (v, _) = coerce(l, v, &from, &to, &init.1)?;
                    (v, to)
                }
            };
            // a scalar whose address is taken anywhere is memory-resident
            let kind = if matches!(l.residents.get(&ident), Some(ResidentKind::Scalar)) {
                let slot = l.b.alloc_local_slots(1);
                l.b.store_local(slot, v);
                VarKind::Local { slot }
            } else {
                let var = l.b.new_var();
                l.b.set(var, v);
                VarKind::Ssa { var }
            };
            l.declare(ident, VarInfo { kind, ty, mutable });
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
            let name = path_ident(&a.left)?;
            let info = l
                .lookup(&name)
                .ok_or_else(|| err(&a.left, format!("undefined variable `{name}`")))?;
            if !info.mutable {
                return Err(err(&a.left, format!("`{name}` is not mutable (declare with `let mut`)")));
            }
            let (kind, ty) = (info.kind.clone(), info.ty.clone());
            let val = expr(l, &a.right)?;
            // fn pointer reassignment: `f = other_fn;`
            if let (Val::FnItem(fname), Ty::FnPtr { .. }) = (&val, &ty) {
                check_fn_sig(l, fname, &ty)?;
                let v = l.b.load_func_addr(fname);
                l.write_var(&kind, v);
                return Ok(());
            }
            let (v, _) = val.reg(l, "assignment")?;
            let (v, _) = coerce(l, v, &ty, &ty, &a.right)?;
            l.write_var(&kind, v);
            Ok(())
        }
        Expr::AssignOp(a) => {
            let name = path_ident(&a.left)?;
            let info = l
                .lookup(&name)
                .ok_or_else(|| err(&a.left, format!("undefined variable `{name}`")))?;
            if !info.mutable {
                return Err(err(&a.left, format!("`{name}` is not mutable (declare with `let mut`)")));
            }
            let (kind, ty) = (info.kind.clone(), info.ty.clone());
            if !ty.is_int() {
                return Err(err(&a.left, "compound assignment only works on integers"));
            }
            let cur = l.read_var(&kind);
            let (rhs, _) = expr(l, &a.right)?.reg(l, "compound assignment")?;
            let op = match a.op {
                SBinOp::AddEq(_) => BinOp::Add,
                SBinOp::SubEq(_) => BinOp::Sub,
                SBinOp::BitAndEq(_) => BinOp::And,
                SBinOp::BitOrEq(_) => BinOp::Or,
                SBinOp::BitXorEq(_) => BinOp::Xor,
                SBinOp::ShlEq(_) | SBinOp::ShrEq(_) => {
                    let amount = shift_amount(l, &a.right)?;
                    let sop = shift_op(&a.op, &ty);
                    let v = l.b.shift(sop, cur, amount);
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
            match (&ret_ty, &r.expr) {
                (Ty::Unit, None) => l.b.ret(&[]),
                (Ty::Unit, Some(e)) => {
                    return Err(err(e, "returning a value from a function without return type"))
                }
                (expected, Some(e)) => {
                    let (v, from) = expr(l, e)?.reg(l, "return value")?;
                    let (v, _) = coerce(l, v, &from, expected, e)?;
                    l.b.ret(&[v]);
                }
                (expected, None) => {
                    return Err(err(r, format!("missing return value (expected {})", expected.display())))
                }
            }
            l.dead = true;
            Ok(())
        }
        Expr::If(_) | Expr::While(_) | Expr::Loop(_) | Expr::ForLoop(_) => {
            control_flow(l, e)
        }
        Expr::Break(_) => {
            l.b.break_();
            l.dead = true;
            Ok(())
        }
        Expr::Continue(_) => {
            l.b.continue_();
            l.dead = true;
            Ok(())
        }
        _ => {
            // expression statement for side effects (calls, stores, intrinsics)
            let val = expr(l, e)?;
            match val {
                Val::Unit | Val::V(_, _) | Val::Bool(_) | Val::Never => Ok(()),
                Val::FnItem(name) => Err(l.err_here(format!(
                    "function `{name}` as a statement; did you mean to call it?"
                ))),
            }
        }
    }
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
            l.b.begin_loop_body(header, body_b, exit);
            block(l, &w.body)?;
            l.b.end_while(header, exit);
            l.dead = false;
            Ok(())
        }
        Expr::Loop(lp) => {
            let (header, body_b, exit) = l.b.begin_while();
            l.b.jmp(body_b);
            l.b.begin_loop_body(header, body_b, exit);
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
                    let to = r
                        .to
                        .as_deref()
                        .ok_or_else(|| err(&fl.expr, "range needs an end"))?;
                    let inclusive = matches!(r.limits, syn::RangeLimits::Closed(_));
                    (from, to, inclusive)
                }
                _ => return Err(err(&fl.expr, "for loops need a range (a..b or a..=b)")),
            };
            let (from_v, from_ty) = expr(l, from)?.reg(l, "range start")?;
            let (to_v, to_ty) = expr(l, to)?.reg(l, "range end")?;
            let ty = match unify_int(from_ty.clone(), to_ty.clone()) {
                Some(t) => t,
                None => {
                    return Err(err(&fl.expr, format!(
                        "range type mismatch: {} vs {}",
                        from_ty.display(),
                        to_ty.display()
                    )))
                }
            };

            l.scopes.push(HashMap::new());
            let ivar = l.b.new_var();
            l.b.set(ivar, from_v);
            l.declare(
                ident,
                VarInfo {
                    kind: VarKind::Ssa { var: ivar },
                    ty: ty.clone(),
                    mutable: true, // incremented by the loop itself
                },
            );

            let (header, body_b, exit) = l.b.begin_while();
            {
                let i = l.b.get(ivar);
                l.b.br(
                    Cmp {
                        lhs: i,
                        rhs: CmpRhs::Reg(to_v),
                        cond: if inclusive { Cond::LessEqual } else { Cond::Less },
                        signed: ty == Ty::I16,
                    },
                    body_b,
                    exit,
                );
            }
            l.b.set_block_line(body_b, line_of(&fl.for_token));
            l.b.begin_loop_body(header, body_b, exit);
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
                        cond: Cond::Equal,
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
                    _ => return Err(err(&lit.lit, format!("unsupported literal suffix `{suffix}`"))),
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
            _ => Err(err(&lit.lit, "unsupported literal (only integers and bools)")),
        },
        Expr::Path(p) => {
            let name = path_ident(e)?;
            if let Some(info) = l.lookup(&name) {
                let (kind, ty) = (info.kind.clone(), info.ty.clone());
                if matches!(ty, Ty::Array(..)) {
                    return Err(err(&p, format!(
                        "array `{name}` used as a value; use {name}.as_ptr() or {name}.read(i)"
                    )));
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
            if l.sigs.contains_key(&name) {
                return Ok(Val::FnItem(intern(&name)));
            }
            Err(err(&p, format!("undefined name `{name}`")))
        }
        Expr::Unary(u) => match u.op {
            SUnOp::Neg(_) => {
                let (v, ty) = expr(l, &u.expr)?.reg(l, "negation")?;
                if ty == Ty::U16 {
                    return Err(err(&u.op, "unary `-` is only allowed on i16"));
                }
                Ok(Val::V(l.b.un(UnOp::Neg, v), Ty::I16))
            }
            SUnOp::Not(_) => {
                let val = expr(l, &u.expr)?;
                match val {
                    Val::V(v, ty) if ty.is_int() => Ok(Val::V(l.b.un(UnOp::Inv, v), ty)),
                    Val::V(_, ty) => Err(err(&u.op, format!("`!` does not apply to {}", ty.display()))),
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
                    let (lhs, lt) = expr(l, &b.left)?.reg(l, "binary operand")?;
                    let (rhs, rt) = expr(l, &b.right)?.reg(l, "binary operand")?;
                    let ty = unify_int(lt.clone(), rt.clone()).ok_or_else(|| {
                        err(e, format!(
                            "type mismatch: {} vs {} (cast with `as`)",
                            lt.display(),
                            rt.display()
                        ))
                    })?;
                    let op = match b.op {
                        Add(_) => BinOp::Add,
                        Sub(_) => BinOp::Sub,
                        BitAnd(_) => BinOp::And,
                        BitOr(_) => BinOp::Or,
                        BitXor(_) => BinOp::Xor,
                        _ => unreachable!(),
                    };
                    Ok(Val::V(l.b.bin(op, lhs, rhs), ty))
                }
                Shl(_) | Shr(_) => {
                    let (lhs, lt) = expr(l, &b.left)?.reg(l, "shift value")?;
                    if !lt.is_int() {
                        return Err(err(&b.left, "shifts only work on integers"));
                    }
                    let amount = shift_amount(l, &b.right)?;
                    Ok(Val::V(l.b.shift(shift_op(&b.op, &lt), lhs, amount), lt))
                }
                Mul(_) | Div(_) | Rem(_) => Err(err(
                    &b.op,
                    "`*`, `/`, `%` are not supported yet (hardware has no mul/div; mul will come via the library)",
                )),
                Lt(_) | Le(_) | Gt(_) | Ge(_) | Eq(_) | Ne(_) => {
                    let (lhs, lt) = expr(l, &b.left)?.reg(l, "comparison")?;
                    let (rhs, rt) = expr(l, &b.right)?.reg(l, "comparison")?;
                    compare(e, b.op, lhs, lt, rhs, rt).map(Val::Bool)
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
            let (v, from) = expr(l, &c.expr)?.reg(l, "cast")?;
            let to = ty_of(&c.ty)?;
            cast(e, v, from, to).map(|(v, t)| Val::V(v, t))
        }
        Expr::Call(call) => call_expr(l, call),
        Expr::MethodCall(m) => method_call(l, m),
        Expr::If(_) => {
            // if used as a value: `let x = if c { a } else { b };`
            control_flow_value(l, e)
        }
        Expr::Block(b) => Err(err(&b, "blocks as expressions are not supported")),
        Expr::Match(_) => Err(err(e, "match is not supported (use if/else)")),
        Expr::Closure(_) => Err(err(e, "closures are not supported")),
        Expr::Macro(_) => Err(err(e, "macros are not supported")),
        Expr::Reference(_) => Err(err(
            e,
            "references `&` are not supported (take addresses with addr_of(&x))",
        )),
        _ => Err(err(e, "expression not supported in this subset (see spec)")),
    }
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
    let r = l.b.new_var();
    l.b.set(r, tv);
    l.b.mid_if_else(else_b, join);

    // else branch value
    let else_blk = match else_e.as_ref() {
        Expr::Block(b) => &b.block,
        _ => return Err(err(else_e, "if-expression branches must be blocks")),
    };
    let (ev, et) = if_expr_branch(l, else_blk)?;
    let ty = unify_int(tt.clone(), et.clone()).ok_or_else(|| {
        err(e, format!(
            "if-expression branches have different types: {} vs {}",
            tt.display(),
            et.display()
        ))
    })?;
    l.b.set(r, ev);
    l.b.end_if_else(join);

    let v = l.b.get(r);
    Ok(Val::V(v, ty))
}

fn if_expr_branch(l: &mut FnLower, blk: &Block) -> Result<(VReg, Ty), syn::Error> {
    if blk.stmts.len() != 1 {
        return Err(err(blk, "if-expression branches must be single expressions"));
    }
    match &blk.stmts[0] {
        Stmt::Expr(e) | Stmt::Semi(e, _) => expr(l, e)?.reg(l, "if-expression"),
        s => Err(err(s, "if-expression branches must be expressions")),
    }
}

// ---------------------------------------------------------------------------
// conditions
// ---------------------------------------------------------------------------

fn cond(l: &mut FnLower, e: &Expr) -> Result<BoolExpr, syn::Error> {
    match expr(l, e)? {
        Val::Bool(c) => Ok(c),
        Val::V(_, ty) => Err(err(
            e,
            format!("condition must be a boolean expression, got {} (compare something)", ty.display()),
        )),
        Val::FnItem(_) => Err(err(e, "function used as a condition")),
        Val::Unit => Err(err(e, "unit used as a condition")),
        Val::Never => Err(err(e, "never used as a condition")),
    }
}

fn compare(
    e: &Expr,
    op: SBinOp,
    lhs: VReg,
    lt: Ty,
    rhs: VReg,
    rt: Ty,
) -> Result<BoolExpr, syn::Error> {
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
    let cond = match op {
        SBinOp::Lt(_) => Cond::Less,
        SBinOp::Le(_) => Cond::LessEqual,
        SBinOp::Gt(_) => Cond::Greater,
        SBinOp::Ge(_) => Cond::GreaterEqual,
        SBinOp::Eq(_) => Cond::Equal,
        SBinOp::Ne(_) => Cond::NotEqual,
        _ => return Err(err(e, "unsupported comparison operator")),
    };
    if matches!(op, SBinOp::Lt(_) | SBinOp::Le(_) | SBinOp::Gt(_) | SBinOp::Ge(_)) && (lt == Ty::Ptr || rt == Ty::Ptr) {
        return Err(err(e, "ordered comparisons on pointers are not supported"));
    }
    Ok(BoolExpr::Cmp(Cmp {
        lhs,
        rhs: CmpRhs::Reg(rhs),
        cond,
        signed,
    }))
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
            _ => Err(err(e, "condition must be a boolean expression (compare something)")),
        },
        _ => {
            // delegate to the eager checker for a precise error message
            match expr(l, e)? {
                Val::Bool(_) => unreachable!(),
                Val::V(_, ty) => Err(err(
                    e,
                    format!("condition must be a boolean expression, got {} (compare something)", ty.display()),
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
        cond: Cond::Equal,
        signed: false,
    })
}
fn false_cond(l: &mut FnLower) -> BoolExpr {
    let zero = l.b.load_imm(0);
    BoolExpr::Cmp(Cmp {
        lhs: zero,
        rhs: CmpRhs::Imm(0),
        cond: Cond::NotEqual,
        signed: false,
    })
}

// ---------------------------------------------------------------------------
// calls and intrinsics
// ---------------------------------------------------------------------------

fn call_expr(l: &mut FnLower, call: &syn::ExprCall) -> Result<Val, syn::Error> {
    let Expr::Path(p) = call.func.as_ref() else {
        return Err(err(&call.func, "unsupported callee"));
    };
    // intrinsic or fn-item path (possibly qualified like Ptr::from_addr)
    let segs: Vec<String> = p.path.segments.iter().map(|s| s.ident.to_string()).collect();
    if segs == ["Ptr", "from_addr"] {
        let (v, _) = exactly_args(l, &call.args, 1, "Ptr::from_addr")?[0].clone().reg(l, "Ptr::from_addr")?;
        let (v, _) = coerce(l, v, &Ty::U16, &Ty::U16, &call.args[0])?;
        return Ok(Val::V(v, Ty::Ptr));
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
                return Err(err(&call.args[0], "addr_of expects a reference: addr_of(&x)"));
            };
            let target = path_ident(&r.expr)?;
            let v = l.addr_of_var(&target, &call.args[0])?;
            return Ok(Val::V(v, Ty::Ptr));
        }
        "assert" => {
            if call.args.len() != 2 {
                return Err(err(call, "assert(cond, sig) takes 2 arguments"));
            }
            let (sig_v, _) = expr(l, &call.args[1])?.reg(l, "assert signal")?;
            let fail = l.b.raw_block(&[]);
            let join = l.b.raw_block(&[]);
            cond_lazy(l, &call.args[0], join, fail)?;
            l.b.enter_block(fail);
            l.b.halt(sig_v);
            l.b.end_if(join);
            return Ok(Val::Unit);
        }
        "dev_recv" | "dev_send" => {
            return Err(err(call, "dev_* intrinsics are not supported yet"));
        }
        _ => {}
    }

    // remaining calls take plain value arguments; function items passed as fn
    // pointer arguments are materialized to their address inline
    let args: Vec<(VReg, Ty)> = call
        .args
        .iter()
        .map(|a| {
            match expr(l, a)? {
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
                val => val.reg(l, "argument"),
            }
        })
        .collect::<Result<_, _>>()?;
    if name.as_str() == "halt" {
        if args.len() != 1 {
            return Err(err(call, "halt(x) takes 1 argument"));
        }
        let v = args[0].0;
        l.b.halt(v);
        l.dead = true;
        return Ok(Val::Never);
    }

    // direct or indirect call
    let arg_vregs: Vec<VReg> = args.iter().map(|(v, _)| *v).collect();
    let arg_tys: Vec<Ty> = args.iter().map(|(_, t)| t.clone()).collect();
    if let Some(sig) = l.sigs.get(name).cloned() {
        check_call_args(call, &sig.params, &arg_tys, name)?;
        let n_rets = if sig.ret == Ty::Unit { 0 } else { 1 };
        let rets = l.b.call(intern(name), &arg_vregs, n_rets);
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
        return Ok(match n_rets {
            0 => Val::Unit,
            _ => Val::V(rets[0], *ret),
        });
    }
    Err(err(&p, format!("undefined function `{name}`")))
}

/// array methods (Slice2 intrinsics): read/write/as_ptr/len
fn array_method(
    l: &mut FnLower,
    base: VReg,
    n: usize,
    mutable: bool,
    name: &str,
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
        "read" => {
            if m.args.len() != 1 {
                return Err(err(&m.method, "read(off) takes 1 argument"));
            }
            let (base2, off) = ptr_with_offset(l, base, &m.args[0])?;
            Ok(Val::V(l.b.load_mem(base2, off), Ty::U16))
        }
        "write" => {
            if !mutable {
                return Err(err(&m.method, format!(
                    "array `{name}` is not mutable (declare with `let mut`)"
                )));
            }
            if m.args.len() != 2 {
                return Err(err(&m.method, "write(off, v) takes 2 arguments"));
            }
            let (base2, off) = ptr_with_offset(l, base, &m.args[0])?;
            let (v, _) = expr(l, &m.args[1])?.reg(l, "write value")?;
            let (v, _) = coerce(l, v, &Ty::U16, &Ty::U16, &m.args[1])?;
            l.b.store_mem(base2, off, v);
            Ok(Val::V(v, Ty::Unit))
        }
        _ => Err(err(&m.method, format!("unknown array method `{method}`"))),
    }
}

fn method_call(l: &mut FnLower, m: &syn::ExprMethodCall) -> Result<Val, syn::Error> {
    // array methods on local arrays / global arrays
    if let Ok(name) = path_ident(&m.receiver) {
        if let Some(info) = l.lookup(&name) {
            if let Ty::Array(_, n) = &info.ty {
                let (kind, n, mutable) = (info.kind.clone(), *n, info.mutable);
                let VarKind::Local { slot } = kind else {
                    unreachable!("arrays are always memory-resident")
                };
                let base = l.b.addr_of_local(slot);
                return array_method(l, base, n, mutable, &name, m);
            }
        }
        if let Some(&(addr, _, n)) = l.globals.arrays.get(&name) {
            let base = l.b.load_imm(addr);
            return array_method(l, base, n, true, &name, m);
        }
    }

    let (base, base_ty) = expr(l, &m.receiver)?.reg(l, "method receiver")?;
    if base_ty != Ty::Ptr {
        return Err(err(&m.receiver, format!(
            "methods only exist on Ptr and arrays (got {})",
            base_ty.display()
        )));
    }
    let method = m.method.to_string();
    match method.as_str() {
        "addr" => {
            if !m.args.is_empty() {
                return Err(err(&m.method, "addr() takes no arguments"));
            }
            Ok(Val::V(base, Ty::U16))
        }
        "add" => {
            let (off, off_ty) = exactly_args(l, &m.args, 1, "add")?[0].clone().reg(l, "add offset")?;
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
            let (v, _) = expr(l, &m.args[1])?.reg(l, "write value")?;
            let (v, _) = coerce(l, v, &Ty::U16, &Ty::U16, &m.args[1])?;
            l.b.store_mem(base2, off, v);
            Ok(Val::V(v, Ty::Unit))
        }
        _ => Err(err(&m.method, format!("unknown Ptr method `{method}`"))),
    }
}

/// compute the effective address for a memory access with offset `off`:
/// a literal becomes the ISA addressing offset; an expression is added to the
/// base (offset 0 remains).
fn ptr_with_offset(l: &mut FnLower, base: VReg, off: &Expr) -> Result<(VReg, i16), syn::Error> {
    // literal offset: use the ISA's addressing offset directly
    if let Expr::Lit(lit) = off {
        if let Lit::Int(i) = &lit.lit {
            if i.suffix().is_empty() || i.suffix() == "i16" {
                let v = lit_int_value(i)? as i64;
                if (-32768..=32767).contains(&v) {
                    return Ok((base, v as i16));
                }
            }
        }
    }
    let (off_v, off_ty) = expr(l, off)?.reg(l, "pointer offset")?;
    if !off_ty.is_int() {
        return Err(err(off, "pointer offset must be an integer"));
    }
    Ok((l.b.bin(BinOp::Add, base, off_v), 0))
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
        _ => None,
    }
}

/// implicit conversion at assignment/argument/return positions: only the
/// same type, or an untyped literal adopting the target type. anything else
/// needs an explicit `as` cast (see cast()).
fn coerce(_l: &mut FnLower, v: VReg, from: &Ty, to: &Ty, at: &Expr) -> Result<(VReg, Ty), syn::Error> {
    if from == to || *from == Ty::Never || (*from == Ty::UntypedInt && to.is_int()) {
        Ok((v, to.clone()))
    } else {
        Err(err(at, format!(
            "type mismatch: expected {}, got {} (cast with `as`)",
            to.display(),
            from.display()
        )))
    }
}

fn cast(e: &Expr, v: VReg, from: Ty, to: Ty) -> Result<(VReg, Ty), syn::Error> {
    let ok = matches!(
        (&from, &to),
        (Ty::U16, Ty::I16)
            | (Ty::I16, Ty::U16)
            | (Ty::U16, Ty::Ptr)
            | (Ty::Ptr, Ty::U16)
    ) || from == to
        || (from == Ty::UntypedInt && to.is_int());
    if ok {
        Ok((v, to))
    } else {
        Err(err(e, format!(
            "cannot cast {} to {}",
            from.display(),
            to.display()
        )))
    }
}

fn shift_amount(_l: &mut FnLower, e: &Expr) -> Result<u8, syn::Error> {
    if let Expr::Lit(lit) = e {
        if let Lit::Int(i) = &lit.lit {
            let v = lit_int_value(i)?;
            if v <= 15 {
                return Ok(v as u8);
            }
        }
    }
    Err(err(e, "shift amount must be a literal constant in 0..=15"))
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
    n: usize,
    what: &str,
) -> Result<Vec<Val>, syn::Error> {
    if args.len() != n {
        return Err(l.err_here(format!("{what} takes {n} arguments, got {}", args.len())));
    }
    args.iter().map(|a| expr(l, a)).collect()
}

fn check_call_args(call: &syn::ExprCall, params: &[Ty], args: &[Ty], name: &str) -> Result<(), syn::Error> {
    if params.len() != args.len() {
        return Err(err(call, format!(
            "`{name}` takes {} arguments, got {}",
            params.len(),
            args.len()
        )));
    }
    for (i, (p, a)) in params.iter().zip(args).enumerate() {
        let ok = p == a || (*a == Ty::UntypedInt && p.is_int());
        if !ok {
            return Err(err(call, format!(
                "argument {} of `{name}`: expected {}, got {}",
                i + 1,
                p.display(),
                a.display()
            )));
        }
    }
    Ok(())
}

fn check_fn_sig(l: &mut FnLower, name: &'static str, expected: &Ty) -> Result<(), syn::Error> {
    let sig = l.sigs.get(name).ok_or_else(|| l.err_here(format!("undefined function `{name}`")))?;
    let Ty::FnPtr { params, ret } = expected else { unreachable!() };
    if *params != sig.params || **ret != sig.ret {
        return Err(l.err_here(format!(
            "fn pointer type mismatch for `{name}`: expected fn({}) -> {}",
            params.iter().map(|t| t.display()).collect::<Vec<_>>().join(", "),
            ret.display()
        )));
    }
    Ok(())
}
