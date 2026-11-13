//! Test-only compiler for frozen legacy contracts; never a boot/frontend option.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Frozen renderers still name the removed capture switch; production has no capture.
pub(super) fn profile_compatibility(mut source: String) -> String {
    source.push_str("\npub const GPU_CAPTURE_CONFIG_ENABLED:u16=0;\n");
    source
}

/// Compose current common modules with byte-preserved retired fixture sources.
/// Explicit custom fixture roots retain their own identity and overrides.
pub fn legacy_test_sources(shared: &Path) -> PathBuf {
    if shared.join("gpu_pipeline.rs").is_file() {
        return shared.to_path_buf();
    }
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let system = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target = system
        .join("../../target/legacy-renderer-tests")
        .join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    snapshot_legacy_sources(shared, &target, &mut |_| {}).unwrap()
}

/// Snapshot only retired contracts that need a physical approved overlay.
/// Ordinary production compilation reads its canonical source root directly.
pub fn snapshot_legacy_sources(
    shared: &Path,
    generated: &Path,
    dependency: &mut dyn FnMut(&Path),
) -> Result<PathBuf, String> {
    let shared = std::fs::canonicalize(shared).map_err(|error| error.to_string())?;
    let mut roots = vec![shared.clone()];
    if !shared.join("gpu_pipeline.rs").is_file() {
        roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy_renderer"));
    }
    let application_support = shared.join("../applications/rendering/rcc");
    if application_support.is_dir() {
        roots.push(application_support);
    }
    std::fs::create_dir_all(generated).map_err(|error| error.to_string())?;
    let generated = std::fs::canonicalize(generated).map_err(|error| error.to_string())?;
    let roots = roots
        .into_iter()
        .map(std::fs::canonicalize)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    for root in &roots {
        if generated.starts_with(root) || root.starts_with(&generated) {
            return Err("legacy snapshot must be separate from source authority".into());
        }
    }
    let mut origins = std::collections::BTreeMap::new();
    for root in roots {
        for entry in std::fs::read_dir(root).map_err(|error| error.to_string())? {
            let input = entry.map_err(|error| error.to_string())?.path();
            if input.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            dependency(&input);
            let name = input.file_name().unwrap();
            let destination = generated.join(name);
            let bytes = std::fs::read(&input).map_err(|error| error.to_string())?;
            if std::fs::read(&destination).ok().as_deref() != Some(&bytes) {
                std::fs::write(destination, bytes).map_err(|error| error.to_string())?;
            }
            origins.insert(name.to_string_lossy().into_owned(), input);
        }
    }
    let bytes = serde_json::to_vec_pretty(&origins).map_err(|error| error.to_string())?;
    let map = generated.join("source-origins.json");
    if std::fs::read(&map).ok().as_deref() != Some(&bytes) {
        std::fs::write(map, bytes).map_err(|error| error.to_string())?;
    }
    Ok(generated)
}

#[allow(clippy::too_many_arguments)]
pub fn compile_renderer_program(
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local: &Path,
    shared: &Path,
    constants: &str,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    compile_renderer_program_with_sources(
        main_name,
        source,
        options,
        local,
        shared,
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
    local: &Path,
    shared: &Path,
    constants: &str,
    ordinary_dirs: &[PathBuf],
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, rcc::frontend::CompileError> {
    let legacy = legacy_test_sources(shared);
    let local = if local == shared {
        legacy.as_path()
    } else {
        local
    };
    GpuSources {
        local_dir: local,
        shared_dir: &legacy,
        constants,
        ordinary_dirs,
    }
    .compile(
        "gpu_pipeline",
        main_name,
        source,
        options,
        None,
        dependency,
        &mut |_, _| {},
    )
}
#[allow(clippy::too_many_arguments)]
pub fn compile_renderer_program_checked(
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local: &Path,
    shared: &Path,
    constants: &str,
    check: &rcc::frontend::rustc_check::RustcCheck,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, String> {
    compile_renderer_program_checked_with_sources(
        main_name,
        source,
        options,
        local,
        shared,
        constants,
        &[],
        check,
        dependency,
    )
}
#[allow(clippy::too_many_arguments)]
pub fn compile_renderer_program_checked_with_sources(
    main_name: &str,
    source: &str,
    options: &impl rcc::RccConfig,
    local: &Path,
    shared: &Path,
    constants: &str,
    ordinary_dirs: &[PathBuf],
    check: &rcc::frontend::rustc_check::RustcCheck,
    dependency: &mut dyn FnMut(&Path),
) -> Result<rcc::frontend::Program, String> {
    let legacy =
        snapshot_legacy_sources(shared, &check.directory.join("renderer-rcc"), dependency)?;
    let local = if local == shared {
        legacy.as_path()
    } else {
        local
    };
    GpuSources {
        local_dir: local,
        shared_dir: &legacy,
        constants,
        ordinary_dirs,
    }
    .compile(
        "gpu_pipeline",
        main_name,
        source,
        options,
        Some(check),
        dependency,
        &mut |_, _| {},
    )
    .map_err(|e| e.to_string())
}
