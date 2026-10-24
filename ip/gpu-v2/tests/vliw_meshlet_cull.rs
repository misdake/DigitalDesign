//! Export the actual arithmetic/ROM composition for bounded culling replay.
#[test]
fn export_current_meshlet_cull() {
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/meshlet-cull/exports");
    std::fs::create_dir_all(&out).unwrap();
    for (name, portable) in [("portable", true), ("native", false)] {
        std::fs::write(
            out.join(format!("frontend-{name}.v")),
            gpu_v2::frontend::vliw::verilog(portable),
        )
        .unwrap();
    }
    let image = gpu_v2::system::vliw_command::verilog(
        gpu_v2::framebuffer::ports::MaterializedSurface {
            color_base_bytes: 0x100000,
            depth_base_bytes: 0x140000,
            width: 400,
            height: 240,
        },
        gpu_v2::system::pixel::quad_ingress::rtl::Backend::Native,
    )
    .unwrap();
    std::fs::write(out.join("gpu-native.v"), image).unwrap();
}
