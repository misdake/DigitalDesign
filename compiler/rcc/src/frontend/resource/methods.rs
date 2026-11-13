//! Narrow inherent-resource sugar, before ownership checking and ABI erasure.
use super::{error, name, type_name, Checker, Model};
use syn::{parse_quote, visit::Visit, visit_mut::VisitMut, Expr, FnArg, Item, Type};

const PREFIX: &str = "__rcc_method_";

fn reserved(ident: &syn::Ident) -> bool {
    ident
        .to_string()
        .trim_start_matches("r#")
        .starts_with(PREFIX)
}

#[derive(Clone, Copy)]
enum Receiver {
    Associated,
    Value,
    Borrow { mutable: bool },
}

pub(super) struct Method {
    function: syn::Ident,
    receiver: Receiver,
}

pub(super) fn prepare(
    files: &mut [syn::File],
    model: &mut Model,
) -> Result<(), (usize, syn::Error)> {
    // Reserve the complete source namespace, including identifiers in attribute
    // token streams (not visited by syn's identifier walker). No source can name
    // an erased method entry or redirect a cleanup attribute to one.
    struct Reserved(Option<syn::Error>);
    impl<'ast> Visit<'ast> for Reserved {
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            if reserved(ident) {
                self.0 = Some(error(ident, "reserved resource method name"));
            }
        }
        fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
            fn check(tokens: proc_macro2::TokenStream) -> Option<syn::Error> {
                for token in tokens {
                    match token {
                        proc_macro2::TokenTree::Ident(i) if reserved(&i) => {
                            return Some(error(&i, "reserved resource method name"));
                        }
                        proc_macro2::TokenTree::Group(g) => {
                            if let Some(e) = check(g.stream()) {
                                return Some(e);
                            }
                        }
                        _ => {}
                    }
                }
                None
            }
            if let Some(e) = check(attr.tokens.clone()) {
                self.0 = Some(e);
            }
            syn::visit::visit_attribute(self, attr);
        }
    }
    for (fi, file) in files.iter().enumerate() {
        let mut reserved = Reserved(None);
        reserved.visit_file(file);
        if let Some(e) = reserved.0 {
            return Err((fi, e));
        }
    }
    for (fi, file) in files.iter_mut().enumerate() {
        let mut functions = Vec::new();
        for item in &file.items {
            let Item::Impl(block) = item else { continue };
            let result = (|| {
                if block.trait_.is_some()
                    || block.unsafety.is_some()
                    || block.defaultness.is_some()
                    || !block.generics.params.is_empty()
                    || block.generics.where_clause.is_some()
                {
                    return Err(error(
                        block,
                        "only nongeneric inherent resource impls are supported",
                    ));
                }
                for attr in &block.attrs {
                    if !attr.path.is_ident("doc") && !attr.path.is_ident("allow") {
                        return Err(error(attr, "unsupported resource impl attribute"));
                    }
                }
                let Type::Path(path) = block.self_ty.as_ref() else {
                    return Err(error(&block.self_ty, "impl needs a plain resource type"));
                };
                if path.qself.is_some()
                    || path.path.leading_colon.is_some()
                    || path.path.segments.len() != 1
                    || !matches!(path.path.segments[0].arguments, syn::PathArguments::None)
                {
                    return Err(error(path, "impl needs a plain resource type"));
                }
                let ty = path.path.segments[0].ident.clone();
                let Some(resource) = model.resources.get(&ty.to_string()) else {
                    return Err(error(&ty, "impl sugar is limited to lexical resources"));
                };
                if resource.file != fi {
                    return Err(error(&ty, "resource impl must be in its definition file"));
                }
                for member in &block.items {
                    let syn::ImplItem::Method(method) = member else {
                        return Err(error(member, "resource impl supports methods only"));
                    };
                    if method.defaultness.is_some()
                        || !method.sig.generics.params.is_empty()
                        || method.sig.generics.where_clause.is_some()
                    {
                        return Err(error(
                            method,
                            "generic/default resource methods are not supported",
                        ));
                    }
                    let key = (ty.to_string(), method.sig.ident.to_string());
                    if model.methods.contains_key(&key) {
                        return Err(error(
                            method,
                            "duplicate resource method; overloading is not supported",
                        ));
                    }
                    let index = model.methods.len();
                    let function =
                        syn::Ident::new(&format!("{PREFIX}{index}"), method.sig.ident.span());
                    let local =
                        syn::Ident::new(&format!("{PREFIX}self_{index}"), method.sig.ident.span());
                    let mut sig = method.sig.clone();
                    sig.ident = function.clone();
                    let receiver = match method.sig.inputs.first() {
                        Some(FnArg::Receiver(receiver)) => {
                            if receiver
                                .reference
                                .as_ref()
                                .is_some_and(|(_, l)| l.is_some())
                                || (receiver.reference.is_some()
                                    && receiver.mutability.is_none()
                                    && !model.checked)
                                || !receiver.attrs.is_empty()
                            {
                                return Err(error(receiver, "resource receivers support only self, mut self, &self or &mut self"));
                            }
                            if receiver.reference.is_some() {
                                let mutable = receiver.mutability.is_some();
                                sig.inputs[0] = if mutable {
                                    parse_quote!(#local: &mut #ty)
                                } else {
                                    parse_quote!(#local: &#ty)
                                };
                                Receiver::Borrow { mutable }
                            } else {
                                sig.inputs[0] = if receiver.mutability.is_some() {
                                    parse_quote!(mut #local: #ty)
                                } else {
                                    parse_quote!(#local: #ty)
                                };
                                Receiver::Value
                            }
                        }
                        _ => {
                            if !matches!(&sig.output, syn::ReturnType::Type(_, result)
                                if type_name(result).is_some_and(|n| n == "Self" || n == key.0))
                            {
                                return Err(error(method, "resource associated factories must return their defining resource"));
                            }
                            Receiver::Associated
                        }
                    };
                    let mut body = method.block.clone();
                    struct Rename<'a> {
                        local: &'a syn::Ident,
                        owner: Option<&'a syn::Ident>,
                        failure: Option<syn::Error>,
                    }
                    impl VisitMut for Rename<'_> {
                        fn visit_path_mut(&mut self, path: &mut syn::Path) {
                            if let Some(owner) = self.owner {
                                if let Some(first) = path.segments.first_mut() {
                                    if first.ident == "Self" {
                                        first.ident = owner.clone();
                                    }
                                }
                            }
                            syn::visit_mut::visit_path_mut(self, path);
                        }
                        fn visit_expr_path_mut(&mut self, path: &mut syn::ExprPath) {
                            if path.path.is_ident("self") {
                                let local = self.local;
                                path.path = parse_quote!(#local);
                            } else {
                                syn::visit_mut::visit_expr_path_mut(self, path);
                            }
                        }
                        fn visit_ident_mut(&mut self, ident: &mut syn::Ident) {
                            if ident == "Self" || ident == "self" {
                                self.failure = Some(error(ident, "Self types and qualified self paths are not supported in resource methods"));
                            }
                        }
                    }
                    let mut rename = Rename {
                        local: &local,
                        owner: (matches!(receiver, Receiver::Associated)
                            || model.aggregates.contains_key(&ty.to_string()))
                        .then_some(&ty),
                        failure: None,
                    };
                    rename.visit_block_mut(&mut body);
                    rename.visit_signature_mut(&mut sig);
                    if let Some(e) = rename.failure {
                        return Err(e);
                    }
                    functions.push(Item::Fn(syn::ItemFn {
                        attrs: method.attrs.clone(),
                        vis: method.vis.clone(),
                        sig,
                        block: Box::new(body),
                    }));
                    model.methods.insert(key, Method { function, receiver });
                }
                Ok(())
            })();
            result.map_err(|e| (fi, e))?;
        }
        file.items.retain(|item| !matches!(item, Item::Impl(_)));
        file.items.extend(functions);
    }
    // Resolve only the narrow Type::factory spelling, then reuse ordinary
    // function visibility, parent-origin, affine and function-pointer checks.
    struct Associated<'a> {
        model: &'a Model,
        failure: Option<syn::Error>,
    }
    impl VisitMut for Associated<'_> {
        fn visit_expr_path_mut(&mut self, path: &mut syn::ExprPath) {
            if path.path.segments.len() == 2 {
                let key = (
                    path.path.segments[0].ident.to_string(),
                    path.path.segments[1].ident.to_string(),
                );
                if let Some(Method {
                    function,
                    receiver: Receiver::Associated,
                }) = self.model.methods.get(&key)
                {
                    if path.qself.is_some()
                        || path.path.leading_colon.is_some()
                        || path.path.segments.iter().any(|s| !s.arguments.is_empty())
                    {
                        self.failure = Some(error(path, "resource factories require plain Type::function paths without type arguments"));
                        return;
                    }
                    path.path = parse_quote!(#function);
                }
            }
            syn::visit_mut::visit_expr_path_mut(self, path);
        }
    }
    for (fi, file) in files.iter_mut().enumerate() {
        let mut associated = Associated {
            model,
            failure: None,
        };
        associated.visit_file_mut(file);
        if let Some(e) = associated.failure {
            return Err((fi, e));
        }
    }
    Ok(())
}

impl Checker<'_> {
    fn resource_receiver(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Field(e) => self.resource_receiver(&e.base),
            Expr::Paren(e) => self.resource_receiver(&e.expr),
            Expr::Group(e) => self.resource_receiver(&e.expr),
            Expr::Reference(e) => self.resource_receiver(&e.expr),
            Expr::Unary(e) => self.resource_receiver(&e.expr),
            Expr::MethodCall(e) => self.resource_receiver(&e.receiver),
            Expr::Call(e) => name(&e.func).is_some_and(|n| {
                self.model
                    .functions
                    .get(&n)
                    .is_some_and(|f| f.result.is_some())
            }),
            Expr::Struct(e) => e
                .path
                .segments
                .last()
                .is_some_and(|s| self.model.resources.contains_key(&s.ident.to_string())),
            _ => name(expr).is_some_and(|n| self.bindings.iter().any(|b| b.name == n)),
        }
    }

    pub(super) fn method(
        &self,
        call: &syn::ExprMethodCall,
    ) -> Result<Option<syn::ExprCall>, syn::Error> {
        let method_name = call.method.to_string();
        let Some(local) = name(&call.receiver) else {
            if self.resource_receiver(&call.receiver) {
                return Err(error(call, "resource method receiver must be a simple bound local; fields, temporaries and chains are not supported"));
            }
            return Ok(None);
        };
        if !self.bindings.iter().any(|b| b.name == local) {
            return Ok(None); // Native builtin methods retain the native path.
        }
        if !matches!(call.receiver.as_ref(), Expr::Field(_))
            && !matches!(call.receiver.as_ref(), Expr::Path(p)
            if p.qself.is_none() && p.path.leading_colon.is_none()
                && matches!(p.path.segments[0].arguments, syn::PathArguments::None))
        {
            return Err(error(
                call,
                "resource method receiver must be an unqualified local name",
            ));
        }
        let binding = self.lookup(&local, call)?.unwrap();
        let key = (binding.value.ty, method_name);
        let Some(method) = self.model.methods.get(&key) else {
            return Err(error(call, "unknown resource method"));
        };
        if call.turbofish.is_some() {
            return Err(error(
                call,
                "resource method type arguments are not supported",
            ));
        }
        let receiver = &call.receiver;
        let first = match method.receiver {
            Receiver::Borrow { mutable: true } => parse_quote!(&mut #receiver),
            Receiver::Borrow { mutable: false } => parse_quote!(&#receiver),
            Receiver::Value => *receiver.clone(),
            Receiver::Associated => {
                return Err(error(
                    call,
                    "resource associated factories require Type::function calls",
                ))
            }
        };
        let mut args = syn::punctuated::Punctuated::new();
        args.push(first);
        args.extend(call.args.iter().cloned());
        let function = &method.function;
        Ok(Some(syn::ExprCall {
            attrs: call.attrs.clone(),
            func: Box::new(parse_quote!(#function)),
            paren_token: call.paren_token,
            args,
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::frontend::parse_source;

    const RESOURCE: &str =
        "#[rcc_resource(cleanup=release)] struct Pass{word:u16}fn release(p:Pass){}fn main(){}";

    #[test]
    fn resource_associated_factories_also_lower_with_legacy_cleanup() {
        parse_source(&format!("{RESOURCE}impl Pass{{fn new(word:u16)->Self{{Self{{word:word}}}}fn relay()->Pass{{Self::new(1)}}}}fn run(){{let p=Pass::relay();release(p);}}"))
            .unwrap();
    }

    #[test]
    fn resource_methods_diagnose_unsupported_declarations_and_identity_errors() {
        for (code, message) in [
            (
                "impl Pass{fn draw(&self){}}",
                "only self, mut self, &self or &mut self",
            ),
            ("impl Pass{fn draw<'a>(&'a mut self){}}", "generic/default"),
            (
                "impl Pass{fn new(){}}",
                "factories must return their defining resource",
            ),
            (
                "impl Pass{fn new<T>()->Self{Self{word:0}}}",
                "generic/default",
            ),
            (
                "impl Pass{fn new()->Self{Self{word:0}}fn new(self){}}",
                "duplicate resource method",
            ),
            (
                "impl Pass{fn new()->Self{Self{word:0}}}fn make(){let p=Pass::new::<u16>();}",
                "plain Type::function",
            ),
            (
                "impl Pass{fn new()->Self{Self{word:0}}}fn make(){let p=Pass::new();let q=p.new();}",
                "Type::function calls",
            ),
            (
                "impl Pass{fn new()->Self{Self{word:0}}}fn make(){let p=Pass::new();release(p);release(p);}",
                "moved or consumed",
            ),
            ("impl<T> Pass{fn draw(self){}}", "nongeneric inherent"),
            (
                "impl Trait for Pass{fn draw(self){}}",
                "nongeneric inherent",
            ),
            ("impl Pass{fn draw<T>(&mut self){}}", "generic/default"),
            (
                "impl Pass{fn draw(&mut self) where u16: Trait{}}",
                "generic/default",
            ),
            ("impl Pass{const WORD:u16=0;}", "methods only"),
            (
                "impl Pass{fn draw(self){}fn draw(&mut self){}}",
                "duplicate resource method",
            ),
            (
                "impl Pass{fn draw(self){}}impl Pass{fn draw(self){}}",
                "duplicate resource method",
            ),
            (
                "struct Native{word:u16}impl Native{fn draw(self){}}",
                "limited to lexical resources",
            ),
            ("impl Pass{fn new(self)->Self{self}}", "Self types"),
            (
                "impl Pass{fn draw(&mut self){self.word=1;}}",
                "reassignment",
            ),
            (
                "impl Pass{fn finish(self){resource_disarm(self);resource_disarm(self);}}",
                "moved or consumed",
            ),
            (
                "impl Pass{fn finish(self){release(self);release(self);}}",
                "moved or consumed",
            ),
            (
                "impl Pass{fn finish(self){resource_disarm(self);let n=self.word;}}",
                "moved or consumed",
            ),
            (
                "fn __rcc_method_0(p:Pass){}",
                "reserved resource method name",
            ),
            (
                "fn r#__rcc_method_0(p:Pass){}",
                "reserved resource method name",
            ),
            (
                "#[allow(__rcc_method_0)] impl Pass{fn draw(self){}}",
                "reserved resource method name",
            ),
        ] {
            let source = format!("{RESOURCE}{code}");
            let e = match parse_source(&source) {
                Ok(_) => panic!("accepted {code}"),
                Err(e) => e.to_string(),
            };
            assert!(e.contains(message), "{code}: expected {message}, got {e}");
        }
        let source = RESOURCE.replace("cleanup=release", "cleanup=__rcc_method_0");
        let e = match parse_source(&format!("{source}impl Pass{{fn finish(self){{}}}}")) {
            Ok(_) => panic!("accepted a generated entry as cleanup"),
            Err(e) => e.to_string(),
        };
        assert!(e.contains("reserved resource method name"), "{e}");
    }
}
