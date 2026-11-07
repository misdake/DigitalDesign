//! Existing-file CLI qualification; no application build is needed to run the tool.
use cpu_v3_boot_format::*;
#[path = "../../tests/support/boot_contract.rs"]
mod contract;
use contract::fixture;
#[test]
fn command_line_repack_uses_the_project_resource_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../../target/cb06-validation/cli-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let package = fixture();
    let code = single_boot_code(&package).unwrap();
    std::fs::write(root.join("application.v3bin"), code).unwrap();
    std::fs::write(root.join("application.static-words"), "17\n").unwrap();
    std::fs::write(root.join("mesh.bin"), b"asset").unwrap();
    std::fs::write(
        root.join("project.conf"),
        "format 2\napplication app.rs\ncopy MESH 0x100000 mesh.bin\nzero 0x4e0000 32\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cpu-v3-pack"))
        .arg(root.join("application.v3bin"))
        .arg("--project")
        .arg(root.join("project.conf"))
        .arg("-o")
        .arg(root.join("image.bin"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(root.join("image.bin")).unwrap(),
        pack_application_resources(
            code,
            17,
            &[
                BootResource::copy(0x100000, b"asset".to_vec()),
                BootResource::zero(0x4e0000, 32)
            ]
        )
        .unwrap()
    );
    // A raw binary alone cannot reveal its static allocation size.
    std::fs::write(root.join("raw.v3bin"), code).unwrap();
    let pack = |extra: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_cpu-v3-pack"))
            .arg(root.join("raw.v3bin"))
            .arg("-o")
            .arg(root.join("raw.bin"))
            .args(extra)
            .output()
            .unwrap()
    };
    assert!(!pack(&[]).status.success());
    assert!(!pack(&["--static-words", "8193"]).status.success());
    assert!(!pack(&["--static-words", "bad"]).status.success());
    assert!(pack(&["--static-words", "0"]).status.success());
    assert_eq!(
        single_boot_commands(&std::fs::read(root.join("raw.bin")).unwrap())
            .unwrap()
            .len(),
        1
    );
    std::fs::write(root.join("raw.static-words"), "bad\n").unwrap();
    assert!(!pack(&[]).status.success());
}
