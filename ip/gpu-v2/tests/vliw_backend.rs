use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::pixel::{quad_ingress, vliw_backend},
    texture::ports::Slot,
};
use std::{fs, path::PathBuf};
#[test]
fn emit_vliw_backend() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-vliw-integration-20261018/stage2/backend-export");
    fs::create_dir_all(&out).unwrap();
    let slots = [Slot {
        base_address: 0,
        max_size_log2: 5,
        has_full_mip: true,
        valid: true,
    }];
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x200000,
        width: 400,
        height: 240,
    };
    for (name, backend) in [
        ("portable", quad_ingress::rtl::Backend::Portable),
        ("native", quad_ingress::rtl::Backend::Native),
    ] {
        let s = vliw_backend::verilog_selected(&slots, surface, backend).unwrap();
        for dead in [
            "module gpu_v2_quad_ingress_closed_basic(",
            "module gpu_v2_quad_authority(",
            "module gpu_v2_pixel_sampling_ingress(",
            "module gpu_v2_texture_runtime_d(",
        ] {
            assert!(!s.contains(dead), "{dead}");
        }
        fs::write(out.join(format!("backend-{name}.v")), s).unwrap();
    }
}
