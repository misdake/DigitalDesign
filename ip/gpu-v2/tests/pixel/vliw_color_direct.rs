//! Selected Color completion ownership; generic Color keeps independent buffering.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::pixel::{postquad_ii4, quad_ingress, vliw_backend},
    texture::ports::Slot,
};

fn source(backend: quad_ingress::rtl::Backend, generic: bool) -> String {
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
    if generic {
        postquad_ii4::verilog(&slots, surface, backend, 9).unwrap()
    } else {
        vliw_backend::verilog_selected(&slots, surface, backend).unwrap()
    }
}

#[test]
fn selected_completion_uses_final_bank_and_generic_retains_fifo() {
    for backend in [
        quad_ingress::rtl::Backend::Portable,
        quad_ingress::rtl::Backend::Native,
    ] {
        let s = source(backend, false);
        let color = s
            .split("module gpu_v2_texture_runtime_color_checked(")
            .nth(1)
            .unwrap()
            .split("endmodule")
            .next()
            .unwrap();
        assert!(color.contains("Closed VLIW parent writes every completed pixel"));
        assert!(!color.contains("result_ram[0:15]"));
        assert!(!color.contains("begin:result_bank"));
        assert!(s.contains("gpu_v2_postquad_sample_bank sample_bank("));
        assert!(source(backend, true).contains("begin:result_bank"));
    }
}

#[test]
#[ignore = "requires Icarus and Gowin simulation primitives"]
fn direct_completion_portable_native_values_pause_reset_and_fault() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let tag = std::env::var("VLIW_COLOR_TEST_TAG").unwrap_or_else(|_| {
        format!(
            "run-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    });
    let out = repo.join("target/vliw-color-direct").join(tag);
    let exports = out.join("exports");
    std::fs::create_dir_all(&exports).unwrap();
    for (name, backend) in [
        ("portable", quad_ingress::rtl::Backend::Portable),
        ("native", quad_ingress::rtl::Backend::Native),
    ] {
        std::fs::write(
            exports.join(format!("backend-{name}.v")),
            source(backend, false),
        )
        .unwrap();
    }
    let mut child = std::process::Command::new("python")
        .arg(repo.join("ip/gpu-v2/tests/vliw_color_direct.py"))
        .arg("--exports")
        .arg(exports)
        .arg("--output")
        .arg(out.join("replay"))
        .arg("--reference")
        .arg(repo.join("ip/gpu-v2/src/system/pixel/postquad_ii4_color.v"))
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(240) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Color replay exceeded bounded timeout");
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
