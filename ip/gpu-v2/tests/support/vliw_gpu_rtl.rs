#[allow(dead_code)]
#[path = "vliw_pixel_source.rs"]
mod source;
use crate::{vertex_support::rtl::Pin, Edge};
use gpu_v2::system::vliw_cycle::vertex::Bits;
use std::fmt::Write as _;
pub fn fields(e: &Edge) -> Vec<Pin> {
    let i = e.i;
    let o = e.o;
    let co = o.control;
    let ro = o.render;
    let bo = ro.backend;
    let c = bo.sink.cache;
    let p = ro.producer.pool;
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
        ("abort", i.abort),
        ("device_read_enable", i.bus.read),
        ("device_write_enable", i.bus.write),
    ] {
        pin!(n, 1, v, false);
    }
    pin!("device_index", 3, i.bus.index, false);
    pin!("device_channel", 4, i.bus.channel, false);
    pin!("device_write_data", 16, i.bus.data, false);
    for (n, physical) in ["ro", "fb_r", "fb_w"].into_iter().zip(i.physical) {
        pin!(
            format!("{n}_request_ready"),
            1,
            physical.request_ready,
            false
        );
        pin!(
            format!("{n}_response_valid"),
            1,
            physical.response.valid,
            false
        );
        pin!(
            format!("{n}_response_last"),
            1,
            physical.response.last,
            false
        );
        pin!(
            format!("{n}_response_error"),
            1,
            physical.response.error,
            false
        );
        if n != "fb_w" {
            pin!(format!("{n}_data"), 64, physical.response.data, false);
        }
    }
    pin!("fb_w_data_ready", 1, i.physical[2].write_ready, false);
    let fault =
        co.processor.stop || bo.fault || co.transport.router.fault || co.transport.dma.fault;
    let idle = ro.idle && !co.transport.router.busy && co.transport.dma.idle;
    for (n, v) in [
        ("busy", co.processor.busy),
        ("pipeline_idle", idle),
        ("idle", idle && !co.processor.busy),
        ("fault", fault),
        ("frontend_idle", ro.producer.frontend.idle),
        ("meshlet_done", ro.producer.frontend.meshlet.done),
        ("meshlet_ready", ro.producer.frontend.meshlet.ready),
        (
            "quad_admitted",
            i.ce && !co.processor.stop && ro.producer.frontend.quad_valid && p.quad_ready,
        ),
        ("quad_empty_skipped", bo.feedback.empty_skip),
        ("quad_published", p.published),
        ("light_result", bo.light_result),
        ("sample_result", bo.sample_result),
        ("final_issue", bo.final_issue),
        ("final_result", bo.final_result),
        ("quad_accepted", bo.sink.quad_accepted),
        ("commit_valid", c.commit_valid),
    ] {
        pin!(n, 1, v, true);
    }
    pin!("device_read_data", 16, co.device_read_data, true);
    pin!("global_used", 6, p.global_used, true);
    pin!("producer_used", 6, p.producer_used, true);
    pin!(
        "work_reserved",
        5,
        ro.sampling.map_or(0, |s| s.reserved),
        true
    );
    pin!(
        "work_count",
        5,
        ro.sampling.map_or(0, |s| s.packets.work.materialized),
        true
    );
    for (n, w, v) in [
        ("commit_x", 9, c.commit_x),
        ("commit_y", 8, c.commit_y),
        ("commit_mask", 4, c.commit_mask),
    ] {
        bits(&mut pins, n, w, v);
    }
    for (n, r) in ["ro", "fb_r", "fb_w"]
        .into_iter()
        .zip(co.transport.router.physical)
    {
        pin!(format!("{n}_request_valid"), 1, r.valid, true);
        pin!(format!("{n}_address"), 22, r.address_halfword, true);
        pin!(format!("{n}_count"), 2, r.count, true);
    }
    pin!("fb_w_data", 64, co.transport.router.write_data, true);
    for (n, v) in [
        ("start", co.processor.start),
        ("slot", co.processor.draw_slot),
        ("stop", co.processor.stop),
        ("cp_re", co.processor.ram_read),
        ("cache_valid", co.processor.cache.valid),
        ("flush_request", co.processor.flush_request),
        ("flush_complete", c.flush_complete),
        ("global_valid", e.installed.0),
        ("render_valid", e.installed.1),
        ("target_valid", e.installed.2),
    ] {
        pin!(format!("obs_{n}"), 1, v, true);
    }
    pin!("obs_cp_addr", 9, co.processor.ram_address, true);
    pin!("obs_state", 5, e.phase, true);
    pin!("obs_pc", 6, e.pc, true);
    pin!("obs_cache_pages", 2, e.pages, true);
    pin!("obs_page_valid", 2, e.installed.3, true);
    pin!("obs_uniform_valid", 4, e.installed.4, true);
    pin!("obs_page_uniform", 4, e.installed.5, true);
    pin!("obs_object_index", 2, e.installed.6, true);
    pins
}
pub fn production(p: &[Pin]) -> String {
    let mut s = source::complete().to_owned();
    let library = std::path::PathBuf::from(
        std::env::var_os("GOWIN_HOME").expect("GOWIN_HOME for fixed native Group35"),
    )
    .join("IDE/simlib/gw2a/prim_sim.v");
    s.push('\n');
    s.push_str(&std::fs::read_to_string(library).unwrap());
    s.push_str("\nmodule g2a(input clk");
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
    s.push_str(");\ngpu_v2_vliw_command dut(.clk(clk),.output_ready(1'b1)");
    for f in p.iter().filter(|f| !f.name.starts_with("obs_")) {
        write!(s, ",.{}({})", f.name, f.name).unwrap();
    }
    s.push_str(");\n");
    for f in p.iter().filter(|f| f.name.starts_with("obs_")) {
        let n = &f.name[4..];
        let path = match n {
            "start" | "slot" | "stop" | "cp_re" | "cp_addr" | "cache_valid" | "flush_request"
            | "flush_complete" => n.into(),
            other => format!("processor.{other}"),
        };
        writeln!(s, "assign {}=dut.{path};", f.name).unwrap();
    }
    s.push_str("endmodule\n");
    s
}
