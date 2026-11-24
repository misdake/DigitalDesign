//! Shared compiler harness for sealed target-replay tests.
use cpu_v3::rcc_backend;
use rcc::frontend::rustc_check::RustcCheck;
use std::path::Path;

pub(super) fn compile(source: &str, debug: bool, label: &str) -> rcc_backend::CpuV3Program {
    use super::shared_build_support_rcc_modules as resolver;
    let mut root = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let output = std::env::var_os("RCC_RUSTC_TEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("../../../target/renderer-stage2/checks"));
    // Parallel consumers may inject different library files under the same label.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let instance = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let directory = output.join(format!(
        "gpu-replay-{label}-{debug}-{}-{instance}",
        std::process::id()
    ));
    let local = directory.join("application");
    std::fs::create_dir_all(&local).unwrap();
    if label.starts_with("storage-failure") {
        // Test-only callee failure. The target relocation itself remains byte-exact
        // production source; this does not qualify a hardware transport error.
        let copy = directory.join("library");
        std::fs::create_dir_all(&copy).unwrap();
        for entry in std::fs::read_dir(&root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|x| x == "rs") {
                std::fs::copy(&path, copy.join(path.file_name().unwrap())).unwrap();
            }
        }
        if label.starts_with("storage-failure") {
            let storage = copy.join("gpu_storage.rs");
            let original = std::fs::read_to_string(&storage).unwrap();
            let needle = "    gpu_storage_copy_owned(src, sector, word, count, false)";
            assert_eq!(original.matches(needle).count(), 1);
            let injected = original.replace(needle,
            "    if dev_recv(7,2)!=0 {TEST_STORAGE_WRITES[0u16]+=1;if TEST_STORAGE_WRITES[0u16]==2{return false;}}\n    gpu_storage_copy_owned(src, sector, word, count, false)");
            std::fs::write(
                storage,
                format!("{injected}\nstatic TEST_STORAGE_WRITES:Buf<u16,1>=Buf::new([0;1]);\n"),
            )
            .unwrap();
            assert_eq!(
                std::fs::read(root.join("gpu_target.rs")).unwrap(),
                std::fs::read(copy.join("gpu_target.rs")).unwrap()
            );
        }
        root = copy;
    }
    let mut options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    options.gpu_debug = debug;
    let ir = resolver::compile_explicit_gpu_program_checked(
        "gpu-replay",
        source,
        &options,
        &local,
        &root,
        "",
        &RustcCheck::new(directory),
        &mut |_| {},
    )
    .unwrap();
    rcc_backend::try_compile(ir, &options, "main").unwrap()
}
pub(super) fn global(code: &rcc_backend::CpuV3Program, name: &str) -> usize {
    match code
        .debug
        .globals
        .iter()
        .find(|g| g.name == name)
        .unwrap()
        .loc
    {
        rcc::VarLoc::Global(at) => usize::from(at),
        _ => unreachable!(),
    }
}

#[path = "gpu_api_target_patch.rs"]
mod target_patch;
