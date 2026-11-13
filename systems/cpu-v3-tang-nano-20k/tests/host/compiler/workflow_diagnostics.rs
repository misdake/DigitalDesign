//! Actual compiler -> canonical artifact -> bounded runner -> same-build diagnostics.
use crate::shared_build_support_rcc_modules as resolver;
use cpu_v3::rcc_backend;
use cpu_v3_tang_nano_20k::boot;
use rcc::frontend::rustc_check::RustcCheck;
use std::{
    path::{Path, PathBuf},
    process::Command,
};
#[path = "../../../build_support/artifact.rs"]
mod artifact;
fn fixture(label: &str, body: &str) -> PathBuf {
    let source = format!("mod gpu_api;use gpu_api::*;\n{body}");
    let source = source.as_str();
    let system = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repository = system.join("../..").canonicalize().unwrap();
    let output = repository.join("target/sdk-workflow/fixtures").join(label);
    std::fs::create_dir_all(&output).unwrap();
    let entry = output.join("main.rs");
    std::fs::write(&entry, source).unwrap();
    let mut inputs = artifact::Inputs::new(&repository, &output);
    inputs.entry(
        entry.to_str().unwrap(),
        &rcc::frontend::ModuleSource {
            source: source.into(),
            origin: Some(entry.clone()),
        },
    );
    inputs.embedded_standard_library();
    let options = rcc_backend::CompilerOptions::for_separate_code_and_data_segments(0);
    let mut ir = resolver::GpuSources {
        local_dir: &output,
        shared_dir: &system.join("rcc"),
        constants: "",
        ordinary_dirs: &[],
        configuration: &resolver::RendererCaptureConfiguration::without_capture(),
    }
    .compile_checked(
        entry.to_str().unwrap(),
        source,
        &options,
        &RustcCheck::new(output.join("rustc")),
        &mut |_| {},
        &mut |name, source| inputs.module(name, source),
    )
    .unwrap();
    let static_words = ir.debug.static_data_end;
    let mut data = vec![0; static_words * 2];
    for (at, value) in ir.take_static_initializers() {
        data[at as usize * 2..at as usize * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }
    let program = rcc_backend::try_compile(ir, &options, "main").unwrap();
    let code = program
        .words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect::<Vec<_>>();
    let package = boot::pack_application_with_data(&code, static_words, &data, &[]).unwrap();
    for (name, bytes) in [
        ("application.v3bin", code),
        ("application.static-data", data),
        (
            "application.static-words",
            static_words.to_string().into_bytes(),
        ),
        (
            "application.dbg",
            artifact::debug_text(&program).into_bytes(),
        ),
        ("application.asm", program.listing.as_bytes().to_vec()),
        ("cpu-v3-boot.bin", package),
    ] {
        std::fs::write(output.join(name), bytes).unwrap();
    }
    let mut files = [
        "application.v3bin",
        "application.static-data",
        "application.static-words",
        "application.dbg",
        "application.asm",
        "cpu-v3-boot.bin",
    ]
    .map(str::to_owned)
    .to_vec();
    artifact::write(
        artifact::BuildRecord {
            output: &output,
            inputs: &inputs,
            program: &program,
            static_words,
            application: serde_json::json!({"source":inputs.describe_path(&entry)}),
            resources: &[],
            options: &options,
        },
        &mut files,
    );
    output
}
#[test]
fn runtime_faults_and_custom_halts_have_authenticated_locations_in_both_backends() {
    for (label, body, kind, signal) in [
        (
            "assertion",
            "fn main(){\n assert!(false);\n halt(0x600d);\n}",
            "assertion",
            65535,
        ),
        (
            "multiline",
            "fn main(){\n assert!(\n false\n );\n halt(0x600d);\n}",
            "assertion",
            65535,
        ),
        (
            "custom-halt",
            "fn main(){\n halt(0xffff);\n}",
            "halt",
            65535,
        ),
        (
            "ambiguous",
            "fn main(){\n assert!(true);halt(0xffff);\n}",
            "halt",
            65535,
        ),
        (
            "gpu-fault",
            "fn main(){\n let memory=unsafe{GpuMemory::from_sectors(65535,2)};\n halt(0x600d);\n}",
            "gpu_fault",
            0xe001,
        ),
    ] {
        let artifact = fixture(label, body);
        for mode in ["instruction", "native"] {
            let output = artifact.join(mode);
            let mut command = Command::new(env!("CARGO_BIN_EXE_cpu-v3-display-sim"));
            command
                .arg("--package")
                .arg(artifact.join("cpu-v3-boot.bin"))
                .arg("--artifact")
                .arg(&artifact)
                .args(["--cpu-mode", mode, "--frames", "2"])
                .arg(if mode == "native" {
                    "--max-cpu-cycles"
                } else {
                    "--max-cpu-steps"
                })
                .arg("2000000")
                .arg("--out-dir")
                .arg(&output);
            let run = command.output().unwrap();
            std::fs::write(artifact.join(format!("{mode}.stdout.log")), &run.stdout).unwrap();
            std::fs::write(artifact.join(format!("{mode}.stderr.log")), &run.stderr).unwrap();
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(output.join("diagnostic.json")).unwrap())
                    .unwrap();
            assert_eq!(report["kind"], kind, "{label}/{mode}: {report}");
            assert_eq!(report["signal"], signal);
            assert!(report["source"]["line"].as_u64().unwrap() > 0);
            assert_eq!(
                run.status.success(),
                kind == "halt",
                "{label}/{mode}: {}",
                String::from_utf8_lossy(&run.stderr)
            );
            if kind == "gpu_fault" {
                assert_eq!(report["fault"]["site"], 1);
                assert_eq!(report["fault"]["code"], 1);
                assert_eq!(report["fault"]["detail0"], 65535);
                assert!(
                    report["call_chain"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|frame| frame["function"] == "main"),
                    "{report}"
                );
            }
        }
    }
    let a = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sdk-workflow/fixtures");
    let run = Command::new(env!("CARGO_BIN_EXE_cpu-v3-display-sim"))
        .arg("--package")
        .arg(a.join("gpu-fault/cpu-v3-boot.bin"))
        .arg("--artifact")
        .arg(a.join("assertion"))
        .args(["--frames", "1", "--max-cpu-steps", "1"])
        .arg("--out-dir")
        .arg(a.join("wrong-binding"))
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("package does not match"));
}
#[test]
fn checked_compilation_preserves_source_location_for_build_reports() {
    let system = Path::new(env!("CARGO_MANIFEST_DIR"));
    let failure = resolver::GpuSources {
        local_dir: &system.join("rcc"),
        shared_dir: &system.join("rcc"),
        constants: "",
        ordinary_dirs: &[],
        configuration: &resolver::RendererCaptureConfiguration::without_capture(),
    }
    .compile_checked_source(
        "broken.rs",
        "mod gpu_api;fn main(){\n let x:u16=true;\n halt(x);\n}",
        &rcc_backend::CompilerOptions::default(),
        &RustcCheck::new(system.join("../../target/sdk-workflow/broken-rustc")),
        &mut |_| {},
        &mut |_, _| {},
    )
    .err()
    .unwrap();
    let resolver::GpuCompileError::Source(error) = failure else {
        panic!("lost source error");
    };
    let (file, line, column) = error.location().unwrap();
    assert_eq!(line, 2);
    assert!(column > 0);
    let mut report = cpu_v3_tools::diagnostics::BuildDiagnostic::new("compilation", &error);
    report.location = Some((file.into(), line, column));
    let path = system.join("../../target/sdk-workflow/broken-source.json");
    report.write(&path).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["location"]["file"], "broken.rs");
    assert_eq!(saved["location"]["line"], 2);
    assert!(saved["rendered"].as_str().unwrap().contains("rustc"));
}
