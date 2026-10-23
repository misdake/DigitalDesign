//! System-owned application/local-first module lookup for the boot compiler.
use std::{io::ErrorKind, path::Path};

pub fn load_module(
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    name: &str,
    dependency: &mut dyn FnMut(&Path),
) -> Result<String, String> {
    if name == "boot_resources" {
        return Ok(constants.to_owned());
    }
    let local = local_dir.join(format!("{name}.rs"));
    match std::fs::read_to_string(&local) {
        Ok(text) => {
            dependency(&local);
            return Ok(text);
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if local_dir == shared_dir {
                return Err(format!(
                    "read module {name} at {}: {error}",
                    local.display()
                ));
            }
        }
        Err(error) => {
            dependency(&local);
            return Err(format!(
                "read module {name} at {}: {error}",
                local.display()
            ));
        }
    }
    // A new local override must invalidate a build previously using the shared
    // file. Watch its existing directory, not a nonexistent file (always dirty).
    dependency(local_dir);
    let shared = shared_dir.join(format!("{name}.rs"));
    dependency(&shared);
    std::fs::read_to_string(&shared).map_err(|error| {
        format!(
            "read module {name}: local {} not found; shared {}: {error}",
            local.display(),
            shared.display()
        )
    })
}
