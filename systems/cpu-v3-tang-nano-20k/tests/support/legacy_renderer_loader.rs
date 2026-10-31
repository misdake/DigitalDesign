//! Test-only compiler for frozen legacy contracts; never a boot/frontend option.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    std::fs::create_dir_all(&target).unwrap();
    for folder in [
        shared.to_path_buf(),
        system.join("tests/fixtures/legacy_renderer"),
    ] {
        for entry in std::fs::read_dir(folder).unwrap() {
            let source = entry.unwrap().path();
            if source.extension().is_some_and(|e| e == "rs") {
                std::fs::copy(&source, target.join(source.file_name().unwrap())).unwrap();
            }
        }
    }
    target
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
    let from_shared = local_dir == shared_dir;
    let legacy = legacy_test_sources(shared_dir);
    let shared_dir = legacy.as_path();
    let local_dir = if from_shared { shared_dir } else { local_dir };
    compile_renderer_capture(
        "gpu_pipeline",
        main_name,
        source,
        options,
        local_dir,
        shared_dir,
        constants,
        ordinary_dirs,
        None,
        &RendererCaptureConfiguration::with_default_hook(RendererLayoutProfile::Column16),
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
    compile_renderer_program_checked_with_sources(
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

/// Checked resource callers may import approved ordinary application modules.
#[allow(clippy::too_many_arguments)]
pub fn compile_renderer_program_checked_with_sources(
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
    let from_shared = local_dir == shared_dir;
    let legacy = legacy_test_sources(shared_dir);
    let shared_dir = legacy.as_path();
    let actual_shared = materialize_renderer_sources(
        shared_dir,
        &check.directory.join("renderer-rcc"),
        configuration.profile,
        dependency,
    )?;
    let local_dir = if from_shared {
        actual_shared.as_path()
    } else {
        local_dir
    };
    compile_renderer_capture(
        "gpu_pipeline",
        main_name,
        source,
        options,
        local_dir,
        &actual_shared,
        constants,
        ordinary_dirs,
        configuration.capture.as_ref(),
        configuration,
        Some(check),
        dependency,
    )
    .map_err(|e| e.to_string())
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
    let from_shared = local_dir == shared_dir;
    let legacy = legacy_test_sources(shared_dir);
    let shared_dir = legacy.as_path();
    let local_dir = if from_shared { shared_dir } else { local_dir };
    compile_renderer_capture(
        "gpu_pipeline",
        main_name,
        source,
        options,
        local_dir,
        shared_dir,
        constants,
        ordinary_dirs,
        capture,
        &RendererCaptureConfiguration::without_capture(RendererLayoutProfile::Column16),
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
    let from_shared = local_dir == shared_dir;
    let legacy = legacy_test_sources(shared_dir);
    let shared_dir = legacy.as_path();
    let actual_shared = materialize_renderer_sources(
        shared_dir,
        generated_sources,
        configuration.profile,
        dependency,
    )?;
    let local_dir = if from_shared {
        actual_shared.as_path()
    } else {
        local_dir
    };
    compile_renderer_capture(
        "gpu_pipeline",
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
