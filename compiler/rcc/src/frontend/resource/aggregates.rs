//! Ordinary structs own resource fields; borrows retain their existing storage.
use super::{error, type_name, Binding, Checker, Model, Resource, Value};
use std::collections::{BTreeMap, HashSet};
use syn::{parse_quote, spanned::Spanned, Item, Type};

const PREFIX: &str = "__rcc_aggregate_";

#[derive(Clone)]
pub(super) struct Aggregate {
    pub fields: Vec<(String, Type)>,
}

pub(crate) fn names(files: &[syn::File]) -> Result<HashSet<String>, (usize, syn::Error)> {
    let mut owned: HashSet<_> = super::drop_syntax::markers(files)?.into_keys().collect();
    let roots = owned.clone();
    loop {
        let previous = owned.len();
        for file in files {
            for item in &file.items {
                if let Item::Struct(s) = item {
                    if s.fields
                        .iter()
                        .any(|field| contains_name(&field.ty, &owned))
                    {
                        owned.insert(s.ident.to_string());
                    }
                }
            }
        }
        if owned.len() == previous {
            return Ok(owned.difference(&roots).cloned().collect());
        }
    }
}

pub(super) fn prepare(
    files: &mut [syn::File],
    model: &mut Model,
) -> Result<(), (usize, syn::Error)> {
    let roots: HashSet<_> = model.resources.keys().cloned().collect();
    let mut owned = roots.clone();
    let mut definitions = BTreeMap::new();
    loop {
        let before = owned.len();
        for (fi, file) in files.iter().enumerate() {
            if let Some(id) = super::super::value_methods::reserved(
                quote::ToTokens::to_token_stream(file),
                PREFIX,
            ) {
                return Err((fi, error(&id, "reserved resource aggregate name")));
            }
            for item in &file.items {
                let Item::Struct(s) = item else { continue };
                if roots.contains(&s.ident.to_string()) {
                    continue;
                }
                let contains_owner = s.fields.iter().any(|f| contains_name(&f.ty, &owned));
                if !contains_owner {
                    continue;
                }
                if !model.checked {
                    return Err((
                        fi,
                        error(s, "resource aggregates require checked compilation"),
                    ));
                }
                if !s.generics.params.is_empty() || !matches!(s.fields, syn::Fields::Named(_)) {
                    return Err((
                        fi,
                        error(s, "resource aggregates need a nongeneric named struct"),
                    ));
                }
                owned.insert(s.ident.to_string());
                definitions.insert(s.ident.to_string(), (fi, s.clone()));
            }
        }
        if owned.len() == before {
            break;
        }
    }
    for (name, (file, source)) in &definitions {
        let fields = source
            .fields
            .iter()
            .map(|f| (f.ident.as_ref().unwrap().to_string(), f.ty.clone()))
            .collect();
        model.aggregates.insert(name.clone(), Aggregate { fields });
        model.resources.insert(
            name.clone(),
            Resource {
                native: true,
                file: *file,
                public: matches!(source.vis, syn::Visibility::Public(_)),
                field: String::new(),
                cleanup: format!("{PREFIX}drop_{name}"),
                parent: None,
            },
        );
    }
    for (name, (file, source)) in &definitions {
        let mut drops: Vec<syn::Stmt> = Vec::new();
        for field in &source.fields {
            if !contains_name(&field.ty, &owned) {
                continue;
            }
            let Some(ty) = type_name(&field.ty).filter(|n| owned.contains(n)) else {
                return Err((
                    *file,
                    error(
                        field,
                        "resource aggregate fields need direct owners or nested aggregates",
                    ),
                ));
            };
            if model.resources[&ty].parent.is_some() {
                return Err((
                    *file,
                    error(
                        field,
                        "a borrowed child resource cannot be stored in an owning aggregate",
                    ),
                ));
            }
            let member = field.ident.as_ref().unwrap();
            drops.push(parse_quote!(drop(value.#member);));
        }
        let cleanup = syn::Ident::new(&model.resources[name].cleanup, source.ident.span());
        let owner = &source.ident;
        files[*file]
            .items
            .push(parse_quote!(fn #cleanup(value: #owner) { #(#drops)* }));
    }
    Ok(())
}

fn contains_name(ty: &Type, names: &HashSet<String>) -> bool {
    struct Find<'a>(&'a HashSet<String>, bool);
    impl<'ast> syn::visit::Visit<'ast> for Find<'_> {
        fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
            self.1 |= path
                .path
                .segments
                .iter()
                .any(|s| self.0.contains(&s.ident.to_string()));
            syn::visit::visit_type_path(self, path);
        }
    }
    let mut find = Find(names, false);
    syn::visit::Visit::visit_type(&mut find, ty);
    find.1
}

impl Model {
    pub(super) fn binding_type(&self, name: &str, borrowed: bool) -> Type {
        let ty = self.erased_type(name);
        if borrowed && self.aggregates.contains_key(name) {
            parse_quote!(Array<#ty>)
        } else {
            ty
        }
    }
}

impl Checker<'_> {
    pub(super) fn bind(&mut self, mut binding: Binding) {
        let fields = self.model.aggregates.get(&binding.value.ty).cloned();
        let cleanup = binding.cleanup;
        if fields.is_some() {
            binding.cleanup = false;
        }
        self.bindings.push(binding.clone());
        let Some(fields) = fields else { return };
        // Scope cleanup walks bindings backwards; struct fields drop in declaration order.
        for (member, ty) in fields.fields.iter().rev() {
            let Some(ty) = type_name(ty).filter(|n| self.model.resources.contains_key(n)) else {
                continue;
            };
            let previous = self.bindings.iter().find_map(|root| {
                if root.value == binding.value && root.name != binding.name {
                    self.bindings
                        .iter()
                        .find(|child| child.name == format!("{}.{member}", root.name))
                        .map(|b| b.value.clone())
                } else {
                    None
                }
            });
            let value = previous.unwrap_or_else(|| self.value(ty, None));
            self.bind(Binding {
                name: format!("{}.{member}", binding.name),
                place: format!("{}.{member}", binding.place),
                value,
                live: true,
                owned: binding.owned,
                cleanup,
                mutable: binding.mutable,
            });
        }
    }

    pub(super) fn place(&self, name: &str) -> syn::Expr {
        syn::parse_str(
            &self
                .bindings
                .iter()
                .rev()
                .find(|b| b.name == name)
                .unwrap()
                .place,
        )
        .unwrap()
    }

    pub(super) fn erase_borrow(&self, binding: &Binding, mutable: bool) -> syn::Expr {
        let place = self.place(&binding.name);
        if self.model.aggregates.contains_key(&binding.value.ty) {
            if mutable {
                parse_quote!(__rcc_value_view(&mut #place))
            } else {
                parse_quote!(__rcc_value_view(&#place))
            }
        } else {
            place
        }
    }

    pub(super) fn ordinary_field(&self, expression: &syn::Expr) -> Option<syn::Expr> {
        let syn::Expr::Field(field) = expression else {
            return None;
        };
        let base_name = super::name(&field.base)?;
        if let Some(base) = self.bindings.iter().rev().find(|b| b.name == base_name) {
            if self.model.aggregates.contains_key(&base.value.ty) {
                let base = self.place(&base_name);
                let member = &field.member;
                return Some(parse_quote!(#base.#member));
            }
            return None; // A leaf resource's payload still belongs to its defining file.
        }
        let base = self.ordinary_field(&field.base)?;
        let member = &field.member;
        Some(parse_quote!(#base.#member))
    }

    pub(super) fn take_descendants(&mut self, name: &str) {
        let prefix = format!("{name}.");
        for b in &mut self.bindings {
            if b.name.starts_with(&prefix) {
                b.live = false;
            }
        }
    }

    pub(super) fn cleanup_place(&self, name: &str) -> Vec<syn::Stmt> {
        let prefix = format!("{name}.");
        self.bindings
            .iter()
            .rev()
            .filter(|b| {
                b.live
                    && (b.name == name || b.name.starts_with(&prefix))
                    && !self.model.aggregates.contains_key(&b.value.ty)
            })
            .map(|b| {
                let function = syn::Ident::new(
                    &self.model.resources[&b.value.ty].cleanup,
                    proc_macro2::Span::call_site(),
                );
                let place: syn::Expr = syn::parse_str(&b.place).unwrap();
                parse_quote!(#function(#place);)
            })
            .collect()
    }

    pub(super) fn replace_binding(&mut self, index: usize, value: Value) {
        let mut root = self.bindings[index].clone();
        root.value = value;
        root.live = true;
        root.cleanup = root.owned;
        let boundary = self.bindings.len();
        self.bind(root);
        let replacements = self.bindings.split_off(boundary);
        for replacement in replacements {
            let slot = self
                .bindings
                .iter()
                .rposition(|b| b.name == replacement.name)
                .expect("same aggregate shape");
            self.bindings[slot] = replacement;
        }
    }

    pub(super) fn aggregate_value(
        &mut self,
        value: &mut syn::ExprStruct,
        ty: &str,
    ) -> Result<Value, syn::Error> {
        if value.rest.is_some() {
            return Err(error(
                value,
                "resource aggregate update syntax is not supported",
            ));
        }
        let fields = self.model.aggregates[ty].fields.clone();
        for field in &mut value.fields {
            let expected = fields
                .iter()
                .find(|(name, _)| {
                    field.member == syn::Member::Named(syn::Ident::new(name, field.member.span()))
                })
                .map(|(_, ty)| ty);
            if let Some(expected) = expected
                .and_then(type_name)
                .filter(|n| self.model.resources.contains_key(n))
            {
                let super::Kind::Resource(actual) = self.expr(&mut field.expr)? else {
                    return Err(error(
                        field,
                        "resource aggregate field needs an owned resource",
                    ));
                };
                if expected != actual.ty {
                    return Err(error(field, "resource aggregate field type mismatch"));
                }
            } else {
                self.scalar(&mut field.expr)?;
            }
        }
        Ok(self.value(ty.to_owned(), None))
    }
}
