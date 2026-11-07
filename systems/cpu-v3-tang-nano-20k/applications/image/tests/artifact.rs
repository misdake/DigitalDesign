//! Actual OUT_DIR export consumed offline, with relocation and tampering negatives.
use std::{path::Path, process::Command};

#[test]
fn same_build_artifact_is_relocatable_and_rejects_mismatches() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"));
    let system = package.parent().unwrap().parent().unwrap();
    let output = system
        .join("../../target/application-artifact")
        .join(std::process::id().to_string());
    let occupied = output.join("occupied");
    std::fs::create_dir_all(&occupied).unwrap();
    std::fs::write(
        occupied.join("boot-assets.manifest"),
        "obsolete inventory\n",
    )
    .unwrap();
    assert!(!Command::new(env!("CARGO_BIN_EXE_cpu-v3-boot-assets"))
        .arg(&occupied)
        .status()
        .unwrap()
        .success());
    assert_eq!(
        std::fs::read_to_string(occupied.join("boot-assets.manifest")).unwrap(),
        "obsolete inventory\n"
    );
    assert_eq!(std::fs::read_dir(&occupied).unwrap().count(), 1);
    let artifact = output.join("export");
    std::fs::create_dir_all(&artifact).unwrap();
    assert!(Command::new(env!("CARGO_BIN_EXE_cpu-v3-boot-assets"))
        .arg(&artifact)
        .status()
        .unwrap()
        .success());
    assert!(!artifact.join("boot-assets.manifest").exists());
    let manifest = std::fs::read(artifact.join("application.build.json")).unwrap();
    assert!(!Command::new(env!("CARGO_BIN_EXE_cpu-v3-boot-assets"))
        .arg(&artifact)
        .status()
        .unwrap()
        .success());
    assert_eq!(
        std::fs::read(artifact.join("application.build.json")).unwrap(),
        manifest
    );
    assert!(Command::new("python")
        .arg("-B")
        .arg(package.join("tests/artifact.py"))
        .arg(&artifact)
        .arg(output.join("contracts"))
        .status()
        .unwrap()
        .success());
}
