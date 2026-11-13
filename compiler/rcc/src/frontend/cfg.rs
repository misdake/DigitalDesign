//! A deliberately closed conditional-compilation vocabulary.
use super::{err, Item, Stmt};
use quote::ToTokens;
use std::collections::HashSet;
use syn::{visit::Visit, visit_mut::VisitMut, Attribute, Expr, Meta, NestedMeta};

fn condition(attr: &Attribute) -> Result<bool, syn::Error> {
    let Meta::List(list) = attr.parse_meta()? else {
        return Err(err(attr, "expected cfg(gpu_debug) or cfg(not(gpu_debug))"));
    };
    if list.nested.len() != 1 {
        return Err(err(attr, "only gpu_debug and not(gpu_debug) are supported"));
    }
    match &list.nested[0] {
        NestedMeta::Meta(Meta::Path(path)) if path.is_ident("gpu_debug") => Ok(true),
        NestedMeta::Meta(Meta::List(not))
            if not.path.is_ident("not")
                && not.nested.len() == 1
                && matches!(&not.nested[0],NestedMeta::Meta(Meta::Path(path)) if path.is_ident("gpu_debug")) =>
        {
            Ok(false)
        }
        _ => Err(err(attr, "only gpu_debug and not(gpu_debug) are supported")),
    }
}
fn reject_raw_condition(attr: &Attribute) -> Result<(), syn::Error> {
    if attr
        .path
        .segments
        .iter()
        .any(|p| matches!(p.ident.to_string().as_str(), "r#cfg" | "r#cfg_attr"))
    {
        Err(err(attr, "raw cfg/cfg_attr spellings are not supported"))
    } else {
        Ok(())
    }
}
pub(super) fn module_attributes(attrs: &[Attribute]) -> Result<(), syn::Error> {
    for attr in attrs {
        reject_raw_condition(attr)?;
        if attr.path.is_ident("cfg_attr") {
            return Err(err(attr, "cfg_attr is not supported"));
        }
        if attr.path.is_ident("cfg") {
            condition(attr)?;
            return Err(err(attr, "cfg is not allowed on module declarations"));
        }
    }
    Ok(())
}
fn has_cfg(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path.is_ident("cfg"))
}
struct Validate<'a> {
    cleanup: &'a HashSet<String>,
    protected: bool,
    conditional: bool,
    error: Option<syn::Error>,
}
impl Validate<'_> {
    fn fail(&mut self, node: &impl syn::spanned::Spanned, message: &str) {
        if self.error.is_none() {
            self.error = Some(err(node, message));
        }
    }
    fn attributes(&mut self, attrs: &[Attribute], allowed: bool) {
        for attr in attrs {
            if let Err(error) = reject_raw_condition(attr) {
                if self.error.is_none() {
                    self.error = Some(error)
                }
                continue;
            }
            if attr.path.is_ident("cfg_attr") {
                self.fail(attr, "cfg_attr is not supported");
            } else if attr.path.is_ident("cfg") {
                if let Err(e) = condition(attr) {
                    if self.error.is_none() {
                        self.error = Some(e);
                    }
                }
                if !allowed || self.protected {
                    self.fail(
                        attr,
                        "cfg is not allowed on this resource, contract or syntax position",
                    );
                }
            } else if self.conditional
                && (attr.path.is_ident("rcc_resource")
                    || attr.path.is_ident("rcc_resource_library"))
            {
                self.fail(attr, "resource and library contracts cannot be conditional");
            }
        }
    }
    fn scope(&mut self, attrs: &[Attribute], protected: bool, f: impl FnOnce(&mut Self)) {
        let previous = (self.protected, self.conditional);
        self.protected |= protected;
        self.conditional |= has_cfg(attrs);
        self.attributes(attrs, true);
        f(self);
        (self.protected, self.conditional) = previous;
    }
}
impl<'ast> Visit<'ast> for Validate<'_> {
    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        self.attributes(std::slice::from_ref(attr), false);
    }
    fn visit_ident(&mut self, id: &'ast syn::Ident) {
        let name = id.to_string();
        let name = name.trim_start_matches("r#");
        if self.conditional && name == "resource_disarm" {
            self.fail(id, "resource_disarm cannot be hidden in conditional source");
        }
    }
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        let protected = self.cleanup.contains(&node.sig.ident.to_string());
        if protected && self.conditional {
            self.fail(node, "cleanup contracts cannot be conditional");
        }
        self.scope(&node.attrs, protected, |s| {
            s.visit_signature(&node.sig);
            s.visit_block(&node.block);
        });
    }
    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        self.scope(&node.attrs, false, |s| {
            s.visit_ident(&node.ident);
            s.visit_type(&node.ty);
            s.visit_expr(&node.expr);
        });
    }
    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        self.scope(&node.attrs, false, |s| {
            s.visit_ident(&node.ident);
            s.visit_type(&node.ty);
            s.visit_expr(&node.expr);
        });
    }
    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        let protected = node.attrs.iter().any(|a| a.path.is_ident("rcc_resource"));
        self.scope(&node.attrs, protected, |s| {
            s.visit_ident(&node.ident);
            s.visit_generics(&node.generics);
            s.visit_fields(&node.fields);
        });
    }
    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        self.scope(&node.attrs, false, |s| {
            s.visit_ident(&node.ident);
            s.visit_generics(&node.generics);
            for v in &node.variants {
                s.visit_variant(v);
            }
        });
    }
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        let previous = self.protected;
        let drop = node
            .trait_
            .as_ref()
            .is_some_and(|(_, path, _)| path.is_ident("Drop"));
        if drop && self.conditional {
            self.fail(node, "Drop contracts cannot be conditional");
        }
        self.protected |= drop;
        syn::visit::visit_item_impl(self, node);
        self.protected = previous;
    }
    fn visit_stmt(&mut self, node: &'ast Stmt) {
        if let Stmt::Expr(Expr::Block(block)) | Stmt::Semi(Expr::Block(block), _) = node {
            self.scope(&block.attrs, false, |s| s.visit_block(&block.block));
        } else {
            syn::visit::visit_stmt(self, node);
        }
    }
    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if node.path.is_ident("fix") {
            if let Err(error) = super::fixed_literals::value(node) {
                if self.error.is_none() {
                    self.error = Some(error);
                }
            }
            return;
        }
        if node.path.is_ident("include_bytes") {
            if syn::parse2::<syn::LitStr>(node.tokens.clone()).is_err() {
                self.fail(node, "include_bytes requires one literal path");
            }
            return;
        }
        if node.path.is_ident("assert") || node.path.is_ident("debug_assert") {
            match syn::parse2::<Expr>(node.tokens.clone()) {
                Ok(expression) => {
                    let mut nested = Validate {
                        cleanup: self.cleanup,
                        protected: self.protected,
                        conditional: self.conditional || node.path.is_ident("debug_assert"),
                        error: None,
                    };
                    nested.visit_expr(&expression);
                    if self.error.is_none() {
                        self.error = nested.error;
                    }
                }
                Err(_) => self.fail(node, "assert requires one boolean expression"),
            }
            return;
        }
        self.fail(node, "macros are not supported, including inactive source");
    }
}
fn item_attrs(item: &Item) -> Option<&[Attribute]> {
    match item {
        Item::Fn(n) => Some(&n.attrs),
        Item::Static(n) => Some(&n.attrs),
        Item::Const(n) => Some(&n.attrs),
        Item::Struct(n) => Some(&n.attrs),
        Item::Enum(n) => Some(&n.attrs),
        _ => None,
    }
}
fn item_attrs_mut(item: &mut Item) -> Option<&mut Vec<Attribute>> {
    match item {
        Item::Fn(n) => Some(&mut n.attrs),
        Item::Static(n) => Some(&mut n.attrs),
        Item::Const(n) => Some(&mut n.attrs),
        Item::Struct(n) => Some(&mut n.attrs),
        Item::Enum(n) => Some(&mut n.attrs),
        _ => None,
    }
}
struct Prune(bool);
impl Prune {
    fn enabled(&self, attrs: &[Attribute]) -> bool {
        attrs
            .iter()
            .filter(|a| a.path.is_ident("cfg"))
            .all(|a| condition(a).expect("validated cfg") == self.0)
    }
    fn item(&self, item: &Item) -> bool {
        item_attrs(item).is_none_or(|a| self.enabled(a))
    }
}
impl VisitMut for Prune {
    fn visit_file_mut(&mut self, file: &mut syn::File) {
        file.items.retain(|i| self.item(i));
        syn::visit_mut::visit_file_mut(self, file);
    }
    fn visit_item_mut(&mut self, item: &mut Item) {
        if let Some(attrs) = item_attrs_mut(item) {
            attrs.retain(|a| !a.path.is_ident("cfg"));
        }
        syn::visit_mut::visit_item_mut(self, item);
    }
    fn visit_block_mut(&mut self, block: &mut syn::Block) {
        block.stmts.retain(|stmt| match stmt {
            Stmt::Item(item) => self.item(item),
            Stmt::Expr(Expr::Block(b)) | Stmt::Semi(Expr::Block(b), _) => self.enabled(&b.attrs),
            _ => true,
        });
        for stmt in &mut block.stmts {
            if let Stmt::Expr(Expr::Block(b)) | Stmt::Semi(Expr::Block(b), _) = stmt {
                b.attrs.retain(|a| !a.path.is_ident("cfg"));
            }
        }
        syn::visit_mut::visit_block_mut(self, block);
    }
}
pub(super) fn prepare(files: &mut [syn::File], enabled: bool) -> Result<(), (usize, syn::Error)> {
    let markers = super::resource::drop_syntax::markers(files)?;
    let cleanup = markers.values().filter_map(|m| m.cleanup.clone()).collect();
    for (fi, file) in files.iter().enumerate() {
        for (prefix, message) in [
            ("__rcc_drop_", "reserved Drop lowering name"),
            ("__rcc_method_", "reserved resource method name"),
            ("__rcc_value_", "reserved value method name"),
            ("__rcc_container_", "reserved ownership container name"),
        ] {
            if let Some(name) = super::value_methods::reserved(file.to_token_stream(), prefix) {
                return Err((fi, err(&name, message)));
            }
        }
        let mut check = Validate {
            cleanup: &cleanup,
            protected: false,
            conditional: false,
            error: None,
        };
        check.visit_file(file);
        if let Some(e) = check.error {
            return Err((fi, e));
        }
    }
    for file in files {
        Prune(enabled).visit_file_mut(file);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpu_debug_low_level_profiles_select_before_layout() {
        let source="#[cfg(gpu_debug)]static EXTRA:Buf<u16,5>=Buf::new([1;5]); #[cfg(not(gpu_debug))]fn release(){} fn main(){}";
        let default = super::super::parse_source(source).unwrap();
        assert!(!default.debug.gpu_debug);
        assert_eq!(default.debug.static_data_end, 0);
        for enabled in [false, true] {
            let options = crate::RccOptions {
                gpu_debug: enabled,
                ..crate::RccOptions::default()
            };
            let program = super::super::parse_source_with_config(source, &options).unwrap();
            assert_eq!(program.debug.gpu_debug, enabled);
            assert_eq!(program.debug.static_data_end, if enabled { 5 } else { 0 });
            assert_eq!(program.funcs.iter().any(|f| f.name == "release"), !enabled);
        }
    }
}
