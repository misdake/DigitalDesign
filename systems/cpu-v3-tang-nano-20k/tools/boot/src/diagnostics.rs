//! Machine-readable build failures, independent of compiler and CPU crates.
use serde_json::{json, Value};
#[derive(Debug)]
pub struct BuildDiagnostic {
    pub stage: &'static str,
    pub rendered: String,
    pub location: Option<(String, usize, usize)>,
}
impl BuildDiagnostic {
    pub fn new(stage: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            stage,
            rendered: error.to_string(),
            location: None,
        }
    }
    pub fn value(&self) -> Value {
        json!({"schema":"digitaldesign.build-diagnostic","version":1,"stage":self.stage,
            "rendered":self.rendered,"location":self.location.as_ref().map(|(file,line,column)|json!({"file":file,"line":line,"column":column}))})
    }
    pub fn write(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&self.value()).map_err(std::io::Error::other)?,
        )
    }
    pub fn report(&self) {
        if let Some(path) = std::env::var_os("CPU_V3_BUILD_DIAGNOSTIC") {
            match self.write(std::path::Path::new(&path)) {
                Ok(()) => (),
                Err(error) => eprintln!(
                    "write build diagnostic {}: {error}",
                    std::path::Path::new(&path).display()
                ),
            }
        }
        eprintln!("{}: {}", self.stage, self.rendered);
    }
}
