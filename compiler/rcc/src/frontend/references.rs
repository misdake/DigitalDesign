//! Ordinary checked Rust references use the existing one-word address ABI.
use super::{
    err, expr, is_aggregate, load_field, peek_type, place_addr, place_addr_of, FnLower, Ty, Val,
};
use crate::VReg;
use syn::{visit::Visit, Expr, Type};

pub(super) fn validate(files: &[syn::File], checked: bool) -> Result<(), (usize, syn::Error)> {
    if checked {
        return Ok(());
    }
    struct Guard(Option<syn::Error>);
    impl<'ast> Visit<'ast> for Guard {
        fn visit_type_reference(&mut self, reference: &'ast syn::TypeReference) {
            self.0.get_or_insert_with(|| {
                err(reference, "ordinary references require checked compilation")
            });
        }
    }
    for (file, source) in files.iter().enumerate() {
        let mut guard = Guard(None);
        guard.visit_file(source);
        if let Some(error) = guard.0 {
            return Err((file, error));
        }
    }
    Ok(())
}

pub(super) fn ty(
    reference: &syn::TypeReference,
    names: &super::TypeNames,
) -> Result<Ty, syn::Error> {
    if reference.lifetime.is_some() || matches!(reference.elem.as_ref(), Type::Slice(_)) {
        return Err(err(
            reference,
            "ordinary references use elided lifetimes and sized target types",
        ));
    }
    let element = super::ty_of(&reference.elem, names)?;
    if matches!(
        element,
        Ty::Unit | Ty::Never | Ty::Vec2 | Ty::Vec3 | Ty::Vec4
    ) {
        return Err(err(reference, "this reference target is not addressable"));
    }
    Ok(Ty::Reference(
        Box::new(element),
        reference.mutability.is_some(),
    ))
}

pub(super) fn borrow(l: &mut FnLower, reference: &syn::ExprReference) -> Result<Val, syn::Error> {
    if !l.globals.checked {
        return Err(err(
            reference,
            "ordinary references require checked compilation",
        ));
    }
    let (base, offset, element, mutable) =
        if super::value_methods::is_value_temporary(l, &reference.expr) {
            let ty = peek_type(l, &reference.expr).expect("recognized value temporary");
            let words = super::word_size(&ty, &l.globals.structs, reference)?;
            let slot = super::temporaries::allocate(
                l,
                words,
                reference,
                "borrowed temporary exceeds the 255-word frame limit",
            )?;
            let address = l.b.addr_of_local(slot);
            if is_aggregate(&ty) {
                super::init_aggregate_at(l, address, &ty, &reference.expr)?;
            } else {
                let (value, _) =
                    expr(l, &reference.expr)?.reg(l, reference, "borrowed temporary")?;
                super::store_field(l, address, 0, value, &ty);
            }
            (address, 0, ty, true)
        } else {
            place_addr_of(l, &reference.expr)?
        };
    if reference.mutability.is_some() && !mutable {
        return Err(err(reference, "mutable reference needs a mutable place"));
    }
    Ok(Val::V(
        place_addr(l, base, offset),
        Ty::Reference(Box::new(element), reference.mutability.is_some()),
    ))
}

pub(super) fn dereference(
    l: &mut FnLower,
    value: &Expr,
) -> Result<(VReg, i16, Ty, bool), syn::Error> {
    let (address, ty) = expr(l, value)?.reg(l, value, "reference")?;
    let Ty::Reference(element, mutable) = ty else {
        return Err(err(
            value,
            "dereferencing requires a checked Rust reference",
        ));
    };
    Ok((address, 0, *element, mutable))
}

pub(super) fn read(l: &mut FnLower, value: &Expr) -> Result<Val, syn::Error> {
    let (base, offset, ty, _) = dereference(l, value)?;
    let value = if is_aggregate(&ty) {
        place_addr(l, base, offset)
    } else {
        load_field(l, base, offset, &ty)
    };
    Ok(Val::V(value, ty))
}

pub(super) fn auto_place(
    l: &mut FnLower,
    value: &Expr,
) -> Result<(VReg, i16, Ty, bool), syn::Error> {
    let (mut address, mut ty) = expr(l, value)?.reg(l, value, "reference place")?;
    let mut mutable = false;
    while let Ty::Reference(element, permission) = ty {
        ty = *element;
        mutable = permission;
        if matches!(ty, Ty::Reference(..)) {
            address = l.b.load_mem(address, 0);
        }
    }
    Ok((address, 0, ty, mutable))
}

pub(super) fn target(ty: &Ty) -> &Ty {
    match ty {
        Ty::Reference(element, _) => target(element),
        _ => ty,
    }
}

pub(super) fn is_dereference(value: &Expr) -> bool {
    matches!(value, Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)))
}

/// Only borrowing the binding itself needs a scalar slot. Fields and indexed
/// elements already have addressable storage; their base pointer stays in SSA.
pub(super) fn resident_name(value: &Expr) -> Option<String> {
    match value {
        Expr::Path(_) => super::path_ident(value).ok(),
        Expr::Paren(paren) => resident_name(&paren.expr),
        _ => None,
    }
}

pub(super) fn compatible(from: &Ty, to: &Ty) -> bool {
    matches!((from, to), (Ty::Reference(a, true), Ty::Reference(b, false)) if a == b)
}

pub(super) fn peek(l: &FnLower, expression: &Expr) -> Option<Ty> {
    match expression {
        Expr::Reference(reference) if !super::byte_slices::is_value(l, &reference.expr) => {
            Some(Ty::Reference(
                Box::new(peek_type(l, &reference.expr)?),
                reference.mutability.is_some(),
            ))
        }
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            match peek_type(l, &unary.expr)? {
                Ty::Reference(element, _) => Some(*element),
                _ => None,
            }
        }
        _ => None,
    }
}
