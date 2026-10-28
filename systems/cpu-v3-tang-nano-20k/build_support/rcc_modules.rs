//! System-owned application/local-first module lookup for the boot compiler.
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

#[path = "capture_profile.rs"]
pub(crate) mod capture_profile;
pub use capture_profile::{
    materialize_renderer_sources, RendererCaptureConfiguration, RendererLayoutProfile,
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

/// Explicit native diagnostic selection; never inferred from application paths.
/// Formal renderer/capture compilation retains its own approved-source contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeLayoutProfile {
    Default,
    Column16,
}
pub fn load_native_module(
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    name: &str,
    profile: NativeLayoutProfile,
    dependency: &mut dyn FnMut(&Path),
) -> Result<String, String> {
    if profile == NativeLayoutProfile::Column16 && name == "gpu_layout" {
        let path = RendererLayoutProfile::Column16.layout_path(shared_dir);
        dependency(&path);
        return std::fs::read_to_string(&path)
            .map_err(|e| format!("column layout {}: {e}", path.display()));
    }
    load_module(local_dir, shared_dir, constants, name, dependency)
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
    compile_renderer_program_with_sources(
        main_name,
        source,
        options,
        local_dir,
        shared_dir,
        constants,
        &[],
        dependency,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn compile_renderer_program_with_sources(
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    ordinary_dirs: &[PathBuf],
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    compile_renderer_program_with_capture(
        main_name,
        source,
        options,
        local_dir,
        shared_dir,
        constants,
        ordinary_dirs,
        None,
        dependency,
    )
}

/// Check actual approved library bodies with rustc before target ownership erasure.
#[allow(clippy::too_many_arguments)]
pub fn compile_renderer_program_checked(
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
    let actual_shared = materialize_renderer_sources(
        shared_dir,
        &check.directory.join("renderer-rcc"),
        configuration.profile,
        dependency,
    )?;
    compile_renderer_capture(
        main_name,
        source,
        options,
        local_dir,
        &actual_shared,
        constants,
        &[],
        configuration.capture.as_ref(),
        configuration,
        Some(check),
        dependency,
    )
    .map_err(|e| e.to_string())
}

/// Only an explicit diagnostic host may replace the ordinary no-op consumer.
/// It gains no resource-library membership or authority over private helpers.
pub struct CaptureConfiguration {
    pub gate_path: PathBuf,
    pub manifest_id: [u16; 16],
}
impl CaptureConfiguration {
    pub fn from_manifest(gate_path: PathBuf, manifest_path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(manifest_path)
            .map_err(|e| format!("read profiler manifest {}: {e}", manifest_path.display()))?;
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("parse profiler manifest: {e}"))?;
        if manifest.get("layout").is_some()
            || manifest
                .get("version")
                .is_some_and(|v| v.as_u64() != Some(1))
            || manifest
                .get("command_abi")
                .is_some_and(|v| v.as_u64() != Some(1))
        {
            return Err("legacy capture constructor accepts row ABI1 only; use RendererCaptureConfiguration for explicit profiles".into());
        }
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

#[allow(clippy::too_many_arguments)]
pub fn compile_renderer_program_with_capture(
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    ordinary_dirs: &[PathBuf],
    capture: Option<&CaptureConfiguration>,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    compile_renderer_capture(
        main_name,
        source,
        options,
        local_dir,
        shared_dir,
        constants,
        ordinary_dirs,
        capture,
        &RendererCaptureConfiguration::without_capture(RendererLayoutProfile::Row),
        None,
        dependency,
    )
}

/// Explicit versioned producer; profile and capture identity are one host selection.
#[allow(clippy::too_many_arguments)]
pub fn compile_renderer_program_with_capture_profile(
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local_dir: &Path,
    shared_dir: &Path,
    constants: &str,
    ordinary_dirs: &[PathBuf],
    configuration: &RendererCaptureConfiguration,
    generated_sources: &Path,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, String> {
    let actual_shared = materialize_renderer_sources(
        shared_dir,
        generated_sources,
        configuration.profile,
        dependency,
    )?;
    compile_renderer_capture(
        main_name,
        source,
        options,
        local_dir,
        &actual_shared,
        constants,
        ordinary_dirs,
        configuration.capture.as_ref(),
        configuration,
        None,
        dependency,
    )
    .map_err(|e| e.to_string())
}
#[allow(clippy::too_many_arguments)]
fn compile_renderer_capture(
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
    let mut loader = |name: &str| {
        if name == "gpu_capture_config" {
            return Ok(rcc::frontend::ModuleSource {
                source: capture_config_source(capture.map(|c| &c.manifest_id), configuration),
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
        let pinned = matches!(name, "device_abi" | "gpu_capture_abi" | "profiler_gate");
        let local = if pinned { shared_dir } else { local_dir };
        let dirs = if pinned { &[][..] } else { ordinary_dirs };
        load_module_source_with_sources(local, shared_dir, constants, name, dirs, dependency)
    };
    let approved = [shared_dir.join("gpu_pipeline.rs")];
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

fn capture_config_source(
    manifest: Option<&[u16; 16]>,
    configuration: &RendererCaptureConfiguration,
) -> String {
    let mut source = format!(
        "pub const GPU_CAPTURE_CONFIG_ENABLED:u16={};\npub fn gpu_capture_manifest_word(index:u16)->u16{{\n",
        u16::from(manifest.is_some())
    );
    if let Some(words) = manifest {
        for (index, word) in words.iter().enumerate() {
            source.push_str(&format!("if index=={index}{{return {word};}}\n"));
        }
    }
    source.push_str("0\n}\n");
    source.push_str(&format!(
        "pub const GPU_CAPTURE_CONFIG_LAYOUT_ABI:u16={};\n",
        configuration.profile.layout_abi()
    ));
    source.push_str(&configuration.asset_check_source());
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
    if name == "gpu_capture_config" {
        return Ok((
            capture_config_source(
                None,
                &RendererCaptureConfiguration::without_capture(RendererLayoutProfile::Row),
            ),
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
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if local_dir == shared_dir && ordinary_dirs.is_empty() {
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
    let result = std::fs::read_to_string(&shared);
    if result
        .as_ref()
        .is_err_and(|e| e.kind() == ErrorKind::NotFound)
    {
        for dir in ordinary_dirs {
            dependency(shared_dir);
            let path = dir.join(format!("{name}.rs"));
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    dependency(&path);
                    return Ok((text, Some(path)));
                }
                Err(e) if e.kind() == ErrorKind::NotFound => {
                    dependency(dir);
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
