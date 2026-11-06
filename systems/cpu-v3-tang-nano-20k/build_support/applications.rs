//! Application selection, asset producers and approved RCC source policy.
use crate::{boot, rcc_modules};
use cpu_v3::rcc_backend::CompilerOptions;
use rcc_modules::capture_profile::layout as gpu_layout;
use std::path::{Path, PathBuf};

#[path = "application_compile.rs"]
mod compile;
use compile::compile_program;

#[derive(Clone, Copy)]
enum ApplicationProfile {
    Default,
    ProfilerMemcpy,
    Column16,
    Handheld,
    Rtt400x240,
    BloxorzCapture,
    BloxorzCaptureFull,
}
enum ApplicationFrontend {
    Native,
    ExplicitGpu,
}
pub struct Application {
    pub project: boot::BootProject,
    pub constants: String,
    pub extra_assets: Vec<String>,
    profile: ApplicationProfile,
    capture_enabled: bool,
}

pub fn prepare(root: &Path, output: &Path) -> Application {
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
    println!("cargo:rerun-if-env-changed=GPU_PROFILER_HOOK");
    let mut generated = String::new();
    let mut capture_enabled = false;
    let mut profile = ApplicationProfile::Default;
    let mut extra_assets: Vec<String> = Vec::new();
    if let Some(example) = std::env::var_os("CPU_V3_BOOT_EXAMPLE") {
        project.gpu_api = false;
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
        } else if example == "gpu_demo_api_column16"
            || example == "gpu_renderer"
            || example == "gpu_renderer_changes"
            || example == "gpu_renderer_members"
            || example == "gpu_renderer_bloxorz"
            || example == "gpu_renderer_bloxorz_capture"
            || example == "gpu_renderer_bloxorz_capture_full"
            || example == "gpu_profiler_board"
            || example == "gpu_renderer_rtt"
            || example == "gpu_renderer_rtt_bilinear"
        {
            let handheld = example == "gpu_demo_api_column16";
            let capture_full = example == "gpu_renderer_bloxorz_capture_full";
            let automatic_capture = capture_full || example == "gpu_renderer_bloxorz_capture";
            let game = automatic_capture || example == "gpu_renderer_bloxorz";
            let profiler = example == "gpu_profiler_board";
            let mut rtt =
                game || example == "gpu_renderer_rtt" || example == "gpu_renderer_rtt_bilinear";
            profile = if capture_full {
                ApplicationProfile::BloxorzCaptureFull
            } else if automatic_capture {
                ApplicationProfile::BloxorzCapture
            } else if rtt {
                ApplicationProfile::Rtt400x240
            } else if handheld {
                ApplicationProfile::Handheld
            } else {
                ApplicationProfile::Default
            };
            if rtt {
                for path in [
                    "tools/gpu_texture_layout.py",
                    "examples/gpu_renderer/tools/assets.py",
                ] {
                    println!("cargo:rerun-if-changed={}", root.join(path).display());
                }
            }
            capture_enabled = profiler;
            let example_dir = root.join(if handheld {
                "examples/gpu_demo"
            } else if profiler {
                "examples/gpu_profiler_board"
            } else if rtt && !game {
                "examples/gpu_renderer_rtt"
            } else if game {
                "examples/gpu_renderer_bloxorz"
            } else {
                "examples/gpu_renderer"
            });
            if game {
                for path in [
                    "applications/bloxorz/tools/batch-scene.py",
                    "applications/bloxorz/tools/static-scene.py",
                    "applications/bloxorz/assets/reference.txt",
                    "applications/bloxorz/geometry.rs",
                    "examples/gpu_demo/tools/assets.py",
                    "examples/gpu_renderer/tools/assets.py",
                ] {
                    println!("cargo:rerun-if-changed={}", root.join(path).display());
                }
            }
            let producer = example_dir.join(if handheld {
                "tools/public_assets.py"
            } else {
                "tools/assets.py"
            });
            for input in [
                producer.clone(),
                example_dir.join("tools/assets.py"),
                root.join("examples/gpu_demo/assets/font-8x16.txt"),
            ] {
                println!("cargo:rerun-if-changed={}", input.display());
            }
            let assets = output.join("gpu-renderer-assets");
            let status = std::process::Command::new("python")
                .env("PYTHONDONTWRITEBYTECODE", "1")
                .arg(&producer)
                .arg("--output")
                .arg(&assets)
                .status()
                .expect("renderer asset packer");
            assert!(status.success(), "renderer asset validation failed");
            let manifest: serde_json::Value =
                serde_json::from_slice(&std::fs::read(assets.join("assets.json")).unwrap())
                    .unwrap();
            if handheld {
                for source in manifest["sources"].as_object().unwrap().keys() {
                    println!(
                        "cargo:rerun-if-changed={}",
                        root.join("../..").join(source).display()
                    );
                }
            }
            if profiler {
                match manifest["profiler_short_case"].as_str().unwrap_or("none") {
                    "none" => {}
                    "rtt" => {
                        rtt = true;
                        profile = ApplicationProfile::Rtt400x240;
                    }
                    "memcpy" => {
                        profile = ApplicationProfile::ProfilerMemcpy;
                    }
                    _ => panic!("unsupported profiler short case"),
                }
            }
            if rtt {
                assert_eq!(manifest["profile"], "production");
                assert_eq!(manifest["surface"], "column-major-16x16-pad16");
            }
            project.application = PathBuf::from(if handheld {
                "examples/gpu_demo/rcc/public_main.rs"
            } else if profiler {
                if rtt {
                    "examples/gpu_profiler_board/rcc/rtt.rs"
                } else {
                    "examples/gpu_profiler_board/rcc/main.rs"
                }
            } else if rtt && !game {
                if example == "gpu_renderer_rtt_bilinear" {
                    "examples/gpu_renderer_rtt/rcc/bilinear.rs"
                } else {
                    "examples/gpu_renderer_rtt/rcc/main.rs"
                }
            } else if game {
                "examples/gpu_renderer_bloxorz/rcc/main.rs"
            } else if example == "gpu_renderer_changes" {
                "examples/gpu_renderer/rcc/changes.rs"
            } else if example == "gpu_renderer_members" {
                "examples/gpu_renderer/rcc/members.rs"
            } else {
                "examples/gpu_renderer/rcc/main.rs"
            });
            project.gpu_api = true;
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
                "format 2\n{}\napplication {}\n",
                if project.gpu_api {
                    "gpu-api"
                } else {
                    "renderer"
                },
                project.application.display()
            );
            for (name, address, path) in &project.copies {
                let relative = path
                    .strip_prefix(output)
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
            let column16 = example == "gpu_demo_column16";
            if column16 {
                profile = ApplicationProfile::Column16;
            }
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
                    .strip_prefix(output)
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
    for (_, _, path) in &project.copies {
        // Generated OUT_DIR resources depend on the audited producer inputs.
        if !path.starts_with(output) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    let constants = project.constants() + &generated;
    Application {
        project,
        constants,
        extra_assets,
        profile,
        capture_enabled,
    }
}

impl Application {
    pub fn compile(
        &mut self,
        root: &Path,
        output: &Path,
        options: &CompilerOptions,
    ) -> (Vec<u16>, usize) {
        let result = compile_program(
            &root.join(&self.project.application),
            options,
            &self.constants,
            &root.join("rcc"),
            if self.project.gpu_api {
                ApplicationFrontend::ExplicitGpu
            } else {
                ApplicationFrontend::Native
            },
            self.capture_enabled,
            self.profile,
        );
        if matches!(self.profile, ApplicationProfile::ProfilerMemcpy) {
            for (source, destination) in [
                ("specialization.json", "profiler-source-specialization.json"),
                ("gpu_execute.rs", "profiler-specialized-gpu-execute.rs"),
                ("gpu_packet.rs", "profiler-specialized-gpu-packet.rs"),
            ] {
                std::fs::copy(
                    output.join("profiler-memcpy-rcc").join(source),
                    output.join(destination),
                )
                .unwrap();
                self.extra_assets.push(destination.to_owned());
            }
        }
        result
    }
}
