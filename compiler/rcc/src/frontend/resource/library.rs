//! Explicit, host-authorized library domains. No dependency-closure trust.
use super::{error, Resource, Sources};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::{spanned::Spanned, visit::Visit, Item};

pub(super) fn key(path: &Path) -> String {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy().replace('/', "\\");
        let text = if let Some(tail) = text.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{tail}")
        } else {
            text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
        };
        text.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        path.to_string_lossy().into_owned()
    }
}
fn origin(path: &Path, node: &impl Spanned) -> Result<PathBuf, syn::Error> {
    if !path.is_absolute() {
        return Err(error(
            node,
            "resource-library origins and approvals must be absolute paths",
        ));
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| error(node, format!("cannot resolve resource-library origin: {e}")))?;
    if !canonical.is_file() {
        return Err(error(
            node,
            "resource-library origin must identify an existing source file",
        ));
    }
    Ok(canonical)
}

pub(super) fn domains(
    files: &[syn::File],
    names: &[String],
    sources: Option<Sources<'_>>,
    resources: &HashMap<String, Resource>,
) -> Result<HashMap<usize, usize>, (usize, syn::Error)> {
    let owners: HashSet<usize> = resources.values().map(|r| r.file).collect();
    let mut domains: HashMap<usize, usize> = owners.iter().map(|&file| (file, file)).collect();
    let mut approvals = HashSet::new();
    if let Some(context) = &sources {
        for allowed in context.approved_roots {
            approvals.insert(key(&origin(allowed, &files[0]).map_err(|e| (0, e))?));
        }
    }
    let mut loaded_roots = HashSet::new();
    let mut claimed_origins = HashSet::new();
    for (fi, file) in files.iter().enumerate() {
        // A misplaced marker must not silently leave the helper boundary open.
        struct Placement(Option<syn::Error>);
        impl<'ast> Visit<'ast> for Placement {
            fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
                if attr.path.is_ident("rcc_resource_library") {
                    self.0 = Some(error(
                        attr,
                        "rcc_resource_library must be a file inner attribute",
                    ))
                }
            }
        }
        for item in &file.items {
            let mut check = Placement(None);
            check.visit_item(item);
            if let Some(e) = check.0 {
                return Err((fi, e));
            }
        }
        let markers: Vec<_> = file
            .attrs
            .iter()
            .filter(|a| a.path.is_ident("rcc_resource_library"))
            .collect();
        if markers.is_empty() {
            continue;
        }
        let result = (|| {
            if markers.len() != 1 || !owners.contains(&fi) {
                return Err(error(
                    markers[0],
                    "a resource library needs one marker and a resource defined in its root",
                ));
            }
            let marker = markers[0];
            let Some(context) = &sources else {
                return Err(error(
                    marker,
                    "resource library requires a host-approved source loader",
                ));
            };
            let Some(Some(path)) = context.origins.get(fi) else {
                return Err(error(
                    marker,
                    "resource library root has no approved host origin",
                ));
            };
            let root = origin(path, marker)?;
            if !approvals.contains(&key(&root)) {
                return Err(error(marker, "resource library root is not host-approved"));
            }
            loaded_roots.insert(key(&root));
            if !claimed_origins.insert(key(&root)) {
                return Err(error(
                    marker,
                    "resource-library source origin is already claimed",
                ));
            }
            let helpers = marker.parse_args_with(
                syn::punctuated::Punctuated::<syn::Ident, syn::Token![,]>::parse_terminated,
            )?;
            let direct: HashSet<_> = file
                .items
                .iter()
                .filter_map(|i| {
                    if let Item::Mod(m) = i {
                        Some(m.ident.to_string())
                    } else {
                        None
                    }
                })
                .collect();
            let mut seen = HashSet::new();
            for helper in helpers {
                let name = helper.to_string();
                if !seen.insert(name.clone()) {
                    return Err(error(&helper, "duplicate resource-library helper"));
                }
                if !direct.contains(&name) {
                    return Err(error(&helper, "resource-library helpers must be explicitly declared by a root-level direct mod"));
                }
                let Some(index) = names
                    .iter()
                    .enumerate()
                    .skip(1)
                    .find_map(|(i, n)| (n == &format!("{name}.rs")).then_some(i))
                else {
                    return Err(error(&helper, "resource-library helper was not loaded"));
                };
                if owners.contains(&index) {
                    return Err(error(
                        &helper,
                        "a resource root cannot be claimed as another library's helper",
                    ));
                }
                let Some(Some(path)) = context.origins.get(index) else {
                    return Err(error(
                        &helper,
                        "resource-library helper has no host source origin",
                    ));
                };
                let source = origin(path, &helper)?;
                let expected = root.parent().unwrap().join(format!("{name}.rs"));
                // Compare to the lexical child of the *resolved* root directory:
                // a symlink/junction to an outside source must not gain trust.
                if key(&source) != key(&expected) {
                    return Err(error(&helper, "resource-library helper origin does not match the approved root directory and loaded name"));
                }
                if domains.insert(index, fi).is_some() {
                    return Err(error(
                        &helper,
                        "resource-library helper is already claimed by another library",
                    ));
                }
                if !claimed_origins.insert(key(&source)) {
                    return Err(error(
                        &helper,
                        "resource-library helper source origin is already claimed",
                    ));
                }
            }
            Ok(())
        })();
        result.map_err(|e| (fi, e))?;
    }
    // Approvals describe mandatory protected roots, not permissions that an
    // application may disable by omitting the root's mod. This check precedes
    // the resource pass's native/no-resource fast path.
    if !approvals.is_subset(&loaded_roots) {
        return Err((0, error(&files[0], "every host-approved resource-library root must be explicitly loaded with its library marker")));
    }
    Ok(domains)
}
