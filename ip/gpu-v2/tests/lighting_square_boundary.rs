//! The low and high split-square operands must retain the same register age.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::{pixel::quad_ingress::rtl::Backend, vliw_command},
};

#[test]
fn selected_square_corrections_keep_the_prescaled_register_boundary() {
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 400,
        height: 240,
    };
    for backend in [Backend::Native, Backend::Portable] {
        let source = vliw_command::verilog(surface, backend).unwrap();
        for (coordinate, producer) in [("z", 12), ("y", 11)] {
            assert!(source.contains(&format!(
                "(* syn_preserve=1, syn_keep=1 *) reg [15:0] unit{producer}_p0;"
            )));
            assert!(source.contains(&format!(
                "(* syn_keep=1 *) wire [6:0] square_{coordinate}_tail_q=unit{producer}_p0[6:0];"
            )));
            assert!(source.contains(&format!(".B({{2'b00,square_{coordinate}_tail_q}})")));
        }
        if matches!(backend, Backend::Native) {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/gpu-lighting-square-boundary");
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join("dut.v"), source).unwrap();
        }
    }
}
