//! System-owned application/local-first module lookup for the boot compiler.
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub fn load_module(
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    name: &str,
    dependency: &mut dyn FnMut(&Path),
) -> Result<String, String> {
    resolve_module(local_dir, shared_dir, constants, name, dependency).map(|(source, _)| source)
}

/// Preserve the identity of the actual file selected by local-first lookup.
/// A local override never receives the shared module's approved identity.
pub fn load_module_source(
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    name: &str,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::ModuleSource, String> {
    let (source, path) = resolve_module(local_dir, shared_dir, constants, name, dependency)?;
    let origin = path
        .map(|path| {
            std::fs::canonicalize(&path)
                .map_err(|e| format!("resolve module {name} at {}: {e}", path.display()))
        })
        .transpose()?;
    Ok(rcc::frontend::ModuleSource { source, origin })
}

/// This explicit host entry always requires the shared GPU root, even when
/// application text omits it. Native diagnostics keep the String-loader path.
pub fn compile_renderer_program(
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    rcc::frontend::compile_program_named_with_libraries(
        main_name,
        source,
        options,
        &[shared_dir.join("gpu_pipeline.rs")],
        &mut |name| {
            // This public constants module is shared with non-GPU clients;
            // it is not a private GPU domain member. Its hardware ABI still
            // comes from the system, never an application's local override.
            let local = if name == "device_abi" {
                shared_dir
            } else {
                local_dir
            };
            load_module_source(local, shared_dir, constants, name, dependency)
        },
    )
}

fn resolve_module(
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    name: &str,
    dependency: &mut dyn FnMut(&Path),
) -> Result<(String, Option<PathBuf>), String> {
    if name == "boot_resources" {
        return Ok((constants.to_owned(), None));
    }
    let local = local_dir.join(format!("{name}.rs"));
    match std::fs::read_to_string(&local) {
        Ok(text) => {
            dependency(&local);
            return Ok((text, Some(local)));
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
    std::fs::read_to_string(&shared)
        .map(|text| (text, Some(shared.clone())))
        .map_err(|error| {
            format!(
                "read module {name}: local {} not found; shared {}: {error}",
                local.display(),
                shared.display()
            )
        })
}
