//! Whole-program code placement metadata; data and assets keep their own layout.
use super::err;
use std::collections::BTreeMap;
use syn::{Attribute, Item, Lit, Meta, NestedMeta};

pub(super) struct Placement {
    pub default: Option<u16>,
    names: BTreeMap<String, u16>,
    files: Vec<Option<u16>>,
}
fn name(attr: &Attribute) -> Result<String, syn::Error> {
    match attr.parse_meta()? {
        Meta::NameValue(value) => match value.lit {
            Lit::Str(value) if !value.value().is_empty() => Ok(value.value()),
            _ => Err(err(attr, "expected link_cseg = \"name\"")),
        },
        _ => Err(err(attr, "expected link_cseg = \"name\"")),
    }
}
impl Placement {
    fn resolve(&self, attr: &Attribute) -> Result<u16, syn::Error> {
        let name = name(attr)?;
        self.names
            .get(&name)
            .copied()
            .ok_or_else(|| err(attr, format!("undeclared CSEG `{name}`")))
    }
    pub fn function(&self, file: usize, attrs: &[Attribute]) -> Result<Option<u16>, syn::Error> {
        let mut selected = None;
        for attr in attrs.iter().filter(|a| a.path.is_ident("link_cseg")) {
            if selected.is_some() {
                return Err(err(attr, "duplicate link_cseg"));
            }
            selected = Some(self.resolve(attr)?);
        }
        Ok(selected.or(self.files[file]).or(self.default))
    }
}
pub(super) fn prepare(files: &mut [syn::File]) -> Result<Placement, (usize, syn::Error)> {
    let mut placement = Placement {
        default: None,
        names: BTreeMap::new(),
        files: vec![None; files.len()],
    };
    let mut default_name = None;
    for (fi, file) in files.iter().enumerate() {
        for attr in file.attrs.iter().filter(|a| a.path.is_ident("link_csegs")) {
            if fi != 0 || default_name.is_some() {
                return Err((
                    fi,
                    err(attr, "declare link_csegs once, in the application root"),
                ));
            }
            let result = (|| {
                let Meta::List(list) = attr.parse_meta()? else {
                    return Err(err(
                        attr,
                        "expected link_csegs(default = \"name\", name = number, ...)",
                    ));
                };
                for entry in list.nested {
                    let NestedMeta::Meta(Meta::NameValue(value)) = entry else {
                        return Err(err(attr, "expected CSEG name = value"));
                    };
                    let key = value
                        .path
                        .get_ident()
                        .ok_or_else(|| err(&value, "expected a CSEG name"))?
                        .to_string();
                    if key == "default" {
                        let Lit::Str(ref name) = value.lit else {
                            return Err(err(&value, "default must name a declared CSEG"));
                        };
                        if default_name.replace(name.value()).is_some() {
                            return Err(err(&value, "duplicate default CSEG"));
                        }
                    } else {
                        let Lit::Int(ref number) = value.lit else {
                            return Err(err(&value, "CSEG must be a u16 number"));
                        };
                        let number = number.base10_parse::<u16>()?;
                        if placement.names.insert(key, number).is_some() {
                            return Err(err(&value, "duplicate CSEG name"));
                        }
                    }
                }
                if default_name.is_none() {
                    return Err(err(attr, "link_csegs requires a default"));
                }
                placement.default = Some(
                    *placement
                        .names
                        .get(default_name.as_ref().unwrap())
                        .ok_or_else(|| err(attr, "default names an undeclared CSEG"))?,
                );
                Ok(())
            })();
            result.map_err(|e| (fi, e))?;
        }
    }
    for (fi, file) in files.iter_mut().enumerate() {
        for attr in file.attrs.iter().filter(|a| a.path.is_ident("link_cseg")) {
            if placement.files[fi].is_some() {
                return Err((fi, err(attr, "duplicate file link_cseg")));
            }
            placement.files[fi] = Some(placement.resolve(attr).map_err(|e| (fi, e))?);
        }
        file.attrs
            .retain(|a| !a.path.is_ident("link_cseg") && !a.path.is_ident("link_csegs"));
        // Only definition files and function/method bodies have code placement.
        // Validate misplaced metadata instead of silently discarding it in rustc.
        let mut copy = file.clone();
        for item in &mut copy.items {
            match item {
                Item::Fn(f) => f.attrs.retain(|a| !a.path.is_ident("link_cseg")),
                Item::Impl(block) => {
                    for member in &mut block.items {
                        if let syn::ImplItem::Method(m) = member {
                            m.attrs.retain(|a| !a.path.is_ident("link_cseg"));
                        }
                    }
                }
                _ => {}
            }
        }
        struct Remaining(Option<syn::Error>);
        impl<'a> syn::visit::Visit<'a> for Remaining {
            fn visit_attribute(&mut self, a: &'a Attribute) {
                if (a.path.is_ident("link_cseg") || a.path.is_ident("link_csegs"))
                    && self.0.is_none()
                {
                    self.0 = Some(err(
                        a,
                        "CSEG metadata belongs on the root, a file, or a function",
                    ));
                }
            }
        }
        let mut remaining = Remaining(None);
        syn::visit::Visit::visit_file(&mut remaining, &copy);
        if let Some(e) = remaining.0 {
            return Err((fi, e));
        }
    }
    Ok(placement)
}
/// Strip only placement metadata in the rustc checking copy; bodies/types stay intact.
pub(super) fn strip_check(item: &mut Item) {
    struct Strip;
    impl syn::visit_mut::VisitMut for Strip {
        fn visit_item_fn_mut(&mut self, f: &mut syn::ItemFn) {
            f.attrs.retain(|a| !a.path.is_ident("link_cseg"));
            syn::visit_mut::visit_item_fn_mut(self, f);
        }
        fn visit_impl_item_method_mut(&mut self, m: &mut syn::ImplItemMethod) {
            m.attrs.retain(|a| !a.path.is_ident("link_cseg"));
            syn::visit_mut::visit_impl_item_method_mut(self, m);
        }
    }
    syn::visit_mut::VisitMut::visit_item_mut(&mut Strip, item);
}
