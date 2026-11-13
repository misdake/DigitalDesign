//! System-owned application/local-first module lookup for the boot compiler.
// Retired resource contracts are available only to regression targets.
#[cfg(test)]
#[path = "../tests/support/legacy_renderer_loader.rs"]
mod legacy_renderer;
#[cfg(test)]
#[allow(unused_imports)]
pub use legacy_renderer::*;

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

#[path = "../rcc/gpu_layout.rs"]
pub mod layout;

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
    load_module_source_with_sources(local_dir, shared_dir, constants, name, &[], dependency)
}

pub fn load_module_source_with_sources(
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    name: &str,
    ordinary_dirs: &[PathBuf],
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::ModuleSource, String> {
    let (source, path) = resolve_module_with_sources(
        local_dir,
        shared_dir,
        constants,
        name,
        ordinary_dirs,
        dependency,
    )?;
    let origin = path
        .map(|path| {
            std::fs::canonicalize(&path)
                .map_err(|e| format!("resolve module {name} at {}: {e}", path.display()))
        })
        .transpose()?;
    Ok(rcc::frontend::ModuleSource { source, origin })
}

/// Explicit API recording uses its own approved root, never both facade domains.
#[allow(clippy::too_many_arguments)]
pub fn compile_explicit_gpu_program_checked(
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    check: &rcc::frontend::rustc_check::RustcCheck,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, String> {
    compile_explicit_gpu_program_checked_with_sources(
        main_name,
        source,
        options,
        local_dir,
        shared_dir,
        constants,
        &[],
        check,
        dependency,
    )
}

/// Ordinary application directories add no resource-library trust.
#[allow(clippy::too_many_arguments)]
pub fn compile_explicit_gpu_program_checked_with_sources(
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    ordinary_dirs: &[PathBuf],
    check: &rcc::frontend::rustc_check::RustcCheck,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, String> {
    GpuSources {
        local_dir,
        shared_dir,
        constants,
        ordinary_dirs,
    }
    .compile_checked(
        main_name,
        source,
        options,
        check,
        dependency,
        &mut |_, _| {},
    )
}

/// System module lookup and approved root; observers cannot change source authority.
#[derive(Clone, Copy)]
pub struct GpuSources<'a> {
    pub local_dir: &'a Path,
    pub shared_dir: &'a Path,
    pub constants: &'a str,
    pub ordinary_dirs: &'a [PathBuf],
}
#[derive(Debug)]
pub enum GpuCompileError {
    Root(String),
    Source(rcc::frontend::CompileError),
}
impl std::fmt::Display for GpuCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Root(error) => f.write_str(error),
            Self::Source(error) => error.fmt(f),
        }
    }
}
impl GpuSources<'_> {
    pub fn compile_checked(
        &self,
        main_name: &str,
        source: &str,
        options: &impl rcc::RccConfig,
        check: &rcc::frontend::rustc_check::RustcCheck,
        dependency: &mut dyn FnMut(&Path),
        observe: &mut dyn FnMut(&str, &rcc::frontend::ModuleSource),
    ) -> Result<rcc::frontend::Program, String> {
        self.compile_checked_source(main_name, source, options, check, dependency, observe)
            .map_err(|error| error.to_string())
    }
    pub fn compile_checked_source(
        &self,
        main_name: &str,
        source: &str,
        options: &impl rcc::RccConfig,
        check: &rcc::frontend::rustc_check::RustcCheck,
        dependency: &mut dyn FnMut(&Path),
        observe: &mut dyn FnMut(&str, &rcc::frontend::ModuleSource),
    ) -> Result<rcc::frontend::Program, GpuCompileError> {
        let shared = std::fs::canonicalize(self.shared_dir).map_err(|error| {
            GpuCompileError::Root(format!(
                "GPU library root {}: {error}",
                self.shared_dir.display()
            ))
        })?;
        let sources = GpuSources {
            shared_dir: &shared,
            ..*self
        };
        sources
            .compile(
                "gpu_api",
                main_name,
                source,
                options,
                Some(check),
                dependency,
                observe,
            )
            .map_err(GpuCompileError::Source)
    }
    fn load(
        &self,
        name: &str,
        dependency: &mut dyn FnMut(&Path),
    ) -> Result<rcc::frontend::ModuleSource, String> {
        let Self {
            local_dir,
            shared_dir,
            constants,
            ordinary_dirs,
        } = *self;
        if name == "gpu_render_profile" {
            return Ok(rcc::frontend::ModuleSource {
                source: render_profile_source(),
                origin: None,
            });
        }
        if name == "gpu_layout" {
            let path = shared_dir.join("gpu_layout.rs");
            dependency(&path);
            let origin = std::fs::canonicalize(&path)
                .map_err(|e| format!("layout origin {}: {e}", path.display()))?;
            let source = std::fs::read_to_string(&origin)
                .map_err(|e| format!("layout source {}: {e}", origin.display()))?;
            return Ok(rcc::frontend::ModuleSource {
                source,
                origin: Some(origin),
            });
        }
        let pinned = name == "device_abi";
        let local = if pinned { shared_dir } else { local_dir };
        let dirs = if pinned { &[][..] } else { ordinary_dirs };
        load_module_source_with_sources(local, shared_dir, constants, name, dirs, dependency)
    }
    #[allow(clippy::too_many_arguments)]
    fn compile(
        &self,
        library_root: &str,
        main_name: &str,
        source: &str,
        options: &impl rcc::RccConfig,
        check: Option<&rcc::frontend::rustc_check::RustcCheck>,
        dependency: &mut dyn FnMut(&Path),
        observe: &mut dyn FnMut(&str, &rcc::frontend::ModuleSource),
    ) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
        let mut loader = |name: &str| {
            let module = self.load(name, dependency)?;
            observe(&format!("{name}.rs"), &module);
            Ok(module)
        };
        let approved = [self.shared_dir.join(format!("{library_root}.rs"))];
        match check {
            Some(check) => rcc::frontend::compile_program_named_with_libraries_checked(
                main_name,
                source,
                options,
                &approved,
                check,
                &mut loader,
            ),
            None => rcc::frontend::compile_program_named_with_libraries(
                main_name,
                source,
                options,
                &approved,
                &mut loader,
            ),
        }
    }
}

