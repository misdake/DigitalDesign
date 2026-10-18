#![allow(dead_code, hidden_glob_reexports, unused_imports)]
pub use cpu_v3::*;
#[path = "src/boot/mod.rs"]
mod boot;
use cpu_v3::rcc_backend::{self, CompilerOptions};
use rcc::frontend::compile_program_named;
use std::path::{Path, PathBuf};
fn compile(path: &Path, options: &CompilerOptions) -> Vec<u16> {
    println!("cargo:rerun-if-changed={}", path.display());
    let source = std::fs::read_to_string(path).unwrap();
    let source_dir = path.parent().unwrap();
    let program =
        compile_program_named(&path.display().to_string(), &source, options, &mut |name| {
            let module = source_dir.join(format!("{name}.rs"));
            println!("cargo:rerun-if-changed={}", module.display());
            std::fs::read_to_string(&module).map_err(|error| format!("read module {name}: {error}"))
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
    let source = boot::single_application_source(&std::fs::read_to_string(config).unwrap())
        .expect("single application project");
    let mut options = CompilerOptions::for_separate_code_and_data_segments(0);
    options.stack_init = boot::SINGLE_BOOT_STACK;
    let words = compile(&root.join(&source), &options);
    let code: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
    let image = boot::pack_single_application(&code).expect("single application image");
    std::fs::write(output.join("application.v3bin"), &code).unwrap();
    std::fs::write(output.join("cpu-v3-boot.bin"), &image).unwrap();
    let map=format!("format=5\napplication={}\ncode_segment=7\nentry=0\ndata_pages=0,1,2,3\nstack=0xc000\ncode_bytes={}\nstatic_zero_bytes=16384\nflash_base=0x100000\npayload_offset=128\n",source.display(),code.len());
    std::fs::write(output.join("cpu-v3-boot.map"), map).unwrap();
    let assets = ["application.v3bin", "cpu-v3-boot.bin", "cpu-v3-boot.map"];
    let mut bindings = String::from("const ASSETS: &[(&str, &[u8])] = &[\n");
    for name in assets {
        bindings.push_str(&format!(
            "({name:?},include_bytes!(concat!(env!(\"OUT_DIR\"),\"/{name}\"))),\n"
        ));
    }
    bindings.push_str("];\n");
    std::fs::write(output.join("boot_asset_bindings.rs"), bindings).unwrap();
}
