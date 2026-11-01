use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::pixel::{quad_ingress, vliw_backend},
    texture::ports::Slot,
};
use std::{fs, path::PathBuf};

/// Freeze this entry at the accepted revision before changing its implementation.
#[test]
#[ignore = "explicit eight-profile join/Q qualification export"]
fn export_join_q_profiles() {
    let out = std::env::var_os("GPU_JOIN_Q_EXPORT_DIR")
        .map(PathBuf::from)
        .expect("GPU_JOIN_Q_EXPORT_DIR must select a fresh output directory");
    assert!(!out.exists(), "preserve previous exports");
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
    for (mode, backend) in [
        ("native", quad_ingress::rtl::Backend::Native),
        ("portable", quad_ingress::rtl::Backend::Portable),
    ] {
        let generic = vliw_backend::verilog_selected(&slots, surface, backend).unwrap();
        for (name, source) in [
            ("generic", generic.clone()),
            (
                "generic-command-ports",
                format!("`define GPU_V2_COMMAND_PORTS\n{generic}"),
            ),
            (
                "command",
                gpu_v2::system::vliw_command::verilog(surface, backend).unwrap(),
            ),
            (
                "full",
                gpu_v2::system::vliw::verilog(&slots, surface, backend).unwrap(),
            ),
        ] {
            fs::write(out.join(format!("{name}-{mode}.v")), source).unwrap();
        }
        for width in [18, 24] {
            let bank = if mode == "native" {
                gpu_v2::system::pixel::row_store::verilog_sdp36(128, width)
            } else {
                gpu_v2::system::pixel::row_store::verilog(128, width)
            }
            .unwrap();
            fs::write(out.join(format!("public-bank{width}-{mode}.v")), bank).unwrap();
        }
    }
}

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
            "module gpu_v2_texture_runtime_rows(",
            "module framebuffer_cache ",
            "module rop_leaf ",
        ] {
            assert!(!s.contains(dead), "{dead}");
        }
        let top = &s[s.find("module gpu_v2_vliw_backend(").unwrap()..];
        let top = &top[..top.find("endmodule").unwrap()];
        for dead in [
            "depth_rows",
            "join_depth_read",
            "depth_reserved",
            "depth_pending",
            "depth_tag",
            "depth_valid",
        ] {
            assert!(!top.contains(dead), "selected join still contains {dead}");
        }
        assert!(top.contains(".depth_read(sink_color_accept)"));
        assert!(top.contains("wire output_issued=issue_ahead||(issue_lane>out_lane)"));
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
