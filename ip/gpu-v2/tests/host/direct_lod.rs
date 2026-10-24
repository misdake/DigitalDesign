use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::pixel::{direct_lod, quad_ingress},
    texture::{self, ports::Slot},
};
use std::{fs, path::PathBuf};
fn output() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/gpu-vliw-integration-20261018/stage1/test-export")
}
#[test]
fn direct_lod_integer_decode() {
    for max_n in 0..=10 {
        for mip in [false, true] {
            for filter in 0..3 {
                for lod in 0..=255 {
                    let c = texture::direct_lod::decode(lod, max_n, mip, filter).unwrap();
                    assert_eq!(c.parents[0] + c.parents[1], 511);
                    assert!(c.levels[0] <= max_n);
                    assert_eq!(c.side[0], 1 << c.levels[0].max(1));
                    if !mip || lod >= max_n * 8 {
                        assert_eq!(c.parents[1], 0);
                    }
                }
            }
        }
    }
    let got: Vec<_> = (0..8)
        .map(|l| texture::direct_lod::decode(l, 10, true, 2).unwrap().parents[1])
        .collect();
    assert_eq!(got, [0, 64, 128, 192, 256, 319, 383, 447]);
}
#[test]
fn emit_direct_lod_complete_chain() {
    let out = output();
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
    for (name, backend, native) in [
        ("portable", quad_ingress::rtl::Backend::Portable, false),
        ("native", quad_ingress::rtl::Backend::Native, true),
    ] {
        fs::write(
            out.join(format!("direct-{name}.v")),
            texture::rtl::direct_lod::verilog(native).unwrap(),
        )
        .unwrap();
        fs::write(
            out.join(format!("baseline-{name}.v")),
            gpu_v2::system::pixel::postquad::verilog_selected(&slots, surface, backend).unwrap(),
        )
        .unwrap();
        let v = direct_lod::verilog_selected(&slots, surface, backend).unwrap();
        assert!(!v.contains("module gpu_v2_texture_runtime_d("));
        assert!(!v.contains("module gpu_v2_texture_serial_lod "));
        fs::write(out.join(format!("postquad-{name}.v")), v).unwrap();
    }
}
