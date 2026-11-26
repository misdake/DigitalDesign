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
        ("in_valid", i.valid),
        ("texture_clamp", i.clamp),
        ("out_ready", i.out_ready),
    ] {
        pin!(n, 1, v, false);
    }
    pin!("texture_min_n", 2, i.min_n, false);
    pin!("in_owner", 38, i.owner.value, false);
    pin!("in_result", 36, i.result.value, false);
    pin!("work_available", 8, i.available, false);
    for (n, v) in [
        ("in_ready", o.ready),
        ("proposal_ready", o.proposal_ready),
        ("in_accept", o.accepted),
        ("out_valid", o.valid),
        ("fault", false),
        ("preflight_bad", false),
        ("weight_write", o.weight_write),
        ("obs_advance", o.advance),
        ("obs_phase", s.phase),
        ("obs_meta_re", o.metadata_read),
        ("obs_meta_we", o.metadata_write),
        ("obs_rrd", s.rrd),
        ("obs_rwr", s.rwr),
    ] {
        pin!(n, 1, v, true);
    }
    for (n, w, v) in [
        ("work_reserved", 2, o.reserved),
        ("obs_valid", 5, s.valid),
        ("obs_cwr", 4, s.cwr),
        ("obs_mrd", 4, s.mrd),
        ("obs_cohorts", 2, s.cohorts),
        ("obs_queued", 2, s.queued),
        ("obs_meta_ra", 4, o.metadata_address),
    ] {
        pin!(n, w, v, true);
    }
    for (n, w, v) in [
        ("out_fine", 36, o.fine),
        ("out_coarse", 36, o.coarse),
        ("out_meta", 36, o.meta),
        ("weight_address", 7, o.weight_address),
        ("weight_value", 36, o.weight_value),
        ("obs_input", 37, s.input_stage),
        ("obs_horizontal", 17, s.horizontal),
        ("obs_rows", 18, s.row_totals),
        ("obs_fine_mask", 4, s.fine_mask),
        ("obs_key0", 8, s.keys[0]),
        ("obs_key1", 8, s.keys[1]),
        ("obs_ready0", 8, s.ready_masks[0]),
        ("obs_ready1", 8, s.ready_masks[1]),
    ] {
        p.push(Pin {
            name: n.into(),
            width: w,
            value: v.value.into(),
            known: v.known.into(),
            output: true,
        });
    }
    for (c, v) in s.products.iter().enumerate() {
        p.push(Pin {
            name: format!("obs_product{c}"),
            width: 17,
            value: v.value.into(),
            known: v.known.into(),
            output: true,
        });
    }
    p
}
pub fn production(p: &[Pin], native: bool) -> String {
    let mut s = String::new();
    if native {
        s.push_str("`define GPU_V2_GOWIN_DSP\n");
        let gowin = std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap());
        s.push_str(&std::fs::read_to_string(gowin.join("IDE/simlib/gw2a/prim_sim.v")).unwrap());
    }
    for name in [
        "gpu_v2_runtime_coefficient_directweight",
        "gpu_v2_texture_coefficient_mul9",
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
    s.push_str(");\n");
    let parent = actual::module("gpu_v2_texture_runtime_backend_unique");
    let start = parent.find(" wire[8:0] p0=").unwrap();
    let end = parent.find(" wire fine_active=").unwrap();
    s.push_str(&parent[start..end]);
    s.push_str("\nwire[71:0]weights;assign out_fine=weights[35:0];assign out_coarse=weights[71:36];\ngpu_v2_runtime_coefficient_directweight dut(.clk(clk),.in_payload(coefficient_input),.out_weights(weights),.direct_weight_write(weight_write),.direct_weight_address(weight_address),.direct_weight_value(weight_value)");
    for f in p.iter().filter(|f| {
        !f.name.starts_with("obs_")
            && !matches!(
                f.name.as_str(),
                "in_owner"
                    | "in_result"
                    | "out_fine"
                    | "out_coarse"
                    | "weight_write"
                    | "weight_address"
                    | "weight_value"
            )
    }) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for (external, internal) in [
        ("advance", "advance"),
        ("phase", "ph"),
        ("meta_re", "meta_re"),
        ("meta_we", "meta_we"),
        ("rrd", "rrd"),
        ("rwr", "rwr"),
        ("valid", "valid"),
        ("cwr", "cwr"),
        ("mrd", "mrd"),
        ("cohorts", "cohorts"),
        ("queued", "queued"),
        ("meta_ra", "meta_ra"),
        ("input", "input_stage"),
        ("horizontal", "horizontal_delay"),
        ("rows", "row_totals"),
        ("fine_mask", "fine_mask"),
        ("key0", "weight_key0"),
        ("key1", "weight_key1"),
        ("ready0", "ready_ff0"),
        ("ready1", "ready_ff1"),
    ] {
        writeln!(s, "assign obs_{external}=dut.{internal};").unwrap();
    }
    for c in 0..3 {
        writeln!(s, "assign obs_product{c}=dut.product[{c}];").unwrap();
    }
    if native {
        s.push_str("GSR GSR(.GSRI(1'b1));\n");
    }
    s.push_str("endmodule\n");
    s
}
