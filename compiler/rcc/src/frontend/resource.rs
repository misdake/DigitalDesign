//! Restricted lexical resources. Check ownership before erasing the one-word ABI.
//! This is deliberately not a general Rust reference or lifetime implementation.
use std::collections::{HashMap, HashSet};
use syn::{parse_quote, spanned::Spanned, visit::Visit, visit_mut::VisitMut};
use syn::{Expr, FnArg, Item, Pat, ReturnType, Stmt, Type};
pub(super) mod drop_syntax;
mod library;
#[cfg(test)]
mod library_tests;
mod methods;

pub(super) struct Sources<'a> {
    pub(super) origins: &'a [Option<std::path::PathBuf>],
    pub(super) approved_roots: &'a [std::path::PathBuf],
}

#[derive(Clone)]
struct Resource {
    file: usize,
    public: bool,
    field: String,
    cleanup: String,
    parent: Option<String>,
}
#[derive(Clone, PartialEq, Eq)]
enum Arg {
    Native,
    Own(String),
    Borrow(String),
}
#[derive(Clone)]
struct Function {
    file: usize,
    public: bool,
    args: Vec<Arg>,
    result: Option<String>,
    unit: bool,
    diverges: bool,
}
struct Model {
    resources: HashMap<String, Resource>,
    functions: HashMap<String, Function>,
    values: HashMap<String, (usize, bool)>,
    domains: HashMap<usize, usize>,
    methods: HashMap<(String, String), methods::Method>,
}
impl Model {
    fn can_access(&self, caller: usize, target: usize, public: bool) -> bool {
        match self.domains.get(&target) {
            None => true,
            Some(root) if self.domains.get(&caller) == Some(root) => true,
            Some(root) => target == *root && public,
        }
    }
    fn check_private_path(&self, file: usize, path: &syn::ExprPath) -> Result<(), syn::Error> {
        // Qualified builtin paths (Buf::new, enum variants, etc.) are handled
        // by the existing frontend; user functions/constants use flat names.
        if path.path.segments.len() != 1 {
            return Ok(());
        }
        let n = path.path.segments.last().unwrap().ident.to_string();
        if let Some((owner, public)) = self.values.get(&n) {
            if !self.can_access(file, *owner, *public) {
                return Err(error(
                    path,
                    "private resource-module storage or internal resource-library storage is not accessible here",
                ));
            }
        }
        if let Some(f) = self.functions.get(&n) {
            if !self.can_access(file, f.file, f.public) {
                return Err(error(
                    path,
                    "private resource-module function or internal resource-library function is not accessible here",
                ));
            }
            if f.result.is_some() || f.args.iter().any(|a| !matches!(a, Arg::Native)) {
                return Err(error(
                    path,
                    "resource functions require direct calls, not function pointers",
                ));
            }
        }
        Ok(())
    }
    fn check_type_paths(&self, file: usize, ty: &Type) -> Result<(), syn::Error> {
        struct Paths<'a> {
            model: &'a Model,
            file: usize,
            error: Option<syn::Error>,
        }
        impl<'ast> Visit<'ast> for Paths<'_> {
            fn visit_expr_path(&mut self, p: &'ast syn::ExprPath) {
                if let Err(e) = self.model.check_private_path(self.file, p) {
                    self.error = Some(e)
                }
                syn::visit::visit_expr_path(self, p);
            }
            fn visit_type_path(&mut self, p: &'ast syn::TypePath) {
                // syn parses a bare Buf<T, LENGTH> identifier as a type argument;
                // RCC resolves that position as a constant during layout.
                if p.path.segments.len() == 1
                    && self
                        .model
                        .values
                        .contains_key(&p.path.segments[0].ident.to_string())
                {
                    let value = syn::ExprPath {
                        attrs: vec![],
                        qself: p.qself.clone(),
                        path: p.path.clone(),
                    };
                    if let Err(e) = self.model.check_private_path(self.file, &value) {
                        self.error = Some(e)
                    }
                }
                syn::visit::visit_type_path(self, p);
            }
        }
        let mut check = Paths {
            model: self,
            file,
            error: None,
        };
        check.visit_type(ty);
        check.error.map_or(Ok(()), Err)
    }
}
fn error(node: &impl Spanned, message: impl std::fmt::Display) -> syn::Error {
    syn::Error::new(node.span(), message)
}
fn name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(p) if p.path.segments.len() == 1 => Some(p.path.segments[0].ident.to_string()),
        _ => None,
    }
}
fn place_root(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Field(f) => place_root(&f.base),
        Expr::Paren(p) => place_root(&p.expr),
        _ => name(expr),
    }
}
fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) if p.path.segments.len() == 1 => Some(p.path.segments[0].ident.to_string()),
        _ => None,
    }
}
fn contains(ty: &Type, resources: &HashMap<String, Resource>) -> bool {
    struct Find<'a>(&'a HashMap<String, Resource>, bool);
    impl<'ast> Visit<'ast> for Find<'_> {
        fn visit_type_path(&mut self, ty: &'ast syn::TypePath) {
            self.1 |= ty
                .path
                .segments
                .iter()
                .any(|s| self.0.contains_key(&s.ident.to_string()));
            syn::visit::visit_type_path(self, ty);
        }
    }
    let mut find = Find(resources, false);
    find.visit_type(ty);
    find.1
}
fn classify(ty: &Type, model: &Model) -> Result<Arg, syn::Error> {
    if let Some(n) = type_name(ty).filter(|n| model.resources.contains_key(n)) {
        return Ok(Arg::Own(n));
    }
    if let Type::Reference(r) = ty {
        if let Some(n) = type_name(&r.elem).filter(|n| model.resources.contains_key(n)) {
            if r.mutability.is_none() || r.lifetime.is_some() {
                return Err(error(
                    ty,
                    "resource references must be direct &mut parameters",
                ));
            }
            return Ok(Arg::Borrow(n));
        }
    }
    if contains(ty, &model.resources) {
        return Err(error(
            ty,
            "resources cannot be embedded in aggregates, arrays, pointers or references",
        ));
    }
    Ok(Arg::Native)
}

