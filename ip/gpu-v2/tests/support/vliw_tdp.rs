#[path = "vliw_pixel_source.rs"]
mod source;
use crate::{vertex_support::rtl::Pin, Edge};
use gpu_v2::system::vliw_cycle::vertex::Bits;
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let (i, o, p) = (&e.i, &e.o, &e.p);
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
        ("reset", i.reset),
        ("ce", i.ce),
        ("commit_allow", i.commit_allow),
        ("shade_phase", i.shade_phase),
        ("row_valid", i.row_valid),
        ("depth_write", i.depth_write),
        ("blend", i.blend),
        ("flush_request", i.flush_request),
        ("prefetch_abort", i.prefetch_abort),
        ("prefetch_valid", i.prefetch_valid),
        ("mem_req_ready", i.mem_req_ready),
        ("mem_read_valid", i.mem_read_valid),
        ("mem_write_ready", i.mem_write_ready),
        ("mem_complete_valid", i.mem_complete_valid),
        ("mem_complete_ok", i.mem_complete_ok),
    ] {
        pin!(n, 1, v, false);
    }
    for (n, w, v) in [
        ("row_tile", 9, u64::from(i.tile)),
        ("row_x", 9, i.x.into()),
        ("row_y", 8, i.y.into()),
        ("row_mask", 4, i.mask.into()),
        ("row_index", 3, i.row_index.into()),
        ("row_data", 32, i.data.get(32, "test row").unwrap()),
        ("row_alpha", 8, i.alpha.get(8, "test alpha").unwrap()),
        ("depth_func", 3, i.depth_func.into()),
        ("color_base", 14, i.color_base.into()),
        ("depth_base", 14, i.depth_base.into()),
        ("prefetch_tile", 10, i.prefetch_tile.into()),
        ("mem_read_data", 64, i.mem_read_data),
    ] {
        pin!(n, w, v, false);
    }
    for (n, v) in [
        ("row_ready", o.row_ready),
        ("row_accept", o.row_accept),
        ("commit_valid", o.commit_valid),
        ("flush_complete", o.flush_complete),
        ("idle", o.idle),
        ("mem_req_valid", o.mem_req_valid),
        ("mem_req_write", o.mem_req_write),
        ("mem_write_valid", o.mem_write_valid),
        ("shared_rop_offer", o.shared_rop_offer),
        ("shared_rop_ce", o.shared_rop_ce),
    ] {
        pin!(n, 1, v, true);
    }
    pin!("fault", 1, 0, true);
    pin!("preflight_bad", 1, 0, true);
    pin!("mem_req_addr", 32, o.mem_req_address, true);
    for (n, w, v) in [
        ("commit_x", 9, o.commit_x),
        ("commit_y", 8, o.commit_y),
        ("commit_mask", 4, o.commit_mask),
        ("mem_write_data", 64, o.mem_write_data),
        ("shared_rop_a", 27, o.shared_rop_a),
        ("shared_rop_b", 8, o.shared_rop_b),
        ("shared_product", 54, p.shared),
    ] {
        bits(&mut pins, n, w, v);
    }
    for (n, v) in [
        ("mt_active", p.mt_active),
        ("mt_write", p.mt_write),
        ("mt_cancel", p.mt_cancel),
        ("mt_request", p.mt_request),
        ("mt_prefetch", p.mt_prefetch),
        ("wr_beat_valid", p.wr_beat_valid),
        ("flushing", p.flushing),
        ("tail", p.tail),
        ("result_owner", p.result_owner),
        ("depth_pending", p.depth_pending),
        ("color_pending", p.color_pending),
        ("prefetch_protect_valid", p.protected),
    ] {
        pin!(format!("obs_{n}"), 1, v, true);
    }
    for (n, w, v) in [
        ("mt_line", 3, u64::from(p.mt_line)),
        ("mt_issue", 3, p.mt_issue.into()),
        ("mt_retire", 3, p.mt_retire.into()),
        ("mt_data", 7, p.mt_data.into()),
        ("mt_remaining", 2, p.mt_remaining.into()),
        ("mt_target", 10, p.mt_target.into()),
        ("cal", 2, p.cal.into()),
        ("fill_row", 3, p.fill_row.into()),
        ("return_key", 2, p.return_key.into()),
        ("hint_count", 2, p.hint_count.into()),
        ("hint0", 10, p.hints[0].into()),
        ("hint1", 10, p.hints[1].into()),
        ("prefetch_protect_line", 3, p.protect_line.into()),
        ("pinned", 8, p.pinned.into()),
    ] {
        pin!(format!("obs_{n}"), w, v, true);
    }
    for n in 0..8 {
        pin!(format!("obs_tvalid{n}"), 1, (p.valid >> n) & 1, true);
        pin!(format!("obs_dirc{n}"), 1, (p.color >> n) & 1, true);
        pin!(format!("obs_dird{n}"), 1, (p.depth >> n) & 1, true);
        pin!(format!("obs_tag{n}"), 10, p.tag[n], true);
    }
    for n in 0..4 {
        bits(&mut pins, &format!("obs_aq{n}"), 16, p.aq[n]);
        bits(&mut pins, &format!("obs_bq{n}"), 16, p.bq[n]);
    }
    pins
}
pub fn production(p: &[Pin]) -> String {
    let mut s=String::from("`define GPU_V2_COMMAND_MAINTENANCE\n`define GPU_V2_COMMAND_PORTS\n`define GPU_VLIW_POOL_PORTABLE\n");
    for name in [
        "gpu_v2_framebuffer_tdp",
        "gpu_v2_framebuffer_tdp_bank",
        "gpu_v2_rop_tdp",
        "gpu_v2_final_rop_bank",
    ] {
        s.push_str(source::module(name));
        s.push('\n');
    }
    s.push_str("module g2a(input clk");
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
    s.push_str(");\ngpu_v2_framebuffer_tdp #(.PREFETCH_ENABLE(1),.PREFETCH_DIRTY(1)) dut(.clk(clk),.shared_rop_product(shared_product),.ui_active(1'b0),.ui_cmd_valid(1'b0),.ui_cmd_write(1'b0),.ui_lower_ready(1'b0),.ui_tile(9'd0),.copy_active(1'b0),.copy_address(14'd0),.ui_pixel_valid(1'b0),.ui_pixel_half(1'b0),.ui_pixel_word(5'd0),.ui_pixels(64'd0)");
    for f in p
        .iter()
        .filter(|f| !f.name.starts_with("obs_") && f.name != "shared_product")
    {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\ngpu_v2_final_rop_bank bank(.clk(clk),.reset(reset),.rop_ce(shared_rop_ce),.rop_a(shared_rop_a),.rop_b(shared_rop_b),.rop_product(shared_product),.spec_ce(1'b0),.spec_a(9'd0),.spec_b(24'd0));\n");
    for f in p.iter().filter(|f| f.name.starts_with("obs_")) {
        let name = &f.name[4..];
        let path = if let Some(n) = name.strip_prefix("tvalid") {
            format!("tvalid[{n}]")
        } else if let Some(n) = name.strip_prefix("dirc") {
            format!("dirc[{n}]")
        } else if let Some(n) = name.strip_prefix("dird") {
            format!("dird[{n}]")
        } else if let Some(n) = name.strip_prefix("tag") {
            format!("tag[{n}]")
        } else if let Some(n) = name.strip_prefix("aq") {
            format!("a_q[{n}]")
        } else if let Some(n) = name.strip_prefix("bq") {
            format!("b_q[{n}]")
        } else {
            name.into()
        };
        writeln!(s, "assign {}=dut.{path};", f.name).unwrap();
    }
    s.push_str("endmodule\n");
    s
}
