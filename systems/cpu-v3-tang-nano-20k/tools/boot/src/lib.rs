//! Host application descriptions and artifact consumers for this system.
#[path = "../../../applications/build/mod.rs"]
pub mod applications;
pub mod diagnostics;
pub mod process;
pub mod workflow;

/// A build/export creates one snapshot and never replaces an existing payload.
pub fn require_empty_output(path: &std::path::Path) -> Result<(), String> {
    if path.exists()
        && (!path.is_dir()
            || std::fs::read_dir(path)
                .map_err(|error| error.to_string())?
                .next()
                .is_some())
    {
        return Err(format!(
            "{} is not an empty directory; choose a new application snapshot",
            path.display()
        ));
    }
    Ok(())
}
