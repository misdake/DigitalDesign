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

#[path = "capture_profile.rs"]
pub(crate) mod capture_profile;
pub use capture_profile::RendererCaptureConfiguration;

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
    configuration: &RendererCaptureConfiguration,
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
        configuration,
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
    configuration: &RendererCaptureConfiguration,
    check: &rcc::frontend::rustc_check::RustcCheck,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, String> {
    GpuSources {
        local_dir,
        shared_dir,
        constants,
        ordinary_dirs,
        configuration,
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

/// Only an explicit diagnostic host may replace the ordinary no-op consumer.
/// It gains no resource-library membership or authority over private helpers.
pub struct CaptureConfiguration {
    pub gate_path: PathBuf,
    pub manifest_id: [u16; 16],
}
impl CaptureConfiguration {
    /// Host-test identity injection only. Production captures use the complete
    /// RendererCaptureConfiguration manifest, layout and resource validation.
    pub fn from_identity(gate_path: PathBuf, path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        Self::parse_id(gate_path, &manifest)
    }
    fn parse_id(gate_path: PathBuf, manifest: &serde_json::Value) -> Result<Self, String> {
        let id = manifest["manifest_id"]
            .as_str()
            .ok_or("profiler manifest_id must be SHA256 hex")?;
        if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("profiler manifest_id must have exactly64 hex digits".into());
        }
        let mut manifest_id = [0; 16];
        for (index, word) in manifest_id.iter_mut().enumerate() {
            let start = index * 4;
            let low = u8::from_str_radix(&id[start..start + 2], 16).unwrap();
            let high = u8::from_str_radix(&id[start + 2..start + 4], 16).unwrap();
            *word = u16::from_le_bytes([low, high]);
        }
        Ok(Self {
            gate_path,
            manifest_id,
        })
    }
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn compile_renderer_capture(
    library_root: &str,
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    ordinary_dirs: &[PathBuf],
    capture: Option<&CaptureConfiguration>,
    configuration: &RendererCaptureConfiguration,
    check: Option<&rcc::frontend::rustc_check::RustcCheck>,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    GpuSources {
        local_dir,
        shared_dir,
        constants,
        ordinary_dirs,
        configuration,
    }
    .compile(
        library_root,
        main_name,
        source,
        options,
        capture,
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
    pub configuration: &'a RendererCaptureConfiguration,
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
        let shared = std::fs::canonicalize(self.shared_dir)
            .map_err(|error| format!("GPU library root {}: {error}", self.shared_dir.display()))?;
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
                self.configuration.capture.as_ref(),
                Some(check),
                dependency,
                observe,
            )
            .map_err(|error| error.to_string())
    }
    fn load(
        &self,
        library_root: &str,
        name: &str,
        capture: Option<&CaptureConfiguration>,
        dependency: &mut dyn FnMut(&Path),
    ) -> Result<rcc::frontend::ModuleSource, String> {
        let Self {
            local_dir,
            shared_dir,
            constants,
            ordinary_dirs,
            configuration,
        } = *self;
        if name == "gpu_render_profile" {
            return Ok(rcc::frontend::ModuleSource {
                source: render_profile_source(),
                origin: None,
            });
        }
        if name == "gpu_capture_config" {
            return Ok(rcc::frontend::ModuleSource {
                source: capture_config_source(capture.map(|c| &c.manifest_id), configuration),
                origin: None,
            });
        }
        if name == "gpu_capture_access" && library_root == "gpu_api" {
            return Ok(rcc::frontend::ModuleSource {
            source: "mod gpu_api;pub use gpu_api::{gpu_capture_error,gpu_capture_range_word,gpu_capture_read_word};".into(),
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
        if name == "profiler_gate" {
            if !configuration.hook_enabled && capture.is_none() {
                return Ok(rcc::frontend::ModuleSource {
                    source:
                        "use crate::dsl_rt::*;pub fn profiler_gate(descriptor:Array<u16>)->u16{0}"
                            .into(),
                    origin: None,
                });
            }
            if let Some(capture) = capture {
                dependency(&capture.gate_path);
                let origin = std::fs::canonicalize(&capture.gate_path).map_err(|e| {
                    format!(
                        "resolve explicit profiler gate {}: {e}",
                        capture.gate_path.display()
                    )
                })?;
                return Ok(rcc::frontend::ModuleSource {
                    source: std::fs::read_to_string(&origin).map_err(|e| {
                        format!("read explicit profiler gate {}: {e}", origin.display())
                    })?,
                    origin: Some(origin),
                });
            }
        }
        // This public constants module is shared with non-GPU clients;
        // it is not a private GPU domain member. Its hardware ABI still
        // comes from the system, never an application's local override.
        let pinned = matches!(
            name,
            "device_abi" | "gpu_capture_abi" | "profiler_gate" | "profiler_runtime"
        );
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
        capture: Option<&CaptureConfiguration>,
        check: Option<&rcc::frontend::rustc_check::RustcCheck>,
        dependency: &mut dyn FnMut(&Path),
        observe: &mut dyn FnMut(&str, &rcc::frontend::ModuleSource),
    ) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
        let mut loader = |name: &str| {
            let module = self.load(library_root, name, capture, dependency)?;
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

fn capture_config_source(
    manifest: Option<&[u16; 16]>,
    configuration: &RendererCaptureConfiguration,
) -> String {
    let mut source = format!(
        "pub const GPU_CAPTURE_CONFIG_ENABLED:u16={};\npub fn gpu_capture_manifest_word(index:u16)->u16{{\n",
        u16::from(configuration.hook_enabled || manifest.is_some())
    );
    if let Some(words) = manifest {
        for (index, word) in words.iter().enumerate() {
            source.push_str(&format!("if index=={index}{{return {word};}}\n"));
        }
    }
    source.push_str("0\n}\n");
    source.push_str(&format!(
        "pub const GPU_CAPTURE_CONFIG_LAYOUT_ABI:u16={};\n",
        configuration.layout().layout_abi()
    ));
    source.push_str(&configuration.asset_check_source());
    if configuration.hook_enabled || manifest.is_some() {
        source.push_str("mod profiler_runtime;use profiler_runtime::*;\npub fn gpu_capture_input_poll()->u16{profiler_button_edges() & 2}\npub fn gpu_capture_input_reset(){profiler_buttons_reset();}\npub fn gpu_capture_input_arm(){profiler_gate_arm();}\n");
    } else {
        source.push_str("pub fn gpu_capture_input_poll()->u16{0}\npub fn gpu_capture_input_reset(){}\npub fn gpu_capture_input_arm(){}\n");
    }
    source
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
    if name == "gpu_capture_config" {
        return Ok((
            capture_config_source(None, &RendererCaptureConfiguration::without_capture()),
            None,
        ));
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
