//! Actual private preparation datapath: only T16 source and commit grant are peers.
use crate::{vertex_support::rtl::Pin, UnitEdge};
use gpu_v2::system::vliw_cycle::vertex::Bits;
use std::fmt::Write as _;

pub fn fields(e: &UnitEdge) -> Vec<Pin> {
    let i = e.i;
    let o = e.o;
    let r = &e.p.state;
    let s = &e.p.storage;
    let d = &e.p.dsp;
    let mut p = Vec::new();
    macro_rules! pin {
        ($name:literal,$width:expr,$value:expr,$output:expr) => {
            p.push(Pin {
                name: $name.into(),
                width: $width,
                value: u128::from($value),
                known: (1u128 << $width) - 1,
                output: $output,
            });
        };
    }
    fn bits(p: &mut Vec<Pin>, name: &str, width: u32, value: Bits) {
        p.push(Pin {
            name: name.into(),
            width,
            value: u128::from(value.value & ((1u64 << width) - 1)),
            known: u128::from(value.known & ((1u64 << width) - 1)),
            output: true,
        });
    }
    pin!("reset", 1, i.reset, false);
    pin!("ce", 1, i.ce, false);
    pin!("start", 1, i.start, false);
    pin!("execute_grant", 1, i.execute_grant, false);
    pin!("descriptor", 32, i.descriptor, false);
    pin!("in_valid", 1, i.raw_valid, false);
    pin!("in_index", 5, i.raw_index, false);
    pin!("in_data", 54, i.raw_data.value, false);
    pin!("cfg_far_z", 32, i.params.far_z, false);
    pin!("cfg_tex_u", 5, i.params.tex_u, false);
    pin!("cfg_tex_v", 5, i.params.tex_v, false);
    pin!("cfg_max_mip", 5, i.params.max_mip, false);
    pin!("cfg_bias", 8, i.params.bias as u8, false);
    pin!("replay_read", 1, i.replay_read, false);
    pin!("replay_address", 4, i.replay_address, false);
    pin!("busy", 1, o.busy, true);
    pin!("import_busy", 1, o.import_busy, true);
    pin!("in_ready", 1, o.raw_ready, true);
    pin!("done", 1, o.done, true);
    pin!("context_ready", 1, o.context_ready, true);
    pin!("cfg_apply", 1, o.cfg_apply, true);
    pin!("geo_we", 1, o.geo_write, true);
    pin!("geo_wa", 4, o.geo_address, true);
    bits(&mut p, "geo_wd", 30, o.geo_data);
    pin!("stream_cfg_z_bias", 8, o.z_bias as u8, true);
    pin!("stream_cfg_far", 32, o.far_z, true);
    pin!("stream_cfg_rgb", 1, o.rgb, true);
    pin!("stream_cfg_shift0", 5, o.shifts[0], true);
    pin!("stream_cfg_shift1", 5, o.shifts[1], true);
    pin!("stream_cfg_shift2", 5, o.shifts[2], true);
    pin!("stream_param_valid", 1, o.param_valid, true);
    pin!("stream_param_index", 2, o.param_index, true);
    pin!("stream_param_bank", 1, o.param_bank, true);
    bits(&mut p, "stream_param_data", 51, o.param_data);
    pin!("bound_valid", 1, o.bound_valid, true);
    pin!("bound_empty", 1, o.bound_empty, true);
    pin!("bound_xlo", 5, o.xlo, true);
    pin!("bound_xhi", 5, o.xhi, true);
    pin!("bound_ylo", 4, o.ylo, true);
    pin!("bound_yhi", 4, o.yhi, true);
    pin!("operand_we", 1, o.ports.a_write, true);
    pin!("operand_wa", 4, o.ports.a_write_address, true);
    bits(&mut p, "operand_wd", 18, o.ports.a_data);
    pin!("operand_re", 1, o.ports.a_read, true);
    pin!("operand_ra", 7, o.ports.a_address, true);
    pin!("pw_low0_we", 1, o.ports.b_write, true);
    pin!("pw_low1_we", 1, o.ports.b_write, true);
    pin!("pw_wa", 4, o.ports.b_write_address, true);
    bits(&mut p, "pw_low_wd", 36, o.ports.b_data);
    pin!("pw_re", 1, o.ports.b_read, true);
    pin!("pw_ra", 4, o.ports.b_address, true);
    pin!("pw_high_we", 1, o.ports.c_write, true);
    pin!("pw_high_wa", 4, o.ports.c_write_address, true);
    bits(&mut p, "pc_di", 48, o.ports.c_data);
    pin!("pw_acc_re", 1, o.ports.c_read, true);
    pin!("pw_acc_ra", 4, o.ports.c_address, true);
    bits(&mut p, "operand_value", 18, s.aq);
    bits(&mut p, "private_b", 36, s.bq());
    bits(&mut p, "private_c", 48, s.cq().slice(0, 48));
    bits(&mut p, "dsp_a", 18, o.dsp_a);
    pin!("dsp_asign", 1, o.dsp_signed, true);
    bits(&mut p, "dsp_b", 36, o.dsp_b);
    bits(&mut p, "dsp_c", 54, o.dsp_c);
    bits(&mut p, "dsp_result", 54, d.result);
    bits(&mut p, "dsp_ar", 18, d.a);
    bits(&mut p, "dsp_br", 36, d.b);
    pin!("dsp_sr", 1, d.signed, true);
    pin!("obs_operation", 6, o.operation, true);
    pin!("obs_edge_no", 2, r.edge_no, true);
    pin!("obs_field", 2, r.field, true);
    pin!("obs_edge_exp", 4, r.edge_exp, true);
    pin!("obs_exp0", 6, r.exp[0], true);
    pin!("obs_exp1", 6, r.exp[1], true);
    pin!("obs_exp2", 6, r.exp[2], true);
    pin!("obs_common_e", 6, r.common_e, true);
    pin!("obs_raw_exponent", 6, r.raw_exponent, true);
    pin!("obs_delta_e", 7, r.delta_e, true);
    pin!("obs_sigma", 1, r.sigma, true);
    pin!("obs_rgb", 1, r.rgb, true);
    bits(&mut p, "obs_depth_mantissa", 31, r.depth_mantissa);
    pin!("obs_seed_sign_x", 3, r.seed_x, true);
    pin!("obs_seed_sign_y", 3, r.seed_y, true);
    pin!("obs_read_valid", 1, r.read_valid, true);
    pin!("obs_read_exp_q", 4, r.read_exp, true);
    pin!("obs_quant_pipe", 2, r.quant_pipe, true);
    pin!("obs_quant_edge", 2, r.quant_edge, true);
    pin!("obs_quant_field", 2, r.quant_field, true);
    pin!("obs_quant_negative0", 1, r.negative0, true);
    pin!("obs_quant_negative1", 1, r.negative1, true);
    pin!("obs_quant_fraction", 3, r.fraction, true);
    pin!("obs_quant_tail", 1, r.quant_tail, true);
    bits(&mut p, "obs_attribute_base", 16, r.attribute_base);
    bits(&mut p, "obs_difference0", 17, r.difference0);
    pin!("obs_pb_read_address", 4, s.b_address, true);
    pin!("obs_pc_read_address", 4, s.c_address, true);
    p
}
pub fn production(p: &[Pin]) -> String {
    let mut source = String::from(
        "`define PORTABLE\n`define GPU_V2_LIVE_GLOBALS\n`define GPU_V2_TRUSTED_FRONTEND\n",
    );
    for part in [
        include_str!("../../src/frontend/vliw/prepare_program.v"),
        include_str!("../../src/frontend/vliw/prepare_core.v"),
        include_str!("../../src/frontend/vliw/prepare.v"),
        include_str!("../../src/frontend/vliw/primitives/ram.v"),
        include_str!("../../src/frontend/vliw/primitives/dsp.v"),
    ] {
        source.push_str(part);
        source.push('\n');
    }
    source.push_str("module g2a(input clk");
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
    source.push_str(");wire[89:0]pw_q;wire[53:0]pw_high_wd;assign pc_di=pw_high_wd[47:0];\n");
    source.push_str("reg[3:0]pb_read_address,pc_read_address;always @(posedge clk)begin if(reset)begin pb_read_address<=0;pc_read_address<=0;end else if(ce)begin if(replay_read)pb_read_address<=replay_address;else if(pw_re)pb_read_address<=pw_ra;if(pw_acc_re)pc_read_address<=pw_acc_ra;end end\n");
    source.push_str("iz_params_async #(.WIDTH(36)) pb(clk,reset,ce,pw_low0_we||pw_low1_we,pw_wa,pw_low_wd,pb_read_address,private_b);\niz_params_async #(.WIDTH(48),.PC_CONSTANTS(1)) pc(clk,reset,ce,pw_high_we,pw_high_wa,pw_high_wd[47:0],pc_read_address,private_c);\nassign pw_q={{6{private_c[47]}},private_c,private_b};\n");
    source.push_str("iz_operand_table pa(clk,reset,ce,operand_we,operand_wa,operand_wd[16:0],operand_re,operand_ra,operand_value);\niz_widemac dsp(clk,reset,ce,dsp_a,dsp_asign,dsp_b,dsp_c,dsp_result);\n");
    source.push_str("rp_prepare dut(.clk(clk),.wide_result_data(dsp_result),.pw_q(pw_q),.pw_high_wd(pw_high_wd)");
    for f in p {
        if !f.name.starts_with("obs_")
            && !matches!(
                f.name.as_str(),
                "private_b"
                    | "private_c"
                    | "pc_di"
                    | "dsp_result"
                    | "dsp_ar"
                    | "dsp_br"
                    | "dsp_sr"
                    | "replay_read"
                    | "replay_address"
            )
        {
            write!(source, ",.{}({})", f.name, f.name).unwrap();
        }
    }
    source.push_str(");\nassign dsp_ar=dsp.ar;assign dsp_br=dsp.br;assign dsp_sr=dsp.sr;\n");
    for f in p.iter().filter(|f| f.name.starts_with("obs_")) {
        let net = f.name.strip_prefix("obs_").unwrap();
        let value = match net {
            "pb_read_address" => "pb_read_address".to_owned(),
            "pc_read_address" => "pc_read_address".to_owned(),
            _ => format!("dut.{net}"),
        };
        writeln!(source, "assign {}={value};", f.name).unwrap();
    }
    source.push_str("endmodule\n");
    source
}
