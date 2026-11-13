//! Source module identity is independent of SDK authority and target symbol names.
use super::{cfg, err, CompileError, ModuleSource};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};
use syn::Item;
mod resolve;
#[derive(Default)]
pub(super) struct Labels {
    pub storage: HashMap<String, String>,
    pub symbols: HashMap<String, String>,
}

#[derive(Default)]
pub(super) struct Graph {
    pub children: Vec<BTreeMap<String, usize>>,
    pub parents: Vec<Option<usize>>,
    pub compatibility: Vec<bool>,
}

pub(super) struct Loaded {
    pub sources: Vec<(String, String)>,
    pub origins: Vec<Option<PathBuf>>,
    pub graph: Graph,
}

pub(super) fn load(
    name: &str,
    source: &str,
    approved_roots: &[PathBuf],
    scoped: bool,
    loader: &mut dyn FnMut(&str) -> Result<ModuleSource, String>,
) -> Result<Loaded, CompileError> {
    let mut loaded = Loaded {
        sources: vec![(name.to_owned(), source.to_owned())],
        origins: vec![None],
        graph: Graph {
            children: vec![BTreeMap::new()],
            parents: vec![None],
            compatibility: vec![false],
        },
    };
    let mut keys = vec![String::new()];
    let mut seen = HashMap::new();
    let mut origins = HashMap::new();
    let mut index = 0;
    while index < loaded.sources.len() {
        let (file_name, text) = loaded.sources[index].clone();
        let file =
            syn::parse_file(&text).map_err(|e| CompileError::from_syn(&file_name, &text, e))?;
        for item in &file.items {
            let Item::Mod(module) = item else { continue };
            cfg::module_attributes(&module.attrs)
                .map_err(|e| CompileError::from_syn(&file_name, &text, e))?;
            if module.content.is_some() {
                return Err(CompileError::from_syn(
                    &file_name,
                    &text,
                    err(module, "inline modules are not supported; use mod name;"),
                ));
            }
            let leaf = module.ident.to_string();
            // Approved SDK implementation files keep their historical sibling lookup.
            // This changes lookup only; actual origin validation still owns authority.
            let sdk = loaded.origins[index].as_ref().is_some_and(|origin| {
                approved_roots
                    .iter()
                    .any(|root| root.parent() == origin.parent())
            });
            let disk_leaf = leaf.trim_start_matches("r#");
            let qualified =
                if !scoped || keys[index].is_empty() || sdk || loaded.graph.compatibility[index] {
                    disk_leaf.to_owned()
                } else {
                    format!("{}/{disk_leaf}", keys[index])
                };
            if let Some(&child) = seen.get(&qualified) {
                loaded.graph.children[index].insert(leaf, child);
                continue;
            }
            let resolved = loader(&qualified).map_err(|error| {
                CompileError::from_syn(
                    &file_name,
                    &text,
                    err(
                        &module.ident,
                        format!("cannot load module `{qualified}`: {error}"),
                    ),
                )
            })?;
            let key = if resolved.namespace == super::ModuleNamespace::Flat {
                disk_leaf.to_owned()
            } else {
                qualified
            };
            if let Some(&child) = seen.get(&key) {
                loaded.graph.children[index].insert(leaf, child);
                continue;
            }
            let existing = resolved
                .origin
                .as_ref()
                .and_then(|path| origins.get(path))
                .copied();
            let child = if let Some(child) = existing {
                child
            } else {
                let child = loaded.sources.len();
                if let Some(origin) = &resolved.origin {
                    origins.insert(origin.clone(), child);
                }
                loaded.sources.push((format!("{key}.rs"), resolved.source));
                loaded.origins.push(resolved.origin);
                loaded.graph.children.push(BTreeMap::new());
                loaded.graph.parents.push(Some(index));
                loaded
                    .graph
                    .compatibility
                    .push(resolved.namespace == super::ModuleNamespace::Flat);
                keys.push(key.clone());
                child
            };
            seen.insert(key, child);
            if loaded.graph.children[index].insert(leaf, child).is_some() {
                return Err(CompileError::from_syn(
                    &file_name,
                    &text,
                    err(module, "module declared twice"),
                ));
            }
        }
        index += 1;
    }
    Ok(loaded)
}

impl Graph {
    pub(super) fn prepare(
        &self,
        files: &mut [syn::File],
        protected: &HashMap<usize, usize>,
        names: &[String],
    ) -> Result<Labels, (usize, syn::Error)> {
        resolve::prepare(self, files, protected, names)
    }
}
