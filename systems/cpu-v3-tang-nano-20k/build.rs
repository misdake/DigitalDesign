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
fn compile(
    path: &Path,
    options: &CompilerOptions,
    constants: &str,
    shared: &Path,
) -> (Vec<u16>, usize) {
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
    let static_words = program.debug.static_data_end;
    let words = rcc_backend::try_compile(program, options, "main")
        .unwrap()
        .words;
    (words, static_words)
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let config = root.join("boot-applications.conf");
    println!("cargo:rerun-if-changed={}", config.display());
    let mut project = boot::parse_boot_project(&std::fs::read_to_string(config).unwrap())
        .expect("single application project");
    println!("cargo:rerun-if-env-changed=CPU_V3_BOOT_EXAMPLE");
    let mut generated = String::new();
    let mut extra_assets: Vec<String> = Vec::new();
    if let Some(example) = std::env::var_os("CPU_V3_BOOT_EXAMPLE") {
        if example == "dualshock2" {
            project.application = PathBuf::from("examples/dualshock2/rcc/main.rs");
            project.copies.clear();
            project.zeros.clear();
        } else {
            assert_eq!(example, "gpu_demo", "unsupported CPU_V3_BOOT_EXAMPLE");
            let example_dir = root.join("examples/gpu_demo");
            for input in [
                example_dir.join("tools/assets.py"),
                example_dir.join("assets/font-8x16.txt"),
                example_dir.join("assets/scene.json"),
                root.join("../../ip/gpu-v2/examples/vliw_integration/solid_scene.py"),
                root.join("../../ip/gpu-v2/examples/vliw_integration/material_scene.py"),
                root.join("../../ip/gpu-v2/examples/vliw_integration/assets/room-tiles.png"),
            ] {
                println!("cargo:rerun-if-changed={}", input.display());
            }
            let assets = output.join("gpu-demo-assets");
            let status = std::process::Command::new("python")
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .arg(example_dir.join("tools/assets.py"))
                .arg("--output")
                .arg(&assets)
                .status()
                .expect("demo native asset packer");
            assert!(status.success(), "demo native asset validation failed");
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap())
                    .unwrap();
            project.application = PathBuf::from("examples/gpu_demo/rcc/main.rs");
            project.copies = manifest["resources"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        r["file"]
                            .as_str()
                            .unwrap()
                            .trim_end_matches(".bin")
                            .to_uppercase(),
                        r["address"].as_u64().unwrap() as usize,
                        assets.join(r["file"].as_str().unwrap()),
                    )
                })
                .collect();
            project.zeros.clear();
            generated = std::fs::read_to_string(assets.join("demo_assets.rs")).unwrap();
            let mut repack = format!("format 2\napplication {}\n", project.application.display());
            for (name, address, path) in &project.copies {
                let relative = path
                    .strip_prefix(&output)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                repack.push_str(&format!("copy {name} {address:#x} {relative}\n"));
                extra_assets.push(relative);
            }
            std::fs::write(output.join("demo-repack.conf"), repack).unwrap();
            extra_assets.extend([
                "gpu-demo-assets/assets.json".to_owned(),
                "demo-repack.conf".to_owned(),
            ]);
        }
    }
    let source = &project.application;
    for (_, _, path) in &project.copies {
        // Generated OUT_DIR resources depend on the audited producer inputs.
        if !path.starts_with(&output) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    let constants = project.constants() + &generated;
    std::fs::write(output.join("boot_resources.rs"), &constants).unwrap();
    let resources = project.resources(&root).expect("boot resources");
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = boot::SINGLE_BOOT_STACK;
    let (words, static_words) =
        compile(&root.join(source), &options, &constants, &root.join("rcc"));
    let code: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
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
        "application={}\n{}",
        source.display(),
        boot::boot_image_map(&code, &image)
    );
    std::fs::write(output.join("cpu-v3-boot.map"), map).unwrap();
    let mut assets: Vec<String> = [
        "application.v3bin",
        "cpu-v3-boot.bin",
        "cpu-v3-boot.map",
        "boot_resources.rs",
        "application.static-words",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assets.extend(extra_assets);
    let mut bindings = String::from("const ASSETS: &[(&str, &[u8])] = &[\n");
    for name in assets {
        bindings.push_str(&format!(
            "({name:?},include_bytes!(concat!(env!(\"OUT_DIR\"),\"/{name}\"))),\n"
        ));
    }
    bindings.push_str("];\n");
    std::fs::write(output.join("boot_asset_bindings.rs"), bindings).unwrap();
}
