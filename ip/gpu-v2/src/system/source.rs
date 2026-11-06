//! Module-level assembly for explicit GPU component libraries.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Modules {
    bodies: BTreeMap<String, String>,
}

pub(super) fn module<'a>(source: &'a str, name: &str) -> Result<&'a str, String> {
    let marker = format!("module {name}");
    let starts: Vec<_> = source
        .match_indices(&marker)
        .filter_map(|(i, _)| {
            matches!(
                source.as_bytes().get(i + marker.len()),
                Some(b'(' | b' ' | b'\n' | b'\r' | b'#')
            )
            .then_some(i)
        })
        .collect();
    if starts.len() != 1 {
        return Err(format!("module {name}: {} declarations", starts.len()));
    }
    let start = starts[0];
    let end = start + source[start..].find("endmodule").ok_or("module end")? + 9;
    Ok(&source[start..end])
}

impl Modules {
    /// A complete single-module owner may retain its documentation and spacing.
    /// This preserves audited source identities while replacing old assemblies.
    pub fn add_complete(&mut self, source: &str) -> Result<(), String> {
        let names: Vec<_> = source
            .lines()
            .filter_map(|line| {
                line.trim_start().strip_prefix("module ").and_then(|rest| {
                    rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                        .next()
                })
            })
            .collect();
        if names.len() != 1 {
            return Err("complete owner must contain one module".into());
        }
        self.add_module(source, names[0])?;
        self.bodies.insert(names[0].to_owned(), source.to_owned());
        Ok(())
    }

    pub fn add_module(&mut self, source: &str, name: &str) -> Result<(), String> {
        let body = module(source, name)?.to_owned();
        if let Some(previous) = self.bodies.get(name) {
            if previous != &body {
                return Err(format!("conflicting module owners: {name}"));
            }
        } else {
            self.bodies.insert(name.to_owned(), body);
        }
        Ok(())
    }

    pub fn add(&mut self, source: &str) -> Result<(), String> {
        for line in source.lines() {
            if let Some(rest) = line.trim_start().strip_prefix("module ") {
                let name = rest
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
                    .ok_or("module name")?;
                self.add_module(source, name)?;
            }
        }
        Ok(())
    }

    /// Keep the established root-first, lexical dependency order. Primitive
    /// bindings and enclosing preprocessor scopes remain the factory's policy.
    pub fn render(&self, root: &str) -> Result<String, String> {
        let mut live = BTreeSet::from([root.to_owned()]);
        loop {
            let mut added = BTreeSet::new();
            for name in &live {
                let body = self
                    .bodies
                    .get(name)
                    .ok_or_else(|| format!("missing root {name}"))?;
                let tokens: BTreeSet<_> = body
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .collect();
                for dependency in self.bodies.keys() {
                    if tokens.contains(dependency.as_str()) {
                        added.insert(dependency.clone());
                    }
                }
            }
            let previous = live.len();
            live.extend(added);
            if live.len() == previous {
                break;
            }
        }
        let mut result = format!("{}\n", self.bodies[root]);
        for name in live.iter().filter(|name| name.as_str() != root) {
            result.push_str(&self.bodies[name]);
            result.push('\n');
        }
        Ok(result)
    }
}
