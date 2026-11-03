use crate::{vertex_support::rtl::Pin, Edge};
use gpu_v2::system::vliw_cycle::vertex::Bits;
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let i = e.i;
    let o = e.o;
    let s = &e.s;
    let h = &e.h;
    let mut p = Vec::new();
    macro_rules! pin {
        ($n:expr,$w:expr,$v:expr,$out:expr) => {
            p.push(Pin {
                name: $n.into(),
                width: $w,
                value: ($v) as u128,
                known: (1u128 << $w) - 1,
                output: $out,
            });
        };
    }
    fn bits(p: &mut Vec<Pin>, n: &str, width: u32, v: Bits) {
        let m = (1u64 << width) - 1;
        p.push(Pin {
            name: n.into(),
            width,
            value: (v.value & m).into(),
            known: (v.known & m).into(),
            output: true,
        });
    }
    for (n, v) in [
        ("reset", i.reset),
        ("front_ce", i.front_ce),
        ("backend_ce", i.backend_ce),
        ("draw_unlit", i.draw_unlit),
        ("quad_valid", i.quad_valid),
        ("quad_rgb", i.quad_rgb),
        ("quad_end_valid", i.quad_end_valid),
        ("result_we", i.result_write),
        ("depth_event_valid", i.depth_valid),
        ("depth_event_pass", i.depth_pass),
        ("lod_valid", i.lod_valid),
        ("publish_ready", i.publish_ready),
        ("basic_read", i.basic_read),
        ("depth_read", i.depth_read),
        ("light_ready", i.light_ready),
        ("sample_ready", i.sample_ready),
        ("retire_valid", i.retire_valid),
        ("empty_skip", i.empty_skip),
    ] {
        pin!(n, 1, v, false);
    }
    for (n, width, v) in [
        ("phase", 2, u64::from(i.phase)),
        ("quad_color", 32, i.quad_color.into()),
        ("quad_x", 8, i.quad_x.into()),
        ("quad_y", 7, i.quad_y.into()),
        ("quad_end_id", 3, i.quad_end_id.into()),
        ("quad_end_mask", 4, i.quad_end_mask.into()),
        ("result_wa", 7, i.result_address.into()),
        ("result_wd", 36, i.result_data.value),
        ("depth_event_pid", 5, i.depth_pid.into()),
        ("depth_event_z16", 16, i.depth_z16.into()),
        ("lod_id", 3, i.lod_id.into()),
        ("lod_value", 8, i.lod_value.into()),
        ("descriptor_issue_slot", 5, i.descriptor_issue_slot.into()),
        ("basic_read_address", 7, i.basic_address.into()),
        ("depth_read_address", 7, i.depth_address.into()),
        ("retire_id", 5, i.retire_id.into()),
    ] {
        pin!(n, width, v, false);
    }
    for (n, v) in [
        ("quad_ready", o.quad_ready),
        ("prefetch_valid", o.prefetch_valid),
        ("published", o.published),
        ("basic_collision", o.basic_collision),
        ("light_valid", o.light_valid),
        ("sample_valid", o.sample_valid),
        ("empty_head", o.empty_head),
        ("empty_skip_ready", o.empty_skip_ready),
        ("busy", o.busy),
    ] {
        pin!(n, 1, v, true);
    }
    for (n, width, v) in [
        ("quad_id", 3, u64::from(o.quad_id)),
        ("prefetch_tile", 10, o.prefetch_tile.into()),
        ("published_slot", 5, o.published_slot.into()),
        ("light_lane", 2, o.light_lane.into()),
        ("light_descriptor", 10, o.light_descriptor.into()),
        ("sample_row", 2, o.sample_row.into()),
        ("sample_descriptor", 19, o.sample_descriptor.into()),
        ("retire_slot", 5, o.retire_slot.into()),
        ("global_used", 6, o.global_used.into()),
        ("producer_used", 6, o.producer_used.into()),
        ("light_pending", 6, o.light_pending.into()),
        ("sample_pending", 6, o.sample_pending.into()),
    ] {
        pin!(n, width, v, true);
    }
    for (n, width, v) in [
        ("published_begin", 23, o.published_begin),
        ("descriptor_issue", 23, o.descriptor_issue),
        ("descriptor_head", 23, o.descriptor_head),
        ("descriptor_next", 23, o.descriptor_next),
        ("descriptor_head_tile", 9, o.descriptor_head_tile),
        ("descriptor_next_tile", 9, o.descriptor_next_tile),
        ("issue_color", 24, o.issue_color),
        ("basic_return_data", 24, o.basic_data),
        ("depth_data", 24, o.depth_data),
        ("light_normal", 36, o.light_normal),
        ("light_position", 32, o.light_position),
        ("sample_data", 36, o.sample_data),
        ("sample_last", 1, o.sample_last),
    ] {
        bits(&mut p, n, width, v);
    }
    pin!("fault", 1, 0, true);
    for (n, width, v) in [
        ("allocated", 6, u64::from(s.allocated)),
        ("publish_ptr", 6, s.publish_ptr.into()),
        ("retired", 6, s.retired.into()),
        ("complete_tail", 6, s.complete_tail.into()),
        ("light_cursor", 6, s.light_cursor.into()),
        ("sample_cursor", 6, s.sample_cursor.into()),
        ("attr_pixel", 8, s.attr_pixel.into()),
        ("light_descriptor_q", 10, s.light_descriptor.into()),
        ("sample_descriptor_q", 19, s.sample_descriptor.into()),
        ("depth_mask", 4, s.depth_mask.into()),
        ("light_state", 2, s.light_state.into()),
        ("sample_loaded", 1, s.sample_loaded.into()),
        ("sample_lane", 2, s.sample_lane.into()),
        ("depth_pixel", 8, h.depth_pixel.into()),
        ("shadow_end_tail", 6, h.end_tail.into()),
        ("shadow_lod_tail", 6, h.lod_tail.into()),
        ("attr_word", 2, h.attr_word.into()),
        ("attribute_rgb", 1, h.attribute_rgb.into()),
    ] {
        pin!(format!("obs_{n}"), width, v, true);
    }
    bits(&mut p, "obs_normal_y_high", 6, s.normal_y_high);
    bits(&mut p, "obs_ndc_x", 16, s.ndc_x);
    for ((n, width), q) in [
        ("normal_q", 36),
        ("uv_q", 36),
        ("basic_q", 36),
        ("depth_data", 24),
        ("ndc_q", 32),
    ]
    .into_iter()
    .zip(e.q)
    {
        bits(&mut p, &format!("obs_{n}"), width, q);
    }
    p
}
pub fn production(p: &[Pin]) -> String {
    let mut source=String::from("`define GPU_V2_COMMAND_PORTS\n`define GPU_V2_LIVE_GLOBALS\n`define GPU_V2_VLIW_POOL_COMMAND\n");
    source.push_str(&gpu_v2::system::pixel::vliw_quad_pool::verilog(true));
    source.push_str("\nmodule g2a(input clk");
    for f in p {
        write!(
            source,
            ",{}[{}:0]{}",
            if f.output { "output" } else { "input" },
            f.width - 1,
            f.name
        )
        .unwrap();
    }
    source.push_str(");gpu_v2_vliw_quad_pool dut(.clk(clk)");
    for f in p.iter().filter(|f| !f.name.starts_with("obs_")) {
        write!(source, ",.{}({})", f.name, f.name).unwrap();
    }
    source.push_str(");\n");
    for f in p.iter().filter(|f| f.name.starts_with("obs_")) {
        writeln!(
            source,
            "assign {}=dut.{};",
            f.name,
            f.name.strip_prefix("obs_").unwrap()
        )
        .unwrap();
    }
    source.push_str("endmodule\n");
    source
}
