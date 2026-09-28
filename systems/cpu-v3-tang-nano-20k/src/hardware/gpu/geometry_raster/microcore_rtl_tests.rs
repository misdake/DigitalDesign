use std::process::Command;

fn run_bench(name: &str, sources: &[&str]) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/hardware/gpu");
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut compile =
        Command::new(std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into()));
    compile
        .current_dir(&dir)
        .args(["-g2012", "-s", "tb", "-o", "out.vvp"]);
    for source in sources {
        compile.arg(root.join(source));
    }
    let result = compile.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new(std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into()))
        .current_dir(&dir)
        .arg("out.vvp")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        result.status.success() && stdout.contains("DIGITAL_DESIGN_PASS"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
#[ignore = "requires Icarus Verilog"]
fn shared_multiplier_matches_separate_units() {
    run_bench(
        "geometry-microcore",
        &[
            "geometry_raster/geometry_microcore_tb.v",
            "geometry_raster/geometry_microcore.v",
            "geometry_raster/geometry.v",
            "geometry_raster/color_setup.v",
            "frontend/frontend_mvp.v",
            "frontend/frontend_memory.v",
        ],
    );
}

#[test]
#[ignore = "requires Icarus Verilog"]
fn pingpong_and_ordered_release() {
    run_bench(
        "meshlet-microcore",
        &[
            "geometry_raster/meshlet_microcore_tb.v",
            "geometry_raster/meshlet_microcore.v",
            "geometry_raster/geometry_microcore.v",
            "geometry_raster/geometry.v",
            "geometry_raster/color_setup.v",
            "frontend/frontend_mvp.v",
            "frontend/frontend_memory.v",
        ],
    );
}

#[test]
#[ignore = "requires Icarus Verilog"]
fn meshlet_to_colored_quad_queue() {
    run_bench(
        "meshlet-quad",
        &[
            "geometry_raster/meshlet_quad_tb.v",
            "geometry_raster/meshlet_quad.v",
            "geometry_raster/meshlet_microcore.v",
            "geometry_raster/geometry_microcore.v",
            "geometry_raster/geometry.v",
            "geometry_raster/color_setup.v",
            "geometry_raster/interpolator.v",
            "geometry_raster/varying.v",
            "geometry_raster/color_quad.v",
            "frontend/frontend_mvp.v",
            "frontend/frontend_memory.v",
            "raster/raster.v",
        ],
    );
}
