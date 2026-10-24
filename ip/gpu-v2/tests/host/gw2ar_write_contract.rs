//! Physical write contracts which a permissive vendor functional model cannot qualify.
use gpu_v2::{
    framebuffer::ports::MaterializedSurface,
    system::{pixel::quad_ingress::rtl::Backend, vliw_command},
    texture::{ports::Slot, rtl::cache},
};

fn full_36_bit_writes(source: &str) -> usize {
    let mut count = 0;
    for tail in source.split(".BIT_WIDTH_0(36)").skip(1) {
        let instance = &tail[..tail.find(");").expect("RAM instance terminator")];
        let ada = instance.split(".ADA(").nth(1).expect("RAM write address");
        let ada = ada.split(".ADB(").next().unwrap();
        assert!(
            ada.contains("4'b1111") || ada.contains("5'b01111"),
            "GW2AR 36-bit writes must replace the complete word: {ada}"
        );
        count += 1;
    }
    count
}

#[test]
fn command_and_component_exports_do_not_depend_on_masked_36_bit_writes() {
    let source = vliw_command::verilog(
        MaterializedSurface {
            color_base_bytes: 0x100000,
            depth_base_bytes: 0x140000,
            width: 400,
            height: 240,
        },
        Backend::Native,
    )
    .unwrap();
    assert!(full_36_bit_writes(&source) > 0);
    assert!(source.contains(".ADA({wa,write_half,4'b0011})"));
    assert!(source.contains(".DI({12'd0,wd}),.ADA({2'd0,wa,1'b0,4'b1111})"));
    // Alpha gates a whole independent 16-bit framebuffer bank; it does not
    // enable only selected bytes inside a native memory word.
    assert!(source.contains("wire uw=ui_pixel_valid&&up[0];"));
    assert!(source.contains(".BIT_WIDTH_0(16),.BIT_WIDTH_1(16)"));
    assert!(source.contains(".ADA({a_addr,2'b00,{2{a_we}}})"));
    let legacy = cache::verilog(&[Slot {
        base_address: 0x1000,
        max_size_log2: 6,
        has_full_mip: true,
        valid: true,
    }]);
    full_36_bit_writes(&legacy);
    for cache in [&source, &legacy] {
        assert!(cache.contains(".BIT_WIDTH_0(18),.BIT_WIDTH_1(36)) bank("));
        assert!(cache.contains("vic_line[5:2],vic_line[0],4'b0011"));
        assert!(!cache.contains("byte_mask"));
    }
}
