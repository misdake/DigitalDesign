use crate::{actual, vertex_support::rtl::Pin, Edge};
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let mut p = Vec::new();
    let (i, o, s) = (e.i, e.o, &e.s);
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
        ("proposal_ce", i.proposal_ce),
        ("commit_allow", i.commit_allow),
        ("publish_valid", i.publish_valid),
        ("in_valid", i.valid),
        ("out_ready", i.out_ready),
    ] {
        pin!(n, 1, v, false);
    }
    for (n, w, v) in [
        ("check_quad", 4, i.check_quad),
        ("publish_quad", 4, i.publish_quad),
        ("publish_mask", 4, i.publish_mask),
    ] {
        pin!(n, w, v, false);
    }
    pin!("in_payload", 72, i.payload.value, false);
    let tex = i
        .texels
        .iter()
        .enumerate()
        .fold(0, |v, (n, t)| v | (u64::from(t.value as u16) << (16 * n)));
    pin!("in_texels", 64, tex, false);
    for (n, v) in [
        ("public_free", o.public_free),
        ("publish_ready", o.publish_ready),
        ("publish_fire", o.publish_fire),
        ("in_ready", o.ready),
        ("input_fire", o.input_fire),
        ("out_valid", o.valid),
        ("quad_last", o.last),
        ("result_fire", o.result_fire),
        ("advance", o.advance),
        ("fault", false),
        ("preflight_fault", false),
        ("eligible", i.proposal_ce && !i.reset),
        ("obs_busy", s.busy),
        ("obs_done", s.done),
        ("obs_q_done", s.q_done),
        ("obs_start", o.started),
        ("obs_accepted", o.accepted),
        ("obs_mul_ce", o.mul_ce),
        ("obs_accum_ce", o.accum_ce),
        ("obs_accum_add", o.accum_add),
        ("obs_core_valid", o.core_valid),
        ("obs_packet_bad", o.packet_bad),
    ] {
        pin!(n, 1, v, true);
    }
    pin!("public_live", 16, o.live, true);
    for (n, w, v) in [
        ("out_key", 6, o.key),
        ("out_rgb", 24, o.rgb),
        ("obs_p_key", 6, s.p_key),
        ("obs_p_last", 1, s.p_last),
        ("obs_q_key", 6, s.q_key),
        ("obs_q_last", 1, s.q_last),
        ("obs_accumulator", 54, s.accumulator),
        ("obs_half", 54, o.packed_half),
    ] {
        p.push(Pin {
            name: n.into(),
            width: w,
            value: v.value.into(),
            known: v.known.into(),
            output: true,
        });
    }
    for c in 0..3 {
        for t in 0..2 {
            let v = s.products[c][t];
            p.push(Pin {
                name: format!("obs_product{c}_{t}"),
                width: 17,
                value: v.value.into(),
                known: v.known.into(),
                output: true,
            });
        }
        let v = o.pair_sum[c];
        p.push(Pin {
            name: format!("obs_pair{c}"),
            width: 8,
            value: v.value.into(),
            known: v.known.into(),
            output: true,
        });
    }
    for (q, v) in s.last_lane.iter().enumerate() {
        p.push(Pin {
            name: format!("obs_last{q}"),
            width: 2,
            value: v.value.into(),
            known: v.known.into(),
            output: true,
        });
    }
    p
}
pub fn production(p: &[Pin], native: bool) -> String {
    let mut s = String::from("`define GPU_V2_VLIW_COLOR_COMMAND\n");
    if native {
        s.push_str("`define GPU_V2_GOWIN_DSP\n");
        let gowin = std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap());
        s.push_str(&std::fs::read_to_string(gowin.join("IDE/simlib/gw2a/prim_sim.v")).unwrap());
    }
    for name in [
        "gpu_v2_texture_runtime_color",
        "gpu_v2_texture_runtime_color_checked",
        "color_serial_registered_mul9",
        "color_packed_accumulator",
    ] {
        s.push_str(actual::module(name));
        s.push('\n');
    }
    s.push_str("module g2a(input wire clk");
    for f in p {
        write!(
            s,
            ",{}wire[{}:0]{}",
            if f.output { "output " } else { "input " },
            f.width - 1,
            f.name
        )
        .unwrap();
    }
    s.push_str(");\ngpu_v2_texture_runtime_color dut(.clk(clk)");
    for f in p.iter().filter(|f| !f.name.starts_with("obs_")) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for (external, internal) in [
        ("busy", "busy"),
        ("done", "done"),
        ("q_done", "q_done"),
        ("start", "start"),
        ("accepted", "accepted"),
        ("mul_ce", "mul_ce"),
        ("accum_ce", "accum_ce"),
        ("accum_add", "accum_add"),
        ("core_valid", "out_valid"),
        ("packet_bad", "packet_bad"),
        ("p_key", "p_key"),
        ("p_last", "p_last"),
        ("q_key", "q_key"),
        ("q_last", "q_last"),
        ("accumulator", "packed_sum"),
        ("half", "packed_half"),
    ] {
        writeln!(s, "assign obs_{external}=dut.color.{internal};").unwrap();
    }
    for c in 0..3 {
        for t in 0..2 {
            writeln!(
                s,
                "assign obs_product{c}_{t}=dut.color.channels[{c}].product[{t}];"
            )
            .unwrap();
        }
        writeln!(s, "assign obs_pair{c}=dut.color.pair_sum[{c}];").unwrap();
    }
    for q in 0..16 {
        writeln!(s, "assign obs_last{q}=dut.last_lane[{q}];").unwrap();
    }
    if native {
        s.push_str("GSR GSR(.GSRI(1'b1));\n");
    }
    s.push_str("endmodule\n");
    s
}
