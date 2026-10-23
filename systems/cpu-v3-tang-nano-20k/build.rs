#![allow(dead_code, hidden_glob_reexports, unused_imports)]
pub use cpu_v3::*;
#[path = "src/boot/mod.rs"]
mod boot;
#[path = "rcc/gpu_layout.rs"]
mod gpu_layout;
use cpu_v3::rcc_backend::{self, CompilerOptions};
use rcc::frontend::compile_program_named;
use std::path::{Path, PathBuf};
#[path = "build_support/rcc_modules.rs"]
mod rcc_modules;
fn compile(path: &Path, options: &CompilerOptions, constants: &str, shared: &Path) -> Vec<u16> {
    println!("cargo:rerun-if-changed={}", path.display());
    let source = std::fs::read_to_string(path).unwrap();
    let source_dir = path.parent().unwrap();
    let program =
        compile_program_named(&path.display().to_string(), &source, options, &mut |name| {
            rcc_modules::load_module(source_dir, shared, constants, name, &mut |module| {
                println!("cargo:rerun-if-changed={}", module.display());
            })
        })
        .unwrap_or_else(|error| panic!("compile {}: {error}", path.display()));
    rcc_backend::try_compile(program, options, "main")
        .unwrap()
        .words
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let config = root.join("boot-applications.conf");
    println!("cargo:rerun-if-changed={}", config.display());
    let project = boot::parse_boot_project(&std::fs::read_to_string(config).unwrap())
        .expect("single application project");
    let source = &project.application;
    for (_, _, path) in &project.copies {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }
    let constants = project.constants();
    std::fs::write(output.join("boot_resources.rs"), &constants).unwrap();
    let resources = project.resources(&root).expect("boot resources");
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = boot::SINGLE_BOOT_STACK;
    let words = compile(&root.join(source), &options, &constants, &root.join("rcc"));
    let code: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    let image =
        boot::pack_application_resources(&code, &resources).expect("single application image");
    std::fs::write(output.join("application.v3bin"), &code).unwrap();
    std::fs::write(output.join("cpu-v3-boot.bin"), &image).unwrap();
    let map = format!(
        "application={}\n{}",
        source.display(),
        boot::boot_image_map(&code, &image)
    );
    std::fs::write(output.join("cpu-v3-boot.map"), map).unwrap();
    let assets = [
        "application.v3bin",
        "cpu-v3-boot.bin",
        "cpu-v3-boot.map",
        "boot_resources.rs",
    ];
    let mut bindings = String::from("const ASSETS: &[(&str, &[u8])] = &[\n");
    for name in assets {
        bindings.push_str(&format!(
            "({name:?},include_bytes!(concat!(env!(\"OUT_DIR\"),\"/{name}\"))),\n"
        ));
    }
    bindings.push_str("];\n");
    std::fs::write(output.join("boot_asset_bindings.rs"), bindings).unwrap();
}
