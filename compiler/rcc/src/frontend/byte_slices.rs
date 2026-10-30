//! Immutable packed byte constants and the restricted two-word slice ABI.
use super::{
    const_eval, const_eval_byte, err, expr, path_ident, peek_type, reserve_static, FnLower,
    Globals, Ty, Val,
};
use crate::{BinOp, IntOperand, ShiftOp, VReg};
use std::collections::HashMap;
use syn::{visit::Visit, Expr, Type};

fn byte(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.is_ident("u8"))
}
pub(super) fn slice(ty: &Type) -> bool {
    matches!(ty, Type::Reference(reference) if reference.mutability.is_none()
        && matches!(reference.elem.as_ref(), Type::Slice(slice) if byte(&slice.elem)))
}
fn constant_type(ty: &Type) -> bool {
    slice(ty) || matches!(ty, Type::Array(array) if byte(&array.elem))
}

fn intern(
    g: &mut Globals,
    bytes: Vec<u8>,
    at: &impl syn::spanned::Spanned,
) -> Result<u16, syn::Error> {
    if let Some(address) = g.byte_literals.get(&bytes) {
        return Ok(*address);
    }
    let length =
        u16::try_from(bytes.len()).map_err(|_| err(at, "byte slice exceeds 65535 bytes"))?;
    let words = bytes.len().div_ceil(2);
    let payload = reserve_static(g, words + 2, at)?;
    for (index, chunk) in bytes.chunks(2).enumerate() {
        let word = u16::from(chunk[0]) | (u16::from(*chunk.get(1).unwrap_or(&0)) << 8);
        g.data_words.push((payload + index as u16, word));
    }
    let descriptor = payload + words as u16;
    g.data_words.push((descriptor, payload));
    g.data_words.push((descriptor + 1, length));
    g.byte_literals.insert(bytes, descriptor);
    Ok(descriptor)
}

pub(super) fn literals(files: &[syn::File], g: &mut Globals) -> Result<(), (usize, syn::Error)> {
    struct Collect(Vec<syn::LitByteStr>);
    impl<'ast> Visit<'ast> for Collect {
        fn visit_lit_byte_str(&mut self, literal: &'ast syn::LitByteStr) {
            self.0.push(literal.clone());
        }
    }
    for (fi, file) in files.iter().enumerate() {
        let mut collect = Collect(Vec::new());
        collect.visit_file(file);
        for literal in collect.0 {
            intern(g, literal.value(), &literal).map_err(|error| (fi, error))?;
        }
    }
    Ok(())
}

fn bytes(
    e: &Expr,
    consts: &HashMap<String, (u16, Ty)>,
    g: &Globals,
) -> Result<Vec<u8>, syn::Error> {
    match e {
        Expr::Paren(p) => bytes(&p.expr, consts, g),
        Expr::Reference(r) if r.mutability.is_none() => bytes(&r.expr, consts, g),
        Expr::Unary(u) if matches!(u.op, syn::UnOp::Deref(_)) => bytes(&u.expr, consts, g),
        Expr::Lit(lit) => match &lit.lit {
            syn::Lit::ByteStr(literal) => Ok(literal.value()),
            _ => Err(err(
                e,
                "byte constants need a byte literal or whole const buffer",
            )),
        },
        Expr::Array(array) => array
            .elems
            .iter()
            .map(|e| {
                let value = const_eval_byte(e, consts)?;
                u8::try_from(value).map_err(|_| err(e, "byte constant element exceeds 255"))
            })
            .collect(),
        Expr::Repeat(repeat) => {
            let value = u8::try_from(const_eval_byte(&repeat.expr, consts)?)
                .map_err(|_| err(e, "byte constant element exceeds 255"))?;
            Ok(vec![value; usize::from(const_eval(&repeat.len, consts)?)])
        }
        Expr::Path(_) => {
            let name = path_ident(e)?;
            g.byte_constants
                .get(&name)
                .cloned()
                .ok_or_else(|| err(e, "expected an earlier whole byte constant"))
        }
        _ => Err(err(e, "only immutable whole byte constants are supported")),
    }
}

pub(super) fn constant(
    item: &syn::ItemConst,
    consts: &HashMap<String, (u16, Ty)>,
    g: &mut Globals,
) -> Result<bool, syn::Error> {
    if !constant_type(&item.ty) {
        return Ok(false);
    }
    let value = bytes(&item.expr, consts, g)?;
    if let Type::Array(array) = item.ty.as_ref() {
        if usize::from(const_eval(&array.len, consts)?) != value.len() {
            return Err(err(item, "byte constant length does not match its type"));
        }
    }
    let name = item.ident.to_string();
    if consts.contains_key(&name)
        || g.aggregates.contains_key(&name)
        || g.scalars.contains_key(&name)
        || g.arrays.contains_key(&name)
    {
        return Err(err(item, "byte constant name is already defined"));
    }
    let descriptor = intern(g, value.clone(), item)?;
    g.byte_constants.insert(name.clone(), value);
    g.aggregates.insert(name, (descriptor, Ty::ByteSlice));
    Ok(true)
}

