//! Check-only Rust ownership gate. Target cleanup is still lowered by RCC.
use super::resource::drop_syntax::{markers, resource_ident, Marker};
use proc_macro2::{Delimiter, Spacing, Span, TokenStream, TokenTree};
use quote::{quote, ToTokens};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
};
use syn::{parse::Parser, parse_quote, visit_mut::VisitMut, FnArg, Item, ReturnType, Type};
mod native;

/// An explicit tool and persistent audit directory, never an implicit best effort.
pub struct RustcCheck {
    pub executable: PathBuf,
    pub directory: PathBuf,
}
impl RustcCheck {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            executable: std::env::var_os("RUSTC")
                .map(PathBuf::from)
                .unwrap_or_else(|| "rustc".into()),
            directory: directory.into(),
        }
    }
}
fn resource_type(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) if p.path.segments.len() == 1 => Some(p.path.segments[0].ident.to_string()),
        _ => None,
    }
}
struct Projection<'a> {
    resources: &'a BTreeMap<String, Marker>,
    receiver: Option<String>,
}
impl Projection<'_> {
    fn child(&self, name: &str) -> bool {
        self.resources.get(name).is_some_and(|r| r.parent.is_some())
    }
}
impl VisitMut for Projection<'_> {
    fn visit_type_path_mut(&mut self, p: &mut syn::TypePath) {
        if p.path.segments.len() == 1 {
            let seg = &mut p.path.segments[0];
            if self.child(&seg.ident.to_string()) && seg.arguments.is_empty() {
                seg.arguments = syn::PathArguments::AngleBracketed(parse_quote!(<'_>));
            }
        }
        syn::visit_mut::visit_type_path_mut(self, p);
    }
    fn visit_item_impl_mut(&mut self, block: &mut syn::ItemImpl) {
        let name = resource_type(&block.self_ty);
        let previous = self.receiver.clone();
        self.receiver = name.clone();
        if name.as_ref().is_some_and(|n| self.child(n)) {
            block.generics.params.push(parse_quote!('__rcc_life));
            let id = resource_ident(name.as_ref().unwrap(), Span::call_site());
            *block.self_ty = parse_quote!(#id<'__rcc_life>);
        }
        syn::visit_mut::visit_item_impl_mut(self, block);
        self.receiver = previous;
    }
    fn visit_signature_mut(&mut self, sig: &mut syn::Signature) {
        let result = match &sig.output {
            ReturnType::Type(_, t) => resource_type(t),
            _ => None,
        };
        if let Some(result) = result.filter(|n| self.child(n)) {
            let parent = self.resources[&result].parent.as_ref().unwrap();
            let mut candidates = vec![];
            for (i, arg) in sig.inputs.iter().enumerate() {
                let matches = match arg {
                    FnArg::Typed(p) => {
                        matches!(p.ty.as_ref(),Type::Reference(r) if r.mutability.is_some() && resource_type(&r.elem).as_ref()==Some(parent))
                    }
                    FnArg::Receiver(r) => {
                        r.reference.is_some()
                            && r.mutability.is_some()
                            && self.receiver.as_ref() == Some(parent)
                    }
                };
                if matches {
                    candidates.push(i);
                }
            }
            let lifetime: syn::Lifetime = if candidates.len() == 1 {
                sig.generics.params.push(parse_quote!('__rcc_borrow));
                match &mut sig.inputs[candidates[0]] {
                    FnArg::Typed(p) => {
                        let Type::Reference(r) = p.ty.as_mut() else {
                            unreachable!()
                        };
                        r.lifetime = Some(parse_quote!('__rcc_borrow));
                    }
                    FnArg::Receiver(r) => {
                        r.reference.as_mut().unwrap().1 = Some(parse_quote!('__rcc_borrow))
                    }
                }
                parse_quote!('__rcc_borrow)
            } else {
                // No borrowed-parent origin cannot promise a borrowed return.
                // A return of a local factory result then produces rustc E0515.
                // RCC separately requires exactly one matching parent argument.
                parse_quote!('static)
            };
            let id = resource_ident(&result, Span::call_site());
            sig.output = parse_quote!(-> #id<#lifetime>);
        }
        syn::visit_mut::visit_signature_mut(self, sig);
    }
    fn visit_expr_struct_mut(&mut self, s: &mut syn::ExprStruct) {
        if s.path.segments.len() == 1 && self.child(&s.path.segments[0].ident.to_string()) {
            s.fields
                .push(parse_quote!(__rcc_life: ::core::marker::PhantomData));
        }
        syn::visit_mut::visit_expr_struct_mut(self, s);
    }
}

/// Token-per-line rendering retains original spans without depending on a
/// pretty-printer's line numbers. Joint punctuation/lifetime tokens stay joined.
struct Render {
    text: String,
    lines: Vec<Option<(usize, Span)>>,
    pending: Option<(usize, Span)>,
}
impl Render {
    fn token(&mut self, s: &str, file: usize, span: Span, joint: bool) {
        self.text.push_str(s);
        if self.pending.is_none() {
            self.pending = Some((file, span));
        }
        if !joint {
            self.text.push('\n');
            self.lines.push(self.pending.take());
        }
    }
    fn tokens(&mut self, stream: TokenStream, file: usize) {
        for token in stream {
            match token {
                TokenTree::Group(g) => {
                    let (a, b) = match g.delimiter() {
                        Delimiter::Parenthesis => ("(", ")"),
                        Delimiter::Brace => ("{", "}"),
                        Delimiter::Bracket => ("[", "]"),
                        Delimiter::None => ("", ""),
                    };
                    if !a.is_empty() {
                        self.token(a, file, g.span(), false);
                    }
                    self.tokens(g.stream(), file);
                    if !b.is_empty() {
                        self.token(b, file, g.span(), false);
                    }
                }
                TokenTree::Punct(p) => self.token(
                    &p.as_char().to_string(),
                    file,
                    p.span(),
                    p.spacing() == Spacing::Joint,
                ),
                t => self.token(&t.to_string(), file, t.span(), false),
            }
        }
    }
}
fn projection(files: &[syn::File]) -> Result<Render, (usize, syn::Error)> {
    let resources = markers(files)?;
    let drops: HashSet<_> = files
        .iter()
        .flat_map(|f| f.items.iter())
        .filter_map(|i| {
            let Item::Impl(b) = i else { return None };
            b.trait_
                .as_ref()
                .filter(|(_, p, _)| p.is_ident("Drop"))
                .and_then(|_| resource_type(&b.self_ty))
        })
        .collect();
    // Use the real host type signatures, not a second resource runtime/model.
    let shim = include_str!("../dsl_rt.rs").replace(
        "use once_cell::sync::Lazy;",
        "use std::sync::LazyLock as Lazy;",
    );
    let prelude=format!("#![feature(generic_const_exprs)]\n#![allow(incomplete_features,dead_code,unused_imports,unused_variables,unused_mut,unused_assignments,unused_unsafe,static_mut_refs)]\nmod dsl_rt{{\n{shim}\n}}\nuse crate::dsl_rt::*;\nmod __rcc_check_intrinsics {{\npub fn signal(_:u16,_:u16){{unreachable!()}}\npub fn resource_disarm<T>(v:T){{std::mem::forget(v)}}\n}}\nuse __rcc_check_intrinsics::*;\n");
    let mut rendered = Render {
        lines: vec![None; prelude.lines().count()],
        text: prelude,
        pending: None,
    };
    let mut projector = Projection {
        resources: &resources,
        receiver: None,
    };
    let mut native = native::Native::new(files, &resources.keys().cloned().collect());
    for (fi, file) in files.iter().enumerate() {
        for item in &file.items {
            if matches!(item, Item::Use(_) | Item::Mod(_)) {
                continue;
            }
            let mut item = item.clone();
            if let Item::Struct(s) = &mut item {
                if resources.contains_key(&s.ident.to_string()) {
                    s.attrs.retain(|a| !a.path.is_ident("rcc_resource"));
                    if projector.child(&s.ident.to_string()) {
                        s.generics.params.push(parse_quote!('__rcc_life));
                        if let syn::Fields::Named(fields) = &mut s.fields {
                            fields.named.push(syn::Field::parse_named.parse2(quote!(__rcc_life: ::core::marker::PhantomData<&'__rcc_life mut ()>)).unwrap());
                        }
                    }
                }
            }
            native.visit_item_mut(&mut item);
            projector.visit_item_mut(&mut item);
            rendered.tokens(item.into_token_stream(), fi);
        }
    }
    for (name, r) in &resources {
        if !drops.contains(name) {
            let id = resource_ident(name, Span::call_site());
            let tokens = if r.parent.is_some() {
                quote!(impl<'__rcc_life> ::core::ops::Drop for #id<'__rcc_life>{fn drop(&mut self){}})
            } else {
                quote!(impl ::core::ops::Drop for #id{fn drop(&mut self){}})
            };
            rendered.tokens(tokens, r.file);
        }
    }
    Ok(rendered)
}

pub(super) fn validate(
    files: &[syn::File],
    file_names: &[String],
    options: &RustcCheck,
) -> Result<(), (usize, syn::Error)> {
    let fail = |message: String| (0, syn::Error::new(Span::call_site(), message));
    let rendered = projection(files)?;
    std::fs::create_dir_all(&options.directory)
        .map_err(|e| fail(format!("rustc audit directory: {e}")))?;
    let dir = std::fs::canonicalize(&options.directory).map_err(|e| fail(e.to_string()))?;
    let source_map: Vec<_> = rendered
        .lines
        .iter()
        .map(|entry| {
            entry.map(|(file, span)| {
                let start = span.start();
                serde_json::json!({"file_index":file,"line":start.line,"column":start.column+1})
            })
        })
        .collect();
    std::fs::write(
        dir.join("source-map.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"files":file_names,"lines":source_map}))
            .unwrap(),
    )
    .map_err(|e| fail(e.to_string()))?;
    let input = dir.join("ownership.rs");
    std::fs::write(&input, &rendered.text).map_err(|e| fail(e.to_string()))?;
    let version = Command::new(&options.executable)
        .args(["--version", "--verbose"])
        .output()
        .map_err(|e| fail(format!("rustc ownership check unavailable: {e}")))?;
    if !version.status.success() {
        return Err(fail("rustc version query failed".into()));
    }
    std::fs::write(dir.join("rustc-version.txt"), version.stdout)
        .map_err(|e| fail(e.to_string()))?;
    let output = Command::new(&options.executable)
        .args([
            "--edition=2021",
            "--crate-type=lib",
            "--crate-name=rcc_ownership_check",
            "--emit=metadata",
            "--error-format=json",
            "--out-dir",
        ])
        .arg(&dir)
        .arg(&input)
        .output()
        .map_err(|e| fail(format!("rustc ownership check unavailable: {e}")))?;
    std::fs::write(dir.join("rustc.stderr.jsonl"), &output.stderr)
        .map_err(|e| fail(e.to_string()))?;
    std::fs::write(dir.join("rustc.stdout"), &output.stdout).map_err(|e| fail(e.to_string()))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value["level"] != "error" {
            continue;
        }
        let code = value["code"]["code"].as_str().unwrap_or("error");
        let message = value["message"]
            .as_str()
            .unwrap_or("ownership check failed");
        for span in value["spans"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["is_primary"] == true)
        {
            if Path::new(span["file_name"].as_str().unwrap_or("")) != input {
                continue;
            }
            let line = span["line_start"].as_u64().unwrap_or(0) as usize;
            if let Some(Some((file, original))) =
                line.checked_sub(1).and_then(|i| rendered.lines.get(i))
            {
                return Err((
                    *file,
                    syn::Error::new(*original, format!("rustc {code}: {message}")),
                ));
            }
        }
        return Err(fail(format!(
            "rustc {code}: {message}; see {}",
            dir.join("rustc.stderr.jsonl").display()
        )));
    }
    Err(fail(format!("rustc ownership check failed: {stderr}")))
}
