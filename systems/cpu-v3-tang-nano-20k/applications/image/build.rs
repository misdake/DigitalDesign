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
    let package = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = package.parent().unwrap().parent().unwrap().to_owned();
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let mut application = applications::prepare(&root, &output);
    std::fs::write(output.join("boot_resources.rs"), &application.constants).unwrap();
    let resources = application
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
    let (program, static_words) = application.compile(&root, &output, &options);
    let source = &application.project.application;
    let code: Vec<u8> = program
        .words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect();
    let image = boot::pack_application_resources(&code, static_words, &resources)
        .expect("single application image");
    std::fs::write(output.join("application.v3bin"), &code).unwrap();
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
        "application.dbg",
        "application.asm",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
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
}
