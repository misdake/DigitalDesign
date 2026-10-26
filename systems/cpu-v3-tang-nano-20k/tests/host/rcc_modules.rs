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

#[test]
fn source_loader_reports_the_selected_file_and_synthetic_modules_have_no_origin() {
    let f = Fixture::new();
    std::fs::write(f.shared.join("value.rs"), "shared").unwrap();
    let load = || {
        resolver::load_module_source(&f.local, &f.shared, "generated", "value", &mut |_| {})
            .unwrap()
    };
    let source = load();
    assert_eq!(source.source, "shared");
    assert_eq!(
        source.origin,
        Some(std::fs::canonicalize(f.shared.join("value.rs")).unwrap())
    );
    std::fs::write(f.local.join("value.rs"), "local").unwrap();
    let source = load();
    assert_eq!(source.source, "local");
    assert_eq!(
        source.origin,
        Some(std::fs::canonicalize(f.local.join("value.rs")).unwrap())
    );
    let source = resolver::load_module_source(
        &f.local,
        &f.shared,
        "generated",
        "boot_resources",
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(source.source, "generated");
    assert!(source.origin.is_none());
}

#[test]
fn approved_resolver_protects_actual_gpu_helpers_and_requires_the_root() {
    let f = Fixture::new();
    let production = Path::new(env!("CARGO_MANIFEST_DIR")).join("rcc");
    let helpers = [
        "gpu_device",
        "gpu_command_buffer",
        "gpu_builder",
        "gpu_display",
        "gpu_encode",
        "gpu_memory",
        "gpu_layout",
        "gpu_assets",
        "gpu_window",
        "gpu_ui",
        "gpu_ui_persistent",
        "gpu_scene",
    ];
    for name in helpers.iter().copied().chain([
        "device_abi",
        "gamepad",
        "dualshock2",
        "stick",
        "frame_stats",
    ]) {
        std::fs::copy(
            production.join(format!("{name}.rs")),
            f.shared.join(format!("{name}.rs")),
        )
        .unwrap();
    }
    // Copy the production root, adding only this trusted host's query probe.
    let root = format!(
        "{}\npub fn resolver_probe()->u16{{GPU_DEVICE}}",
        std::fs::read_to_string(production.join("gpu_pipeline.rs")).unwrap()
    );
    std::fs::write(f.shared.join("gpu_pipeline.rs"), root).unwrap();
    let opts = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let parse = |source: &str| {
        resolver::compile_renderer_program(
            "real System resolver",
            source,
            &opts,
            &f.local,
            &f.shared,
            super::gpu_pipeline::RENDERER_METADATA,
            &mut |_| {},
        )
    };
    assert!(parse("mod gpu_pipeline;fn main(){halt(resolver_probe());}").is_ok());
    std::fs::write(f.local.join("device_abi.rs"), "pub const GPU_DEVICE:u16=7;").unwrap();
    let p = cpu_v3::rcc_backend::try_compile(
        parse("mod gpu_pipeline;fn main(){halt(resolver_probe());}").unwrap(),
        &opts,
        "main",
    )
    .unwrap();
    let mut cpu = cpu_v3::CpuV3Sim::default();
    cpu.set_boot_window(&p.words);
    assert!(
        matches!(
            cpu.run(100_000).unwrap(),
            cpu_v3::RunOutcome::Halted { signal: 4, .. }
        ),
        "local ABI override was used"
    );
    for body in [
        "let p=GPU_DRIVER.as_array();",
        "let n=gpu_driver_owned_buffer();",
        "gpu_pipeline_init(0);",
        "let f=gpu_pipeline_cancel;",
    ] {
        assert!(
            parse(&format!("mod gpu_pipeline;fn main(){{{body}halt(0);}}")).is_err(),
            "accepted {body}"
        );
    }
    assert!(parse("mod gpu_device;fn main(){let p=GPU_DRIVER.as_array();}").is_err());
    std::fs::write(f.local.join("gpu_device.rs"), "pub fn fake_device(){} ").unwrap();
    let error = match parse("mod gpu_pipeline;fn main(){halt(resolver_probe());}") {
        Err(e) => e.to_string(),
        Ok(_) => panic!("local helper was trusted"),
    };
    assert!(error.contains("helper origin"), "{error}");
    std::fs::remove_file(f.local.join("gpu_device.rs")).unwrap();
    std::fs::write(
        f.local.join("gpu_pipeline.rs"),
        "pub fn resolver_probe()->u16{7}",
    )
    .unwrap();
    assert!(parse("mod gpu_pipeline;fn main(){halt(resolver_probe());}").is_err());
}

#[test]
fn renderer_application_cannot_replace_external_shared_device_constants() {
    let f = Fixture::new();
    std::fs::write(
        f.shared.join("gpu_pipeline.rs"),
        r#"#![rcc_resource_library(helper)]
mod helper;
#[rcc_resource(cleanup=owner_cleanup)]pub struct Owner{word:u16}
fn owner_cleanup(o:Owner){}
pub fn renderer_device()->u16{helper_device()}
"#,
    )
    .unwrap();
    std::fs::write(
        f.shared.join("helper.rs"),
        "mod device_abi;fn helper_device()->u16{GPU_DEVICE}",
    )
    .unwrap();
    std::fs::write(
        f.shared.join("device_abi.rs"),
        "pub const GPU_DEVICE:u16=4;",
    )
    .unwrap();
    let opts = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let result = resolver::compile_renderer_program(
        "external ABI constant collision",
        "mod gpu_pipeline;const GPU_DEVICE:u16=7;fn main(){halt(renderer_device());}",
        &opts,
        &f.local,
        &f.shared,
        "",
        &mut |_| {},
    );
    let error = match result {
        Err(error) => error.to_string(),
        Ok(_) => panic!("application constant replaced ABI used inside approved helper"),
    };
    assert!(
        error.contains("const `GPU_DEVICE` defined twice"),
        "{error}"
    );
}

#[test]
fn explicit_ordinary_source_dirs_preserve_priority_errors_and_real_origins() {
    let f = Fixture::new();
    let extra = f.root.join("ordinary");
    std::fs::create_dir_all(&extra).unwrap();
    let dirs = vec![extra.clone()];
    std::fs::write(extra.join("value.rs"), "ordinary").unwrap();
    let load = || {
        resolver::load_module_source_with_sources(
            &f.local,
            &f.shared,
            "generated",
            "value",
            &dirs,
            &mut |_| {},
        )
    };
    let result = load().unwrap();
    assert_eq!(result.source, "ordinary");
    assert_eq!(
        result.origin,
        Some(std::fs::canonicalize(extra.join("value.rs")).unwrap())
    );
    std::fs::write(f.shared.join("value.rs"), "shared").unwrap();
    assert_eq!(load().unwrap().source, "shared");
    std::fs::write(f.shared.join("value.rs"), [255]).unwrap();
    assert!(load().is_err());
    std::fs::write(f.local.join("value.rs"), "local").unwrap();
    assert_eq!(load().unwrap().source, "local");
    std::fs::write(f.local.join("value.rs"), [255]).unwrap();
    assert!(load().is_err());
    // A resource marker in an ordinary source cannot gain the approved GPU identity.
    std::fs::write(
        f.shared.join("gpu_pipeline.rs"),
        "#![rcc_resource_library()] #[rcc_resource(cleanup=drop_owner)]pub struct Owner{word:u16}fn drop_owner(o:Owner){} pub fn okay()->u16{7}",
    )
    .unwrap();
    std::fs::write(extra.join("intruder.rs"), "pub fn forged()->u16{9}").unwrap();
    let opts = cpu_v3::rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let compile = || {
        resolver::compile_renderer_program_with_sources(
            "ordinary privilege",
            "mod gpu_pipeline;mod intruder;fn main(){halt(forged());}",
            &opts,
            &f.local,
            &f.shared,
            "",
            &dirs,
            &mut |_| {},
        )
    };
    assert!(
        compile().is_ok(),
        "ordinary source or approved root did not compile"
    );
    std::fs::write(extra.join("intruder.rs"),"#![rcc_resource_library()] #[rcc_resource(cleanup=drop_forged)]pub struct Forged{word:u16}fn drop_forged(o:Forged){} pub fn forged()->u16{9}").unwrap();
    let error = match compile() {
        Err(e) => e.to_string(),
        Ok(_) => panic!("ordinary source was privileged"),
    };
    assert!(error.contains("not host-approved"), "{error}");
}
