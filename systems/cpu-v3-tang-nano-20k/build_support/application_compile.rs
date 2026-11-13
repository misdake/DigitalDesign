//! Approved application source loading, entry and diagnostic specialization.
use super::{ApplicationFrontend, ApplicationSource, CapturePolicy, SourceSpecialization};
use crate::artifact::Inputs;
use crate::{application_entry, rcc_modules};
use cpu_v3::rcc_backend::{self, CompilerOptions};
use rcc::frontend::compile_program_named;
use std::path::{Path, PathBuf};
#[path = "asset_relocation.rs"]
mod asset_relocation;

pub struct CompiledApplication {
    pub options: CompilerOptions,
    pub program: rcc_backend::CpuV3Program,
    pub static_words: usize,
    pub static_data: Vec<u8>,
    pub assets: Vec<crate::boot::BootResource>,
    pub buffers: Vec<rcc::frontend::static_buffers::StaticBuffer>,
}

pub(super) struct CompileRequest<'a> {
    pub path: &'a Path,
    pub constants: &'a str,
    pub shared: &'a Path,
    pub frontend: ApplicationFrontend,
    pub policy: &'a ApplicationSource,
    pub reserved: &'a [crate::boot::BootResource],
}

pub(super) fn compile_program(
    request: CompileRequest<'_>,
    options: &CompilerOptions,
    inputs: &mut Inputs,
) -> CompiledApplication {
    let CompileRequest {
        path,
        constants,
        shared,
        frontend,
        policy,
        reserved,
    } = request;
    let mut options = options.clone();
    options.asset_banks = asset_banks(reserved);
    let options = &options;
    let explicit_gpu = matches!(frontend, ApplicationFrontend::ExplicitGpu);
    let entry = policy.entry;
    inputs.watch(path);
    let source = std::fs::read_to_string(path).unwrap();
    let source = if explicit_gpu {
        application_entry::select(&source, entry).expect("explicit GPU application entry")
    } else {
        source
    };
    let entry = if explicit_gpu { "main" } else { entry };
    let source_dir = path.parent().unwrap();
    let hook_enabled = policy
        .capture
        .hook_enabled(std::env::var("GPU_PROFILER_HOOK").ok().as_deref());
    let capture = if policy.capture == CapturePolicy::Diagnostic && hook_enabled {
        let manifest_path = PathBuf::from(
            std::env::var_os("GPU_PROFILER_MANIFEST")
                .expect("gpu_profiler_board requires GPU_PROFILER_MANIFEST"),
        );
        inputs.watch(&manifest_path);
        Some(
            rcc_modules::RendererCaptureConfiguration::from_manifest(
                source_dir.join("profiler_gate.rs"),
                &manifest_path,
            )
            .expect("explicit GPU profiler identity"),
        )
    } else if explicit_gpu && hook_enabled {
        if let Some(path) = std::env::var_os("GPU_PROFILER_MANIFEST") {
            let path = PathBuf::from(path);
            inputs.watch(&path);
            Some(
                rcc_modules::RendererCaptureConfiguration::from_manifest(
                    shared.join("profiler_gate.rs"),
                    &path,
                )
                .expect("GPU profiler build identity"),
            )
        } else {
            Some(rcc_modules::RendererCaptureConfiguration::with_default_hook())
        }
    } else {
        None
    };
    let native_root;
    let shared = if matches!(policy.specialization, SourceSpecialization::ProfilerMemcpy) {
        assert!(
            policy.capture == CapturePolicy::Diagnostic && explicit_gpu,
            "MEMCPY specialization requires the explicit GPU diagnostic"
        );
        let root = shared.parent().expect("system RCC source directory");
        let helper = root.join("examples/gpu_profiler_board/tools/native_sources.py");
        inputs.watch(&helper);
        inputs.watch(&root.join("tests/fixtures/capture_native_explicit_tail.rcc"));
        for file in std::fs::read_dir(shared).unwrap().flatten() {
            if file.path().extension().is_some_and(|s| s == "rs") {
                inputs.watch(&file.path());
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
            .arg(&native_root)
            .arg("--explicit-api");
        let status = producer
            .status()
            .expect("native diagnostic source producer");
        assert!(status.success(), "native diagnostic specialization failed");
        native_root.as_path()
    } else {
        shared
    };
    inputs.entry(
        &path.display().to_string(),
        &rcc::frontend::ModuleSource {
            source: source.clone(),
            origin: Some(path.to_owned()),
        },
    );
    inputs.embedded_standard_library();
    let mut dependencies = vec![];
    let mut program = if explicit_gpu {
        let disabled = rcc_modules::RendererCaptureConfiguration::without_capture();
        rcc_modules::GpuSources {
            local_dir: source_dir,
            shared_dir: shared,
            constants,
            ordinary_dirs: &policy.ordinary_dirs,
            configuration: capture.as_ref().unwrap_or(&disabled),
        }
        .compile_checked(
            &path.display().to_string(),
            &source,
            options,
            &rcc::frontend::rustc_check::RustcCheck::new(
                PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("member-ownership"),
            ),
            &mut |module| dependencies.push(module.to_owned()),
            &mut |name, module| inputs.module(name, module),
        )
    } else {
        compile_program_named(&path.display().to_string(), &source, options, &mut |name| {
            let module = rcc_modules::load_module_source(
                source_dir,
                shared,
                constants,
                name,
                &mut |module| dependencies.push(module.to_owned()),
            )?;
            inputs.module(&format!("{name}.rs"), &module);
            Ok(module.source)
        })
        .map_err(|e| e.to_string())
    }
    .unwrap_or_else(|error| panic!("compile {}: {error}", path.display()));
    let static_words = program.debug.static_data_end;
    let mut static_data = vec![0; static_words * 2];
    for (address, value) in program.take_static_initializers() {
        static_data[address as usize * 2..address as usize * 2 + 2]
            .copy_from_slice(&value.to_le_bytes());
    }
    for input in &program.assets.inputs {
        inputs.watch(&input.path);
        assert_eq!(
            std::fs::read(&input.path).unwrap(),
            input.data,
            "included bytes changed during compilation"
        );
    }
    asset_relocation::apply(&mut program.assets.placed, &policy.sector_relocations)
        .expect("native asset sector relocation");
    let assets = program
        .assets
        .placed
        .iter()
        .map(|asset| crate::boot::BootResource::copy(asset.address, asset.load_data()))
        .chain(
            program
                .assets
                .buffers
                .iter()
                .map(|buffer| crate::boot::BootResource::zero(buffer.address, buffer.bytes)),
        )
        .collect();
    for path in dependencies {
        inputs.watch(&path);
    }
    let buffers = program.assets.buffers.clone();
    let program = rcc_backend::try_compile(program, options, entry).unwrap();
    CompiledApplication {
        options: options.clone(),
        program,
        static_words,
        static_data,
        assets,
        buffers,
    }
}

fn asset_banks(
    reserved: &[crate::boot::BootResource],
) -> Vec<rcc::frontend::static_assets::AssetBank> {
    use crate::rcc_modules::capture_profile::layout;
    let mut banks = Vec::new();
    for (first, end) in [
        (layout::GPU_ASSET_FIRST_SECTOR, layout::GPU_ASSET_END_SECTOR),
        (
            layout::GPU_ASSET_LOW_FIRST_SECTOR,
            layout::GPU_ASSET_LOW_END_SECTOR,
        ),
    ] {
        let mut ranges =
            std::iter::once(first as usize * 128..end as usize * 128).collect::<Vec<_>>();
        for resource in reserved {
            let occupied =
                resource.address..resource.address + resource.data.len().max(resource.zero_bytes);
            ranges = ranges
                .into_iter()
                .flat_map(|range| {
                    let mut free = Vec::new();
                    if occupied.end <= range.start || occupied.start >= range.end {
                        free.push(range);
                    } else {
                        if range.start < occupied.start {
                            free.push(range.start..occupied.start);
                        }
                        if occupied.end < range.end {
                            free.push(occupied.end..range.end);
                        }
                    }
                    free
                })
                .collect();
        }
        banks.extend(
            ranges
                .into_iter()
                .map(|bytes| rcc::frontend::static_assets::AssetBank { bytes }),
        );
    }
    banks
}
