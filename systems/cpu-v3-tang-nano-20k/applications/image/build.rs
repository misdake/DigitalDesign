#![allow(dead_code, unused_imports)]
use cpu_v3::rcc_backend::CompilerOptions;
use cpu_v3_boot_format as boot;
use rcc_modules::capture_profile::layout as gpu_layout;
use std::path::PathBuf;
#[path = "../../build_support/application_entry.rs"]
mod application_entry;
#[path = "../../build_support/applications.rs"]
mod applications;
#[path = "../../build_support/artifact.rs"]
mod artifact;
#[path = "../../build_support/rcc_modules.rs"]
mod rcc_modules;
fn main() {
    // Recursive frontend passes handle imported syntax and bounded const calls.
    // Windows' small main-thread stack must not limit ordinary application size.
    let result = std::thread::Builder::new()
        .name("application build".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(build)
        .expect("start application compiler")
        .join()
        .unwrap_or_else(|failure| std::panic::resume_unwind(failure));
    if let Err(error) = result {
        error.report();
        std::process::exit(1);
    }
}
fn build() -> Result<(), cpu_v3_tools::diagnostics::BuildDiagnostic> {
    let package = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = package.parent().unwrap().parent().unwrap().to_owned();
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let mut application = applications::prepare(&root, &output);
    std::fs::write(output.join("boot_resources.rs"), &application.constants).unwrap();
    let mut resources = application
        .project
        .resources(&root)
        .expect("boot resources");
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = boot::SINGLE_BOOT_STACK;
    println!("cargo:rerun-if-env-changed=RCC_GPU_DEBUG");
    options.gpu_debug = match std::env::var("RCC_GPU_DEBUG") {
        Err(std::env::VarError::NotPresent) => false,
        Ok(value) if value == "0" => false,
        Ok(value) if value == "1" => true,
        _ => panic!("RCC_GPU_DEBUG must be 0 or 1"),
    };
    let compiled = application.compile(&root, &output, &options, &resources)?;
    let buffers: Vec<_> = compiled
        .buffers
        .iter()
        .map(|b| {
            serde_json::json!({
                "name": b.name, "address": b.address, "bytes": b.bytes, "alignment": b.alignment,
            })
        })
        .collect();
    std::fs::write(
        output.join("application.buffers.json"),
        serde_json::to_vec_pretty(&buffers).unwrap(),
    )
    .unwrap();
    let options = compiled.options;
    let program = compiled.program;
    let static_words = compiled.static_words;
    resources.extend(compiled.assets);
    let source = &application.project.application;
    let code: Vec<u8> = program
        .words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let image =
        boot::pack_application_with_data(&code, static_words, &compiled.static_data, &resources)
            .expect("single application image");
    std::fs::write(output.join("application.v3bin"), &code).unwrap();
    std::fs::write(
        output.join("application.static-data"),
        &compiled.static_data,
    )
    .unwrap();
    std::fs::write(
        output.join("application.static-words"),
        format!("{static_words}\n"),
    )
    .unwrap();
    std::fs::write(output.join("cpu-v3-boot.bin"), &image).unwrap();
    let map = format!(
        "application={}\nrcc_gpu_debug={}\n{}",
        source.display(),
        options.gpu_debug,
        boot::boot_image_map(&code, &image)
    );
    std::fs::write(output.join("cpu-v3-boot.map"), map).unwrap();
    std::fs::write(
        output.join("application.dbg"),
        artifact::debug_text(&program),
    )
    .unwrap();
    std::fs::write(output.join("application.asm"), &program.listing).unwrap();
    let mut assets: Vec<String> = [
        "application.v3bin",
        "cpu-v3-boot.bin",
        "cpu-v3-boot.map",
        "boot_resources.rs",
        "application.static-words",
        "application.static-data",
        "application.dbg",
        "application.asm",
        "application.buffers.json",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    // One complete existing-input recipe includes both declared and source assets.
    let mut repack = String::from("format 2\napplication app.rs\n");
    std::fs::create_dir_all(output.join("application.resources")).unwrap();
    for (index, resource) in resources.iter().enumerate() {
        if resource.zero_bytes != 0 {
            repack.push_str(&format!(
                "zero {:#x} {}\n",
                resource.address, resource.zero_bytes
            ));
        } else {
            let name = format!("application.resources/{index}.bin");
            std::fs::write(output.join(&name), &resource.data).unwrap();
            repack.push_str(&format!(
                "copy RESOURCE_{index} {:#x} {name}\n",
                resource.address
            ));
            assets.push(name);
        }
    }
    std::fs::write(output.join("application.repack.conf"), repack).unwrap();
    assets.push("application.repack.conf".into());
    assets.extend(application.extra_assets.iter().cloned());
    artifact::write(
        artifact::BuildRecord {
            output: &output,
            inputs: &application.inputs,
            program: &program,
            static_words,
            application: application.description(&root),
            resources: &resources,
            options: &options,
        },
        &mut assets,
    );
    let mut bindings = String::from("const ASSETS: &[(&str, &[u8])] = &[\n");
    for name in assets {
        bindings.push_str(&format!(
            "({name:?},include_bytes!(concat!(env!(\"OUT_DIR\"),\"/{name}\"))),\n"
        ));
    }
    bindings.push_str("];\n");
    std::fs::write(output.join("boot_asset_bindings.rs"), bindings).unwrap();
    Ok(())
}
