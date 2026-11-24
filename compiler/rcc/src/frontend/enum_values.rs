//! Shared fieldless discriminants and checked integer-to-enum intrinsics.
use super::{err, expr, EnumTable, FnLower, Ty, Val};
use crate::{BoolExpr, CmpRhs, CompareOp};
use syn::{Expr, GenericArgument, Lit, PathArguments, Type};

pub(super) fn discriminants(item: &syn::ItemEnum) -> Result<Vec<(String, u16)>, syn::Error> {
    let mut next = 0u32;
    let mut result = Vec::new();
    for variant in &item.variants {
        if !matches!(variant.fields, syn::Fields::Unit) {
            return Err(err(variant, "only fieldless variants are supported"));
        }
        let value = match &variant.discriminant {
            Some((_, Expr::Lit(value))) => match &value.lit {
                Lit::Int(integer) if matches!(integer.suffix(), "" | "u16") => {
                    integer.base10_parse::<u32>()?
                }
                _ => {
                    return Err(err(
                        value,
                        "enum discriminants require a u16 integer literal",
                    ))
                }
            },
            Some((_, value)) => {
                return Err(err(
                    value,
                    "enum discriminants require a u16 integer literal",
                ))
            }
            None => next,
        };
        let value =
            u16::try_from(value).map_err(|_| err(variant, "enum discriminant exceeds u16"))?;
        let name = variant.ident.to_string();
        if result.iter().any(|(n, _)| n == &name) {
            return Err(err(variant, "duplicate enum variant"));
        }
        if result.iter().any(|(_, v)| *v == value) {
            return Err(err(variant, "duplicate enum discriminant"));
        }
        result.push((name, value));
        next = u32::from(value) + 1;
    }
    Ok(result)
}

pub(super) fn is_conversion(call: &syn::ExprCall) -> bool {
    matches!(call.func.as_ref(), Expr::Path(p) if p.path.segments.len()==1 &&
        matches!(p.path.segments[0].ident.to_string().as_str(), "enum_from_u16" | "enum_from_u16_unchecked"))
}

pub(super) fn target(call: &syn::ExprCall, enums: &EnumTable) -> Result<String, syn::Error> {
    let Expr::Path(path) = call.func.as_ref() else {
        unreachable!()
    };
    let PathArguments::AngleBracketed(args) = &path.path.segments[0].arguments else {
        return Err(err(
            call,
            "enum conversion needs an explicit enum type: enum_from_u16::<E>(value)",
        ));
    };
    if args.args.len() != 1 {
        return Err(err(args, "enum conversion takes one type argument"));
    }
    let Some(GenericArgument::Type(Type::Path(ty))) = args.args.first() else {
        return Err(err(args, "enum conversion requires a repr(u16) enum"));
    };
    let name = ty
        .path
        .get_ident()
        .map(ToString::to_string)
        .ok_or_else(|| err(ty, "enum conversion requires a named repr(u16) enum"))?;
    if !enums.get(&name).is_some_and(|def| def.repr_u16) {
        return Err(err(ty, "enum conversion requires a repr(u16) enum"));
    }
    Ok(name)
}

pub(super) fn lower(l: &mut FnLower, call: &syn::ExprCall) -> Result<Val, syn::Error> {
    let name = target(call, &l.globals.enums)?;
    if call.args.len() != 1 {
        return Err(err(call, "enum conversion takes one u16 argument"));
    }
    let Expr::Path(path) = call.func.as_ref() else {
        unreachable!()
    };
    let unchecked = path.path.segments[0].ident == "enum_from_u16_unchecked";
    if unchecked {
        if l.unsafe_depth == 0 {
            return Err(err(
                call,
                "enum_from_u16_unchecked requires an unsafe block",
            ));
        }
        l.unsafe_ops += 1;
    }
    let (value, ty) = expr(l, &call.args[0])?.reg(l, &call.args[0], "enum value")?;
    let (value, _) = super::coerce(l, value, &ty, &Ty::U16, &call.args[0])?;
    if !unchecked {
        let mut values: Vec<_> = l.globals.enums[&name]
            .variants
            .iter()
            .map(|(_, value)| *value)
            .collect();
        values.sort_unstable();
        let mut ranges: Vec<(u16, u16)> = Vec::new();
        for value in values {
            if let Some((_, end)) = ranges.last_mut() {
                if end.checked_add(1) == Some(value) {
                    *end = value;
                    continue;
                }
            }
            ranges.push((value, value));
        }
        let valid = ranges
            .into_iter()
            .map(|(first, last)| {
                if first == last {
                    BoolExpr::Cmp(l.b.cmp(value, CmpRhs::Imm(first), CompareOp::Equal))
                } else {
                    BoolExpr::Cmp(l.b.cmp(value, CmpRhs::Imm(first), CompareOp::GreaterEqual)).and(
                        BoolExpr::Cmp(l.b.cmp(value, CmpRhs::Imm(last), CompareOp::LessEqual)),
                    )
                }
            })
            .reduce(BoolExpr::or)
            .expect("nonempty enum");
        l.b.if_bool(BoolExpr::Not(Box::new(valid)), |b| {
            let signal = b.load_imm(0xffe5);
            b.halt(signal);
        });
    }
    Ok(Val::V(value, Ty::Enum(name)))
}
