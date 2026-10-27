#[path = "../examples/gpu_demo/support.rs"]
mod support;
#[test]
fn compiled_camera_covers_one_complete_512_step_orbit() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gpu-demo-camera");
    std::fs::create_dir_all(&out).unwrap();
    support::write_matrices(&out.join("matrices.json"));
}
