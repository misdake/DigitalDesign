#[allow(dead_code)]
#[path = "vliw_pixel_source.rs"]
mod source;
use crate::{vertex_support::rtl::Pin, Edge};
use gpu_v2::system::vliw_cycle::vertex::Bits;
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let (i, o, s) = (&e.i, &e.o, &e.s);
    let f = i.producer.frontend;
    let b = i.backend;
    let p = o.producer.pool;
    let front = o.producer.frontend;
    let bo = o.backend;
    let c = bo.sink.cache;
    let mut pins = Vec::new();
    macro_rules! pin {
        ($n:expr,$w:expr,$v:expr,$out:expr) => {
            pins.push(Pin {
                name: $n.into(),
                width: $w,
                value: ($v) as u128,
                known: (1u128 << $w) - 1,
                output: $out,
            });
        };
    }
    fn bits(p: &mut Vec<Pin>, n: &str, w: u32, b: Bits) {
        let mask = if w == 64 { u64::MAX } else { (1u64 << w) - 1 };
        p.push(Pin {
            name: n.into(),
            width: w,
            value: (b.value & mask).into(),
            known: (b.known & mask).into(),
            output: true,
        });
    }
    for (n, v) in [
        ("reset", f.reset),
        ("ce", f.ce),
        ("abort", b.abort),
        ("start", f.start),
        ("slot", f.slot),
        ("dma_we", f.dma_write),
        ("output_ready", f.output_ready),
        ("draw_unlit", i.producer.draw_unlit),
        ("depth_write", b.depth_write),
        ("blend", b.blend),
        ("texture_mip", f.params.max_mip != 0),
        ("flush_request", b.flush_request),
        ("framebuffer_request_ready", b.memory.request_ready),
        ("framebuffer_read_valid", b.memory.read_valid),
        ("framebuffer_write_ready", b.memory.write_ready),
        ("framebuffer_complete", b.memory.complete),
        ("framebuffer_ok", b.memory.ok),
    ] {
        pin!(n, 1, v, false);
    }
    for (n, w, v) in [
        ("mesh_color", 32, u64::from(f.mesh_color)),
        ("dma_addr", 10, f.dma_address.into()),
        ("dma_data", 64, f.dma_data),
        ("cfg_far_z", 32, f.params.far_z.into()),
        ("texture_n", 4, f.params.tex_u.into()),
        ("texture_min_n", 2, 0),
        ("cfg_lod_bias", 8, u64::from(f.params.bias as u8)),
        ("specular", 24, b.specular.into()),
        ("depth_func", 3, b.depth_func.into()),
        ("color_base", 14, b.color_base.into()),
        ("depth_base", 14, b.depth_base.into()),
        ("framebuffer_read_data", 64, b.memory.read_data),
    ] {
        pin!(n, w, v, false);
    }
    for (n, v) in [
        ("dma_ready", front.meshlet.dma_ready),
        ("meshlet_done", front.meshlet.done),
        ("meshlet_ready", front.meshlet.ready),
        ("frontend_idle", front.idle),
        ("pipeline_idle", front.idle && bo.idle),
        ("idle", o.idle),
        ("fault", bo.fault),
        ("quad_admitted", f.ce && front.quad_valid && p.quad_ready),
        ("quad_empty_skipped", bo.feedback.empty_skip),
        ("quad_published", p.published),
        ("light_result", bo.light_result),
        ("sample_result", bo.sample_result),
        ("final_issue", bo.final_issue),
        ("final_result", bo.final_result),
        ("quad_accepted", bo.sink.quad_accepted),
        ("commit_valid", c.commit_valid),
        ("flush_complete", c.flush_complete),
        ("framebuffer_request_valid", c.mem_req_valid),
        ("framebuffer_request_write", c.mem_req_write),
        ("framebuffer_write_valid", c.mem_write_valid),
    ] {
        pin!(n, 1, v, true);
    }
    pin!("texture_request_valid", 1, 0, true);
    pin!("global_used", 6, p.global_used, true);
    pin!("producer_used", 6, p.producer_used, true);
    pin!("work_reserved", 5, 0, true);
    pin!("work_count", 5, 0, true);
    pin!("framebuffer_request_address", 32, c.mem_req_address, true);
    for (n, w, v) in [
        ("commit_x", 9, c.commit_x),
        ("commit_y", 8, c.commit_y),
        ("commit_mask", 4, c.commit_mask),
        ("framebuffer_write_data", 64, c.mem_write_data),
        ("obs_header", 23, s.header),
        ("obs_light_q", 18, e.light),
        ("obs_sample_q", 24, e.sample),
        ("obs_final_rgb", 24, bo.final_stage.rgb),
        ("obs_spec_a", 9, bo.final_stage.shared_spec_a),
        ("obs_spec_b", 24, bo.final_stage.shared_spec_b),
        ("obs_rop_a", 27, c.shared_rop_a),
        ("obs_rop_b", 8, c.shared_rop_b),
    ] {
        bits(&mut pins, n, w, v);
    }
    for (n, v) in [
        ("active", s.active),
        ("state", s.wait),
        ("shade_phase", s.shade_phase),
        ("light_complete", s.light_complete),
        ("sample_complete", s.sample_complete),
    ] {
        pin!(format!("obs_{n}"), 1, v, true);
    }
    pin!("obs_issue_slot", 5, s.issue_slot, true);
    pin!("obs_issue_lane", 2, s.issue_lane, true);
    pin!("obs_input_bypass", 2, s.bypass, true);
    pin!("obs_light_last", 5, s.light_last, true);
    pin!("obs_sample_last", 5, s.sample_last, true);
    pins
}
pub fn production(p: &[Pin]) -> String {
    let mut s = source::complete().to_owned();
    // Group35 is a fixed native SDP36 even in the portable numerical factory.
    // Embed the actual vendor library so the stored DUT is self-contained.
    let library = std::path::PathBuf::from(
        std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME for fixed native Group35"),
    )
    .join("IDE/simlib/gw2a/prim_sim.v");
    s.push('\n');
    s.push_str(&std::fs::read_to_string(library).unwrap());
    s.push_str("\nmodule g2a(input clk");
    for f in p {
        write!(
            s,
            ",{}[{}:0]{}",
            if f.output { "output" } else { "input" },
            f.width - 1,
            f.name
        )
        .unwrap();
    }
    s.push_str(");\ngpu_v2_vliw_three_port dut(.clk(clk),.cp_re(1'b0),.cp_addr(9'd0),.texture_base(16'd0),.texture_filter(2'd0),.texture_invalidate(1'b0),.texture_fb400240(1'b0),.context_epoch(16'd0),.context_mode(2'd0),.context_code(5'd0),.light_x(16'd0),.light_y(16'd0),.light_z(16'd0),.ray_x(16'd0),.ray_y(16'd0),.ray_k(16'd0),.ambient(9'd0),.directional(9'd0),.ui_active(1'b0),.ui_cmd_valid(1'b0),.ui_cmd_write(1'b0),.ui_lower_ready(1'b0),.ui_tile(9'd0),.copy_active(1'b0),.copy_address(14'd0),.ui_pixel_valid(1'b0),.ui_pixel_half(1'b0),.ui_pixel_word(5'd0),.ui_pixels(64'd0),.texture_request_ready(1'b0),.texture_read_valid(1'b0),.texture_read_index(4'd0),.texture_read_data(64'd0),.texture_complete(1'b0),.texture_ok(1'b1)");
    for f in p.iter().filter(|f| !f.name.starts_with("obs_")) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for f in p.iter().filter(|f| f.name.starts_with("obs_")) {
        let n = &f.name[4..];
        let path = match n {
            "light_q" => "light_data",
            "sample_q" => "sample_data",
            "final_rgb" => "final_RGB",
            "spec_a" => "shared_spec_a",
            "spec_b" => "shared_spec_b",
            "rop_a" => "shared_rop_a",
            "rop_b" => "shared_rop_b",
            other => other,
        };
        writeln!(s, "assign {}=dut.back.{path};", f.name).unwrap();
    }
    s.push_str("endmodule\n");
    s
}
