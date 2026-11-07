//! Commands that do not build must not invoke the application toolchain.
use std::process::Command;

#[test]
fn list_help_and_invalid_requests_work_without_a_cargo_or_python_process() {
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_cpu-v3-app"))
            .args(args)
            .env("PATH", "")
            .env("CPU_V3_BOOT_EXAMPLE", "invalid-intentionally")
            .env_remove("GPU_PROFILER_MANIFEST")
            .env_remove("GPU_PROFILER_HOOK")
            .output()
            .unwrap()
    };
    assert!(run(&["--help"]).status.success());
    assert!(run(&["build", "gpu-bench", "--help"]).status.success());
    let listed = run(&["list"]);
    assert!(listed.status.success());
    assert_eq!(
        String::from_utf8(listed.stdout)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        cpu_v3_tools::applications::definitions()
            .map(|definition| definition.name)
            .collect::<Vec<_>>()
    );
    for args in [
        vec!["build", "unknown"],
        vec!["build", "bloxorz", "--scene", "full"],
        vec!["build", "gpu-bench", "--scene", "invalid"],
        vec!["build", "default"],
        vec!["build", "bloxorz", "--start-level", "0"],
        vec!["build", "bloxorz", "--start-level", "34"],
        vec!["build", "bloxorz", "--start-level", "-1"],
        vec!["build", "bloxorz", "--start-level", "invalid"],
        vec!["build", "bloxorz", "--start-phase", "invalid"],
        vec!["build", "gpu-profiler", "--out", "target/not-created"],
    ] {
        let result = run(&args);
        assert!(!result.status.success());
        assert!(!String::from_utf8(result.stderr)
            .unwrap()
            .contains("start application build"));
    }
    let occupied = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../target/application-output-policy")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&occupied).unwrap();
    let note = occupied.join("user-note.txt");
    std::fs::write(&note, "preserve this file").unwrap();
    let result = run(&["build", "default", "--out", occupied.to_str().unwrap()]);
    assert!(!result.status.success());
    assert!(String::from_utf8(result.stderr)
        .unwrap()
        .contains("not an empty directory"));
    assert_eq!(std::fs::read_to_string(note).unwrap(), "preserve this file");
    assert_eq!(std::fs::read_dir(occupied).unwrap().count(), 1);
}
