//! Existing-file CLI qualification; no application build is needed to run the tool.
use cpu_v3_boot_format::*;
#[path = "../../tests/support/boot_contract.rs"]
mod contract;
use contract::fixture;

#[test]
fn static_data_copy_is_separate_from_resource_permissions_and_repackable() {
    let code = single_boot_code(&fixture()).unwrap().to_vec();
    let data = [0x34, 0x12, 0, 0, 0xcd, 0xab];
    let package = pack_application_with_data(&code, 3, &data, &[]).unwrap();
    let commands = single_boot_commands(&package).unwrap();
    assert_eq!(commands[0].address, 0);
    assert_eq!(commands[0].length, data.len());
    assert_eq!(commands[0].payload, Some(data.as_slice()));
    assert!(boot_image_map(&code, &package).contains("static_data_bytes=6"));
    assert!(pack_application_with_data(&code, 3, &data[..4], &[]).is_err());
    assert!(pack_application_with_data(&code, 8193, &[], &[]).is_err());
    assert!(
        pack_application_with_data(&code, 3, &data, &[BootResource::copy(0, vec![1, 2])]).is_err()
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/sdk-static-assets/repack");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("application.v3bin"), &code).unwrap();
    std::fs::write(root.join("application.static-words"), "3\n").unwrap();
    std::fs::write(root.join("application.static-data"), data).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cpu-v3-pack"))
        .arg(root.join("application.v3bin"))
        .arg("-o")
        .arg(root.join("packed.bin"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read(root.join("packed.bin")).unwrap(), package);
    std::fs::write(root.join("application.static-data"), [1, 2]).unwrap();
    assert!(
        !std::process::Command::new(env!("CARGO_BIN_EXE_cpu-v3-pack"))
            .arg(root.join("application.v3bin"))
            .arg("-o")
            .arg(root.join("bad.bin"))
            .status()
            .unwrap()
            .success()
    );
}
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
