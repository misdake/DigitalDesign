use crate::{actual, vertex_support::rtl::Pin, Edge};
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let (i, o) = (e.i, e.o);
    let mut p = Vec::new();
    macro_rules! pin {
        ($n:expr, $w:expr, $v:expr, $out:expr) => {
            p.push(Pin {
                name: $n.into(),
                width: $w,
                value: ($v) as u128 & ((1u128 << $w) - 1),
                known: (1u128 << $w) - 1,
                output: $out,
            });
        };
    }
    for (n, v) in [
        ("reset", i.reset),
        ("ce", i.ce),
        ("compute_tick", i.compute_tick),
        ("source_valid", i.source_valid),
        ("out_ready", i.out_ready),
    ] {
        pin!(n, 1, v, false);
    }
    for (n, w, v) in [
        ("source_lane", 2, u64::from(i.source_lane)),
        ("source_slot", 5, u64::from(i.source_slot)),
        ("source_mask", 4, u64::from(i.source_mask)),
        ("source_normal", 36, i.normal.value),
        ("source_position", 32, i.position.value),
        ("context_epoch", 16, u64::from(i.globals.epoch)),
        ("context_mode", 2, u64::from(i.globals.mode)),
        ("context_code", 5, u64::from(i.globals.code)),
        ("ambient", 9, u64::from(i.globals.ambient)),
        ("directional", 9, u64::from(i.globals.directional)),
    ] {
        pin!(n, w, v, false);
    }
    for (n, v) in ["light_x", "light_y", "light_z", "ray_x", "ray_y", "ray_k"]
        .into_iter()
        .zip(i.globals.light.into_iter().chain(i.globals.ray))
    {
        pin!(n, 16, v as u16, false);
    }
    for (n, v) in [
        ("source_ready", o.source_ready),
        ("pixel_accepted", o.pixel_accepted),
        ("in_ready", o.in_ready),
        ("out_valid", o.result.valid),
    ] {
        pin!(n, 1, v, true);
    }
    pin!("out_epoch", 16, o.epoch, true);
    pin!("phase", 2, o.phase, true);
    pin!("valid_pipe", 35, o.valid_pipe, true);
    p.push(Pin {
        name: "out_id".into(),
        width: 32,
        value: o.id.value.into(),
        known: o.id.known.into(),
        output: true,
    });
    for (n, v) in [("out_g", o.result.g), ("out_h", o.result.h)] {
        p.push(Pin {
            name: n.into(),
            width: 9,
            value: if o.result.valid { v.value.into() } else { 0 },
            known: if o.result.valid { v.known.into() } else { 0 },
            output: true,
        });
    }
    p
}
pub fn production(p: &[Pin], native: bool) -> String {
    let mut s = String::new();
    if native {
        s.push_str("`define GPU_V2_GOWIN_DSP\n");
        let gowin = std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME"));
        s.push_str(&std::fs::read_to_string(gowin.join("IDE/simlib/gw2a/prim_sim.v")).unwrap());
    }
    s.push_str(actual::complete());
    s.push_str("\nmodule g2a(input wire clk");
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
    s.push_str(");\nwire[8:0]raw_g,raw_h;wire[6:0]out_key;wire out_quad_last;\ngpu_v2_pixel_lighting_consumer dut(.clk(clk),.out_g(raw_g),.out_h(raw_h),.out_key(out_key),.out_quad_last(out_quad_last)");
    for f in p.iter().filter(|f| {
        !matches!(
            f.name.as_str(),
            "out_id" | "out_g" | "out_h" | "in_ready" | "phase" | "valid_pipe"
        )
    }) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\nassign in_ready=dut.core_in_ready;assign out_id=dut.core_out_id;assign phase=dut.core.phase;assign valid_pipe=dut.core.valid_pipe;\n// Project only valid numerical payloads; invalid free-running DSP values are outside this observation.\nassign out_g=out_valid?raw_g:9'bx;assign out_h=out_valid?raw_h:9'bx;\n");
    if native {
        s.push_str("GSR GSR(.GSRI(1'b1));\n");
    }
    s.push_str("endmodule\n");
    s
}
