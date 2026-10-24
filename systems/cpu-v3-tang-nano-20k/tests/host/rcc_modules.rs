//! The build script and these tests execute the same system module resolver.
use super::shared_build_support_rcc_modules as resolver;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    local: PathBuf,
    shared: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/rcc-module-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        let local = root.join("app");
        let shared = root.join("shared");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        Self {
            root,
            local,
            shared,
        }
    }
    fn load(&self, name: &str) -> (Result<String, String>, Vec<PathBuf>) {
        let mut deps = vec![];
        let r = resolver::load_module(
            &self.local,
            &self.shared,
            "const GENERATED:u16=7;",
            name,
            &mut |p| deps.push(p.into()),
        );
        (r, deps)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let root = std::fs::canonicalize(&self.root).unwrap();
        let base = std::fs::canonicalize(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/rcc-module-tests"),
        )
        .unwrap();
        assert_eq!(root.parent(), Some(base.as_path()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn local_overrides_shared_and_missing_local_override_is_observed() {
    let f = Fixture::new();
    std::fs::write(f.shared.join("value.rs"), "shared").unwrap();
    let (text, deps) = f.load("value");
    assert_eq!(text.unwrap(), "shared");
    assert_eq!(deps, vec![f.local.clone(), f.shared.join("value.rs")]);
    std::fs::write(f.local.join("value.rs"), "local").unwrap();
    let (text, deps) = f.load("value");
    assert_eq!(text.unwrap(), "local");
    assert_eq!(deps, vec![f.local.join("value.rs")]);
}
#[test]
fn non_not_found_read_and_parse_errors_do_not_use_shared() {
    let f = Fixture::new();
    std::fs::write(f.shared.join("value.rs"), "const VALUE:u16=7;").unwrap();
    std::fs::write(f.local.join("value.rs"), [255]).unwrap();
    assert!(f
        .load("value")
        .0
        .unwrap_err()
        .contains(&f.local.display().to_string()));
    std::fs::remove_file(f.local.join("value.rs")).unwrap();
    std::fs::create_dir(f.local.join("value.rs")).unwrap();
    assert!(f.load("value").0.is_err());
    std::fs::remove_dir(f.local.join("value.rs")).unwrap();
    std::fs::write(f.local.join("value.rs"), "not valid rust").unwrap();
    let opts = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    assert!(rcc::frontend::compile_program_named(
        "app",
        "mod value;fn main(){halt(VALUE);}",
        &opts,
        &mut |n| f.load(n).0
    )
    .is_err());
}
#[test]
fn missing_module_reports_both_paths_and_generated_constants_win() {
    let f = Fixture::new();
    let error = f.load("absent").0.unwrap_err();
    assert!(error.contains(&f.local.join("absent.rs").display().to_string()));
    assert!(error.contains(&f.shared.join("absent.rs").display().to_string()));
    for dir in [&f.local, &f.shared] {
        std::fs::write(dir.join("boot_resources.rs"), "invalid").unwrap();
    }
    let (text, deps) = f.load("boot_resources");
    assert_eq!(text.unwrap(), "const GENERATED:u16=7;");
    assert!(deps.is_empty());
}
#[test]
fn shared_gpu_and_time_transitive_modules_compile_from_independent_application() {
    let f = Fixture::new();
    std::fs::write(f.local.join("value.rs"), "const VALUE:u16=7;").unwrap();
    let shared = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let mut deps = vec![];
    let opts = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let p = rcc::frontend::compile_program_named(
        "example/main.rs",
        "mod gpu;mod time;mod value;fn main(){halt(VALUE+time_milliseconds());}",
        &opts,
        &mut |n| resolver::load_module(&f.local, &shared, "", n, &mut |p| deps.push(p.into())),
    )
    .unwrap();
    let p = cpu_v3::rcc_backend::try_compile(p, &opts, "main").unwrap();
    assert!(!p.words.is_empty());
    assert!(deps.contains(&shared.join("gpu.rs")));
    assert!(deps.contains(&shared.join("gpu_ui.rs")));
    assert!(deps.contains(&shared.join("device_abi.rs")));
    assert!(deps.contains(&shared.join("time.rs")));
    assert!(deps.contains(&f.local.join("value.rs")));
}
#[test]
fn default_boot_bytes_are_unchanged_from_local_only_resolution() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let project = cpu_v3_tang_nano_20k::boot::parse_boot_project(
        &std::fs::read_to_string(root.join("boot-applications.conf")).unwrap(),
    )
    .unwrap();
    let path = root.join(&project.application);
    let source = std::fs::read_to_string(&path).unwrap();
    let local = path.parent().unwrap();
    let shared = root.join("rcc");
    let constants = project.constants();
    let mut opts = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    opts.stack_init = cpu_v3_tang_nano_20k::boot::SINGLE_BOOT_STACK;
    let modern = rcc::frontend::compile_program_named("default", &source, &opts, &mut |n| {
        resolver::load_module(local, &shared, &constants, n, &mut |_| {})
    })
    .unwrap();
    let original = rcc::frontend::compile_program_named("default", &source, &opts, &mut |n| {
        if n == "boot_resources" {
            Ok(constants.clone())
        } else {
            std::fs::read_to_string(local.join(format!("{n}.rs"))).map_err(|e| e.to_string())
        }
    })
    .unwrap();
    assert_eq!(
        cpu_v3::rcc_backend::try_compile(modern, &opts, "main")
            .unwrap()
            .words,
        cpu_v3::rcc_backend::try_compile(original, &opts, "main")
            .unwrap()
            .words
    );
}
