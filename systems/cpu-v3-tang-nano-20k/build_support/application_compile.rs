//! Approved application source loading, entry and diagnostic specialization.
use super::{ApplicationFrontend, ApplicationProfile};
use crate::{application_entry, rcc_modules};
use cpu_v3::rcc_backend::{self, CompilerOptions};
use rcc::frontend::compile_program_named;
use std::path::{Path, PathBuf};

pub(super) fn compile_program(
    path: &Path,
    options: &CompilerOptions,
    constants: &str,
    shared: &Path,
    frontend: ApplicationFrontend,
    capture_enabled: bool,
    profile: ApplicationProfile,
) -> (Vec<u16>, usize) {
    let explicit_gpu = matches!(frontend, ApplicationFrontend::ExplicitGpu);
    let renderer = explicit_gpu;
    let rtt = matches!(
        profile,
        ApplicationProfile::Rtt400x240
            | ApplicationProfile::BloxorzCapture
            | ApplicationProfile::BloxorzCaptureFull
    );
    let entry = match profile {
        ApplicationProfile::BloxorzCapture => "renderer_game_capture",
        ApplicationProfile::BloxorzCaptureFull => "renderer_game_capture_full",
        _ => "main",
    };
    println!("cargo:rerun-if-changed={}", path.display());
    let source = std::fs::read_to_string(path).unwrap();
    let source = if explicit_gpu {
        application_entry::select(&source, entry).expect("explicit GPU application entry")
    } else {
        source
    };
    let entry = if explicit_gpu { "main" } else { entry };
    let source_dir = path.parent().unwrap();
    let hook_enabled = !matches!(profile, ApplicationProfile::Handheld)
        && std::env::var("GPU_PROFILER_HOOK").as_deref() != Ok("0");
    let capture = if capture_enabled && hook_enabled {
        let manifest_path = PathBuf::from(
            std::env::var_os("GPU_PROFILER_MANIFEST")
                .expect("gpu_profiler_board requires GPU_PROFILER_MANIFEST"),
        );
        println!("cargo:rerun-if-changed={}", manifest_path.display());
        Some(
            rcc_modules::RendererCaptureConfiguration::from_manifest(
                source_dir.join("profiler_gate.rs"),
                &manifest_path,
            )
            .expect("explicit GPU profiler identity"),
        )
    } else if renderer && hook_enabled {
        if let Some(path) = std::env::var_os("GPU_PROFILER_MANIFEST") {
            let path = PathBuf::from(path);
            println!("cargo:rerun-if-changed={}", path.display());
            Some(
                rcc_modules::RendererCaptureConfiguration::from_manifest(
                    shared.join("profiler_gate.rs"),
                    &path,
                )
                .expect("GPU profiler build identity"),
            )
        } else {
            Some(
                rcc_modules::RendererCaptureConfiguration::with_default_hook(
                    rcc_modules::RendererLayoutProfile::Column16,
                ),
            )
        }
    } else {
        None
    };
    let native_root;
    let shared = if matches!(profile, ApplicationProfile::ProfilerMemcpy) {
        assert!(
            capture_enabled,
            "MEMCPY specialization requires the explicit diagnostic"
        );
        let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let helper = root.join("examples/gpu_profiler_board/tools/native_sources.py");
        println!("cargo:rerun-if-changed={}", helper.display());
        println!(
            "cargo:rerun-if-changed={}",
            root.join(if explicit_gpu {
                "tests/fixtures/capture_native_explicit_tail.rcc"
            } else {
                "tests/fixtures/capture_native_tail.rcc"
            })
            .display()
        );
        for file in std::fs::read_dir(shared).unwrap().flatten() {
            if file.path().extension().is_some_and(|s| s == "rs") {
                println!("cargo:rerun-if-changed={}", file.path().display());
            }
        }
        native_root =
            PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("profiler-memcpy-rcc");
        let mut producer = std::process::Command::new("python");
        producer
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .arg(helper)
            .arg("--shared")
            .arg(shared)
            .arg("--output")
            .arg(&native_root);
        if explicit_gpu {
            producer.arg("--explicit-api");
        }
        let status = producer
            .status()
            .expect("native diagnostic source producer");
        assert!(status.success(), "native diagnostic specialization failed");
        native_root.as_path()
    } else {
        shared
    };
    let program = if explicit_gpu {
        let dirs = if source_dir.ends_with("gpu_renderer_bloxorz/rcc") {
            vec![shared.parent().unwrap().join("examples/bloxorz/rcc")]
        } else if source_dir.ends_with("gpu_profiler_board/rcc") && rtt {
            vec![shared
                .parent()
                .unwrap()
                .join("examples/gpu_renderer_rtt/rcc")]
        } else {
            vec![]
        };
        let disabled = rcc_modules::RendererCaptureConfiguration::without_capture(
            rcc_modules::RendererLayoutProfile::Column16,
        );
        rcc_modules::compile_explicit_gpu_program_checked_with_sources(
            &path.display().to_string(),
            &source,
            options,
            source_dir,
            shared,
            constants,
            &dirs,
            capture.as_ref().unwrap_or(&disabled),
            &rcc::frontend::rustc_check::RustcCheck::new(
                PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("member-ownership"),
            ),
            &mut |module| println!("cargo:rerun-if-changed={}", module.display()),
        )
    } else {
        compile_program_named(&path.display().to_string(), &source, options, &mut |name| {
            rcc_modules::load_module(source_dir, shared, constants, name, &mut |module| {
                println!("cargo:rerun-if-changed={}", module.display());
            })
        })
        .map_err(|e| e.to_string())
    }
    .unwrap_or_else(|error| panic!("compile {}: {error}", path.display()));
    let static_words = program.debug.static_data_end;
    let words = rcc_backend::try_compile(program, options, entry)
        .unwrap()
        .words;
    (words, static_words)
}
