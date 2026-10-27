//! Small selected tuple export; the generic Light2 factory remains unchanged.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::pixel::{quad_ingress, vliw_backend},
    texture::ports::Slot,
};
#[test]
fn export_tuple_complete_fixture() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-chain-tight-20261018/light-tuple/complete-export");
    std::fs::create_dir_all(&root).unwrap();
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
        assert!(s.contains(".source_normal(light_normal)"));
        assert!(!s.contains("reg [35:0] normal;"));
        std::fs::write(root.join(format!("backend-{name}.v")), s).unwrap();
    }
}

/// Bounded native/portable old-consumer event replay using the actual selected core.
#[test]
#[ignore = "requires Icarus and Gowin simulation primitives"]
fn tuple_consumer_portable_native_event_qualification() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = repo.join("target/gpu-chain-tight-20261018/light-tuple/exports");
    std::fs::create_dir_all(&out).unwrap();
    let slots = [Slot {
        base_address: 0x1000,
        max_size_log2: 6,
        has_full_mip: true,
        valid: true,
    }];
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 400,
        height: 240,
    };
    for (name, backend) in [
        ("portable", quad_ingress::rtl::Backend::Portable),
        ("native", quad_ingress::rtl::Backend::Native),
    ] {
        std::fs::write(
            out.join(format!("backend-{name}.v")),
            vliw_backend::verilog_selected(&slots, surface, backend).unwrap(),
        )
        .unwrap();
    }
    let tag = std::env::var("VLIW_LIGHT_TUPLE_TEST_TAG")
        .unwrap_or_else(|_| "consumer-maintained-v1".into());
    let mut child = std::process::Command::new("python")
        .arg(repo.join("ip/gpu-v2/tests/vliw_lighting_tuple.py"))
        .arg("--tag")
        .arg(tag)
        .arg("--export-root")
        .arg(out)
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(180) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("tuple event replay exceeded bounded timeout");
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
