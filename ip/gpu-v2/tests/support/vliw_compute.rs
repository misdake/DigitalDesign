use crate::{vertex_support::rtl::Pin, Edge};
use gpu_v2::system::vliw_cycle::vertex::Bits;
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let i = e.i;
    let o = e.o;
    let s = &e.p.state;
    let l = &e.p.lod;
    let w = &e.p.window;
    let mut p = Vec::new();
    macro_rules! pin {
        ($name:expr,$width:expr,$v:expr,$out:expr) => {
            p.push(Pin {
                name: $name.into(),
                width: $width,
                value: ($v) as u128,
                known: (1u128 << $width) - 1,
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
        ("ce", i.ce),
        ("read_bank", i.read_bank),
        ("write_bank", i.write_bank),
        ("coefficient_bwe", i.coefficient_bwe),
        ("coefficient_cwe", i.coefficient_cwe),
        ("sample_valid", i.sample_valid),
        ("sample_cover", i.sample_cover),
        ("sample_anchor", i.sample_anchor),
        ("cfg_valid", i.cfg_valid),
        ("cfg_rgb", i.cfg_rgb),
        ("param_valid", i.param_valid),
        ("param_bank", i.param_bank),
        ("quad_end_valid", i.quad_end_valid),
    ] {
        pin!(n, 1, v, false);
    }
    pin!("coefficient_bwa", 4, i.coefficient_bwa, false);
    pin!("coefficient_cwa", 4, i.coefficient_cwa, false);
    pin!("coefficient_bwd", 36, i.coefficient_bwd.value, false);
    pin!("coefficient_cwd", 36, i.coefficient_cwd.value, false);
    pin!("sample_pid", 5, i.sample_pid, false);
    pin!("sample_h", 30, i.sample_h, false);
    pin!("cfg_z_bias", 8, i.cfg_z_bias as u8, false);
    pin!("cfg_far", 32, i.cfg_far, false);
    pin!("cfg_shift0", 5, i.cfg_shifts[0], false);
    pin!("cfg_shift1", 5, i.cfg_shifts[1], false);
    pin!("cfg_shift2", 5, i.cfg_shifts[2], false);
    pin!("param_index", 2, i.param_index, false);
    pin!("param_data", 51, i.param_data.value, false);
    pin!("cfg_tex_u", 5, i.cfg_tex_u, false);
    pin!("cfg_tex_v", 5, i.cfg_tex_v, false);
    pin!("cfg_max_mip", 5, i.cfg_max_mip, false);
    pin!("cfg_bias", 8, i.cfg_bias as u8, false);
    pin!("quad_end_qid", 3, i.quad_end_qid, false);
    pin!("quad_end_mask", 4, i.quad_end_mask, false);
    pin!("phase", 2, o.phase, true);
    pin!("busy", 1, o.busy, true);
    pin!("result_we", 1, o.result_we, true);
    pin!("result_wa", 7, o.result_address, true);
    bits(&mut p, "result_wd", 36, o.result_data);
    pin!("depth_event_valid", 1, o.depth_valid, true);
    pin!("depth_event_pid", 5, o.depth_pid, true);
    pin!("depth_event_z16", 16, o.depth_z16, true);
    pin!("depth_event_pass", 1, o.depth_pass, true);
    pin!("lod_valid", 1, o.lod_valid, true);
    pin!("lod_id", 3, o.lod_id, true);
    pin!("lod_value", 8, o.lod_value, true);
    pin!("tap_valid", 1, o.tap_valid, true);
    pin!("tap_pid", 5, o.tap_pid, true);
    pin!("tap_index", 2, o.tap_index, true);
    bits(&mut p, "tap_a", 18, o.tap_a);
    bits(&mut p, "tap_b", 18, o.tap_b);
    for (n, width, v) in [
        ("valid_pipe", 4, s.valid_pipe),
        ("cover_pipe", 4, s.cover_pipe),
        ("anchor_pipe", 4, s.anchor_pipe),
        ("pid0", 5, s.pid[0]),
        ("pid1", 5, s.pid[1]),
        ("pid2", 5, s.pid[2]),
        ("pid3", 5, s.pid[3]),
        ("z_bias", 8, s.z_bias as u8),
        ("shift0", 5, s.shifts[0]),
        ("shift1", 5, s.shifts[1]),
        ("shift2", 5, s.shifts[2]),
        ("hm_exponent", 5, s.hm_exponent),
        ("wide_valid_pipe", 2, s.wide_valid_pipe),
        ("wide_tag0", 5, s.wide_tag0),
        ("wide_tag1", 5, s.wide_tag1),
        ("log_return_tag", 5, s.log_return_tag),
        ("param_read_row", 2, s.param_row),
    ] {
        pin!(format!("obs_{n}"), width, v, true);
    }
    for (n, v) in [
        ("rgb", s.rgb),
        ("d_zero", s.d_zero),
        ("negative_prefix", s.negative_prefix),
        ("hm_zero", s.hm_zero),
        ("z_dzero_early", s.z_dzero_early),
        ("z_dzero", s.z_dzero),
        ("z_new_pass", s.z_new_pass),
        ("z_early_pass", s.z_early_pass),
        ("log_return_valid", s.log_return_valid),
    ] {
        pin!(format!("obs_{n}"), 1, v, true);
    }
    for (n, width, v) in [
        ("d_acc", 32, u64::from(s.d_acc)),
        ("fraction", 9, s.fraction.into()),
        ("seed", 18, s.seed.into()),
        ("r", 18, s.r.into()),
        ("weight0", 18, s.weight0.into()),
        ("weight1", 18, s.weight1.into()),
        ("clipped0", 17, s.clipped0.into()),
        ("hm0", 17, s.hm0.into()),
        ("hm1", 17, s.hm1.into()),
        ("z_shift", 9, (s.z_shift as u16 & 511).into()),
        ("z_shift_early", 9, (s.z_shift_early as u16 & 511).into()),
        ("z_new", 16, s.z_new.into()),
        ("z_early", 16, s.z_early.into()),
    ] {
        pin!(format!("obs_{n}"), width, v, true);
    }
    bits(&mut p, "obs_table_q", 36, e.p.table_q);
    bits(&mut p, "obs_wide_result", 54, e.p.wide.result);
    bits(&mut p, "obs_operand_value", 18, e.p.anchor_q);
    pin!("obs_mul_result", 36, e.p.multiply.result, true);
    pin!("obs_mant", 18, w.mant, true);
    pin!("obs_exponent", 5, w.d_exponent, true);
    for (n, width, v) in [
        ("arrival", 1, u64::from(l.arrival)),
        ("arrival_run", 1, l.arrival_run.into()),
        ("calendar_valid", 3, l.calendar_valid.into()),
        ("calendar_run", 3, l.calendar_run.into()),
        ("kernel_valid", 1, l.kernel_valid.into()),
        ("kernel_run", 1, l.kernel_run.into()),
        ("kernel_id", 3, l.kernel_id.into()),
        ("axis", 2, l.axis.into()),
        ("issue_live", 1, l.issue_live.into()),
        ("issue_tag", 5, l.issue_tag.into()),
        ("finish_valid", 2, l.finish_valid.into()),
        ("finish_run", 2, l.finish_run.into()),
        ("result_id", 3, l.result_id.into()),
        ("finish_pending", 1, l.finish_pending.into()),
        ("pending_run", 1, l.pending_run.into()),
        ("bias_base", 16, (l.bias_base as u16).into()),
        ("log_context", 5, l.log_context.into()),
        ("sampled_zero", 1, l.sampled_zero.into()),
        ("sampled_exp", 6, l.sampled_exp.into()),
        ("maximum", 15, l.maximum.into()),
        ("have_gradient", 1, l.have_gradient.into()),
        ("pw_read_address", 4, l.b_address.into()),
        ("pw_acc_read_address", 4, l.c_address.into()),
        ("offset_read_address", 3, l.offset_address.into()),
        ("anchor_known", 4, l.anchor_known.into()),
        ("dlog_known", 8, l.dlog_known.into()),
        ("end_id", 3, l.end_id.into()),
    ] {
        pin!(format!("lod_{n}"), width, v, true);
    }
    bits(&mut p, "lod_finish_offset", 16, l.finish_offset);
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
    source.push_str(");rc_compute dut(.clk(clk)");
    for f in p.iter().filter(|f| {
        !f.name.starts_with("obs_") && !f.name.starts_with("lod_")
            || matches!(f.name.as_str(), "lod_valid" | "lod_id" | "lod_value")
    }) {
        write!(source, ",.{}({})", f.name, f.name).unwrap();
    }
    source.push_str(");\n");
    for f in p.iter().filter(|f| {
        f.name.starts_with("obs_")
            || f.name.starts_with("lod_")
                && !matches!(f.name.as_str(), "lod_valid" | "lod_id" | "lod_value")
    }) {
        let path = if let Some(n) = f.name.strip_prefix("obs_") {
            n.to_owned()
        } else {
            format!("lod.{}", f.name.strip_prefix("lod_").unwrap())
        };
        writeln!(source, "assign {}=dut.{path};", f.name).unwrap();
    }
    source.push_str("endmodule\n");
    source
}
