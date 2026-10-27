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
    rcc::frontend::compile_program_named_with_libraries(
        main_name,
        source,
        options,
        &[shared_dir.join("gpu_pipeline.rs")],
        &mut |name| {
            if name == "gpu_capture_config" {
                return Ok(rcc::frontend::ModuleSource {
                    source: capture_config_source(capture.map(|c| &c.manifest_id)),
                    origin: None,
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
        },
    )
}

fn capture_config_source(manifest: Option<&[u16; 16]>) -> String {
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
        return Ok((capture_config_source(None), None));
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
