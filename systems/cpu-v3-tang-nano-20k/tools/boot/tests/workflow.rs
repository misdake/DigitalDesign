//! Real process/CLI boundaries: error protocol, retained logs and deadlines.
use std::{path::PathBuf, process::Command};
fn output(label: &str) -> PathBuf {
    cpu_v3_tools::workflow::unique(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../target/sdk-workflow/tool-tests")
            .join(label),
    )
    .unwrap()
}
#[test]
fn child_output_exit_and_timeout_are_retained() {
    let root = output("process");
    let failure = cpu_v3_tools::process::logged(
        Command::new("python").args([
            "-c",
            "import sys;print('all stdout');print('all stderr',file=sys.stderr);sys.exit(7)",
        ]),
        &root,
        "failure",
        10,
    )
    .unwrap();
    assert!(!failure.success);
    assert_eq!(failure.code, Some(7));
    assert!(std::fs::read_to_string(failure.stdout)
        .unwrap()
        .contains("all stdout"));
    assert!(std::fs::read_to_string(failure.stderr)
        .unwrap()
        .contains("all stderr"));
    let timeout = cpu_v3_tools::process::logged(
        Command::new("python").args(["-c", "import time;time.sleep(30)"]),
        &root,
        "timeout",
        1,
    )
    .unwrap();
    assert!(timeout.timed_out);
    assert!(!timeout.success);
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("timeout.result.json")).unwrap()).unwrap();
    assert_eq!(receipt["timed_out"], true);
}
#[test]
fn cli_displays_typed_source_failure_without_reprinting_cargo_panic_noise() {
    let root = output("source-error");
    let template = root.join("diagnostic-template.json");
    std::fs::write(&template,serde_json::to_vec(&serde_json::json!({"schema":"digitaldesign.build-diagnostic","version":1,
        "stage":"compilation","rendered":"error: incompatible value\n --> actual-source.rs:7:3","location":{"file":"actual-source.rs","line":7,"column":3}})).unwrap()).unwrap();
    #[cfg(windows)]
    let cargo = {
        let path = root.join("cargo.cmd");
        std::fs::write(&path,"@echo off\r\ncopy /Y \"%DIAGNOSTIC_TEMPLATE%\" \"%CPU_V3_BUILD_DIAGNOSTIC%\" >nul\r\necho FULL_STDOUT_DETAIL\r\necho PANIC_NOISE_FOR_RAW_LOG 1>&2\r\nexit /b 101\r\n").unwrap();
        path
    };
    #[cfg(unix)]
    let cargo = {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join("cargo");
        std::fs::write(&path,"#!/bin/sh\ncp \"$DIAGNOSTIC_TEMPLATE\" \"$CPU_V3_BUILD_DIAGNOSTIC\"\necho FULL_STDOUT_DETAIL\necho PANIC_NOISE_FOR_RAW_LOG >&2\nexit 101\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    let run = root.join("run");
    let result = Command::new(env!("CARGO_BIN_EXE_cpu-v3-app"))
        .args(["preview", "core-demo", "--out"])
        .arg(&run)
        .env("CARGO", cargo)
        .env("DIAGNOSTIC_TEMPLATE", template)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let message = String::from_utf8_lossy(&result.stderr);
    assert!(message.contains("actual-source.rs:7:3"), "{message}");
    assert!(!message.contains("PANIC_NOISE_FOR_RAW_LOG"));
    assert!(
        std::fs::read_to_string(run.join("logs/build/build.stderr.log"))
            .unwrap()
            .contains("PANIC_NOISE_FOR_RAW_LOG")
    );
    assert!(
        std::fs::read_to_string(run.join("logs/build/build.stdout.log"))
            .unwrap()
            .contains("FULL_STDOUT_DETAIL")
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("run.json")).unwrap()).unwrap();
    assert_eq!(receipt["success"], false);
}
