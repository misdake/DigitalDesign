use crate::{actual, vertex_support::rtl::Pin, Edge};
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let mut p = Vec::new();
    let (i, o) = (e.i, e.o);
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
        ("commit_allow", i.commit_allow),
        ("in_valid", i.valid),
        ("in_has_mip", i.mip),
        ("texture_fb400240", i.framebuffer),
        ("texture_clamp", i.clamp),
        ("out_ready", i.out_ready),
    ] {
        pin!(n, 1, v, false);
    }
    for (n, w, v) in [
        ("in_uv", 32, u64::from(i.uv)),
        ("in_quad", 4, u64::from(i.quad)),
        ("in_lane", 2, u64::from(i.lane)),
        ("in_mask", 4, u64::from(i.mask)),
        ("in_lod", 8, u64::from(i.lod)),
        ("in_max_n", 4, u64::from(i.max_n)),
        ("texture_min_n", 2, u64::from(i.min_n)),
        ("in_filter", 2, u64::from(i.filter)),
    ] {
        pin!(n, w, v, false);
    }
    for (n, v) in [
        ("in_ready", o.ready),
        ("advance", o.advance),
        ("out_valid", o.valid),
        ("issue", o.issue),
        ("returned", o.returned),
        ("preflight_bad", false),
        ("fault", false),
    ] {
        pin!(n, 1, v, true);
    }
    for (n, w, v) in [
        ("phase", 2, o.phase),
        ("occupied", 5, o.occupied),
        ("completed", 5, o.completed),
        ("write_row", 4, o.write_row),
        ("return_row", 4, o.return_row),
        ("read_row", 4, o.read_row),
        ("coordinate_valid", 3, o.coordinate_valid),
    ] {
        pin!(n, w, v, true);
    }
    for (n, w, v) in [
        ("out_owner", 38, o.owner),
        ("out_result", 36, o.result),
        ("coordinate_result", 36, o.coordinate_result),
    ] {
        p.push(Pin {
            name: n.into(),
            width: w,
            value: v.value.into(),
            known: v.known.into(),
            output: true,
        });
    }
    p
}
pub fn production(p: &[Pin], native: bool) -> String {
    let mut s = String::new();
    let native_source;
    let source = if native {
        let gowin = std::path::PathBuf::from(std::env::var_os("GOWIN_HOME").unwrap());
        s.push_str(&std::fs::read_to_string(gowin.join("IDE/simlib/gw2a/prim_sim.v")).unwrap());
        native_source = gpu_v2::system::vliw_command::verilog(
            gpu_v2::framebuffer::ports::MaterializedSurface {
                color_base_bytes: 0x100000,
                depth_base_bytes: 0x140000,
                width: 400,
                height: 240,
            },
            gpu_v2::system::pixel::quad_ingress::rtl::Backend::Native,
        )
        .unwrap();
        native_source.as_str()
    } else {
        actual::complete()
    };
    for name in [
        "gpu_v2_texture_direct_lod",
        "gpu_v2_texture_serial_coord",
        "gpu_v2_direct_coordinate_store",
    ] {
        let marker = format!("module {name}");
        let start = source.find(&marker).unwrap();
        let end = start + source[start..].find("endmodule").unwrap() + 9;
        s.push_str(&source[start..end]);
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
    s.push_str(");\ngpu_v2_texture_direct_lod dut(.clk(clk)");
    for f in p.iter().filter(|f| {
        !matches!(
            f.name.as_str(),
            "write_row" | "return_row" | "read_row" | "coordinate_valid" | "coordinate_result"
        )
    }) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\nassign write_row=dut.queue.write_row;assign return_row=dut.queue.return_row;assign read_row=dut.queue.read_row;assign coordinate_valid=dut.coord.valid;assign coordinate_result=dut.coord.result_bank;\n");
    if native {
        s.push_str("GSR GSR(.GSRI(1'b1));\n");
    }
    s.push_str("endmodule\n");
    s
}