pub(super) fn lower(
    files: &mut [syn::File],
    file_names: &[String],
    sources: Option<Sources<'_>>,
) -> Result<(), (usize, syn::Error)> {
    let mut model = Model {
        resources: HashMap::new(),
        functions: HashMap::new(),
        values: HashMap::new(),
        domains: HashMap::new(),
        methods: HashMap::new(),
    };
    for (fi, file) in files.iter().enumerate() {
        for item in &file.items {
            let Item::Struct(s) = item else { continue };
            let attrs: Vec<_> = s
                .attrs
                .iter()
                .filter(|a| a.path.is_ident("rcc_resource"))
                .collect();
            if attrs.is_empty() {
                continue;
            }
            let result = (|| {
                if attrs.len() != 1
                    || s.attrs.iter().any(|a| {
                        !a.path.is_ident("rcc_resource")
                            && !a.path.is_ident("doc")
                            && !a.path.is_ident("allow")
                    })
                    || !s.generics.params.is_empty()
                {
                    return Err(error(
                        s,
                        "a resource accepts only one rcc_resource attribute and no generics/derive",
                    ));
                }
                let syn::Fields::Named(fields) = &s.fields else {
                    return Err(error(s, "resource needs one private u16 field"));
                };
                if fields.named.len() != 1 {
                    return Err(error(s, "resource needs one private u16 field"));
                }
                let field = &fields.named[0];
                if !matches!(field.vis, syn::Visibility::Inherited)
                    || type_name(&field.ty).as_deref() != Some("u16")
                    || !field.attrs.is_empty()
                {
                    return Err(error(field, "resource needs one private u16 field"));
                }
                Ok::<_, syn::Error>(())
            })();
            result.map_err(|e| (fi, e))?;
            // Parse the narrow identifier = identifier grammar directly.
            let parsed = attrs[0]
                .parse_args_with(|input: syn::parse::ParseStream| {
                    let mut cleanup: Option<String> = None;
                    let mut parent: Option<String> = None;
                    while !input.is_empty() {
                        let key: syn::Ident = input.parse()?;
                        input.parse::<syn::Token![=]>()?;
                        let value: syn::Ident = input.parse()?;
                        let slot = match key.to_string().as_str() {
                            "cleanup" => &mut cleanup,
                            "parent" => &mut parent,
                            _ => return Err(error(&key, "unknown resource attribute key")),
                        };
                        if slot.replace(value.to_string()).is_some() {
                            return Err(error(&key, "duplicate resource attribute key"));
                        }
                        if !input.is_empty() {
                            input.parse::<syn::Token![,]>()?;
                        }
                    }
                    Ok((
                        cleanup.ok_or_else(|| error(s, "resource needs cleanup = function"))?,
                        parent,
                    ))
                })
                .map_err(|e| (fi, e))?;
            let field = s
                .fields
                .iter()
                .next()
                .unwrap()
                .ident
                .as_ref()
                .unwrap()
                .to_string();
            let resource = Resource {
                file: fi,
                public: matches!(s.vis, syn::Visibility::Public(_)),
                field,
                cleanup: parsed.0,
                parent: parsed.1,
            };
            if model
                .resources
                .insert(s.ident.to_string(), resource)
                .is_some()
            {
                return Err((fi, error(s, "duplicate resource type")));
            }
        }
    }
    model.domains = library::domains(files, file_names, sources, &model.resources)?;
    if model.resources.is_empty() {
        return Ok(());
    }
    methods::prepare(files, &mut model)?;
    // The native frontend has separate function/constant/storage tables. A
    // protected declaration must never borrow another kind's access decision
    // while lowering still resolves the original storage or callable.
    let mut definitions: HashMap<String, (usize, &str)> = HashMap::new();
    for (fi, file) in files.iter().enumerate() {
        for item in &file.items {
            let (ident, kind) = match item {
                Item::Fn(f) => (&f.sig.ident, "function"),
                Item::Static(s) => (&s.ident, "static"),
                Item::Const(c) => (&c.ident, "const"),
                _ => continue,
            };
            let name = ident.to_string();
            if let Some((previous, previous_kind)) = definitions.insert(name.clone(), (fi, kind)) {
                if model.domains.contains_key(&previous) || model.domains.contains_key(&fi) {
                    return Err((fi, error(ident, format!("resource-domain value name `{name}` is defined more than once ({previous_kind} and {kind})"))));
                }
            }
        }
    }
    for resource in model.resources.values() {
        if resource
            .parent
            .as_ref()
            .is_some_and(|n| !model.resources.contains_key(n))
        {
            return Err((
                resource.file,
                syn::Error::new(proc_macro2::Span::call_site(), "unknown parent resource"),
            ));
        }
    }
    for (fi, file) in files.iter().enumerate() {
        for item in &file.items {
            match item {
                Item::Static(s) => {
                    model.values.insert(
                        s.ident.to_string(),
                        (fi, matches!(s.vis, syn::Visibility::Public(_))),
                    );
                }
                Item::Const(c) => {
                    model.values.insert(
                        c.ident.to_string(),
                        (fi, matches!(c.vis, syn::Visibility::Public(_))),
                    );
                }
                _ => {}
            }
            if let Item::Fn(f) = item {
                if f.sig.ident.to_string().starts_with("__rcc_resource_") {
                    return Err((fi, error(&f.sig.ident, "reserved resource temporary name")));
                }
                let result = (|| {
                    let mut args = Vec::new();
                    for arg in &f.sig.inputs {
                        let FnArg::Typed(p) = arg else { return Err(error(arg, "resource methods are not supported")) };
                        if matches!(p.pat.as_ref(), Pat::Ident(id) if id.ident.to_string().starts_with("__rcc_resource_")) {
                            return Err(error(&p.pat, "reserved resource temporary name"));
                        }
                        args.push(classify(&p.ty, &model)?);
                    }
                    let result = match &f.sig.output {
                        ReturnType::Default => None,
                        ReturnType::Type(_, ty) => match classify(ty, &model)? {
                            Arg::Own(n) => Some(n),
                            Arg::Borrow(_) => return Err(error(ty, "resource references cannot escape a call")),
                            Arg::Native => None,
                        },
                    };
                    let unit = matches!(&f.sig.output, ReturnType::Default) || matches!(&f.sig.output, ReturnType::Type(_, ty) if matches!(ty.as_ref(), Type::Tuple(t) if t.elems.is_empty()));
                    let diverges = matches!(&f.sig.output, ReturnType::Type(_, ty) if matches!(ty.as_ref(), Type::Never(_)));
                    Ok(Function { file: fi, public: matches!(f.vis, syn::Visibility::Public(_)), args, result, unit, diverges })
                })().map_err(|e| (fi, e))?;
                model.functions.insert(f.sig.ident.to_string(), result);
            }
        }
    }
    // All names must be collected before checking module-level expressions:
    // visibility cannot depend on module loading order.
    for (fi, file) in files.iter().enumerate() {
        for item in &file.items {
            if let Item::Fn(f) = item {
                for parameter in &f.sig.inputs {
                    if let FnArg::Typed(p) = parameter {
                        model.check_type_paths(fi, &p.ty).map_err(|e| (fi, e))?;
                    }
                }
                if let ReturnType::Type(_, ty) = &f.sig.output {
                    model.check_type_paths(fi, ty).map_err(|e| (fi, e))?;
                }
            }
            if matches!(item, Item::Fn(_))
                || matches!(item, Item::Struct(s) if model.resources.contains_key(&s.ident.to_string()))
            {
                continue;
            }
            {
                struct Check<'a>(&'a Model, usize, Option<syn::Error>);
                impl<'ast> Visit<'ast> for Check<'_> {
                    fn visit_type(&mut self, ty: &'ast Type) {
                        if let Err(e) = self.0.check_type_paths(self.1, ty) {
                            self.2 = Some(e)
                        }
                        if contains(ty, &self.0.resources) {
                            self.2 = Some(error(ty, "resource storage is limited to local owners and function signatures"))
                        }
                        syn::visit::visit_type(self, ty);
                    }
                    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
                        if let Err(e) = self.0.check_private_path(self.1, path) {
                            self.2 = Some(e)
                        }
                        syn::visit::visit_expr_path(self, path);
                    }
                }
                let mut check = Check(&model, fi, None);
                check.visit_item(item);
                if let Some(e) = check.2 {
                    return Err((fi, e));
                }
            }
        }
    }
    for (n, r) in &model.resources {
        let Some(f) = model.functions.get(&r.cleanup) else {
            return Err((
                r.file,
                syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "resource cleanup function is missing",
                ),
            ));
        };
        if f.file != r.file || f.public || f.args != [Arg::Own(n.clone())] || !f.unit {
            return Err((r.file, syn::Error::new(proc_macro2::Span::call_site(), "cleanup must be private, same-file, take its resource by value and return unit")));
        }
    }
    for (fi, file) in files.iter_mut().enumerate() {
        for item in &mut file.items {
            if let Item::Fn(f) = item {
                Checker::function(&model, fi, f).map_err(|e| (fi, e))?;
            }
        }
        file.items.retain(|item| !matches!(item, Item::Struct(s) if model.resources.contains_key(&s.ident.to_string())));
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct Value {
    id: usize,
    ty: String,
    parent: Option<usize>,
}
#[derive(Clone, PartialEq, Eq)]
struct Binding {
    name: String,
    value: Value,
    live: bool,
    owned: bool,
    cleanup: bool,
    mutable: bool,
}
#[derive(Clone)]
enum Kind {
    Scalar,
    Unit,
    Resource(Value),
}
struct Checker<'a> {
    model: &'a Model,
    file: usize,
    bindings: Vec<Binding>,
    next: usize,
    reachable: bool,
    loops: Vec<(usize, Vec<Binding>)>,
    result: Option<String>,
    native_names: Vec<String>,
    pending_owned: Vec<Value>,
    pending_borrows: Vec<Value>,
}
impl<'a> Checker<'a> {
    fn function(model: &'a Model, file: usize, f: &mut syn::ItemFn) -> Result<(), syn::Error> {
        let signature = &model.functions[&f.sig.ident.to_string()];
        let mut this = Self {
            model,
            file,
            bindings: vec![],
            next: 0,
            reachable: true,
            loops: vec![],
            result: signature.result.clone(),
            native_names: vec![],
            pending_owned: vec![],
            pending_borrows: vec![],
        };
        for (param, mode) in f.sig.inputs.iter_mut().zip(&signature.args) {
            let FnArg::Typed(param) = param else {
                unreachable!()
            };
            if matches!(mode, Arg::Native) {
                if let Pat::Ident(p) = param.pat.as_ref() {
                    this.native_names.push(p.ident.to_string());
                }
            }
            if let Arg::Own(n) | Arg::Borrow(n) = mode {
                let Pat::Ident(p) = param.pat.as_mut() else {
                    return Err(error(param, "resource parameters need a simple name"));
                };
                if p.by_ref.is_some() || p.subpat.is_some() {
                    return Err(error(p, "resource parameters need a simple owned name"));
                }
                this.visible(n, &param.ty)?;
                let value = this.value(n.clone(), None);
                this.bindings.push(Binding {
                    name: p.ident.to_string(),
                    value,
                    live: true,
                    owned: matches!(mode, Arg::Own(_)),
                    cleanup: matches!(mode, Arg::Own(_))
                        && f.sig.ident != model.resources[n].cleanup,
                    mutable: p.mutability.is_some() || matches!(mode, Arg::Borrow(_)),
                });
                // Reborrowing a mutable resource binding does not mutate the
                // erased scalar identity. Keep owned/borrowed mutability in
                // the ownership model, not in the native u16 parameter ABI.
                p.mutability = None;
                *param.ty = parse_quote!(u16);
            }
        }
        if signature.result.is_some() && matches!(f.block.stmts.last(), Some(Stmt::Expr(_))) {
            let Some(Stmt::Expr(value)) = f.block.stmts.pop() else {
                unreachable!()
            };
            f.block.stmts.push(parse_quote!(return #value;));
        }
        this.block_at(&mut f.block, 0)?;
        if signature.result.is_some() {
            f.sig.output = parse_quote!(-> u16);
        }
        Ok(())
    }
    fn visible(&self, ty: &str, node: &impl Spanned) -> Result<(), syn::Error> {
        let r = &self.model.resources[ty];
        if r.file != self.file && !r.public {
            return Err(error(node, "private resource type"));
        }
        Ok(())
    }
    fn value(&mut self, ty: String, parent: Option<usize>) -> Value {
        let id = self.next;
        self.next += 1;
        Value { id, ty, parent }
    }
    fn lookup(&self, n: &str, node: &impl Spanned) -> Result<Option<Binding>, syn::Error> {
        let Some(b) = self.bindings.iter().rev().find(|b| b.name == n) else {
            return Ok(None);
        };
        if !b.live {
            return Err(error(node, "resource was moved or consumed"));
        }
        if self.pending_borrows.iter().any(|v| v.id == b.value.id) {
            return Err(error(
                node,
                "resource is locked by aliased call arguments (pending call)",
            ));
        }
        if self
            .bindings
            .iter()
            .any(|v| v.live && v.value.parent == Some(b.value.id))
            || self
                .pending_owned
                .iter()
                .any(|v| v.parent == Some(b.value.id))
        {
            return Err(error(
                node,
                "parent resource is exclusively borrowed by a live child",
            ));
        }
        Ok(Some(b.clone()))
    }
    fn take(&mut self, n: &str, node: &impl Spanned) -> Result<Value, syn::Error> {
        let b = self
            .lookup(n, node)?
            .ok_or_else(|| error(node, "expected a resource owner"))?;
        if !b.owned {
            return Err(error(node, "cannot move a borrowed resource"));
        }
        self.bindings
            .iter_mut()
            .rev()
            .find(|x| x.name == n)
            .unwrap()
            .live = false;
        Ok(b.value)
    }
    fn cleanup(&self, start: usize) -> Vec<Stmt> {
        self.bindings[start..]
            .iter()
            .rev()
            .filter(|b| b.live && b.cleanup)
            .map(|b| {
                let function = drop_syntax::resource_ident(
                    &self.model.resources[&b.value.ty].cleanup,
                    proc_macro2::Span::call_site(),
                );
                let local = syn::Ident::new(&b.name, proc_macro2::Span::call_site());
                parse_quote!(#function(#local);)
            })
            .collect()
    }
    fn block(&mut self, block: &mut syn::Block) -> Result<Kind, syn::Error> {
        let start = self.bindings.len();
        self.block_at(block, start)
    }
    fn block_at(&mut self, block: &mut syn::Block, start: usize) -> Result<Kind, syn::Error> {
        let native_start = self.native_names.len();
        let mut kind = Kind::Unit;
        for stmt in &mut block.stmts {
            if !self.reachable {
                return Err(error(
                    stmt,
                    "unreachable statement after a resource control-flow exit",
                ));
            }
            kind = match stmt {
                Stmt::Local(local) => {
                    let local_pat = if let Pat::Type(p) = &local.pat {
                        p.pat.as_ref()
                    } else {
                        &local.pat
                    };
                    if let Pat::Ident(p) = local_pat {
                        if p.ident.to_string().starts_with("__rcc_resource_")
                            || self.bindings.iter().any(|b| p.ident == b.name)
                        {
                            return Err(error(p, "resource shadowing or reserved resource temporary name is not supported"));
                        }
                    }
                    let declared = if let Pat::Type(p) = &local.pat {
                        self.model.check_type_paths(self.file, &p.ty)?;
                        Some(classify(&p.ty, self.model)?)
                    } else {
                        None
                    };
                    if matches!(declared, Some(Arg::Borrow(_))) {
                        return Err(error(local, "resource references cannot be stored"));
                    }
                    let Some((_, expr)) = &mut local.init else {
                        continue;
                    };
                    let value = self.expr(expr)?;
                    if let Kind::Resource(v) = value {
                        if matches!(declared, Some(Arg::Native)) {
                            return Err(error(expr, "resource cannot initialize an ordinary type"));
                        }
                        let pat = if let Pat::Type(p) = &mut local.pat {
                            *p.ty = parse_quote!(u16);
                            p.pat.as_mut()
                        } else {
                            &mut local.pat
                        };
                        let Pat::Ident(p) = pat else {
                            return Err(error(pat, "resource locals need a simple name"));
                        };
                        if p.by_ref.is_some() || p.subpat.is_some() {
                            return Err(error(p, "resource locals need a simple owned name"));
                        }
                        if self.bindings.iter().any(|b| p.ident == b.name) {
                            return Err(error(p, "resource shadowing is not supported"));
                        }
                        if let Some(Arg::Own(n)) = declared {
                            if n != v.ty {
                                return Err(error(p, "resource type mismatch"));
                            }
                        }
                        self.bindings.push(Binding {
                            name: p.ident.to_string(),
                            value: v,
                            live: true,
                            owned: true,
                            cleanup: true,
                            mutable: p.mutability.is_some(),
                        });
                    } else if matches!(declared, Some(Arg::Own(_))) {
                        return Err(error(
                            expr,
                            "cannot forge a resource from an ordinary value",
                        ));
                    } else {
                        let pattern = if let Pat::Type(p) = &local.pat {
                            p.pat.as_ref()
                        } else {
                            &local.pat
                        };
                        if let Pat::Ident(p) = pattern {
                            self.native_names.push(p.ident.to_string());
                        }
                    }
                    Kind::Unit
                }
                Stmt::Expr(e) => self.expr(e)?,
                Stmt::Semi(e, _) => {
                    if matches!(self.expr(e)?, Kind::Resource(_)) {
                        return Err(error(e, "bind or consume a returned resource"));
                    }
                    Kind::Unit
                }
                Stmt::Item(_) => Kind::Unit,
            };
        }
        if self.reachable {
            if matches!(kind, Kind::Resource(_)) {
                return Err(error(
                    block,
                    "resource values cannot escape a block; use an explicit factory return",
                ));
            }
            let cleanup = self.cleanup(start);
            if !cleanup.is_empty() {
                if let Some(Stmt::Expr(tail)) = block.stmts.last() {
                    let tail = tail.clone();
                    block.stmts.pop();
                    if matches!(kind, Kind::Scalar) {
                        let temp = syn::Ident::new(
                            &format!("__rcc_resource_value_{}", self.next),
                            proc_macro2::Span::call_site(),
                        );
                        self.next += 1;
                        block.stmts.push(parse_quote!(let #temp = #tail;));
                        block.stmts.extend(cleanup);
                        block.stmts.push(Stmt::Expr(parse_quote!(#temp)));
                    } else {
                        block.stmts.push(parse_quote!(#tail;));
                        block.stmts.extend(cleanup);
                    }
                } else {
                    block.stmts.extend(cleanup);
                }
            }
        }
        self.bindings.truncate(start);
        self.native_names.truncate(native_start);
        Ok(kind)
    }
    fn expr(&mut self, expr: &mut Expr) -> Result<Kind, syn::Error> {
        if let Expr::MethodCall(method) = expr {
            if let Some(call) = self.method(method)? {
                *expr = Expr::Call(call);
            }
        }
        match expr {
            Expr::Path(p) => {
                let n = p.path.segments.last().unwrap().ident.to_string();
                if self.bindings.iter().any(|b| b.name == n) {
                    return Ok(Kind::Resource(self.take(&n, p)?));
                }
                if !self.native_names.contains(&n) {
                    self.model.check_private_path(self.file, p)?;
                }
                Ok(Kind::Scalar)
            }
            Expr::Call(call) => {
                let result = self.call(call)?;
                if name(&call.func).as_deref() == Some("__rcc_resource_disarmed") {
                    *expr = parse_quote!({});
                }
                Ok(result)
            }
            Expr::Struct(s)
                if self
                    .model
                    .resources
                    .contains_key(&s.path.segments.last().unwrap().ident.to_string()) =>
            {
                let ty = s.path.segments.last().unwrap().ident.to_string();
                let r = &self.model.resources[&ty];
                if r.file != self.file
                    || s.rest.is_some()
                    || s.fields.len() != 1
                    || s.fields[0].member
                        != syn::Member::Named(syn::Ident::new(
                            &r.field,
                            proc_macro2::Span::call_site(),
                        ))
                {
                    return Err(error(
                        s,
                        "resource construction is private to its definition file",
                    ));
                }
                let parent = if let Some(parent) = &r.parent {
                    let matches: Vec<_> = self
                        .bindings
                        .iter()
                        .filter(|b| b.live && &b.value.ty == parent)
                        .collect();
                    if matches.len() != 1 {
                        return Err(error(
                            s,
                            "child construction needs one unambiguous live parent",
                        ));
                    }
                    let b = self.lookup(&matches[0].name, s)?.unwrap();
                    Some(b.value.id)
                } else {
                    None
                };
                let mut word = s.fields[0].expr.clone();
                if matches!(self.expr(&mut word)?, Kind::Resource(_)) {
                    return Err(error(s, "resource construction needs an ordinary word"));
                }
                if let Some(parent) = parent {
                    let Some(binding) = self
                        .bindings
                        .iter()
                        .find(|b| b.live && b.value.id == parent)
                    else {
                        return Err(error(s, "child construction consumed its actual parent"));
                    };
                    self.lookup(&binding.name, s)?;
                }
                *expr = word;
                Ok(Kind::Resource(self.value(ty, parent)))
            }
            Expr::Field(f) => {
                if let Some(n) = name(&f.base) {
                    if let Some(b) = self.lookup(&n, f)? {
                        let r = &self.model.resources[&b.value.ty];
                        if r.file != self.file
                            || f.member
                                != syn::Member::Named(syn::Ident::new(
                                    &r.field,
                                    proc_macro2::Span::call_site(),
                                ))
                        {
                            return Err(error(f, "resource fields are private"));
                        }
                        *expr = *f.base.clone();
                        return Ok(Kind::Scalar);
                    }
                }
                self.native(expr)
            }
            Expr::Reference(r) => {
                if place_root(&r.expr).is_some_and(|n| self.bindings.iter().any(|b| b.name == n)) {
                    return Err(error(
                        r,
                        "resource borrowing is limited to direct &mut call arguments",
                    ));
                }
                self.native(expr)
            }
            Expr::Cast(c) if contains(&c.ty, &self.model.resources) => {
                Err(error(c, "cannot cast to a resource"))
            }
            Expr::Assign(a) => {
                if place_root(&a.left).is_some_and(|n| self.bindings.iter().any(|b| b.name == n)) {
                    return Err(error(
                        a,
                        "resource reassignment is not supported; bind a new owner",
                    ));
                }
                self.native(expr)
            }
            Expr::AssignOp(a)
                if place_root(&a.left)
                    .is_some_and(|n| self.bindings.iter().any(|b| b.name == n)) =>
            {
                Err(error(a, "resource identity cannot be reassigned"))
            }
            Expr::Block(b) => self.block(&mut b.block),
            Expr::Unsafe(b) => self.block(&mut b.block),
            Expr::Return(r) => {
                if !self.pending_owned.is_empty() || !self.pending_borrows.is_empty() {
                    return Err(error(r, "return cannot exit while resource call arguments are pending; compute arguments before moving the owner"));
                }
                let kind = if let Some(value) = &mut r.expr {
                    self.expr(value)?
                } else {
                    Kind::Unit
                };
                if !self.reachable {
                    if let Some(value) = &r.expr {
                        *expr = *value.clone();
                    }
                    return Ok(Kind::Unit);
                }
                match (&self.result, &kind) {
                    (Some(expected), Kind::Resource(v)) if expected == &v.ty => {
                        if self.model.resources[expected].parent.is_some() && v.parent.is_none() {
                            return Err(error(r, "child return has no provable borrowed parent; consumed children cannot be reparented"));
                        }
                        if let Some(parent) = v.parent {
                            if !self
                                .bindings
                                .iter()
                                .any(|b| b.live && !b.owned && b.value.id == parent)
                            {
                                return Err(error(
                                    r,
                                    "child resource cannot escape its actual parent",
                                ));
                            }
                        }
                    }
                    (Some(_), _) | (None, Kind::Resource(_)) => {
                        return Err(error(r, "resource return type mismatch"))
                    }
                    _ => {}
                }
                let cleanup = self.cleanup(0);
                if cleanup.is_empty() {
                    self.reachable = false;
                    return Ok(Kind::Unit);
                }
                if let Some(value) = &r.expr {
                    let value = value.clone();
                    let temp = syn::Ident::new(
                        &format!("__rcc_resource_return_{}", self.next),
                        proc_macro2::Span::call_site(),
                    );
                    self.next += 1;
                    *expr = parse_quote!({ let #temp = #value; #(#cleanup)* return #temp; });
                } else {
                    *expr = parse_quote!({ #(#cleanup)* return; });
                }
                self.reachable = false;
                Ok(Kind::Unit)
            }
            Expr::If(i) => {
                self.scalar(&mut i.cond)?;
                let entry = self.bindings.clone();
                let left_kind = self.block(&mut i.then_branch)?;
                let left = self.bindings.clone();
                let left_live = self.reachable;
                self.bindings = entry.clone();
                self.reachable = true;
                let right_kind = if let Some((_, branch)) = &mut i.else_branch {
                    self.expr(branch)?
                } else {
                    Kind::Unit
                };
                let right_live = self.reachable;
                if matches!(left_kind, Kind::Resource(_)) || matches!(right_kind, Kind::Resource(_))
                {
                    return Err(error(i, "resource-valued conditionals are not supported"));
                }
                if left_live && right_live && left != self.bindings {
                    return Err(error(i, "resource ownership differs at branch merge"));
                }
                if left_live && !right_live {
                    self.bindings = left;
                }
                self.reachable = left_live || right_live;
                Ok(
                    if matches!(left_kind, Kind::Scalar) && matches!(right_kind, Kind::Scalar) {
                        Kind::Scalar
                    } else {
                        Kind::Unit
                    },
                )
            }
            Expr::While(w) => {
                let entry = self.bindings.clone();
                self.scalar(&mut w.cond)?;
                if self.bindings != entry {
                    return Err(error(w, "loop condition cannot change resource ownership"));
                }
                self.loop_body(&mut w.body)?;
                Ok(Kind::Unit)
            }
            Expr::Loop(l) => {
                self.loop_body(&mut l.body)?;
                Ok(Kind::Unit)
            }
            Expr::Match(_) | Expr::ForLoop(_) if !self.bindings.is_empty() => Err(error(
                expr,
                "resource control flow supports if, while and loop; match/for are not supported",
            )),
            Expr::Binary(b) if matches!(b.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => {
                self.scalar(&mut b.left)?;
                let entry = self.bindings.clone();
                self.scalar(&mut b.right)?;
                if self.bindings != entry {
                    return Err(error(
                        b,
                        "short-circuit expression cannot change resource ownership",
                    ));
                }
                Ok(Kind::Scalar)
            }
            Expr::Break(_) | Expr::Continue(_) => {
                if !self.pending_owned.is_empty() || !self.pending_borrows.is_empty() {
                    return Err(error(
                        expr,
                        "loop exit cannot cross pending resource call arguments",
                    ));
                }
                if matches!(expr, Expr::Break(b) if b.expr.is_some() || b.label.is_some())
                    || matches!(expr, Expr::Continue(c) if c.label.is_some())
                {
                    return Err(error(
                        expr,
                        "resource loops do not support labels or break values",
                    ));
                }
                let Some((start, entry)) = self.loops.last() else {
                    return Err(error(expr, "loop exit outside loop"));
                };
                if &self.bindings[..*start] != entry.as_slice() {
                    return Err(error(
                        expr,
                        "resource ownership differs at loop exit/backedge",
                    ));
                }
                let cleanup = self.cleanup(*start);
                let exit = expr.clone();
                *expr = parse_quote!({ #(#cleanup)* #exit; });
                self.reachable = false;
                Ok(Kind::Unit)
            }
            _ => self.native(expr),
        }
    }
    fn loop_body(&mut self, block: &mut syn::Block) -> Result<(), syn::Error> {
        let entry = self.bindings.clone();
        self.loops.push((self.bindings.len(), entry.clone()));
        self.block(block)?;
        if self.reachable && self.bindings != entry {
            return Err(error(block, "resource ownership differs at loop backedge"));
        }
        self.loops.pop();
        self.bindings = entry;
        self.reachable = true;
        Ok(())
    }
    fn scalar(&mut self, expr: &mut Expr) -> Result<(), syn::Error> {
        if matches!(self.expr(expr)?, Kind::Resource(_)) {
            return Err(error(
                expr,
                "resource cannot be copied, converted or used as an ordinary value",
            ));
        }
        Ok(())
    }
    fn native(&mut self, expr: &mut Expr) -> Result<Kind, syn::Error> {
        struct Walk<'a, 'm> {
            checker: &'a mut Checker<'m>,
            failure: Option<syn::Error>,
        }
        impl VisitMut for Walk<'_, '_> {
            fn visit_expr_mut(&mut self, e: &mut Expr) {
                if self.failure.is_none() {
                    if let Err(error) = self.checker.scalar(e) {
                        self.failure = Some(error)
                    }
                }
            }
            fn visit_type_mut(&mut self, ty: &mut Type) {
                if let Err(e) = self.checker.model.check_type_paths(self.checker.file, ty) {
                    self.failure = Some(e)
                }
                if contains(ty, &self.checker.model.resources) {
                    self.failure = Some(error(
                        ty,
                        "resource cannot appear inside an ordinary expression type",
                    ))
                }
            }
        }
        let mut walk = Walk {
            checker: self,
            failure: None,
        };
        syn::visit_mut::visit_expr_mut(&mut walk, expr);
        if let Some(e) = walk.failure {
            return Err(e);
        }
        Ok(Kind::Scalar)
    }
    fn call(&mut self, call: &mut syn::ExprCall) -> Result<Kind, syn::Error> {
        let Some(n) = name(&call.func) else {
            return self.native_call(call);
        };
        if n == "drop" && !self.model.functions.contains_key(&n) {
            if call.args.len() != 1 {
                return Err(error(call, "drop needs one owned resource"));
            }
            let Some(local) = name(&call.args[0]) else {
                return Err(error(call, "drop needs a bound resource owner"));
            };
            let value = self.take(&local, call)?;
            let cleanup = drop_syntax::resource_ident(
                &self.model.resources[&value.ty].cleanup,
                call.paren_token.span,
            );
            *call.func = parse_quote!(#cleanup);
            return Ok(Kind::Unit);
        }
        if n == "resource_disarm" {
            if call.args.len() != 1 {
                return Err(error(call, "resource_disarm needs one owned resource"));
            }
            let Some(local) = name(&call.args[0]) else {
                return Err(error(call, "resource_disarm needs an owned local"));
            };
            let v = self.take(&local, call)?;
            if self.model.resources[&v.ty].file != self.file {
                return Err(error(
                    call,
                    "resource_disarm is private to the resource definition file",
                ));
            }
            *call.func = parse_quote!(__rcc_resource_disarmed);
            call.args.clear();
            // Replaced by the enclosing expression walker below after validation.
            return Ok(Kind::Unit);
        }
        let Some(function) = self.model.functions.get(&n).cloned() else {
            return self.native_call(call);
        };
        if !self
            .model
            .can_access(self.file, function.file, function.public)
        {
            return Err(error(
                call,
                "private resource-module function or internal resource-library function is not callable here",
            ));
        }
        if function.args.len() != call.args.len() {
            return Err(error(call, "function argument count mismatch"));
        }
        let mut used = HashSet::new();
        let owned_start = self.pending_owned.len();
        let borrowed_start = self.pending_borrows.len();
        let mut borrowed = Vec::new();
        let mut owned = Vec::new();
        for (expr, mode) in call.args.iter_mut().zip(&function.args) {
            match mode {
                Arg::Native => self.scalar(expr)?,
                Arg::Own(expected) => {
                    if name(expr).is_none() {
                        return Err(error(expr, "owned resource arguments must name a bound local; bind factory results first"));
                    }
                    let Kind::Resource(v) = self.expr(expr)? else {
                        return Err(error(expr, "expected an owned resource argument"));
                    };
                    if &v.ty != expected || !used.insert(v.id) {
                        return Err(error(
                            expr,
                            "resource type mismatch or aliased call arguments",
                        ));
                    }
                    self.pending_owned.push(v.clone());
                    owned.push(v);
                }
                Arg::Borrow(expected) => {
                    let (local, erased, implicit) = match expr {
                        Expr::Reference(r) => {
                            if r.mutability.is_none() {
                                return Err(error(r, "resource borrowing requires &mut"));
                            }
                            let Some(local) = name(&r.expr) else {
                                return Err(error(r, "resource borrowing needs a local owner"));
                            };
                            (local, *r.expr.clone(), false)
                        }
                        _ => {
                            let Some(local) = name(expr) else {
                                return Err(error(
                                    expr,
                                    "resource borrowing requires direct &mut owner",
                                ));
                            };
                            (local, expr.clone(), true)
                        }
                    };
                    let b = self
                        .lookup(&local, expr)?
                        .ok_or_else(|| error(expr, "expected resource owner"))?;
                    if implicit && b.owned {
                        return Err(error(expr,"owned resources require explicit &mut; only borrowed parameters reborrow implicitly"));
                    }
                    if !b.mutable {
                        return Err(error(expr, "mutable resource borrow needs a mutable owner"));
                    }
                    if &b.value.ty != expected || !used.insert(b.value.id) {
                        return Err(error(
                            expr,
                            "resource type mismatch or aliased call arguments",
                        ));
                    }
                    self.pending_borrows.push(b.value.clone());
                    borrowed.push(b.value.clone());
                    *expr = erased;
                }
            }
        }
        self.pending_borrows.truncate(borrowed_start);
        for value in &borrowed {
            let Some(binding) = self
                .bindings
                .iter()
                .find(|b| b.live && b.value.id == value.id)
            else {
                return Err(error(
                    call,
                    "borrowed call argument was consumed by another argument",
                ));
            };
            self.lookup(&binding.name, call)?;
            if owned.iter().any(|v| v.parent == Some(value.id)) {
                return Err(error(
                    call,
                    "call cannot borrow a parent while owning its child",
                ));
            }
        }
        self.pending_owned.truncate(owned_start);
        if function.diverges {
            self.reachable = false;
            return Ok(Kind::Unit);
        }
        if let Some(result) = &function.result {
            self.visible(result, call)?;
            let parent = if let Some(parent) = &self.model.resources[result].parent {
                let candidates: Vec<_> = borrowed.iter().filter(|v| &v.ty == parent).collect();
                if candidates.len() != 1 {
                    return Err(error(
                        call,
                        "child factory needs exactly one matching borrowed parent",
                    ));
                }
                Some(candidates[0].id)
            } else {
                None
            };
            Ok(Kind::Resource(self.value(result.clone(), parent)))
        } else {
            Ok(if function.unit {
                Kind::Unit
            } else {
                Kind::Scalar
            })
        }
    }
    fn native_call(&mut self, call: &mut syn::ExprCall) -> Result<Kind, syn::Error> {
        for expr in &mut call.args {
            self.scalar(expr)?;
        }
        if matches!(
            name(&call.func).as_deref(),
            Some("halt" | "jseg" | "icache_invalidate_delayed_and_jump")
        ) {
            self.reachable = false;
            return Ok(Kind::Unit);
        }
        Ok(Kind::Scalar)
    }
}
