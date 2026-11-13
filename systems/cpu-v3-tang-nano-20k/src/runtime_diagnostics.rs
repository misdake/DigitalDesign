//! Same-build host diagnostics using the canonical Python artifact reader.
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use syn::{spanned::Spanned, visit::Visit};
pub struct RuntimeDiagnostics {
    artifact: PathBuf,
    package: PathBuf,
    output: PathBuf,
    pub fault_logical: Option<u16>,
    pub stack_top: u16,
}
impl RuntimeDiagnostics {
    pub fn new(artifact: &Path, package: &Path, output: &Path) -> Result<Self, String> {
        let context = invoke("context", artifact, package, None)?;
        Ok(Self {
            artifact: artifact.to_owned(),
            package: package.to_owned(),
            output: output.to_owned(),
            fault_logical: context["fault_logical"].as_u64().map(|n| n as u16),
            stack_top: context["stack_top"].as_u64().ok_or("missing stack top")? as u16,
        })
    }
    pub fn finish(&self, state: &Value) -> Result<Value, String> {
        std::fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        let path = self.output.join("runtime-state.json");
        std::fs::write(&path, serde_json::to_vec_pretty(state).unwrap())
            .map_err(|e| e.to_string())?;
        let mut result = invoke("diagnose", &self.artifact, &self.package, Some(&path))?;
        if let Some(source) = result
            .as_object_mut()
            .and_then(|v| v.remove("_assertion_source"))
        {
            if let (Some(text), Some(line)) = (source["text"].as_str(), source["line"].as_u64()) {
                if let Some((first, last)) = assertion_at(text, line as usize) {
                    let signal = result["signal"].as_u64().unwrap_or(0);
                    result["kind"] = serde_json::json!("assertion");
                    result["message"] =
                        serde_json::json!(format!("Assertion failed (signal 0x{signal:04x})"));
                    result["source"]["line"] = serde_json::json!(first);
                    result["source"]["text"] = serde_json::json!(text
                        .lines()
                        .skip(first - 1)
                        .take(last - first + 1)
                        .collect::<Vec<_>>()
                        .join("\n"));
                    let frame = result["source"].clone();
                    if let Some(chain) = result["call_chain"].as_array_mut() {
                        if let Some(first) = chain.first_mut() {
                            *first = frame;
                        }
                    }
                }
            }
        }
        std::fs::write(
            self.output.join("diagnostic.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        Ok(result)
    }
}
// Source AST + verified linked HALT, never signal values or text-pattern guesses.
fn assertion_at(source: &str, line: usize) -> Option<(usize, usize)> {
    #[derive(Default)]
    struct Sites {
        assertions: Vec<(usize, usize)>,
        halts: Vec<(usize, usize)>,
    }
    impl<'ast> Visit<'ast> for Sites {
        fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
            if let syn::Expr::Path(path) = node.func.as_ref() {
                let span = node.span();
                let range = (span.start().line, span.end().line);
                if path.path.is_ident("assert") {
                    self.assertions.push(range);
                }
                if path.path.is_ident("halt") {
                    self.halts.push(range);
                }
            }
            syn::visit::visit_expr_call(self, node);
        }
        fn visit_expr_macro(&mut self, node: &'ast syn::ExprMacro) {
            if node.mac.path.is_ident("assert") || node.mac.path.is_ident("debug_assert") {
                if let Ok(condition) = syn::parse2::<syn::Expr>(node.mac.tokens.clone()) {
                    let span = node.span();
                    self.assertions.push((span.start().line, span.end().line));
                    // Macro token bodies are not visited automatically.
                    let mut nested = Sites::default();
                    nested.visit_expr(&condition);
                    self.halts.extend(nested.halts);
                }
            }
        }
    }
    let file = syn::parse_file(source).ok()?;
    let mut sites = Sites::default();
    sites.visit_file(&file);
    if sites
        .halts
        .iter()
        .any(|&(first, last)| first <= line && line <= last)
    {
        return None;
    }
    sites
        .assertions
        .into_iter()
        .find(|&(first, last)| first <= line && line <= last)
}
fn invoke(
    command: &str,
    artifact: &Path,
    package: &Path,
    state: Option<&Path>,
) -> Result<Value, String> {
    let mut process = Command::new("python");
    process
        .args(["-X", "utf8", "-B"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/application_artifact.py"))
        .arg(command)
        .arg(artifact)
        .arg("--package")
        .arg(package);
    if let Some(state) = state {
        process.arg("--state").arg(state);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        process.creation_flags(0x08000000);
    }
    let result = process
        .output()
        .map_err(|e| format!("artifact reader: {e}"))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().into());
    }
    serde_json::from_slice(&result.stdout).map_err(|e| format!("artifact diagnostic response: {e}"))
}
pub fn render(value: &Value) -> String {
    let mut text = value["message"]
        .as_str()
        .unwrap_or("Application stopped")
        .to_owned();
    if let Some(source) = value.get("source") {
        text.push_str(&format!(
            "\n --> {}:{} ({})\n     {}",
            source["file"]
                .as_str()
                .unwrap_or("?")
                .trim_start_matches(r"\\?\"),
            source["line"],
            source["function"].as_str().unwrap_or("?"),
            source["text"].as_str().unwrap_or("")
        ));
    }
    if let Some(chain) = value["call_chain"].as_array() {
        for source in chain.iter().skip(1) {
            text.push_str(&format!(
                "\n via {}:{}\n     {}",
                source["file"]
                    .as_str()
                    .unwrap_or("?")
                    .trim_start_matches(r"\\?\"),
                source["line"],
                source["text"].as_str().unwrap_or("")
            ));
        }
    }
    text
}
