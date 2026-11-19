//! Conservative source dependency roots for storage, before physical placement.
//!
//! Unknown receiver types retain every method with the same name. This can keep
//! extra storage, but must never remove an implicit Drop or indirect-call target.
use std::collections::{HashMap, HashSet};
use syn::{visit::Visit, Item};

#[derive(Clone, Default)]
pub(super) struct Live {
    names: Option<HashSet<String>>,
}
impl Live {
    pub(super) fn follow(&mut self, edges: &[(String, String)], labels: &HashMap<String, String>) {
        let Some(names) = &mut self.names else { return };
        let internal = |label: &String| {
            labels
                .iter()
                .find(|(_, v)| *v == label)
                .map(|(k, _)| k)
                .unwrap_or(label)
                .clone()
        };
        loop {
            let mut changed = false;
            for (source, target) in edges {
                if names.contains(&internal(source)) {
                    changed |= names.insert(internal(target));
                }
            }
            if !changed {
                break;
            }
        }
    }
    pub(super) fn contains(&self, name: &str) -> bool {
        self.names.as_ref().is_none_or(|names| names.contains(name))
    }
    pub(super) fn item(&self, item: &Item) -> bool {
        match item {
            Item::Fn(f) => self.contains(&f.sig.ident.to_string()),
            Item::Const(c) => self.contains(&c.ident.to_string()),
            Item::Static(s) => self.contains(&s.ident.to_string()),
            _ => true,
        }
    }
}

#[derive(Default)]
struct References {
    names: HashSet<String>,
    methods: HashSet<String>,
    owner: Option<String>,
    division: bool,
}
impl<'ast> Visit<'ast> for References {
    fn visit_bin_op(&mut self, op: &'ast syn::BinOp) {
        self.division |= matches!(
            op,
            syn::BinOp::Div(_) | syn::BinOp::Rem(_) | syn::BinOp::DivEq(_) | syn::BinOp::RemEq(_)
        );
    }
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let parts: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        if let Some(first) = parts.first() {
            let first = if first == "Self" {
                self.owner.as_ref().unwrap_or(first)
            } else {
                first
            };
            self.names.insert(first.clone());
            if parts.len() == 2 {
                self.names.insert(format!("{first}::{}", parts[1]));
            }
        }
        syn::visit::visit_path(self, path);
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.methods.insert(call.method.to_string());
        syn::visit::visit_expr_method_call(self, call);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        fn tokens(stream: proc_macro2::TokenStream, names: &mut HashSet<String>) {
            for token in stream {
                match token {
                    proc_macro2::TokenTree::Ident(i) => {
                        names.insert(i.to_string());
                    }
                    proc_macro2::TokenTree::Group(g) => tokens(g.stream(), names),
                    _ => {}
                }
            }
        }
        let mut names = HashSet::new();
        tokens(mac.tokens.clone(), &mut names);
        // Until supported macros expand, identifiers may denote associated or
        // receiver methods as well as plain values/functions.
        self.methods.extend(names.iter().cloned());
        self.names.extend(names);
    }
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        // Resource markers name implicit cleanup functions in their arguments.
        let mut mac: syn::Macro = syn::parse_quote!(dependencies!());
        mac.tokens = attr.tokens.clone();
        self.visit_macro(&mac);
    }
}

pub(super) fn collect(
    files: &[syn::File],
    methods: Option<&super::value_methods::Methods>,
) -> Live {
    let mut dependencies: HashMap<String, References> = HashMap::new();
    let mut method_names: HashMap<String, HashSet<String>> = HashMap::new();
    for item in files.iter().flat_map(|file| &file.items) {
        let name = match item {
            Item::Fn(f) => Some(f.sig.ident.to_string()),
            Item::Const(c) => Some(c.ident.to_string()),
            Item::Static(s) => Some(s.ident.to_string()),
            Item::Struct(s) => Some(s.ident.to_string()),
            Item::Enum(e) => Some(e.ident.to_string()),
            _ => None,
        };
        if let Some(name) = name {
            let mut refs = References::default();
            refs.visit_item(item);
            dependencies.insert(name, refs);
        }
        if let Item::Impl(block) = item {
            let syn::Type::Path(owner) = block.self_ty.as_ref() else {
                continue;
            };
            let Some(owner) = owner.path.get_ident() else {
                continue;
            };
            let owner = owner.to_string();
            for member in &block.items {
                let syn::ImplItem::Method(method) = member else {
                    continue;
                };
                let name = format!("{owner}::{}", method.sig.ident);
                let mut refs = References {
                    owner: Some(owner.clone()),
                    ..Default::default()
                };
                refs.visit_impl_item_method(method);
                dependencies.insert(name.clone(), refs);
                method_names
                    .entry(method.sig.ident.to_string())
                    .or_default()
                    .insert(name.clone());
                if block
                    .trait_
                    .as_ref()
                    .is_some_and(|(_, p, _)| p.is_ident("Drop"))
                {
                    dependencies
                        .entry(owner.clone())
                        .or_default()
                        .names
                        .insert(name);
                }
            }
        }
    }
    if !dependencies.contains_key("main") {
        return Live::default();
    }
    if let Some(methods) = methods {
        for ((owner, method), target) in methods {
            let target = target.function.to_string();
            method_names
                .entry(method.clone())
                .or_default()
                .insert(target.clone());
            dependencies
                .entry(format!("{owner}::{method}"))
                .or_default()
                .names
                .insert(target);
        }
    }
    let mut live = HashSet::new();
    // Aggregate lowering introduces mem_copy after this source pass.
    let mut pending = ["main", "mem_copy"].map(str::to_owned).to_vec();
    while let Some(name) = pending.pop() {
        if !live.insert(name.clone()) {
            continue;
        }
        if let Some(refs) = dependencies.get(&name) {
            if refs.division {
                pending.extend(["div_u16", "rem_u16", "div_i16", "rem_i16"].map(str::to_owned));
            }
            pending.extend(refs.names.iter().cloned());
            for method in &refs.methods {
                pending.extend(method_names.get(method).into_iter().flatten().cloned());
            }
        }
        if matches!(name.as_str(), "malloc" | "try_malloc" | "free") {
            pending.push("init_heap".to_owned());
        }
        if name.starts_with("vec_") {
            pending.push("init_vec".to_owned());
        }
    }
    Live { names: Some(live) }
}
