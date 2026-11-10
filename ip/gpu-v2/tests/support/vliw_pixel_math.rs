use crate::{vertex_support::rtl::Pin, Edge};
use gpu_v2::system::vliw_cycle::vertex::Bits;
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let mut pins = Vec::new();
    let (i, o, r, q, s) = (&e.fi, &e.fo, &e.ri, &e.ro, &e.final_state);
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
    fn bits(p: &mut Vec<Pin>, n: &str, w: u32, v: Bits, out: bool) {
        let m = (1u64 << w) - 1;
        p.push(Pin {
            name: n.into(),
            width: w,
            value: (v.value & m).into(),
            known: (v.known & m).into(),
            output: out,
        });
    }
    for (n, v) in [
        ("reset", i.reset),
        ("f_ce", i.ce),
        ("f_proposal_ce", i.proposal_ce),
        ("f_commit_allow", i.commit_allow),
        ("f_abort", i.abort),
        ("shade_phase", i.shade_phase),
        ("f_in_valid", i.in_valid),
        ("f_out_ready", i.out_ready),
        ("r_ce", r.ce),
        ("r_in_valid", r.valid),
        ("r_texture_capture", r.texture_capture),
        ("r_blend", r.blend),
        ("r_depth_write", r.depth_write),
    ] {
        pin!(n, 1, v, false);
    }
    pin!("r_depth_func", 3, r.depth_func, false);
    for (n, w, v) in [
        ("f_in_tint", 24, i.tint),
        ("f_in_tex", 24, i.texture),
        ("f_in_tex_alpha", 8, i.texture_alpha),
        ("f_in_spec", 24, i.specular),
        ("f_in_g", 9, i.g),
        ("f_in_h", 9, i.h),
        ("r_covered", 1, r.covered),
        ("r_old_color", 16, r.old_color),
        ("r_old_depth", 16, r.old_depth),
        ("r_src_rgb", 24, r.rgb),
        ("r_src_a", 8, r.alpha),
        ("r_src_texture_alpha", 8, r.texture_alpha),
        ("r_src_depth", 16, r.depth),
    ] {
        assert!(v.is_known(w));
        bits(&mut pins, n, w, v, false);
    }
    for (n, v) in [
        ("f_fault", o.fault),
        ("f_proposal_ready", o.proposal_ready),
        ("f_in_ready", o.in_ready),
        ("f_advance", o.advance),
        ("f_in_accept", o.in_accept),
        ("f_out_valid", o.out_valid),
        ("f_out_fire", o.out_fire),
        ("f_shared_spec_ce", o.shared_spec_ce),
        ("r_in_ready", q.ready),
        ("r_out_valid", q.valid),
        ("r_shared_mul_pending", q.shared_mul_pending),
        ("r_shared_rop_ce", q.shared_rop_ce),
        ("obs_arithmetic_live", s.arithmetic_live),
        ("obs_q_head", s.q_head),
        ("obs_q_tail", s.q_tail),
        ("obs_check_reservation", s.expected_reservation.is_some()),
    ] {
        pin!(n, 1, v, true);
    }
    pin!("f_credits", 4, o.credits, true);
    pin!("f_queued", 4, o.queued, true);
    pin!("obs_q_count", 2, s.q_count, true);
    pin!(
        "obs_expected_reservation",
        3,
        s.expected_reservation.unwrap_or(0),
        true
    );
    for (n, w, v) in [
        ("f_out_rgb", 24, o.rgb),
        ("f_out_tex_alpha", 8, o.texture_alpha),
        ("f_shared_spec_a", 9, o.shared_spec_a),
        ("f_shared_spec_b", 24, o.shared_spec_b),
        ("r_new_color", 16, q.color),
        ("r_new_depth", 16, q.depth),
        ("r_color_written", 1, q.color_written),
        ("r_depth_written", 1, q.depth_written),
        ("r_shared_rop_a", 27, q.shared_rop_a),
        ("r_shared_rop_b", 8, q.shared_rop_b),
        ("shared_rop_product", 54, e.shared.rop_product()),
        ("shared_spec_product", 51, e.shared.spec_product()),
        ("obs_result0", 24, s.results[0]),
        ("obs_result1", 24, s.results[1]),
    ] {
        bits(&mut pins, n, w, v, true);
    }
    for c in 0..3 {
        bits(&mut pins, &format!("obs_p{c}"), 17, s.products[c], true);
        bits(&mut pins, &format!("obs_sh{c}"), 8, s.spec_terms[c], true);
    }
    pins
}
fn production_module<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("module {name}");
    assert_eq!(source.matches(&marker).count(), 1, "{name}");
    let start = source.find(&marker).unwrap();
    &source[start..start + source[start..].find("endmodule").unwrap() + 9]
}
pub fn production(p: &[Pin]) -> String {
    static ACTUAL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let actual = ACTUAL.get_or_init(|| {
        gpu_v2::system::vliw_command::verilog(
            gpu_v2::framebuffer::ports::MaterializedSurface {
                color_base_bytes: 0x100000,
                depth_base_bytes: 0x140000,
                width: 400,
                height: 240,
            },
            gpu_v2::system::pixel::quad_ingress::rtl::Backend::Portable,
        )
        .unwrap()
    });
    let mut source = String::from("`define GPU_V2_COMMAND_PORTS\n");
    for name in [
        "gpu_v2_final_ordered_two_cycle",
        "gpu_v2_final_two_cycle_mul9",
        "gpu_v2_rop_tdp",
        "gpu_v2_final_rop_bank",
    ] {
        source.push_str(production_module(actual, name));
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
    source.push_str(");\ngpu_v2_final_ordered_two_cycle f(.clk(clk),.reset(reset),.shade_phase(shade_phase),.shared_rop_offer(r_shared_mul_pending&&f_ce),.shared_spec_product(shared_spec_product)");
    for f in p.iter().filter(|f| f.name.starts_with("f_")) {
        write!(source, ",.{}({})", &f.name[2..], f.name).unwrap();
    }
    source.push_str(");\ngpu_v2_rop_tdp r(.clk(clk),.reset(reset),.shade_phase(shade_phase),.shared_rop_product(shared_rop_product),.src_r(r_src_rgb[7:0]),.src_g(r_src_rgb[15:8]),.src_b(r_src_rgb[23:16])");
    for f in p
        .iter()
        .filter(|f| f.name.starts_with("r_") && f.name != "r_src_rgb")
    {
        write!(source, ",.{}({})", &f.name[2..], f.name).unwrap();
    }
    source.push_str(");\ngpu_v2_final_rop_bank bank(.clk(clk),.reset(reset),.rop_ce(r_shared_rop_ce),.rop_a(r_shared_rop_a),.rop_b(r_shared_rop_b),.rop_product(shared_rop_product),.spec_ce(f_shared_spec_ce),.spec_a(f_shared_spec_a),.spec_b(f_shared_spec_b),.spec_product(shared_spec_product));\n");
    for n in [
        "arithmetic_live",
        "q_head",
        "q_tail",
        "q_count",
        "check_reservation",
        "expected_reservation",
        "result0",
        "result1",
    ] {
        writeln!(source, "assign obs_{n}=f.{n};").unwrap();
    }
    for c in 0..3 {
        writeln!(
            source,
            "assign obs_p{c}=f.channels[{c}].product;assign obs_sh{c}=f.channels[{c}].sh;"
        )
        .unwrap();
    }
    source.push_str("endmodule\n");
    source
}
