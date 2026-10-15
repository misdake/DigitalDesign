//! Actual factories with two legal draw texture slots for drain/rebind qualification.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::pixel::{postquad, quad_ingress},
    texture::ports::Slot,
};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        args.len() == 4
            && matches!(args[2].as_str(), "group34" | "weight-order")
            && matches!(args[3].as_str(), "native" | "portable"),
        "usage: unique_group34_slots OUTPUT group34|weight-order native|portable"
    );
    let slots = [
        Slot {
            base_address: 0x1000,
            max_size_log2: 6,
            has_full_mip: true,
            valid: true,
        },
        Slot {
            base_address: 0x3000,
            max_size_log2: 6,
            has_full_mip: true,
            valid: true,
        },
    ];
    let surface = MaterializedSurface {
        color_base_bytes: 0x100000,
        depth_base_bytes: 0x140000,
        width: 512,
        height: 256,
    };
    let native = args[3] == "native";
    let backend = if native {
        quad_ingress::rtl::Backend::Native
    } else {
        quad_ingress::rtl::Backend::Portable
    };
    let s = if args[2] == "group34" {
        postquad::verilog_combined_unique_group34_candidate(&slots, surface, backend)
    } else {
        postquad::verilog_combined_weight_order_candidate(&slots, surface, backend)
    }
    .unwrap();
    std::fs::write(
        &args[1],
        format!(
            "{}{}",
            if native {
                "`define GPU_V2_GOWIN_DSP\n"
            } else {
                ""
            },
            s
        ),
    )
    .unwrap();
}
