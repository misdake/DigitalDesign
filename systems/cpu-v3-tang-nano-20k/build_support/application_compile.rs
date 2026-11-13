//! Approved application source loading, entry and diagnostic specialization.
use super::{ApplicationFrontend, ApplicationSource};
use crate::artifact::Inputs;
use crate::{application_entry, rcc_modules};
use cpu_v3::rcc_backend::{self, CompilerOptions};
use cpu_v3_tools::diagnostics::BuildDiagnostic;
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
) -> Result<CompiledApplication, BuildDiagnostic> {
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
    let source = std::fs::read_to_string(path).map_err(|e| BuildDiagnostic::new("source", e))?;
    let source = if explicit_gpu {
        application_entry::select(&source, entry).map_err(|e| BuildDiagnostic::new("entry", e))?
    } else {
        source
    };
    let entry = if explicit_gpu { "main" } else { entry };
    let source_dir = path.parent().unwrap();
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
        rcc_modules::GpuSources {
            local_dir: source_dir,
            shared_dir: shared,
            constants,
            ordinary_dirs: &policy.ordinary_dirs,
        }
        .compile_checked_source(
            &path.display().to_string(),
            &source,
            options,
            &rcc::frontend::rustc_check::RustcCheck::new(
                std::env::var_os("CPU_V3_BUILD_DIAGNOSTIC")
                    .and_then(|path| {
                        PathBuf::from(path)
                            .parent()
                            .map(|parent| parent.join("rustc"))
                    })
                    .unwrap_or_else(|| {
                        PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("member-ownership")
                    }),
            ),
            &mut |module| dependencies.push(module.to_owned()),
            &mut |name, module| inputs.module(name, module),
        )
        .map_err(|error| match error {
            rcc_modules::GpuCompileError::Root(error) => BuildDiagnostic::new("source", error),
            rcc_modules::GpuCompileError::Source(error) => source_failure(error),
        })
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
        .map_err(source_failure)
    }?;
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
        .map_err(|e| BuildDiagnostic::new("asset relocation", e))?;
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
    let program = rcc_backend::try_compile(program, options, entry)
        .map_err(|e| BuildDiagnostic::new("target compilation", e))?;
    Ok(CompiledApplication {
        options: options.clone(),
        program,
        static_words,
        static_data,
        assets,
        buffers,
    })
}

fn source_failure(error: rcc::frontend::CompileError) -> BuildDiagnostic {
    let location = error
        .location()
        .map(|(file, line, column)| (file.to_owned(), line, column));
    let mut result = BuildDiagnostic::new("compilation", error);
    result.location = location;
    result
}

fn asset_banks(
    reserved: &[crate::boot::BootResource],
) -> Vec<rcc::frontend::static_assets::AssetBank> {
    use crate::rcc_modules::layout;
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
