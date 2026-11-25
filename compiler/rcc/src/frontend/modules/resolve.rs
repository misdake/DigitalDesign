//! Resolve ordinary module paths before the existing flat target frontend.
use super::Graph;
use crate::frontend::{err, value_methods};
use quote::{quote, ToTokens};
use std::collections::{BTreeMap, HashMap, HashSet};
use syn::{visit_mut::VisitMut, Expr, Item, Pat, Path, UseTree};

const PREFIX: &str = "__rcc_module_";

struct AttributeValue {
    key: syn::Ident,
    equal: syn::Token![=],
    value: Expr,
}
impl syn::parse::Parse for AttributeValue {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self {
            key: input.parse()?,
            equal: input.parse()?,
            value: input.parse()?,
        })
    }
}
impl ToTokens for AttributeValue {
    fn to_tokens(&self, out: &mut proc_macro2::TokenStream) {
        let Self { key, equal, value } = self;
        out.extend(quote!(#key #equal #value));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Module(usize),
    Symbol { file: usize, name: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Binding {
    target: Target,
    scope: Option<usize>,
    // Legacy symbol definitions retain their existing export behavior.
    // An ordinary import still has its own visibility, even for those symbols.
    reexport_scope: Option<usize>,
}

struct Names<'a> {
    graph: &'a Graph,
    scopes: Vec<BTreeMap<String, Binding>>,
    module_scopes: Vec<BTreeMap<String, Binding>>,
    emitted: HashMap<(usize, String), String>,
    legacy: HashMap<String, Binding>,
    protected: HashSet<usize>,
    priorities: HashMap<(usize, String), u8>,
    ambiguous: HashSet<(usize, String)>,
}

impl Names<'_> {
    fn descendant(&self, mut child: usize, ancestor: usize) -> bool {
        loop {
            if child == ancestor {
                return true;
            }
            let Some(parent) = self.graph.parents.get(child).copied().flatten() else {
                return false;
            };
            child = parent;
        }
    }
    fn lexical_module(&self, module: usize) -> Binding {
        let scope = self.graph.parents[module].and_then(|parent| {
            let (name, _) = self.graph.children[parent]
                .iter()
                .find(|(_, child)| **child == module)?;
            self.module_scopes[parent].get(name)?.scope
        });
        Binding {
            target: Target::Module(module),
            scope,
            reexport_scope: scope,
        }
    }

    fn member(
        &self,
        current: usize,
        module: usize,
        name: &str,
        at: &impl syn::spanned::Spanned,
        module_context: bool,
    ) -> Result<Option<Binding>, syn::Error> {
        let binding = if module_context {
            self.module_scopes[module]
                .get(name)
                .or_else(|| self.scopes[module].get(name))
        } else {
            self.scopes[module]
                .get(name)
                .or_else(|| self.module_scopes[module].get(name))
        };
        let Some(binding) = binding else {
            return Ok(None);
        };
        let key = if matches!(binding.target, Target::Module(_)) {
            format!("::{name}")
        } else {
            name.to_owned()
        };
        if self.ambiguous.contains(&(module, key)) {
            return Err(err(at, format!("ambiguous module import `{name}`")));
        }
        if binding
            .scope
            .is_some_and(|scope| !self.descendant(current, scope))
            && !self.protected.contains(&module)
        {
            return Err(err(at, format!("private module item `{name}`")));
        }
        Ok(Some(binding.clone()))
    }
    fn resolve(
        &self,
        current: usize,
        parts: &[String],
        at: &impl syn::spanned::Spanned,
        module_context: bool,
    ) -> Result<Option<(Binding, usize)>, syn::Error> {
        let Some(first) = parts.first() else {
            return Ok(None);
        };
        let mut binding = match first.as_str() {
            "crate" => self.lexical_module(0),
            "self" => self.lexical_module(current),
            "super" => {
                let parent = self.graph.parents[current]
                    .ok_or_else(|| err(at, "super escapes the crate root"))?;
                self.lexical_module(parent)
            }
            _ => {
                let binding = if module_context || parts.len() > 1 {
                    self.module_scopes[current]
                        .get(first)
                        .or_else(|| self.scopes[current].get(first))
                } else {
                    self.scopes[current]
                        .get(first)
                        .or_else(|| self.module_scopes[current].get(first))
                }
                .or_else(|| self.legacy.get(first));
                let Some(binding) = binding else {
                    return Ok(None);
                };
                binding.clone()
            }
        };
        let key = if matches!(binding.target, Target::Module(_)) {
            format!("::{first}")
        } else {
            first.clone()
        };
        if self.ambiguous.contains(&(current, key)) {
            return Err(err(at, format!("ambiguous module import `{first}`")));
        }
        let mut consumed = 1;
        while consumed < parts.len() {
            let Target::Module(module) = binding.target else {
                break;
            };
            binding = if parts[consumed] == "super" {
                self.lexical_module(
                    self.graph.parents[module]
                        .ok_or_else(|| err(at, "super escapes the crate root"))?,
                )
            } else {
                let Some(member) = self.member(
                    current,
                    module,
                    &parts[consumed],
                    at,
                    module_context || consumed + 1 < parts.len(),
                )?
                else {
                    return Ok(None);
                };
                member
            };
            consumed += 1;
        }
        Ok(Some((binding, consumed)))
    }
}

fn visibility(
    vis: &syn::Visibility,
    file: usize,
    graph: &Graph,
) -> Result<Option<usize>, syn::Error> {
    match vis {
        syn::Visibility::Public(_) => Ok(None),
        syn::Visibility::Crate(_) => Ok(Some(0)),
        syn::Visibility::Inherited => Ok(Some(file)),
        syn::Visibility::Restricted(restricted) => {
            let parts: Vec<_> = restricted
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            let mut scope = file;
            for (index, part) in parts.iter().enumerate() {
                scope = match part.as_str() {
                    "crate" if index == 0 => 0,
                    "self" if index == 0 => file,
                    "super" => graph.parents[scope]
                        .ok_or_else(|| err(vis, "visibility escapes the crate root"))?,
                    name => *graph.children[scope]
                        .get(name)
                        .ok_or_else(|| err(vis, "unknown visibility module"))?,
                };
            }
            let mut ancestor = Some(file);
            while let Some(node) = ancestor {
                if node == scope {
                    return Ok(Some(scope));
                }
                ancestor = graph.parents[node];
            }
            Err(err(vis, "visibility must name an ancestor module"))
        }
    }
}

fn definition(item: &Item) -> Option<(&syn::Ident, &syn::Visibility)> {
    match item {
        Item::Fn(x) => Some((&x.sig.ident, &x.vis)),
        Item::Struct(x) => Some((&x.ident, &x.vis)),
        Item::Enum(x) => Some((&x.ident, &x.vis)),
        Item::Const(x) => Some((&x.ident, &x.vis)),
        Item::Static(x) => Some((&x.ident, &x.vis)),
        _ => None,
    }
}

fn uses(tree: &UseTree, prefix: &[String], out: &mut Vec<(Vec<String>, Option<String>)>) {
    match tree {
        UseTree::Path(path) => {
            let mut prefix = prefix.to_vec();
            prefix.push(path.ident.to_string());
            uses(&path.tree, &prefix, out);
        }
        UseTree::Name(name) => {
            let mut path = prefix.to_vec();
            path.push(name.ident.to_string());
            out.push((path, Some(name.ident.to_string())));
        }
        UseTree::Rename(rename) => {
            let mut path = prefix.to_vec();
            path.push(rename.ident.to_string());
            out.push((path, Some(rename.rename.to_string())));
        }
        UseTree::Glob(_) => out.push((prefix.to_vec(), None)),
        UseTree::Group(group) => {
            for item in &group.items {
                uses(item, prefix, out)
            }
        }
    }
}

struct Rewrite<'a, 'b> {
    names: &'a Names<'b>,
    file: usize,
    locals: Vec<HashSet<String>>,
    failure: Option<syn::Error>,
}
impl Rewrite<'_, '_> {
    fn bind(&mut self, pat: &Pat) {
        match pat {
            Pat::Ident(name) => {
                self.locals
                    .last_mut()
                    .unwrap()
                    .insert(name.ident.to_string());
            }
            Pat::Type(typed) => self.bind(&typed.pat),
            Pat::Tuple(tuple) => {
                for p in &tuple.elems {
                    self.bind(p)
                }
            }
            _ => {}
        }
    }
    fn emitted(&self, name: &syn::Ident) -> syn::Ident {
        let value = self
            .names
            .emitted
            .get(&(self.file, name.to_string()))
            .cloned()
            .unwrap_or_else(|| name.to_string());
        syn::Ident::new(&value, name.span())
    }
    fn path(&mut self, path: &mut Path, local: bool) {
        let parts: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        if local
            && parts.len() == 1
            && self
                .locals
                .iter()
                .rev()
                .any(|scope| scope.contains(&parts[0]))
        {
            return;
        }
        let result = self.names.resolve(self.file, &parts, path, false);
        match result {
            Ok(Some((
                Binding {
                    target: Target::Symbol { file, name },
                    ..
                },
                used,
            ))) => {
                let emitted = &self.names.emitted[&(file, name)];
                let mut segments = syn::punctuated::Punctuated::new();
                let mut head = path.segments[used - 1].clone();
                head.ident = syn::Ident::new(emitted, head.ident.span());
                segments.push(head);
                segments.extend(path.segments.iter().skip(used).cloned());
                path.leading_colon = None;
                path.segments = segments;
            }
            Err(error) => {
                self.failure.get_or_insert(error);
            }
            _ => {}
        }
    }
}
impl VisitMut for Rewrite<'_, '_> {
    fn visit_attribute_mut(&mut self, attr: &mut syn::Attribute) {
        if (attr.path.is_ident("asset") || attr.path.is_ident("static_buffer"))
            && !attr.tokens.is_empty()
        {
            if let Ok(mut values) = attr.parse_args_with(
                syn::punctuated::Punctuated::<AttributeValue, syn::Token![,]>::parse_terminated,
            ) {
                for value in &mut values {
                    self.visit_expr_mut(&mut value.value);
                }
                attr.tokens = quote!((#values));
            }
        }
    }
    fn visit_item_fn_mut(&mut self, f: &mut syn::ItemFn) {
        f.sig.ident = self.emitted(&f.sig.ident);
        self.locals.push(HashSet::new());
        for arg in &f.sig.inputs {
            if let syn::FnArg::Typed(arg) = arg {
                self.bind(&arg.pat);
            }
        }
        syn::visit_mut::visit_item_fn_mut(self, f);
        self.locals.pop();
    }
    fn visit_impl_item_method_mut(&mut self, f: &mut syn::ImplItemMethod) {
        self.locals.push(HashSet::new());
        for arg in &f.sig.inputs {
            if let syn::FnArg::Typed(arg) = arg {
                self.bind(&arg.pat);
            }
        }
        syn::visit_mut::visit_impl_item_method_mut(self, f);
        self.locals.pop();
    }
    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        self.locals.push(HashSet::new());
        syn::visit_mut::visit_block_mut(self, block);
        self.locals.pop();
    }
    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        syn::visit_mut::visit_local_mut(self, local);
        self.bind(&local.pat);
    }
    fn visit_expr_for_loop_mut(&mut self, loop_: &mut syn::ExprForLoop) {
        self.visit_expr_mut(&mut loop_.expr);
        self.locals.push(HashSet::new());
        self.bind(&loop_.pat);
        self.visit_block_mut(&mut loop_.body);
        self.locals.pop();
    }
    fn visit_item_struct_mut(&mut self, x: &mut syn::ItemStruct) {
        x.ident = self.emitted(&x.ident);
        syn::visit_mut::visit_item_struct_mut(self, x);
    }
    fn visit_item_enum_mut(&mut self, x: &mut syn::ItemEnum) {
        x.ident = self.emitted(&x.ident);
        syn::visit_mut::visit_item_enum_mut(self, x);
    }
    fn visit_item_const_mut(&mut self, x: &mut syn::ItemConst) {
        x.ident = self.emitted(&x.ident);
        syn::visit_mut::visit_item_const_mut(self, x);
    }
    fn visit_item_static_mut(&mut self, x: &mut syn::ItemStatic) {
        x.ident = self.emitted(&x.ident);
        syn::visit_mut::visit_item_static_mut(self, x);
    }
    fn visit_item_use_mut(&mut self, _: &mut syn::ItemUse) {}
    fn visit_expr_path_mut(&mut self, path: &mut syn::ExprPath) {
        self.path(&mut path.path, true);
        syn::visit_mut::visit_expr_path_mut(self, path);
    }
    fn visit_type_path_mut(&mut self, path: &mut syn::TypePath) {
        self.path(&mut path.path, false);
        syn::visit_mut::visit_type_path_mut(self, path);
    }
    fn visit_expr_struct_mut(&mut self, value: &mut syn::ExprStruct) {
        self.path(&mut value.path, false);
        syn::visit_mut::visit_expr_struct_mut(self, value);
    }
    fn visit_field_value_mut(&mut self, field: &mut syn::FieldValue) {
        syn::visit_mut::visit_field_value_mut(self, field);
        if field.colon_token.is_none() {
            if let (syn::Member::Named(member), Expr::Path(path)) = (&field.member, &field.expr) {
                if !path.path.is_ident(member) {
                    field.colon_token = Some(Default::default());
                }
            }
        }
    }
    fn visit_macro_mut(&mut self, mac: &mut syn::Macro) {
        if mac.path.is_ident("assert") || mac.path.is_ident("debug_assert") {
            use syn::parse::Parser;
            if let Ok(mut args) =
                syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated
                    .parse2(mac.tokens.clone())
            {
                for arg in &mut args {
                    self.visit_expr_mut(arg);
                }
                mac.tokens = args.to_token_stream();
            }
        }
    }
}

pub(super) fn prepare(
    graph: &Graph,
    files: &mut [syn::File],
    protected: &HashMap<usize, usize>,
    file_names: &[String],
) -> Result<super::Labels, (usize, syn::Error)> {
    let mut names = Names {
        graph,
        scopes: vec![BTreeMap::new(); files.len()],
        module_scopes: vec![BTreeMap::new(); files.len()],
        emitted: HashMap::new(),
        legacy: HashMap::new(),
        protected: protected.keys().copied().collect(),
        priorities: HashMap::new(),
        ambiguous: HashSet::new(),
    };
    let mut storage_counts: HashMap<String, usize> = HashMap::new();
    let mut symbol_counts: HashMap<String, usize> = HashMap::new();
    for file in files.iter() {
        for item in &file.items {
            if let Some((ident, _)) = definition(item) {
                *symbol_counts.entry(ident.to_string()).or_default() += 1;
            }
            if let Item::Const(value) = item {
                if value
                    .attrs
                    .iter()
                    .any(|a| a.path.is_ident("asset") || a.path.is_ident("static_buffer"))
                {
                    *storage_counts.entry(value.ident.to_string()).or_default() += 1;
                }
            }
        }
    }
    let mut storage_labels = HashMap::new();
    let mut symbol_labels = HashMap::new();
    for (fi, file) in files.iter().enumerate() {
        if let Some(ident) = value_methods::reserved(file.to_token_stream(), PREFIX) {
            return Err((fi, err(&ident, "reserved module symbol prefix")));
        }
        let legacy = protected.contains_key(&fi)
            || graph.compatibility[fi]
            || file_names[fi].starts_with("rcc_std/");
        for item in &file.items {
            if let Some((ident, vis)) = definition(item) {
                let name = ident.to_string();
                let scope = visibility(vis, fi, graph).map_err(|e| (fi, e))?;
                let binding = Binding {
                    target: Target::Symbol {
                        file: fi,
                        name: name.clone(),
                    },
                    scope,
                    reexport_scope: if legacy { None } else { scope },
                };
                if names.scopes[fi]
                    .insert(name.clone(), binding.clone())
                    .is_some()
                {
                    return Err((fi, err(ident, "duplicate module symbol")));
                }
                names.emitted.insert(
                    (fi, name.clone()),
                    if fi == 0 || legacy {
                        name.clone()
                    } else {
                        format!("{PREFIX}{fi}_{}", name.trim_start_matches("r#"))
                    },
                );
                let label = if symbol_counts[&name] == 1 {
                    name.clone()
                } else {
                    format!(
                        "{}::{name}",
                        file_names[fi].trim_end_matches(".rs").replace('/', "::")
                    )
                };
                symbol_labels.insert(names.emitted[&(fi, name.clone())].clone(), label);
                if let Item::Const(value) = item {
                    if value
                        .attrs
                        .iter()
                        .any(|a| a.path.is_ident("asset") || a.path.is_ident("static_buffer"))
                    {
                        let label = if storage_counts[&name] == 1 {
                            name.clone()
                        } else {
                            format!(
                                "{}::{name}",
                                file_names[fi].trim_end_matches(".rs").replace('/', "::")
                            )
                        };
                        storage_labels.insert(names.emitted[&(fi, name.clone())].clone(), label);
                    }
                }
                names.priorities.insert((fi, name.clone()), 3);
                if legacy {
                    names.legacy.entry(name).or_insert(binding);
                }
            }
            if let Item::Mod(module) = item {
                if let Some(&child) = graph.children[fi].get(&module.ident.to_string()) {
                    let scope = visibility(&module.vis, fi, graph).map_err(|e| (fi, e))?;
                    names.module_scopes[fi].insert(
                        module.ident.to_string(),
                        Binding {
                            target: Target::Module(child),
                            scope,
                            reexport_scope: scope,
                        },
                    );
                    names
                        .priorities
                        .insert((fi, format!("::{}", module.ident)), 3);
                }
            }
        }
    }
    let mut imports = vec![];
    for (fi, file) in files.iter().enumerate() {
        if protected.contains_key(&fi)
            || graph.compatibility[fi]
            || file_names[fi].starts_with("rcc_std/")
        {
            continue;
        }
        for item in &file.items {
            if let Item::Use(import) = item {
                let mut paths = vec![];
                uses(&import.tree, &[], &mut paths);
                for (path, alias) in paths {
                    if path.starts_with(&["crate".into(), "dsl_rt".into()]) {
                        continue;
                    }
                    imports.push((
                        fi,
                        path,
                        alias,
                        visibility(&import.vis, fi, graph).map_err(|e| (fi, e))?,
                        import,
                    ));
                }
            }
        }
    }
    for _ in 0..=files.len() {
        let mut changed = false;
        for (fi, path, alias, scope, at) in &imports {
            let Some((source, used)) = names.resolve(*fi, path, *at, true).map_err(|e| (*fi, e))?
            else {
                continue;
            };
            if used != path.len() {
                continue;
            }
            let entries = if let Some(alias) = alias {
                vec![(alias.clone(), source)]
            } else if let Target::Module(module) = source.target {
                names.scopes[module]
                    .iter()
                    .chain(names.module_scopes[module].iter())
                    .filter(|(name, b)| {
                        !names.ambiguous.contains(&(module, (*name).clone()))
                            && (b.scope.is_none_or(|scope| names.descendant(*fi, scope)))
                    })
                    .map(|(name, binding)| (name.clone(), binding.clone()))
                    .collect()
            } else {
                continue;
            };
            for (name, source) in entries {
                let scope = match source.reexport_scope {
                    Some(limit)
                        if scope.is_none_or(|requested| !names.descendant(requested, limit)) =>
                    {
                        if alias.is_some() {
                            return Err((
                                *fi,
                                err(
                                    *at,
                                    format!("cannot re-export private module item `{name}`"),
                                ),
                            ));
                        }
                        Some(limit)
                    }
                    _ => *scope,
                };
                let binding = Binding {
                    target: source.target,
                    scope,
                    reexport_scope: scope,
                };
                let is_module = matches!(binding.target, Target::Module(_));
                let key = (
                    *fi,
                    if is_module {
                        format!("::{name}")
                    } else {
                        name.clone()
                    },
                );
                let priority = if alias.is_some() { 2 } else { 1 };
                let scope = if is_module {
                    &mut names.module_scopes[*fi]
                } else {
                    &mut names.scopes[*fi]
                };
                if let Some(existing) = scope.get(&name) {
                    if existing.target == binding.target {
                        continue;
                    }
                    let old = names.priorities[&key];
                    if priority == 2 && old >= 2 {
                        return Err((*fi, err(*at, format!("conflicting module import `{name}`"))));
                    }
                    if priority < old {
                        continue;
                    }
                    if priority == old {
                        names.ambiguous.insert(key);
                        continue;
                    }
                }
                scope.insert(name, binding);
                names.priorities.insert(key.clone(), priority);
                names.ambiguous.remove(&key);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for (fi, file) in files.iter_mut().enumerate() {
        if protected.contains_key(&fi)
            || graph.compatibility[fi]
            || file_names[fi].starts_with("rcc_std/")
        {
            continue;
        }
        let mut rewrite = Rewrite {
            names: &names,
            file: fi,
            locals: vec![HashSet::new()],
            failure: None,
        };
        rewrite.visit_file_mut(file);
        if let Some(error) = rewrite.failure {
            return Err((fi, error));
        }
    }
    Ok(super::Labels {
        storage: storage_labels,
        symbols: symbol_labels,
    })
}
