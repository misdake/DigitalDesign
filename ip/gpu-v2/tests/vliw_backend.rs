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
            "module framebuffer_cache ",
            "module rop_leaf ",
        ] {
            assert!(!s.contains(dead), "{dead}");
        }
        fs::write(out.join(format!("backend-{name}.v")), s).unwrap();
    }
}

#[test]
fn full_memory_binding_preserves_private_cache_ports() {
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
    let source =
        gpu_v2::system::vliw_sdram::verilog(&slots, surface, quad_ingress::rtl::Backend::Portable)
            .unwrap();
    assert!(source.contains("module gpu_v2_vliw_three_port("));
    let cache_start = source.find("module gpu_v2_framebuffer_tdp #(").unwrap();
    let cache_end = cache_start + source[cache_start..].find("endmodule").unwrap();
    let cache = &source[cache_start..cache_end];
    assert!(cache.contains("output mem_req_valid,mem_req_write"));
    assert!(!cache.contains("output framebuffer_request_valid"));
}
