//! Proven scratch lifetimes; named locals retain stable frame storage.
use super::{err, FnLower, StructTable, Ty};

/// Without an escape proof, an address-bearing result retains its call storage.
pub(super) fn carries_address(ty: &Ty, structs: &StructTable) -> bool {
    match ty {
        Ty::Ptr | Ty::ArrayRef(_) | Ty::ByteSlice => true,
        Ty::Struct(name) => structs[name]
            .fields
            .iter()
            .any(|field| carries_address(&field.ty, structs)),
        Ty::Tuple(fields) => fields.iter().any(|field| carries_address(field, structs)),
        Ty::Array(element, _) => carries_address(element, structs),
        _ => false,
    }
}

pub(super) fn scope<T>(
    l: &mut FnLower,
    preserve: bool,
    operation: impl FnOnce(&mut FnLower) -> Result<T, syn::Error>,
) -> Result<T, syn::Error> {
    let mark = l.temporaries.len();
    let result = operation(l);
    finish(l, mark, preserve);
    result
}

pub(super) fn finish(l: &mut FnLower, mark: usize, preserve: bool) {
    let allocations = l.temporaries.split_off(mark);
    if !preserve {
        for allocation in allocations {
            l.b.release_temporary(allocation);
        }
    }
}

pub(super) fn allocate(
    l: &mut FnLower,
    words: u16,
    at: &impl syn::spanned::Spanned,
    diagnostic: &str,
) -> Result<u8, syn::Error> {
    let allocation =
        l.b.try_alloc_temporary(words)
            .ok_or_else(|| err(at, diagnostic))?;
    let slot = allocation.slot();
    l.temporaries.push(allocation);
    Ok(slot)
}
