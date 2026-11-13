//! Additional application directories never acquire private GPU-library authority.
use super::shared_build_support_rcc_modules as resolver;
use cpu_v3::rcc_backend::CompilerOptions;
use rcc::frontend::rustc_check::RustcCheck;
use std::path::Path;
#[test]
fn explicit_gpu_ordinary_helpers_use_only_public_factories() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::path::PathBuf::from(std::env::var_os("RCC_RUSTC_TEST_DIR").unwrap())
        .join("api-ordinary-directories");
    let local = output.join("application");
    let ordinary = output.join("ordinary");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::create_dir_all(&ordinary).unwrap();

    for (label,helper,allowed) in [
        ("public","mod gpu_api;pub fn surface()->ColorBuffer{ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)})}",true),
        ("private","mod gpu_api;pub fn surface()->ColorBuffer{gpu_execute_init();ColorBuffer::from_memory(unsafe{GpuMemory::from_sectors(0x8000,2048)})}",false),
    ]{
        std::fs::write(ordinary.join("scene_factory.rs"),helper).unwrap();
        let result=resolver::compile_explicit_gpu_program_checked_with_sources(
            "ordinary application", "mod gpu_api;mod scene_factory;fn main(){let color=surface();drop(color);halt(0);}",
            &CompilerOptions::for_separate_code_and_data_segments(0),&local,&package.join("rcc"),"",std::slice::from_ref(&ordinary),
            &RustcCheck::new(output.join(label)),&mut |_|{},
        );
        if allowed {result.unwrap_or_else(|error|panic!("{error}"));}
        else {assert!(result.err().expect("ordinary helper gained private authority").contains("private"));}
    }
}
