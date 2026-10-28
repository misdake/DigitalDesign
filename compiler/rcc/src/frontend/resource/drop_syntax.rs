//! Standard Drop spelling for the existing one-word lexical resource ABI.
use super::{error, type_name};
use std::collections::{BTreeMap, HashSet};
use syn::{parse_quote, visit::Visit, visit_mut::VisitMut, ExprPath, FnArg, Item};

pub(crate) const PREFIX: &str = "__rcc_drop_";
pub(crate) fn resource_ident(name: &str, span: proc_macro2::Span) -> syn::Ident {
    match name.strip_prefix("r#") {
        Some(name) => syn::Ident::new_raw(name, span),
        None => syn::Ident::new(name, span),
    }
}

#[derive(Clone)]
pub(crate) struct Marker {
    pub file: usize,
    pub cleanup: Option<String>,
    pub parent: Option<String>,
}

pub(crate) fn validate_attributes(files: &[syn::File]) -> Result<(), (usize, syn::Error)> {
    // Run before rustc can remove conditional impls/methods. Only attributes
    // that cannot change the checked destructor body may be discarded.
    for (fi, file) in files.iter().enumerate() {
        for item in &file.items {
            let Item::Impl(block) = item else { continue };
            if !block
                .trait_
                .as_ref()
                .is_some_and(|(_, path, _)| path.is_ident("Drop"))
            {
                continue;
            }
            let method_attrs = block.items.iter().filter_map(|item| match item {
                syn::ImplItem::Method(method) => Some(&method.attrs),
                _ => None,
            });
            for attr in block.attrs.iter().chain(method_attrs.flatten()) {
                if !attr.path.is_ident("doc") && !attr.path.is_ident("allow") {
                    return Err((fi, error(attr, "unsupported resource Drop attribute")));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn markers(
    files: &[syn::File],
) -> Result<BTreeMap<String, Marker>, (usize, syn::Error)> {
    let mut result = BTreeMap::new();
    for (file, source) in files.iter().enumerate() {
        for item in &source.items {
            let Item::Struct(s) = item else { continue };
            for attr in s.attrs.iter().filter(|a| a.path.is_ident("rcc_resource")) {
                let mut marker = Marker {
                    file,
                    cleanup: None,
                    parent: None,
                };
                if !attr.tokens.is_empty() {
                    attr.parse_args_with(|input: syn::parse::ParseStream| {
                        while !input.is_empty() {
                            let key: syn::Ident = input.parse()?;
                            input.parse::<syn::Token![=]>()?;
                            let value: syn::Ident = input.parse()?;
                            let slot = match key.to_string().as_str() {
                                "cleanup" => &mut marker.cleanup,
                                "parent" => &mut marker.parent,
                                _ => return Err(error(&key, "unknown resource attribute key")),
                            };
                            if slot.replace(value.to_string()).is_some() {
                                return Err(error(&key, "duplicate resource attribute key"));
                            }
                            if !input.is_empty() {
                                input.parse::<syn::Token![,]>()?;
                            }
                        }
                        Ok(())
                    })
                    .map_err(|e| (file, e))?;
                }
                if result.insert(s.ident.to_string(), marker).is_some() {
                    return Err((file, error(s, "duplicate resource type/marker")));
                }
            }
        }
    }
    Ok(result)
}

pub(crate) fn lower(files: &mut [syn::File], checked: bool) -> Result<(), (usize, syn::Error)> {
    // Neither a source function nor an attribute may forge the private bridge.
    struct Reserved(Option<syn::Error>);
    impl<'a> Visit<'a> for Reserved {
        fn visit_ident(&mut self, id: &'a syn::Ident) {
            if id.to_string().trim_start_matches("r#").starts_with(PREFIX) {
                self.0 = Some(error(id, "reserved Drop lowering name"));
            }
        }
        fn visit_attribute(&mut self, attr: &'a syn::Attribute) {
            fn walk(stream: proc_macro2::TokenStream) -> bool {
                stream.into_iter().any(|t| match t {
                    proc_macro2::TokenTree::Ident(i) => {
                        i.to_string().trim_start_matches("r#").starts_with(PREFIX)
                    }
                    proc_macro2::TokenTree::Group(g) => walk(g.stream()),
                    _ => false,
                })
            }
            if walk(attr.tokens.clone()) {
                self.0 = Some(error(attr, "reserved Drop lowering name"));
            }
        }
    }
    for (fi, file) in files.iter().enumerate() {
        let mut check = Reserved(None);
        check.visit_file(file);
        if let Some(e) = check.0 {
            return Err((fi, e));
        }
    }
    let resources = markers(files)?;
    let mut drops = HashSet::new();
    for (fi, file) in files.iter_mut().enumerate() {
        let mut items = vec![];
        for item in std::mem::take(&mut file.items) {
            let Item::Impl(block) = &item else {
                items.push(item);
                continue;
            };
            if !block
                .trait_
                .as_ref()
                .is_some_and(|(_, p, _)| p.is_ident("Drop"))
            {
                items.push(item);
                continue;
            }
            if !checked {
                return Err((
                    fi,
                    error(block, "Drop requires the rustc-checked frontend entry"),
                ));
            }
            let ty = type_name(&block.self_ty)
                .ok_or_else(|| (fi, error(block, "Drop requires one resource type")))?;
            let r = resources.get(&ty).ok_or_else(|| {
                (
                    fi,
                    error(block, "Drop is limited to one-word rcc_resource types"),
                )
            })?;
            if r.file != fi || r.cleanup.is_some() || !drops.insert(ty.clone()) {
                return Err((fi,error(block,"Drop must be same-file and cannot coexist with a cleanup function or another Drop")));
            }
            if !block.generics.params.is_empty()
                || block.generics.where_clause.is_some()
                || block.unsafety.is_some()
                || block.defaultness.is_some()
                || block.trait_.as_ref().unwrap().0.is_some()
                || block.items.len() != 1
            {
                return Err((
                    fi,
                    error(block, "resource Drop supports one nongeneric drop method"),
                ));
            }
            let syn::ImplItem::Method(method) = &block.items[0] else {
                return Err((
                    fi,
                    error(block, "resource Drop supports a drop method only"),
                ));
            };
            let valid = method.sig.ident == "drop"
                && method.sig.inputs.len() == 1
                && matches!(method.sig.inputs.first(),Some(FnArg::Receiver(r)) if r.reference.is_some() && r.mutability.is_some() && r.reference.as_ref().unwrap().1.is_none())
                && method.sig.generics.params.is_empty()
                && method.sig.generics.where_clause.is_none()
                && method.sig.constness.is_none()
                && method.sig.asyncness.is_none()
                && method.sig.unsafety.is_none()
                && method.sig.variadic.is_none();
            if !valid {
                return Err((fi, error(method, "resource Drop needs fn drop(&mut self)")));
            }
            let owner = resource_ident(&ty, block.impl_token.span);
            let cleanup = syn::Ident::new(
                &format!("{PREFIX}cleanup_{}", ty.trim_start_matches("r#")),
                method.sig.ident.span(),
            );
            let body_name = syn::Ident::new(
                &format!("{PREFIX}body_{}", ty.trim_start_matches("r#")),
                method.sig.ident.span(),
            );
            let local: syn::Ident = parse_quote!(__rcc_drop_value);
            let mut body = method.block.clone();
            struct Rename<'a>(&'a syn::Ident);
            impl VisitMut for Rename<'_> {
                fn visit_expr_path_mut(&mut self, p: &mut ExprPath) {
                    if p.path.is_ident("self") {
                        let id = self.0;
                        *p = parse_quote!(#id);
                    } else {
                        syn::visit_mut::visit_expr_path_mut(self, p);
                    }
                }
            }
            Rename(&local).visit_block_mut(&mut body);
            items.push(parse_quote!(fn #body_name(#local:&mut #owner) #body));
            // The ordinary resource checker exempts this cleanup's owned argument.
            // It borrows the same identity once; no recursive/drop-by-copy path.
            items.push(parse_quote!(fn #cleanup(mut #local:#owner){#body_name(&mut #local);}));
        }
        file.items = items;
    }
    for (fi, file) in files.iter_mut().enumerate() {
        for item in &mut file.items {
            let Item::Struct(s) = item else { continue };
            if !drops.contains(&s.ident.to_string()) {
                continue;
            }
            let cleanup = syn::Ident::new(
                &format!(
                    "{PREFIX}cleanup_{}",
                    s.ident.to_string().trim_start_matches("r#")
                ),
                s.ident.span(),
            );
            let parent = resources[&s.ident.to_string()]
                .parent
                .as_ref()
                .map(|n| resource_ident(n, s.ident.span()));
            let attr = if let Some(parent) = parent {
                parse_quote!(#[rcc_resource(cleanup=#cleanup,parent=#parent)])
            } else {
                parse_quote!(#[rcc_resource(cleanup=#cleanup)])
            };
            let a = s
                .attrs
                .iter_mut()
                .find(|a| a.path.is_ident("rcc_resource"))
                .unwrap();
            *a = attr;
            if resources[&s.ident.to_string()].file != fi {
                unreachable!()
            }
        }
    }
    Ok(())
}
