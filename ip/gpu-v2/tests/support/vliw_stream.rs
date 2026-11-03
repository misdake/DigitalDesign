use crate::{compute_fixture, vertex_support::rtl::Pin, Edge, StreamEdge};
use std::fmt::Write as _;
pub fn fields(e: &StreamEdge) -> Vec<Pin> {
    let i = e.i;
    let c = i.control;
    let o = e.o.context;
    let p = o.prepare;
    let co = e.o.compute;
    let s = &e.p.context.state;
    let mut pins = Vec::new();
    macro_rules! pin {
        ($name:expr,$width:expr,$v:expr,$out:expr) => {
            pins.push(Pin {
                name: $name.into(),
                width: $width,
                value: ($v) as u128,
                known: (1u128 << $width) - 1,
                output: $out,
            });
        };
    }
    for (n, v) in [
        ("reset", c.reset),
        ("ce", c.ce),
        ("prepare_start", c.prepare_start),
        ("execution_idle", c.execution_idle),
        ("raw_valid", c.raw_valid),
        ("quad_valid", c.quad_valid),
        ("sample_valid", c.sample_valid),
        ("sample_cover", i.sample_cover),
        ("sample_anchor", i.sample_anchor),
        ("quad_end_valid", i.quad_end_valid),
        ("pool_ready", c.pool_ready),
    ] {
        pin!(n, 1, v, false);
    }
    pin!("descriptor", 32, c.descriptor, false);
    pin!("raw_index", 5, c.raw_index, false);
    pin!("raw_data", 54, c.raw_data.value, false);
    pin!("cfg_depth_shift", 2, c.params.depth_shift, false);
    pin!("cfg_far_z", 32, c.params.far_z, false);
    pin!("cfg_tex_u", 5, c.params.tex_u, false);
    pin!("cfg_tex_v", 5, c.params.tex_v, false);
    pin!("cfg_max_mip", 5, c.params.max_mip, false);
    pin!("cfg_bias", 8, c.params.bias as u8, false);
    pin!("pool_quad_id", 3, c.pool_quad_id, false);
    pin!("sample_lane", 2, c.sample_lane, false);
    pin!("sample_h", 30, i.sample_h, false);
    pin!("quad_mask", 4, i.quad_mask, false);
    for (n, v) in [
        ("bound_valid", o.bound_valid),
        ("bound_empty", o.bound_empty),
        ("raster_start", o.raster_start),
        ("exec_idle", o.exec_idle),
        ("raw_ready", p.raw_ready),
        ("geo_we", o.geo_write),
        ("prepare_import_busy", p.import_busy),
        ("quad_ready", o.quad_ready),
        ("pool_rgb", o.pool_rgb),
        ("run_enable", o.run_enable),
        ("busy", o.busy),
        ("drained", !o.busy),
        ("s_busy", p.busy),
        ("s_done", p.done),
        ("result_we", co.result_we),
        ("depth_event_valid", co.depth_valid),
        ("depth_event_pass", co.depth_pass),
        ("lod_valid", co.lod_valid),
    ] {
        pin!(n, 1, v, true);
    }
    pin!("bound_xlo", 5, p.xlo, true);
    pin!("bound_xhi", 5, p.xhi, true);
    pin!("bound_ylo", 4, p.ylo, true);
    pin!("bound_yhi", 4, p.yhi, true);
    pin!("pool_color", 32, o.pool_color, true);
    pin!("pool_end_id", 3, o.pool_end_id, true);
    pin!("geo_wa", 4, o.geo_address, true);
    pins.push(Pin {
        name: "geo_wd".into(),
        width: 30,
        value: o.geo_data.value.into(),
        known: o.geo_data.known.into(),
        output: true,
    });
    pin!("phase", 2, co.phase, true);
    pin!("prepare_error", 4, 0, true);
    pin!("error", 17, 0, true);
    pin!("result_wa", 7, co.result_address, true);
    pins.push(Pin {
        name: "result_wd".into(),
        width: 36,
        value: co.result_data.value.into(),
        known: co.result_data.known.into(),
        output: true,
    });
    pin!("depth_event_pid", 5, co.depth_pid, true);
    pin!("depth_event_z16", 16, co.depth_z16, true);
    pin!("lod_id", 3, co.lod_id, true);
    pin!("lod_value", 8, co.lod_value, true);
    for (n, width, v) in [
        ("rgb", 1, u64::from(s.rgb)),
        ("active_bank", 1, s.active_bank.into()),
        ("raster_pending", 1, s.raster_pending.into()),
        ("current_id", 3, s.current_id.into()),
        ("replay", 1, s.replay.into()),
        ("replay_slot", 4, s.replay_slot.into()),
        ("prepared_color", 32, s.prepared_color.into()),
        ("p_busy", 1, co.busy.into()),
    ] {
        pin!(format!("obs_{n}"), width, v, true);
    }
    let edge = Edge {
        i: e.o.compute_input,
        o: co,
        p: e.p.compute.clone(),
    };
    for mut f in compute_fixture::fields(&edge) {
        // These are real child input wires, rather than driven tape peers.
        if !f.output {
            let bits = match f.name.as_str() {
                "coefficient_bwd" => Some(edge.i.coefficient_bwd),
                "coefficient_cwd" => Some(edge.i.coefficient_cwd),
                "param_data" => Some(edge.i.param_data),
                _ => None,
            };
            if let Some(b) = bits {
                f.value = b.value.into();
                f.known = b.known.into();
            }
        }
        f.name = format!("compute_{}", f.name);
        f.output = true;
        pins.push(f);
    }
    pins
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
    source.push_str(");gpu_v2_vliw_interpolator dut(.clk(clk),.quad_x(8'd98),.quad_y(7'd58)");
    for f in p
        .iter()
        .filter(|f| !f.name.starts_with("obs_") && !f.name.starts_with("compute_"))
    {
        write!(source, ",.{}({})", f.name, f.name).unwrap();
    }
    source.push_str(");\n");
    for f in p
        .iter()
        .filter(|f| f.name.starts_with("obs_") || f.name.starts_with("compute_"))
    {
        let path = if let Some(n) = f.name.strip_prefix("obs_") {
            n.to_owned()
        } else {
            let n = f.name.strip_prefix("compute_").unwrap();
            let suffix = if let Some(n) = n.strip_prefix("obs_") {
                n.to_owned()
            } else if n.starts_with("lod_") && !matches!(n, "lod_valid" | "lod_id" | "lod_value") {
                format!("lod.{}", n.strip_prefix("lod_").unwrap())
            } else {
                n.to_owned()
            };
            format!("compute.{suffix}")
        };
        writeln!(source, "assign {}=dut.{path};", f.name).unwrap();
    }
    source.push_str("endmodule\n");
    source
}