pub(super) fn literal(l: &mut FnLower, literal: &syn::LitByteStr) -> Result<Val, syn::Error> {
    let address = l
        .globals
        .byte_literals
        .get(&literal.value())
        .ok_or_else(|| err(literal, "byte slices require checked compilation"))?;
    Ok(Val::V(l.b.load_imm(*address), Ty::ByteSlice))
}

pub(super) fn initialize(l: &mut FnLower, destination: VReg, e: &Expr) -> Result<(), syn::Error> {
    let (source, ty) = expr(l, e)?.reg(l, e, "byte slice")?;
    if ty != Ty::ByteSlice {
        return Err(err(e, "expected an immutable whole byte slice"));
    }
    let length = l.b.load_imm(2);
    l.b.call("mem_copy", &[destination, source, length], 0);
    Ok(())
}

pub(super) fn index(l: &mut FnLower, index: &syn::ExprIndex) -> Result<Val, syn::Error> {
    let (descriptor, _) = expr(l, &index.expr)?.reg(l, &index.expr, "byte slice")?;
    // `usize` is accepted only as the host-compatible cast on a slice index.
    let offset = match index.index.as_ref() {
        Expr::Cast(cast) if matches!(cast.ty.as_ref(), Type::Path(p) if p.path.is_ident("usize")) => {
            cast.expr.as_ref()
        }
        offset => offset,
    };
    let (offset, ty) = expr(l, offset)?.reg(l, offset, "byte index")?;
    if !matches!(ty, Ty::U8 | Ty::U16 | Ty::UntypedInt) {
        return Err(err(index, "byte indices require an unsigned word"));
    }
    let base = l.b.load_mem(descriptor, 0);
    let word = l.b.shift_op(ShiftOp::Lsr, offset, IntOperand::Imm(1));
    let address = l.b.bin_op(BinOp::Add, base, IntOperand::Reg(word));
    let data = l.b.load_mem(address, 0);
    let parity = l.b.bin_op(BinOp::And, offset, IntOperand::Imm(1));
    let shift = l.b.shift_op(ShiftOp::Lsl, parity, IntOperand::Imm(3));
    Ok(Val::V(
        l.b.shift_op(ShiftOp::Lsr, data, IntOperand::Reg(shift)),
        Ty::U8,
    ))
}

pub(super) fn reference(l: &mut FnLower, r: &syn::ExprReference) -> Result<Val, syn::Error> {
    let name = path_ident(&r.expr)?;
    if r.mutability.is_some() || !l.globals.byte_constants.contains_key(&name) {
        return Err(err(
            r,
            "only immutable whole const byte buffers can be borrowed",
        ));
    }
    expr(l, &r.expr)
}

pub(super) fn length(l: &mut FnLower, method: &syn::ExprMethodCall) -> Result<Val, syn::Error> {
    if method.method != "len" || !method.args.is_empty() || method.turbofish.is_some() {
        return Err(err(
            method,
            "byte slices support len() and read indexing only",
        ));
    }
    let (descriptor, _) = expr(l, &method.receiver)?.reg(l, &method.receiver, "byte slice")?;
    Ok(Val::V(l.b.load_mem(descriptor, 1), Ty::U16))
}

pub(super) fn is_value(l: &FnLower, e: &Expr) -> bool {
    peek_type(l, e) == Some(Ty::ByteSlice)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn odd_literal_zero_pads_final_word_and_interns_exact_byte_length() {
        let mut globals = Globals {
            next_addr: 0x200,
            limit: 0x300,
            ..Globals::default()
        };
        let literal: syn::LitByteStr = syn::parse_quote!(b"\xff\x80\x03");
        let descriptor = intern(&mut globals, literal.value(), &literal).unwrap();
        assert_eq!(descriptor, 0x202);
        assert_eq!(
            globals.data_words,
            [(0x200, 0x80ff), (0x201, 3), (0x202, 0x200), (0x203, 3)]
        );
        assert_eq!(
            intern(&mut globals, literal.value(), &literal).unwrap(),
            descriptor
        );
        assert_eq!(globals.next_addr, 0x204);
    }
}
