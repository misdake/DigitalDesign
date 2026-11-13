//! Checked inherent value methods lowered through the existing value/view ABI.
use super::{err, peek_type, FnLower, Ty};
use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use std::collections::{HashMap, HashSet};
use syn::{parse_quote, visit_mut::VisitMut, Expr, FnArg, Item, Type};

const PREFIX: &str = "__rcc_value_";
#[derive(Clone, Copy)]
enum Receiver {
    Associated,
    Value,
    Borrow { mutable: bool },
}
#[derive(Clone)]
pub(super) struct Method {
    pub(super) function: syn::Ident,
    receiver: Receiver,
}
pub(super) type Methods = HashMap<(String, String), Method>;

pub(super) fn constant_value_function(
    methods: &Methods,
    owner: &str,
    name: &str,
) -> Option<syn::Ident> {
    let method = methods.get(&(owner.to_owned(), name.to_owned()))?;
    matches!(method.receiver, Receiver::Value).then(|| method.function.clone())
}

fn plain(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else { return None };
    path.path.get_ident().map(ToString::to_string)
}

// The supported assertion macro still contains source expressions, including
// `self` and associated calls. Keep it in sync with ordinary method lowering.
fn rewrite_assertion(mac: &mut syn::Macro, visit: impl FnOnce(&mut Expr)) {
    if mac.path.is_ident("assert") || mac.path.is_ident("debug_assert") {
        if let Ok(mut expression) = syn::parse2::<Expr>(mac.tokens.clone()) {
            visit(&mut expression);
            mac.tokens = expression.to_token_stream();
        }
    }
}
pub(super) fn reserved(tokens: TokenStream, prefix: &str) -> Option<syn::Ident> {
    for token in tokens {
        match token {
            TokenTree::Ident(name)
                if name
                    .to_string()
                    .trim_start_matches("r#")
                    .starts_with(prefix) =>
            {
                return Some(name)
            }
            TokenTree::Group(group) => {
                if let Some(name) = reserved(group.stream(), prefix) {
                    return Some(name);
                }
            }
            _ => {}
        }
    }
    None
}

