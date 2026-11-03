use crate::{vertex_support::rtl::Pin, Edge};
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let i = e.i;
    let o = e.o;
    let s = &e.s;
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
    for (n, v) in [
        ("reset", i.reset),
        ("ce", i.ce),
        ("start", i.start),
        ("bbox_done", i.bbox_done),
        ("bbox_empty", i.bbox_empty),
        ("run_enable", i.run_enable),
        ("geo_we", i.geo_write),
        ("quad_ready", i.quad_ready),
    ] {
        pin!(n, 1, v, false);
    }
    pin!("bbox_xlo", 5, i.bbox_xlo, false);
    pin!("bbox_xhi", 5, i.bbox_xhi, false);
    pin!("bbox_ylo", 4, i.bbox_ylo, false);
    pin!("bbox_yhi", 4, i.bbox_yhi, false);
    pin!("geo_wa", 4, i.geo_address, false);
    pin!("geo_wd", 30, i.geo_data.value, false);
    // The phase peer is a real enabled counter in the wrapper, so the original
    // negedge replica assertion cannot race a tape-driven phase assignment.
    pin!("phase", 2, s.phase, true);
    for (n, v) in [
        ("quad_valid", o.quad_valid),
        ("quad_take", o.quad_take),
        ("sample_valid", o.sample_valid),
        ("sample_cover", o.sample_cover),
        ("sample_anchor", o.sample_anchor),
        ("quad_end_valid", o.quad_end_valid),
        ("busy", o.busy),
        ("done", o.done),
        ("full_scan", o.full_scan),
        ("evt_import", o.evt_import),
        ("evt_prep", o.evt_prep),
        ("evt_traverse", o.evt_traverse),
        ("evt_row", o.evt_row),
        ("evt_align", o.evt_align),
        ("evt_credit_wait", o.evt_credit_wait),
        ("evt_leaf", o.evt_leaf),
        ("evt_empty_quad", o.evt_empty_quad),
        ("evt_node", o.evt_node),
    ] {
        pin!(n, 1, v, true);
    }
    pin!("quad_x", 8, o.quad_x, true);
    pin!("quad_y", 7, o.quad_y, true);
    pin!("sample_lane", 2, o.sample_lane, true);
    pin!("quad_mask", 4, o.quad_mask, true);
    pin!("error", 4, 0, true);
    p.push(Pin {
        name: "sample_h".into(),
        width: 30,
        value: o.sample_h.value.into(),
        known: o.sample_h.known.into(),
        output: true,
    });
    for (n, width, v) in [
        ("raster_phase", 2, u64::from(s.phase)),
        ("life", 2, s.life.into()),
        ("scan_mode", 3, s.scan_mode.into()),
        ("tau", 3, s.tau.into()),
        ("w_pc", 5, s.w_pc.into()),
        ("w_edge", 2, s.w_edge.into()),
        ("s_edge", 2, s.s_edge.into()),
        ("prep_level", 4, s.prep_level.into()),
        ("level", 4, s.level.into()),
        ("nx", 9, s.nx.into()),
        ("ny", 9, s.ny.into()),
        ("w_pass", 2, s.w_pass.into()),
        ("tile_head", 3, s.tile_head.into()),
        ("tile_tail", 3, s.tile_tail.into()),
        ("tile_meta0", 23, s.tile_meta[0].into()),
        ("tile_meta1", 23, s.tile_meta[1].into()),
        ("check_mask", 4, s.check_mask.into()),
        ("origin_x", 9, s.origin_x.into()),
        ("sx", 9, s.sx.into()),
        ("sy", 9, s.sy.into()),
        ("end_x", 9, s.end_x.into()),
        ("end_y", 9, s.end_y.into()),
        ("lane", 2, s.lane.into()),
        ("cover_mask", 4, s.cover_mask.into()),
        ("rom_pc", 5, s.rom_pc.into()),
        ("r_edge", 2, s.r_edge.into()),
        ("r_tw", 8, s.r_tw.into()),
        ("r_uop", 36, s.r_uop),
        ("r_pc", 5, s.r_pc.into()),
        ("s_kind", 3, s.s_kind.into()),
        ("s_return_edge", 2, s.s_return_edge.into()),
        ("origin_valid", 2, s.origin_valid.into()),
        ("origin_edge0", 2, s.origin_edge0.into()),
        ("origin_edge1", 2, s.origin_edge1.into()),
        ("sim_geo_count", 32, s.sim_geo_count.into()),
        ("sim_mail_written0", 3, s.mail_written[0].into()),
        ("sim_mail_written1", 3, s.mail_written[1].into()),
    ] {
        pin!(format!("obs_{n}"), width, v, true);
    }
    for (n, v) in [
        ("walk_done", s.walk_done),
        ("w_epoch", s.w_epoch),
        ("row_first", s.row_first),
        ("scan_slot", s.scan_slot),
        ("scan_full", s.scan_full),
        ("scan_qvalid", s.scan_qvalid),
        ("scan_inflight", s.scan_inflight),
        ("anchor_seen", s.anchor_seen),
        ("scan_bank", s.scan_bank),
        ("prefetch_active", s.prefetch_active),
        ("scan_classify_pending", s.scan_classify_pending),
        ("cover_acc", s.cover_acc),
        ("rom_valid", s.rom_valid),
        ("r_valid", s.r_valid),
        ("r_sub", s.r_sub),
        ("r_epoch", s.r_epoch),
        ("s_valid", s.s_valid),
        ("s_sub", s.s_sub),
        ("check_tau_q", s.check_tau),
    ] {
        pin!(format!("obs_{n}"), 1, v, true);
    }
    pin!("obs_uop", 36, e.u, true);
    for ((n, width), q) in [
        ("work_q", 30),
        ("mailbox_q", 30),
        ("scan_work_q", 30),
        ("quad_nx_q", 18),
        ("quad_ny_q", 18),
        ("packed_step_q", 36),
        ("scan_step_q", 30),
        ("origin_result", 54),
    ]
    .into_iter()
    .zip(e.q)
    {
        p.push(Pin {
            name: format!("obs_{n}"),
            width,
            value: q.value.into(),
            known: q.known.into(),
            output: true,
        });
    }
    p
}
pub fn production(p: &[Pin]) -> String {
    let mut source=String::from("`define GPU_V2_COMMAND_PORTS\n`define GPU_V2_LIVE_GLOBALS\n`define GPU_V2_TRUSTED_FRONTEND\n");
    source.push_str(&gpu_v2::frontend::vliw::verilog(true));
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
    source.push_str(");reg[1:0]producer_phase;assign phase=producer_phase;always @(posedge clk)if(reset)producer_phase<=0;else if(ce)producer_phase<=producer_phase+1'b1;\nrc_raster dut(.clk(clk)");
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