fn render_profile_source() -> String {
    include_str!("../rcc/gpu_render_profile.rs").to_owned()
}

fn resolve_module(
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    name: &str,
    dependency: &mut dyn FnMut(&Path),
) -> Result<(String, Option<PathBuf>), String> {
    resolve_module_with_sources(local_dir, shared_dir, constants, name, &[], dependency)
}

fn resolve_module_with_sources(
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    name: &str,
    ordinary_dirs: &[PathBuf],
    dependency: &mut dyn FnMut(&Path),
) -> Result<(String, Option<PathBuf>), String> {
    if name == "gpu_render_profile" {
        return Ok((render_profile_source(), None));
    }
    if name == "boot_resources" {
        return Ok((constants.to_owned(), None));
    }
    let local = local_dir.join(format!("{name}.rs"));
    match std::fs::read_to_string(&local) {
        Ok(text) => {
            dependency(&local);
            return Ok((text, Some(local)));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
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
    let result = std::fs::read_to_string(&shared);
    dependency(if result.is_ok() { &shared } else { shared_dir });
    if result
        .as_ref()
        .is_err_and(|e| e.kind() == ErrorKind::NotFound)
    {
        // System application support is ordinary source, never GPU-library trust.
        // Production recipes also declare this root in their exported context.
        let common = shared_dir.join("../applications/rendering/rcc");
        for dir in ordinary_dirs.iter().chain(std::iter::once(&common)) {
            dependency(shared_dir);
            let path = dir.join(format!("{name}.rs"));
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    dependency(&path);
                    return Ok((text, Some(path)));
                }
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    if let Some(existing) = dir.ancestors().find(|path| path.is_dir()) {
                        dependency(existing);
                    }
                }
                Err(e) => {
                    dependency(&path);
                    return Err(format!("read module {name} at {}: {e}", path.display()));
                }
            }
        }
    }
    result
        .map(|text| (text, Some(shared.clone())))
        .map_err(|error| {
            format!(
                "read module {name}: local {} not found; shared {}: {error}",
                local.display(),
                shared.display()
            )
        })
}