pub(super) fn prepare(
    files: &mut [syn::File],
    checked: bool,
) -> Result<Methods, (usize, syn::Error)> {
    let mut methods = Methods::new();
    for (fi, file) in files.iter().enumerate() {
        if let Some(name) = reserved(file.to_token_stream(), PREFIX) {
            return Err((fi, err(&name, "reserved value method name")));
        }
    }
    if !checked {
        return Ok(methods);
    }
    let values: HashSet<_> = files
        .iter()
        .flat_map(|file| &file.items)
        .filter_map(|item| match item {
            Item::Struct(value) => (!value
                .attrs
                .iter()
                .any(|attr| attr.path.is_ident("rcc_resource")))
            .then(|| value.ident.to_string()),
            Item::Enum(value) => Some(value.ident.to_string()),
            _ => None,
        })
        .collect();
    for (fi, file) in files.iter_mut().enumerate() {
        let mut items = Vec::new();
        for item in std::mem::take(&mut file.items) {
            let Item::Impl(block) = &item else {
                items.push(item);
                continue;
            };
            let Some(owner) = plain(&block.self_ty).filter(|name| values.contains(name)) else {
                items.push(item);
                continue;
            };
            if block.trait_.is_some()
                || block.unsafety.is_some()
                || block.defaultness.is_some()
                || !block.generics.params.is_empty()
                || block.generics.where_clause.is_some()
                || block
                    .attrs
                    .iter()
                    .any(|a| !a.path.is_ident("doc") && !a.path.is_ident("allow"))
            {
                return Err((
                    fi,
                    err(
                        block,
                        "value impls support nongeneric inherent methods only",
                    ),
                ));
            }
            for member in &block.items {
                let syn::ImplItem::Method(method) = member else {
                    return Err((fi, err(member, "value impls support functions only")));
                };
                if method.defaultness.is_some()
                    || !method.sig.generics.params.is_empty()
                    || method.sig.generics.where_clause.is_some()
                {
                    return Err((
                        fi,
                        err(method, "generic/default value methods are not supported"),
                    ));
                }
                let key = (owner.clone(), method.sig.ident.to_string());
                if methods.contains_key(&key) {
                    return Err((fi, err(method, "duplicate value method")));
                }
                let index = methods.len();
                let function =
                    syn::Ident::new(&format!("{PREFIX}{index}"), method.sig.ident.span());
                let local =
                    syn::Ident::new(&format!("{PREFIX}self_{index}"), method.sig.ident.span());
                let mut sig = method.sig.clone();
                sig.ident = function.clone();
                let receiver = match method.sig.inputs.first() {
                    Some(FnArg::Receiver(r)) => {
                        if !r.attrs.is_empty()
                            || r.reference.as_ref().is_some_and(|(_, l)| l.is_some())
                        {
                            return Err((
                                fi,
                                err(r, "unsupported value receiver attribute/lifetime"),
                            ));
                        }
                        let ty = syn::Ident::new(&owner, r.self_token.span);
                        if r.reference.is_some() {
                            let mutable = r.mutability.is_some();
                            sig.inputs[0] = if mutable {
                                parse_quote!(mut #local: Array<#ty>)
                            } else {
                                parse_quote!(#local: Array<#ty>)
                            };
                            Receiver::Borrow { mutable }
                        } else {
                            sig.inputs[0] = if r.mutability.is_some() {
                                parse_quote!(mut #local: #ty)
                            } else {
                                parse_quote!(#local: #ty)
                            };
                            Receiver::Value
                        }
                    }
                    _ => Receiver::Associated,
                };
                struct Rename<'a> {
                    owner: &'a str,
                    local: &'a syn::Ident,
                    receiver: Receiver,
                }
                impl VisitMut for Rename<'_> {
                    fn visit_macro_mut(&mut self, mac: &mut syn::Macro) {
                        rewrite_assertion(mac, |expression| self.visit_expr_mut(expression));
                    }
                    fn visit_path_mut(&mut self, path: &mut syn::Path) {
                        if let Some(segment) = path.segments.first_mut() {
                            if segment.ident == "Self" {
                                segment.ident = syn::Ident::new(self.owner, segment.ident.span());
                            }
                        }
                        syn::visit_mut::visit_path_mut(self, path);
                    }
                    fn visit_expr_mut(&mut self, expression: &mut Expr) {
                        let is_self =
                            matches!(expression, Expr::Path(p) if p.path.is_ident("self"));
                        let deref_self = matches!(expression, Expr::Unary(u) if matches!(u.op, syn::UnOp::Deref(_)) && matches!(u.expr.as_ref(), Expr::Path(p) if p.path.is_ident("self")));
                        if is_self
                            || (deref_self && matches!(self.receiver, Receiver::Borrow { .. }))
                        {
                            let local = self.local;
                            *expression = match self.receiver {
                                Receiver::Borrow { .. } => parse_quote!(#local[0u16]),
                                _ => parse_quote!(#local),
                            };
                        } else {
                            syn::visit_mut::visit_expr_mut(self, expression);
                        }
                    }
                }
                let mut body = method.block.clone();
                let mut rename = Rename {
                    owner: &owner,
                    local: &local,
                    receiver,
                };
                rename.visit_signature_mut(&mut sig);
                rename.visit_block_mut(&mut body);
                items.push(Item::Fn(syn::ItemFn {
                    attrs: method.attrs.clone(),
                    vis: method.vis.clone(),
                    sig,
                    block: Box::new(body),
                }));
                methods.insert(key, Method { function, receiver });
            }
        }
        file.items = items;
    }
    struct Associated<'a>(&'a Methods);
    impl VisitMut for Associated<'_> {
        fn visit_macro_mut(&mut self, mac: &mut syn::Macro) {
            rewrite_assertion(mac, |expression| self.visit_expr_mut(expression));
        }
        fn visit_expr_path_mut(&mut self, expression: &mut syn::ExprPath) {
            if expression.qself.is_none() && expression.path.segments.len() == 2 {
                let key = (
                    expression.path.segments[0].ident.to_string(),
                    expression.path.segments[1].ident.to_string(),
                );
                if let Some(Method {
                    function,
                    receiver: Receiver::Associated,
                }) = self.0.get(&key)
                {
                    expression.path = parse_quote!(#function);
                }
            }
            syn::visit_mut::visit_expr_path_mut(self, expression);
        }
    }
    for file in files {
        Associated(&methods).visit_file_mut(file);
    }
    Ok(methods)
}

pub(super) fn call(l: &FnLower, expression: &syn::ExprMethodCall) -> Option<syn::ExprCall> {
    let owner = match peek_type(l, &expression.receiver)? {
        Ty::Struct(owner) | Ty::Enum(owner) => owner,
        _ => return None,
    };
    let method = l
        .globals
        .value_methods
        .get(&(owner.clone(), expression.method.to_string()))?;
    if expression.turbofish.is_some() {
        return None;
    }
    let receiver = &expression.receiver;
    let argument: Expr = match method.receiver {
        Receiver::Associated => return None,
        Receiver::Value => *receiver.clone(),
        Receiver::Borrow { mutable } => {
            // Rustc already checked the original receiver; the internal view retains
            // its mutability in the generated callee and borrows the existing place.
            if mutable {
                parse_quote!(__rcc_value_view(&mut #receiver))
            } else {
                parse_quote!(__rcc_value_view(&#receiver))
            }
        }
    };
    let function = &method.function;
    let mut args = syn::punctuated::Punctuated::new();
    args.push(argument);
    args.extend(expression.args.iter().cloned());
    Some(syn::ExprCall {
        attrs: expression.attrs.clone(),
        func: Box::new(parse_quote!(#function)),
        paren_token: expression.paren_token,
        args,
    })
}

pub(super) fn is_value_temporary(l: &FnLower, expression: &Expr) -> bool {
    if !matches!(peek_type(l, expression), Some(Ty::Struct(_) | Ty::Enum(_))) {
        return false;
    }
    match expression {
        Expr::Paren(paren) => is_value_temporary(l, &paren.expr),
        Expr::Path(path) => {
            let Some(name) = path.path.get_ident() else {
                return true;
            };
            let name = name.to_string();
            l.lookup(&name).is_none() && !l.globals.scalars.contains_key(&name)
        }
        Expr::Field(_) | Expr::Index(_) => false,
        _ => true,
    }
}
