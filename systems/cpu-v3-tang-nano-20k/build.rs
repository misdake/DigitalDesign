#![allow(dead_code, hidden_glob_reexports, unused_imports)]
pub use cpu_v3::*;
#[path = "src/boot/mod.rs"]
mod boot;
use cpu_v3::rcc_backend::{self, CompilerOptions};
use rcc::frontend::compile_program_named;
use rcc_modules::capture_profile::layout as gpu_layout;
use std::path::{Path, PathBuf};
#[path = "build_support/rcc_modules.rs"]
mod rcc_modules;
enum ApplicationProfile {
    Default,
    Column16,
    Rtt400x240,
    BloxorzCapture,
    BloxorzCaptureFull,
}
fn compile(
    path: &Path,
    options: &CompilerOptions,
    constants: &str,
    shared: &Path,
    renderer: bool,
    capture_enabled: bool,
    profile: ApplicationProfile,
) -> (Vec<u16>, usize) {
    let rtt = matches!(
        profile,
        ApplicationProfile::Rtt400x240
            | ApplicationProfile::BloxorzCapture
            | ApplicationProfile::BloxorzCaptureFull
    );
    let entry = match profile {
        ApplicationProfile::BloxorzCapture => "renderer_game_capture",
        ApplicationProfile::BloxorzCaptureFull => "renderer_game_capture_full",
        _ => "main",
    };
    println!("cargo:rerun-if-changed={}", path.display());
    let source = std::fs::read_to_string(path).unwrap();
    let source_dir = path.parent().unwrap();
    let capture = if capture_enabled {
        let manifest_path = PathBuf::from(
            std::env::var_os("GPU_PROFILER_MANIFEST")
                .expect("gpu_profiler_board requires GPU_PROFILER_MANIFEST"),
        );
        println!("cargo:rerun-if-changed={}", manifest_path.display());
        Some(
            rcc_modules::RendererCaptureConfiguration::from_manifest(
                source_dir.join("profiler_gate.rs"),
                &manifest_path,
            )
            .expect("explicit GPU profiler identity"),
        )
    } else {
        None
    };
    let program = if renderer {
        let compile_renderer = |dependency: &mut dyn FnMut(&Path)| {
            let dirs = if source_dir.ends_with("gpu_renderer_bloxorz/rcc") {
                vec![shared.parent().unwrap().join("examples/bloxorz/rcc")]
            } else {
                vec![]
            };
            if let Some(configuration) = capture.as_ref() {
                rcc_modules::compile_renderer_program_with_capture_profile(
                    &path.display().to_string(),
                    &source,
                    options,
                    source_dir,
                    shared,
                    constants,
                    &dirs,
                    configuration,
                    &PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("capture-rcc"),
                    dependency,
                )
            } else if rtt
                || (source_dir.ends_with("gpu_renderer/rcc")
                    && path.file_name().is_some_and(|name| name == "members.rs"))
            {
                rcc_modules::compile_renderer_program_checked_with_sources(
                    &path.display().to_string(),
                    &source,
                    options,
                    source_dir,
                    shared,
                    constants,
                    &dirs,
                    &rcc_modules::RendererCaptureConfiguration::without_capture(
                        rcc_modules::RendererLayoutProfile::Column16,
                    ),
                    &rcc::frontend::rustc_check::RustcCheck::new(
                        PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
                            .join("member-ownership"),
                    ),
                    dependency,
                )
            } else {
                rcc_modules::compile_renderer_program_with_capture(
                    &path.display().to_string(),
                    &source,
                    options,
                    source_dir,
                    shared,
                    constants,
                    &dirs,
                    None,
                    dependency,
                )
                .map_err(|e| e.to_string())
            }
        };
        compile_renderer(&mut |module| println!("cargo:rerun-if-changed={}", module.display()))
    } else {
        compile_program_named(&path.display().to_string(), &source, options, &mut |name| {
            rcc_modules::load_module(source_dir, shared, constants, name, &mut |module| {
                println!("cargo:rerun-if-changed={}", module.display());
            })
        })
        .map_err(|e| e.to_string())
    }
    .unwrap_or_else(|error| panic!("compile {}: {error}", path.display()));
    let static_words = program.debug.static_data_end;
    let words = rcc_backend::try_compile(program, options, entry)
        .unwrap()
        .words;
    (words, static_words)
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let config = root.join("boot-applications.conf");
    println!("cargo:rerun-if-changed={}", config.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("tools/gpu_layout.py").display()
    );
    let mut project = boot::parse_boot_project(&std::fs::read_to_string(config).unwrap())
        .expect("single application project");
    println!("cargo:rerun-if-env-changed=CPU_V3_BOOT_EXAMPLE");
    println!("cargo:rerun-if-env-changed=GPU_PROFILER_MANIFEST");
    let mut generated = String::new();
    let mut capture_enabled = false;
    let mut column16 = false;
    let mut rtt_enabled = false;
    let mut automatic_capture = false;
    let mut capture_full = false;
    let mut extra_assets: Vec<String> = Vec::new();
    if let Some(example) = std::env::var_os("CPU_V3_BOOT_EXAMPLE") {
        if example == "gpu_bench" {
            println!("cargo:rerun-if-env-changed=GPU_BENCH_SCENE");
            println!(
                "cargo:rerun-if-changed={}",
                root.join("tools/gpu_layout.py").display()
            );
            if std::env::var("GPU_BENCH_SCENE").as_deref() == Ok("full") {
                for path in [
                    "examples/gpu_demo/tools/assets.py",
                    "examples/gpu_demo/assets/scene.json",
                    "../../ip/gpu-v2/examples/vliw_integration/material_scene.py",
                    "../../ip/gpu-v2/examples/vliw_integration/assets/room-tiles.png",
                ] {
                    println!("cargo:rerun-if-changed={}", root.join(path).display());
                }
            }
            let producer = root.join("examples/gpu_bench/tools/assets.py");
            println!("cargo:rerun-if-changed={}", producer.display());
            let assets = output.join("gpu-bench-assets");
            let status = std::process::Command::new("python")
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .arg(producer)
                .arg("--output")
                .arg(&assets)
                .status()
                .expect("GPU benchmark native asset producer");
            assert!(status.success(), "GPU benchmark asset validation failed");
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap())
                    .unwrap();
            project.application = PathBuf::from("examples/gpu_bench/rcc/main.rs");
            project.copies = manifest["resources"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        "GPU_BENCH".to_owned(),
                        r["address"].as_u64().unwrap() as usize,
                        assets.join(r["file"].as_str().unwrap()),
                    )
                })
                .collect();
            project.zeros.clear();
            generated = std::fs::read_to_string(assets.join("bench_assets.rs")).unwrap();
            std::fs::write(
                output.join("bench-repack.conf"),
                format!(
                    "format 2\napplication {}\ncopy GPU_BENCH {:#x} gpu-bench-assets/bench.bin\n",
                    project.application.display(),
                    manifest["resources"][0]["address"].as_u64().unwrap()
                ),
            )
            .unwrap();
            extra_assets.extend([
                "gpu-bench-assets/bench.bin".to_owned(),
                "gpu-bench-assets/assets.json".to_owned(),
                "gpu-bench-assets/bench_assets.rs".to_owned(),
                "bench-repack.conf".to_owned(),
            ]);
        } else if example == "dualshock2" {
            project.application = PathBuf::from("examples/dualshock2/rcc/main.rs");
            project.copies.clear();
            project.zeros.clear();
        } else if example == "dualshock2_ui" {
            let producer = root.join("examples/dualshock2_ui/tools/assets.py");
            println!("cargo:rerun-if-changed={}", producer.display());
            let assets = output.join("ds2-ui-assets");
            let status = std::process::Command::new("python")
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .arg(producer)
                .arg("--output")
                .arg(&assets)
                .status()
                .expect("DS2 UI native atlas packer");
            assert!(status.success(), "DS2 UI asset validation failed");
            project.application = PathBuf::from("examples/dualshock2_ui/rcc/main.rs");
            project.copies = vec![(
                "DS2_UI_ATLAS".to_owned(),
                usize::from(gpu_layout::GPU_ASSET_FIRST_SECTOR) * 128,
                assets.join("atlas.bin"),
            )];
            project.zeros.clear();
            generated = std::fs::read_to_string(assets.join("ui_layout.rs")).unwrap();
            std::fs::write(
                output.join("ds2-ui-repack.conf"),
                format!("format 2\napplication examples/dualshock2_ui/rcc/main.rs\ncopy DS2_UI_ATLAS {:#x} ds2-ui-assets/atlas.bin\n", usize::from(gpu_layout::GPU_ASSET_FIRST_SECTOR)*128),
            )
            .unwrap();
            extra_assets.extend([
                "ds2-ui-assets/atlas.bin".to_owned(),
                "ds2-ui-assets/assets.json".to_owned(),
                "ds2-ui-assets/ui_layout.rs".to_owned(),
                "ds2-ui-repack.conf".to_owned(),
            ]);
        } else if example == "gpu_renderer"
            || example == "gpu_renderer_changes"
            || example == "gpu_renderer_members"
            || example == "gpu_renderer_bloxorz"
            || example == "gpu_renderer_bloxorz_capture"
            || example == "gpu_renderer_bloxorz_capture_full"
            || example == "gpu_profiler_board"
            || example == "gpu_renderer_rtt"
            || example == "gpu_renderer_rtt_bilinear"
        {
            capture_full = example == "gpu_renderer_bloxorz_capture_full";
            automatic_capture = capture_full || example == "gpu_renderer_bloxorz_capture";
            let game = automatic_capture || example == "gpu_renderer_bloxorz";
            let profiler = example == "gpu_profiler_board";
            let rtt =
                game || example == "gpu_renderer_rtt" || example == "gpu_renderer_rtt_bilinear";
            if rtt {
                rtt_enabled = true;
                for path in [
                    "tools/gpu_texture_layout.py",
                    "examples/gpu_renderer/tools/assets.py",
                ] {
                    println!("cargo:rerun-if-changed={}", root.join(path).display());
                }
            }
            capture_enabled = profiler;
            let example_dir = root.join(if rtt && !game {
                "examples/gpu_renderer_rtt"
            } else if game {
                "examples/gpu_renderer_bloxorz"
            } else if profiler {
                "examples/gpu_profiler_board"
            } else {
                "examples/gpu_renderer"
            });
            if game {
                for path in [
                    "examples/bloxorz/tools/batch-scene.py",
                    "examples/bloxorz/tools/static-scene.py",
                    "examples/bloxorz/assets/reference.txt",
                    "examples/bloxorz/geometry.rs",
                    "examples/gpu_demo/tools/assets.py",
                    "examples/gpu_renderer/tools/assets.py",
                ] {
                    println!("cargo:rerun-if-changed={}", root.join(path).display());
                }
            }
            for input in [
                example_dir.join("tools/assets.py"),
                root.join("examples/gpu_demo/assets/font-8x16.txt"),
            ] {
                println!("cargo:rerun-if-changed={}", input.display());
            }
            let assets = output.join("gpu-renderer-assets");
            let status = std::process::Command::new("python")
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .arg(example_dir.join("tools/assets.py"))
                .arg("--output")
                .arg(&assets)
                .status()
                .expect("renderer asset packer");
            assert!(status.success(), "renderer asset validation failed");
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap())
                    .unwrap();
            if rtt {
                assert_eq!(manifest["profile"], "production");
                assert_eq!(manifest["surface"], "column-major-16x16-pad16");
            }
            project.application = PathBuf::from(if rtt && !game {
                if example == "gpu_renderer_rtt_bilinear" {
                    "examples/gpu_renderer_rtt/rcc/bilinear.rs"
                } else {
                    "examples/gpu_renderer_rtt/rcc/main.rs"
                }
            } else if game {
                "examples/gpu_renderer_bloxorz/rcc/main.rs"
            } else if profiler {
                "examples/gpu_profiler_board/rcc/main.rs"
            } else if example == "gpu_renderer_changes" {
                "examples/gpu_renderer/rcc/changes.rs"
            } else if example == "gpu_renderer_members" {
                "examples/gpu_renderer/rcc/members.rs"
            } else {
                "examples/gpu_renderer/rcc/main.rs"
            });
            project.renderer = true;
            project.copies = manifest["resources"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        format!(
                            "GPU_RENDERER_{}",
                            r["file"]
                                .as_str()
                                .unwrap()
                                .trim_end_matches(".bin")
                                .to_uppercase()
                        ),
                        r["address"].as_u64().unwrap() as usize,
                        assets.join(r["file"].as_str().unwrap()),
                    )
                })
                .collect();
            project.zeros.clear();
            generated = std::fs::read_to_string(assets.join("renderer_assets.rs")).unwrap();
            let mut repack = format!(
                "format 2\nrenderer\napplication {}\n",
                project.application.display()
            );
            for (name, address, path) in &project.copies {
                let relative = path
                    .strip_prefix(&output)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                repack.push_str(&format!("copy {name} {address:#x} {relative}\n"));
                extra_assets.push(relative);
            }
            std::fs::write(output.join("renderer-repack.conf"), repack).unwrap();
            extra_assets.extend([
                "gpu-renderer-assets/assets.json".to_owned(),
                "gpu-renderer-assets/renderer_assets.rs".to_owned(),
                "renderer-repack.conf".to_owned(),
            ]);
        } else {
            assert!(
                example == "gpu_demo" || example == "gpu_demo_column16",
                "unsupported CPU_V3_BOOT_EXAMPLE"
            );
            column16 = example == "gpu_demo_column16";
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
            if column16 {
                let layout = root.join("rcc/gpu_layout.rs");
                println!("cargo:rerun-if-changed={}", layout.display());
                std::fs::copy(layout, output.join("gpu-column-layout.rs")).unwrap();
                extra_assets.push("gpu-column-layout.rs".to_owned());
            }
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
    println!("cargo:rerun-if-env-changed=RCC_GPU_DEBUG");
    options.gpu_debug = match std::env::var("RCC_GPU_DEBUG") {
        Err(std::env::VarError::NotPresent) => false,
        Ok(value) if value == "0" => false,
        Ok(value) if value == "1" => true,
        _ => panic!("RCC_GPU_DEBUG must be 0 or 1"),
    };
    let (words, static_words) = compile(
        &root.join(source),
        &options,
        &constants,
        &root.join("rcc"),
        project.renderer,
        capture_enabled,
        if capture_full {
            ApplicationProfile::BloxorzCaptureFull
        } else if automatic_capture {
            ApplicationProfile::BloxorzCapture
        } else if rtt_enabled {
            ApplicationProfile::Rtt400x240
        } else if column16 {
            ApplicationProfile::Column16
        } else {
            ApplicationProfile::Default
        },
    );
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
        "application={}\nrcc_gpu_debug={}\n{}",
        source.display(),
        options.gpu_debug,
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
