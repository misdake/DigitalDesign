//! Project target-native storage operations while preserving resource bodies/borrows.
use std::collections::HashSet;
use syn::{parse_quote, visit_mut::VisitMut, Expr, FnArg, Item, Pat, ReturnType, Type};

fn plain(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) if p.path.segments.len() == 1 => Some(p.path.segments[0].ident.to_string()),
        _ => None,
    }
}
fn view(ty: &Type) -> bool {
    plain(ty).is_some_and(|n| matches!(n.as_str(), "Array" | "Buf"))
}

pub(super) struct Native {
    copies: HashSet<String>,
    views: HashSet<String>,
    statics: HashSet<String>,
    in_static: bool,
}
impl Native {
    pub(super) fn new(files: &[syn::File], resources: &HashSet<String>) -> Self {
        let mut noncopy = resources.clone();
        for item in files.iter().flat_map(|f| &f.items) {
            if let Item::Impl(i) = item {
                if i.trait_
                    .as_ref()
                    .is_some_and(|(_, p, _)| p.is_ident("Drop"))
                {
                    if let Some(name) = plain(&i.self_ty) {
                        noncopy.insert(name);
                    }
                }
            }
        }
        let mut copies: HashSet<String> = [
            "u16", "i16", "bool", "Ptr", "Array", "fix32", "vec2", "vec3", "vec4",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        loop {
            let before = copies.len();
            for item in files.iter().flat_map(|f| &f.items) {
                if let Item::Struct(s) = item {
                    if !noncopy.contains(&s.ident.to_string())
                        && s.fields
                            .iter()
                            .all(|f| plain(&f.ty).is_some_and(|n| copies.contains(&n)))
                    {
                        copies.insert(s.ident.to_string());
                    }
                }
            }
            if before == copies.len() {
                break;
            }
        }
        let mut views: HashSet<String> = ["view_of"].into_iter().map(str::to_owned).collect();
        for item in files.iter().flat_map(|f| &f.items) {
            if let Item::Fn(f) = item {
                if matches!(&f.sig.output,ReturnType::Type(_,ty) if view(ty)) {
                    views.insert(f.sig.ident.to_string());
                }
            }
        }
        let statics = files
            .iter()
            .flat_map(|f| &f.items)
            .filter_map(|i| match i {
                Item::Static(s) => Some(s.ident.to_string()),
                _ => None,
            })
            .collect();
        Self {
            copies,
            views,
            statics,
            in_static: false,
        }
    }
    fn view_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Call(c) => {
                matches!(c.func.as_ref(),Expr::Path(p) if p.path.get_ident().is_some_and(|n|self.views.contains(&n.to_string())))
            }
            Expr::MethodCall(c) => matches!(
                c.method.to_string().as_str(),
                "as_array" | "as_u16_array" | "as_i16_array"
            ),
            Expr::Unsafe(e) => e
                .block
                .stmts
                .last()
                .is_some_and(|s| matches!(s,syn::Stmt::Expr(e) if self.view_expr(e))),
            _ => false,
        }
    }
    fn static_place(&self, e: &Expr) -> bool {
        match e {
            Expr::Path(p) => p
                .path
                .get_ident()
                .is_some_and(|i| self.statics.contains(&i.to_string())),
            Expr::Field(f) => self.static_place(&f.base),
            Expr::Index(i) => self.static_place(&i.expr),
            Expr::Paren(p) => self.static_place(&p.expr),
            _ => false,
        }
    }
}
impl VisitMut for Native {
    fn visit_item_mut(&mut self, item: &mut Item) {
        match item {
            Item::Struct(s) if self.copies.contains(&s.ident.to_string()) => {
                // Ordinary target aggregates are copied by value; lexical resources never are.
                s.attrs.push(parse_quote!(#[derive(Copy,Clone)]));
            }
            Item::Static(s) => {
                s.mutability = Some(Default::default());
            }
            _ => {}
        }
        syn::visit_mut::visit_item_mut(self, item);
    }
    fn visit_signature_mut(&mut self, sig: &mut syn::Signature) {
        for arg in &mut sig.inputs {
            if let FnArg::Typed(p) = arg {
                if view(&p.ty) {
                    if let Pat::Ident(i) = p.pat.as_mut() {
                        i.mutability = Some(Default::default());
                    }
                }
            }
        }
        syn::visit_mut::visit_signature_mut(self, sig);
    }
    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        let native = local.init.as_ref().is_some_and(|(_, e)| self.view_expr(e))
            || matches!(&local.pat,Pat::Type(t) if view(&t.ty));
        if native {
            let pattern = match &mut local.pat {
                Pat::Type(t) => t.pat.as_mut(),
                p => p,
            };
            if let Pat::Ident(i) = pattern {
                i.mutability = Some(Default::default());
            }
        }
        syn::visit_mut::visit_local_mut(self, local);
    }
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        let needs_static = match expr {
            Expr::Assign(a) => self.static_place(&a.left),
            Expr::AssignOp(a) => self.static_place(&a.left),
            Expr::Reference(r) => self.static_place(&r.expr),
            Expr::MethodCall(c) => self.static_place(&c.receiver),
            _ => self.static_place(expr),
        };
        let old = self.in_static;
        self.in_static |= needs_static;
        syn::visit_mut::visit_expr_mut(self, expr);
        if let Expr::Cast(c) = expr {
            if plain(&c.ty).as_deref() == Some("Ptr") {
                let value = &c.expr;
                *expr = parse_quote!(crate::dsl_rt::Ptr((#value) as u16));
            }
        }
        if let Expr::Call(c) = expr {
            if matches!(c.func.as_ref(),Expr::Path(p) if p.path.is_ident("dev_send")||p.path.is_ident("dev_recv"))
            {
                for argument in c.args.iter_mut().take(2) {
                    let old = argument.clone();
                    *argument = parse_quote!((#old) as u8);
                }
            }
        }
        self.in_static = old;
        if needs_static && !old {
            let body = expr.clone();
            *expr = parse_quote!((unsafe{#body}));
        }
    }
    fn visit_expr_unsafe_mut(&mut self, expr: &mut syn::ExprUnsafe) {
        let old = self.in_static;
        self.in_static = true;
        syn::visit_mut::visit_expr_unsafe_mut(self, expr);
        self.in_static = old;
    }
}
