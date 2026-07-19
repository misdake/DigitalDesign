//! rcc frontend: parse the Rust subset (see spec.md) with `syn`, validate it,
//! and lower it onto `FuncBuilder` to produce `IrFunc`s.
//!
//! anything outside the subset is a hard error with a source span.

use crate::compiler::builder::{BoolExpr, FuncBuilder, VarId};
use crate::compiler::ir::*;
use crate::isa::Cond;
use std::collections::HashMap;
use syn::{BinOp as SBinOp, Block, Expr, Item, ItemFn, Lit, Pat, Stmt, Type, UnOp as SUnOp};

#[cfg(doc)]
pub mod spec {}

/// parse rcc source text into a list of functions (IR), or report the first
/// subset violation with a span
pub fn parse_source(src: &str) -> Result<Vec<IrFunc>, syn::Error> {
    let file = syn::parse_file(src)?;
    let mut fns = vec![];
    for item in &file.items {
        match item {
            Item::Fn(f) => fns.push(f),
            Item::Use(_) => { /* ignored: for the IDE only */ }
            Item::Verbatim(_) => { /* attributes on use items land here */ }
            _ => return Err(err(item, "item not supported (only fn and use are allowed)")),
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
    names
        .into_iter()
        .zip(fns)
        .map(|(name, f)| lower_fn(name, f, &sigs))
        .collect()
}

fn intern(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
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
    Unit,
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
            Ty::FnPtr { .. } => "fn pointer".into(),
            Ty::Unit => "()".into(),
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
        _ => Err(err(ty, "unsupported type")),
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
    let ret = match &f.sig.output {
        syn::ReturnType::Default => Ty::Unit,
        syn::ReturnType::Type(_, t) => ty_of(t)?,
    };
    Ok(Sig { params, ret })
}

// ---------------------------------------------------------------------------
// per-function lowering
// ---------------------------------------------------------------------------

struct VarInfo {
    var: VarId,
    ty: Ty,
    mutable: bool,
}

struct FnLower<'a> {
    b: FuncBuilder,
    sigs: &'a HashMap<String, Sig>,
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
}

fn lower_fn(name: &'static str, f: &ItemFn, sigs: &HashMap<String, Sig>) -> Result<IrFunc, syn::Error> {
    let sig = sigs.get(&f.sig.ident.to_string()).unwrap().clone();
    let n_rets = if sig.ret == Ty::Unit { 0 } else { 1 };
    let (b, param_vars) = FuncBuilder::new(name, sig.params.len(), n_rets);

    let mut param_names = vec![];
    let mut l = FnLower {
        b,
        sigs,
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
        l.declare(
            ident,
            VarInfo {
                var: *var,
                ty: ty.clone(),
                mutable: false,
            },
        );
    }
    let ret_names: Vec<&'static str> = if sig.ret == Ty::Unit { vec![] } else { vec!["r"] };
    l.b.set_names(&param_names, &ret_names);

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
    if let Some(t) = tail {
        if l.dead {
            return Err(err(t, "unreachable code (after return/halt)"));
        }
        let (v, _) = expr(&mut l, t)?.reg(&mut l, "tail expression")?;
        let expected = sig.ret.clone();
        let (v, _) = coerce(&mut l, v, &expected, &expected, t)?;
        l.b.ret(&[v]);
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
                    let ty = ty_of(&pt.ty)?;
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
            let var = l.b.new_var();
            l.b.set(var, v);
            l.declare(ident, VarInfo { var, ty, mutable });
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
            let (var, ty) = (info.var, info.ty.clone());
            let val = expr(l, &a.right)?;
            let (v, _) = val.reg(l, "assignment")?;
            let (v, _) = coerce(l, v, &ty, &ty, &a.right)?;
            l.b.set(var, v);
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
            let (var, ty) = (info.var, info.ty.clone());
            if !ty.is_int() {
                return Err(err(&a.left, "compound assignment only works on integers"));
            }
            let cur = l.b.get(var);
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
                    l.b.set(var, v);
                    return Ok(());
                }
                _ => return Err(err(&a.op, "unsupported compound assignment operator")),
            };
            let v = l.b.bin(op, cur, rhs);
            l.b.set(var, v);
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
                    let (v, _) = expr(l, e)?.reg(l, "return value")?;
                    let (v, _) = coerce(l, v, expected, expected, e)?;
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
                Val::Unit | Val::V(_, _) | Val::Bool(_) => Ok(()),
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
            let cond = cond(l, &i.cond)?;
            match &i.else_branch {
                None => {
                    let join = l.b.begin_if(cond);
                    block(l, &i.then_branch)?;
                    l.b.end_if(join);
                    l.dead = false;
                }
                Some((_, else_e)) => {
                    let (else_b, join) = l.b.begin_if_else(cond);
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
            let cond = cond(l, &w.cond)?;
            l.b.while_cond(cond, header, body_b, exit);
            block(l, &w.body)?;
            l.b.end_while(header, exit);
            l.dead = false;
            Ok(())
        }
        Expr::Loop(lp) => {
            let (header, body_b, exit) = l.b.begin_while();
            let true_c = true_cond(l);
            l.b.while_cond(true_c, header, body_b, exit);
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
                    var: ivar,
                    ty: ty.clone(),
                    mutable: true, // incremented by the loop itself
                },
            );

            let (header, body_b, exit) = l.b.begin_while();
            let cond = {
                let i = l.b.get(ivar);
                BoolExpr::Cmp(Cmp {
                    lhs: i,
                    rhs: CmpRhs::Reg(to_v),
                    cond: if inclusive { Cond::LessEqual } else { Cond::Less },
                    signed: ty == Ty::I16,
                })
            };
            l.b.while_cond(cond, header, body_b, exit);
            block(l, &fl.body)?;
            if l.dead {
                // body always terminates (e.g. only break): skip the increment
                l.dead = false;
            } else {
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
            _ => Err(err(&lit.lit, "unsupported literal (only integers and bools)")),
        },
        Expr::Path(p) => {
            let name = path_ident(e)?;
            if let Some(info) = l.lookup(&name) {
                let (var, ty) = (info.var, info.ty.clone());
                let v = l.b.get(var);
                return Ok(Val::V(v, ty));
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
        _ => Err(err(e, "expression not supported in this subset (see spec)")),
    }
}

/// if as an expression: `if c { a } else { b }`
fn control_flow_value(l: &mut FnLower, e: &Expr) -> Result<Val, syn::Error> {
    let Expr::If(i) = e else { unreachable!() };
    let Some((_, else_e)) = &i.else_branch else {
        return Err(err(e, "if-expression needs an else branch"));
    };
    let cond = cond(l, &i.cond)?;
    let (else_b, join) = l.b.begin_if_else(cond);

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
    // evaluation (assert takes a condition, dev_* take literals)
    match name.as_str() {
        "assert" => {
            if call.args.len() != 2 {
                return Err(err(call, "assert(cond, sig) takes 2 arguments"));
            }
            let c = cond(l, &call.args[0])?;
            let (sig_v, _) = expr(l, &call.args[1])?.reg(l, "assert signal")?;
            let join = l.b.begin_if(BoolExpr::Not(Box::new(c)));
            l.b.halt(sig_v);
            l.b.end_if(join);
            return Ok(Val::Unit);
        }
        "dev_recv" | "dev_send" => {
            return Err(err(call, "dev_* intrinsics are not supported yet"));
        }
        _ => {}
    }

    // remaining calls take plain value arguments
    let args: Vec<(VReg, Ty)> = call
        .args
        .iter()
        .map(|a| expr(l, a).and_then(|v| v.reg(l, "argument")))
        .collect::<Result<_, _>>()?;
    if name.as_str() == "halt" {
        if args.len() != 1 {
            return Err(err(call, "halt(x) takes 1 argument"));
        }
        let v = args[0].0;
        l.b.halt(v);
        l.dead = true;
        return Ok(Val::Unit);
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
        let addr = l.b.get(info.var);
        let n_rets = if *ret == Ty::Unit { 0 } else { 1 };
        let rets = l.b.call_ptr(addr, &arg_vregs, n_rets);
        return Ok(match n_rets {
            0 => Val::Unit,
            _ => Val::V(rets[0], *ret),
        });
    }
    Err(err(&p, format!("undefined function `{name}`")))
}

fn method_call(l: &mut FnLower, m: &syn::ExprMethodCall) -> Result<Val, syn::Error> {
    let (base, base_ty) = expr(l, &m.receiver)?.reg(l, "method receiver")?;
    if base_ty != Ty::Ptr {
        return Err(err(&m.receiver, format!(
            "methods only exist on Ptr (got {})",
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

/// convert a value to type `to` (only trivial int/ptr reinterpretation)
fn coerce(_l: &mut FnLower, v: VReg, from: &Ty, to: &Ty, at: &Expr) -> Result<(VReg, Ty), syn::Error> {
    if from == to {
        return Ok((v, to.clone()));
    }
    cast(at, v, from.clone(), to.clone()).map_err(|_| {
        err(at, format!(
            "type mismatch: expected {}, got {} (cast with `as`)",
            to.display(),
            from.display()
        ))
    })
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
