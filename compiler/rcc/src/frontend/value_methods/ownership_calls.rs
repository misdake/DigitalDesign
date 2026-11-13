//! Resolve value-method calls that participate in resource ownership before erasure.
use super::{Methods, Receiver};
use std::collections::{HashMap, HashSet};
use syn::{
    parse_quote, visit::Visit, visit_mut::VisitMut, Expr, FnArg, Item, Pat, ReturnType, Type,
};

struct Calls<'a> {
    methods: &'a Methods,
    ownership: HashSet<String>,
    returns: HashMap<String, Type>,
    fields: HashMap<(String, String), Type>,
    scopes: Vec<HashMap<String, Type>>,
}

fn owner(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) => path.path.get_ident().map(ToString::to_string),
        Type::Reference(reference) => owner(&reference.elem),
        Type::Paren(paren) => owner(&paren.elem),
        _ => None,
    }
}

impl Calls<'_> {
    fn expression_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Path(path) => {
                let name = path.path.get_ident()?.to_string();
                self.scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(&name))
                    .cloned()
            }
            Expr::Struct(value) => {
                let path = &value.path;
                Some(parse_quote!(#path))
            }
            Expr::Reference(reference) => {
                let ty = self.expression_type(&reference.expr)?;
                Some(if reference.mutability.is_some() {
                    parse_quote!(&mut #ty)
                } else {
                    parse_quote!(&#ty)
                })
            }
            Expr::Paren(paren) => self.expression_type(&paren.expr),
            Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                let Type::Reference(reference) = self.expression_type(&unary.expr)? else {
                    return None;
                };
                Some(*reference.elem)
            }
            Expr::Field(field) => {
                let ty = self.expression_type(&field.base)?;
                let syn::Member::Named(name) = &field.member else {
                    return None;
                };
                self.fields.get(&(owner(&ty)?, name.to_string())).cloned()
            }
            Expr::Call(call) => {
                let Expr::Path(path) = call.func.as_ref() else {
                    return None;
                };
                self.returns
                    .get(&path.path.get_ident()?.to_string())
                    .cloned()
            }
            Expr::MethodCall(call) => {
                let ty = self.expression_type(&call.receiver)?;
                let method = self.methods.get(&(owner(&ty)?, call.method.to_string()))?;
                self.returns.get(&method.function.to_string()).cloned()
            }
            Expr::Index(index) => {
                let mut ty = self.expression_type(&index.expr)?;
                while let Type::Reference(reference) = ty {
                    ty = *reference.elem;
                }
                let Type::Path(path) = ty else { return None };
                let syn::PathArguments::AngleBracketed(arguments) =
                    &path.path.segments.last()?.arguments
                else {
                    return None;
                };
                let syn::GenericArgument::Type(element) = arguments.args.first()? else {
                    return None;
                };
                Some(element.clone())
            }
            Expr::If(branch) => branch.then_branch.stmts.last().and_then(|stmt| match stmt {
                syn::Stmt::Expr(value) => self.expression_type(value),
                _ => None,
            }),
            _ => None,
        }
    }
}

impl VisitMut for Calls<'_> {
    fn visit_item_fn_mut(&mut self, function: &mut syn::ItemFn) {
        let mut params = HashMap::new();
        for arg in &function.sig.inputs {
            if let FnArg::Typed(arg) = arg {
                if let Pat::Ident(name) = arg.pat.as_ref() {
                    params.insert(name.ident.to_string(), *arg.ty.clone());
                }
            }
        }
        self.scopes.push(params);
        self.visit_block_mut(&mut function.block);
        self.scopes.pop();
    }
    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        self.scopes.push(HashMap::new());
        syn::visit_mut::visit_block_mut(self, block);
        self.scopes.pop();
    }
    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        syn::visit_mut::visit_local_mut(self, local);
        let (pat, ty) = match &local.pat {
            Pat::Type(annotation) => (annotation.pat.as_ref(), Some(*annotation.ty.clone())),
            pat => (
                pat,
                local
                    .init
                    .as_ref()
                    .and_then(|(_, value)| self.expression_type(value)),
            ),
        };
        if let (Pat::Ident(name), Some(ty)) = (pat, ty) {
            self.scopes
                .last_mut()
                .unwrap()
                .insert(name.ident.to_string(), ty);
        }
    }
    fn visit_macro_mut(&mut self, mac: &mut syn::Macro) {
        super::rewrite_assertion(mac, |expr| self.visit_expr_mut(expr));
    }
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        if let Expr::MethodCall(call) = expr {
            let resolved = self
                .expression_type(&call.receiver)
                .and_then(|ty| self.methods.get(&(owner(&ty)?, call.method.to_string())));
            if let Some(method) =
                resolved.filter(|method| self.ownership.contains(&method.function.to_string()))
            {
                let mut receiver = *call.receiver.clone();
                if let Some(mut ty) = self.expression_type(&receiver) {
                    while let Type::Reference(reference) = ty {
                        receiver = parse_quote!(*#receiver);
                        ty = *reference.elem;
                    }
                }
                let first: Expr = match method.receiver {
                    Receiver::Borrow { mutable: true } => {
                        parse_quote!(__rcc_value_view(&mut #receiver))
                    }
                    Receiver::Borrow { mutable: false } => {
                        parse_quote!(__rcc_value_view(&#receiver))
                    }
                    Receiver::Value => receiver.clone(),
                    Receiver::Associated => return,
                };
                let function = &method.function;
                let args = &call.args;
                *expr = parse_quote!(#function(#first, #args));
            }
        }
        syn::visit_mut::visit_expr_mut(self, expr);
    }
}

pub(in crate::frontend) fn prepare(
    files: &mut [syn::File],
    methods: &Methods,
) -> Result<(), (usize, syn::Error)> {
    if methods.is_empty() {
        return Ok(());
    }
    let mut resources = super::super::resource::aggregate_names(files)?;
    for file in files.iter() {
        for item in &file.items {
            if let Item::Struct(value) = item {
                if value.attrs.iter().any(|a| a.path.is_ident("rcc_resource")) {
                    resources.insert(value.ident.to_string());
                }
            }
        }
    }
    struct ResourceTypes<'a>(&'a HashSet<String>, bool);
    impl<'ast> Visit<'ast> for ResourceTypes<'_> {
        fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
            self.1 |= path
                .path
                .segments
                .iter()
                .any(|s| self.0.contains(&s.ident.to_string()));
            syn::visit::visit_type_path(self, path);
        }
    }
    let mut calls = Calls {
        methods,
        ownership: HashSet::new(),
        returns: HashMap::new(),
        fields: HashMap::new(),
        scopes: vec![],
    };
    for file in files.iter() {
        for item in &file.items {
            match item {
                Item::Fn(function) => {
                    if let ReturnType::Type(_, ty) = &function.sig.output {
                        calls
                            .returns
                            .insert(function.sig.ident.to_string(), *ty.clone());
                    }
                    let mut types = ResourceTypes(&resources, false);
                    types.visit_signature(&function.sig);
                    if types.1 {
                        calls.ownership.insert(function.sig.ident.to_string());
                    }
                }
                Item::Struct(value) => {
                    for field in &value.fields {
                        if let Some(name) = &field.ident {
                            calls.fields.insert(
                                (value.ident.to_string(), name.to_string()),
                                field.ty.clone(),
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for file in files {
        calls.visit_file_mut(file);
    }
    Ok(())
}
