//! Export only the production modules used by the bounded RAM consumption checks.
use cpu_v3::{CpuV3Core, CpuV3DataCache};
use cpu_v3_tang_nano_20k::FramebufferHdmi;
use digital_design_hardware::VerilogProject;
use std::{collections::BTreeMap, fs, path::Path, process::Command};

#[test]
#[ignore = "exports RAM collision inputs; run ram_collision/run.py afterwards"]
fn export_ram_collision_inputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("target/ram-collision-consumption/rtl");
    let mut files = BTreeMap::new();
    for project in [
        VerilogProject::generate::<CpuV3Core>().unwrap(),
        VerilogProject::generate::<CpuV3DataCache>().unwrap(),
        VerilogProject::generate::<FramebufferHdmi>().unwrap(),
    ] {
        for (path, source) in project.files {
            if path.extension().is_some_and(|extension| extension == "v")
                && !source.contains("module tb")
            {
                if let Some(old) = files.insert(path, source.clone()) {
                    assert_eq!(old, source, "conflicting module exports");
                }
            }
        }
    }
    for (path, source) in &files {
        let path = out.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let git = |arguments: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    fs::write(
        out.join("export.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "git_head": git(&["rev-parse", "HEAD"]),
            "git_status": git(&["status", "--short"]),
            "modules": ["CpuV3Core", "CpuV3DataCache", "FramebufferHdmi"],
            "files": files.keys().collect::<Vec<_>>()
        }))
        .unwrap(),
    )
    .unwrap();
    println!("Exported {} RTL sources to {}", files.len(), out.display());
}
