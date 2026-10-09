//! Actual Icarus per-edge co-simulation of the registered `rop_leaf` RTL against
//! the real Rust register emulator.
//!
//! The testbench drives one input edge at a time (with `ce` pauses and bubbles)
//! and compares the pre-edge `out_valid`/payload with the emulator's live result;
//! it never embeds a precomputed RTL answer. Four consecutive lanes with distinct
//! keys and reused keys exercise the two-bit key pipeline.
//!
//! Both tool invocations run under a wall-clock watchdog that kills and reaps the
//! child, and stdout/stderr go to files rather than pipes so a large diagnostic
//! can never fill a pipe. The generated bench also keeps its own finite HDL
//! watchdog (`#200000; $fatal`).
//!
//! The Icarus test is `#[ignore]` so `cargo test` stays tool-free; run it with
//! `-- --ignored` when `IVERILOG_EXE`/`VVP_EXE` are available.

use gpu_v2::framebuffer::arithmetic::{
    timed, LeafInput, LeafOutput, LeafTick, Pipeline, Pixel, LEAF_LATENCY, RTL_SOURCE,
};
use gpu_v2::framebuffer::ports::{Blend, DepthFunc, Fragment};
use std::fs::File;
use std::process::Stdio;
use std::time::{Duration, Instant};

struct Stim {
    ce: bool,
    input: Option<LeafInput>,
    output: Option<LeafOutput>,
}

fn bit(value: bool) -> u8 {
    u8::from(value)
}

fn depth_funcs() -> [DepthFunc; 8] {
    [
        DepthFunc::Never,
        DepthFunc::Less,
        DepthFunc::Equal,
        DepthFunc::LessEqual,
        DepthFunc::Greater,
        DepthFunc::NotEqual,
        DepthFunc::GreaterEqual,
        DepthFunc::Always,
    ]
}

/// Deterministic lanes with distinct payloads; keys cycle 0,1,2,3 so the four
/// consecutive lane destinations are covered and then reused.
fn pixels() -> Vec<LeafInput> {
    let mut state = 0x1234_5678_9abc_def0u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u32
    };
    let funcs = depth_funcs();
    (0..64)
        .map(|i| LeafInput {
            key: (i % 4) as u8,
            blend: if i % 2 == 0 {
                Blend::Replace
            } else {
                Blend::SrcOver
            },
            depth: funcs[i % 8],
            depth_write: i % 3 != 0,
            covered: i % 5 != 0,
            old: Pixel {
                color: next() as u16,
                depth: next() as u16,
            },
            source: Fragment {
                rgba: [
                    (next() % 256) as u8,
                    (next() % 256) as u8,
                    (next() % 256) as u8,
                    (next() % 256) as u8,
                ],
                depth: next() as u16,
            },
        })
        .collect()
}

/// Drive the emulator exactly as the testbench drives the RTL: one enabled edge
/// per stimulus, with `ce` low pauses, bubbles and a bounded number of edges.
fn stimulus(max_edges: usize) -> Vec<Stim> {
    let inputs = pixels();
    let mut pipe = Pipeline::new(max_edges as u64).unwrap();
    let mut ptr = 0usize;
    let mut edges = Vec::new();
    for wall in 0..max_edges {
        let ce = !(wall % 17 == 13 || wall % 17 == 14 || matches!(wall % 23, 5 | 6));
        let input = (ptr < inputs.len() && ce).then(|| inputs[ptr]);
        let step = pipe.tick(LeafTick { ce, input }).unwrap();
        assert_eq!(
            step.accepted,
            ce && input.is_some(),
            "accepted mismatch on edge {wall}"
        );
        if step.accepted {
            ptr += 1;
        }
        edges.push(Stim {
            ce,
            input,
            output: step.output,
        });
        if ptr == inputs.len() && pipe.idle() {
            break;
        }
    }
    assert_eq!(ptr, inputs.len(), "stimulus did not offer every lane");
    assert!(pipe.idle(), "stimulus did not drain");
    edges
}

fn testbench(edges: &[Stim]) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,blend=0,depth_write=0,covered=0;\n");
    tb.push_str("reg [1:0] in_key=0;\n");
    tb.push_str("reg [2:0] depth_func=0;\n");
    tb.push_str("reg [15:0] old_color=0,old_depth=0,src_depth=0;\n");
    tb.push_str("reg [7:0] src_r=0,src_g=0,src_b=0,src_a=0;\n");
    tb.push_str("wire in_ready,out_valid,color_written,depth_written;\n");
    tb.push_str("wire [1:0] out_key;\n");
    tb.push_str("wire [15:0] new_color,new_depth;\n");
    tb.push_str(
        "rop_leaf dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_key(in_key),\
         .blend(blend),.depth_func(depth_func),.depth_write(depth_write),.covered(covered),\
         .old_color(old_color),.old_depth(old_depth),.src_r(src_r),.src_g(src_g),.src_b(src_b),\
         .src_a(src_a),.src_depth(src_depth),.in_ready(in_ready),.out_valid(out_valid),\
         .out_key(out_key),.new_color(new_color),.new_depth(new_depth),\
         .color_written(color_written),.depth_written(depth_written));\n",
    );
    tb.push_str("initial begin #200000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\n");
    tb.push_str("clk=0;reset=1;ce=0;in_valid=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    for (edge, stim) in edges.iter().enumerate() {
        tb.push_str(&format!(
            "ce={};in_valid={};blend={};depth_write={};covered={};depth_func=3'd{};\n",
            bit(stim.ce),
            bit(stim.input.is_some()),
            stim.input.map_or(0, |i| i.blend as u8),
            bit(stim.input.is_some_and(|i| i.depth_write)),
            bit(stim.input.is_some_and(|i| i.covered)),
            stim.input.map_or(0, |i| i.depth as u8),
        ));
        if let Some(input) = stim.input {
            tb.push_str(&format!(
                "in_key=2'd{};old_color=16'd{};old_depth=16'd{};src_depth=16'd{};\n",
                input.key, input.old.color, input.old.depth, input.source.depth
            ));
            tb.push_str(&format!(
                "src_r=8'd{};src_g=8'd{};src_b=8'd{};src_a=8'd{};\n",
                input.source.rgba[0],
                input.source.rgba[1],
                input.source.rgba[2],
                input.source.rgba[3]
            ));
        }
        tb.push_str("#1;\n");
        tb.push_str(&format!(
            "if (in_ready !== 1'b{}) $fatal(1,\"in_ready edge {edge}\");\n",
            bit(stim.ce)
        ));
        tb.push_str(&format!(
            "if (out_valid !== 1'b{}) $fatal(1,\"out_valid edge {edge}\");\n",
            bit(stim.output.is_some())
        ));
        if let Some(output) = stim.output {
            tb.push_str(&format!(
                "if (out_key !== 2'd{} || new_color !== 16'd{} || new_depth !== 16'd{} || \
                 color_written !== 1'b{} || depth_written !== 1'b{}) \
                 $fatal(1,\"payload edge {edge}: key=%0d color=%0d depth=%0d cw=%0d dw=%0d\",\
                 out_key,new_color,new_depth,color_written,depth_written);\n",
                output.key,
                output.result.pixel.color,
                output.result.pixel.depth,
                bit(output.result.color_written),
                bit(output.result.depth_written),
            ));
        }
        tb.push_str("clk=1;#1;clk=0;#1;\n");
    }
    tb.push_str(&format!(
        "$display(\"PASS edges={}\");$finish;end endmodule\n",
        edges.len()
    ));
    tb
}

#[test]
fn rtl_source_declares_the_registered_calendar() {
    let certificate = timed::audit(RTL_SOURCE).expect("certificate audit");
    assert_eq!(certificate.latency, LEAF_LATENCY);
    assert_eq!(
        certificate.measured.first_return_edge,
        u64::from(LEAF_LATENCY) + 1
    );
    assert!(RTL_SOURCE.contains("module rop_leaf"));
    assert!(RTL_SOURCE.contains("assign in_ready = ce;"));
    assert!(RTL_SOURCE.contains("end else if (ce) begin"));
    for binding in &certificate.bindings {
        assert!(
            RTL_SOURCE.contains(binding.rtl_anchor),
            "missing stage {} anchor {}",
            binding.stage.number(),
            binding.rtl_anchor
        );
    }
    // Inventory evidence; run with --nocapture to capture it.
    println!(
        "ROP leaf stage certificate (latency {} II {})",
        certificate.latency, certificate.initiation_interval
    );
    for binding in &certificate.bindings {
        let bits: u32 = binding.registers.iter().map(|r| r.width).sum();
        println!(
            "  stage {} {:<16} emu {:<3} regs {:>2} bits {:>3}  rtl \"{}\"",
            binding.stage.number(),
            binding.stage.name(),
            binding.emulator_type,
            binding.registers.len(),
            bits,
            binding.rtl_anchor
        );
    }
    let resources = certificate.resources;
    println!(
        "resources: mul8x8={} adders={} compares={} selects={} div255={} expand={} registers={} bits={}",
        resources.logical_multiplies,
        resources.adders,
        resources.compares,
        resources.selects,
        resources.div255_units,
        resources.expand_units,
        resources.pipeline_registers,
        resources.pipeline_bits
    );
    println!(
        "measured: lanes={} first_accept={} first_return={} last_return={} latency={}",
        certificate.measured.lanes,
        certificate.measured.first_accept_edge,
        certificate.measured.first_return_edge,
        certificate.measured.last_return_edge,
        certificate.measured.latency
    );
}

/// Run `command` with a wall-clock watchdog. Output goes to `{name}.out` and
/// `{name}.err` (never pipes), and a timeout kills and reaps the child.
fn bounded(mut command: std::process::Command, dir: &std::path::Path, name: &str, limit: Duration) {
    let stdout = File::create(dir.join(format!("{name}.out"))).unwrap();
    let stderr = File::create(dir.join(format!("{name}.err"))).unwrap();
    let mut child = command
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "{name} failed: {}\n{}",
                std::fs::read_to_string(dir.join(format!("{name}.out"))).unwrap(),
                std::fs::read_to_string(dir.join(format!("{name}.err"))).unwrap()
            );
            return;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{name} wall watchdog");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_matches_emu_every_edge() {
    let edges = stimulus(600);
    assert!(edges.len() >= 64 + usize::from(LEAF_LATENCY));
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/opencode/rop-leaf-actual-20261006/rtl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("rop_leaf.v"), RTL_SOURCE).unwrap();
    std::fs::write(dir.join("tb.v"), testbench(&edges)).unwrap();

    let compiler = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let runtime = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let mut compile = std::process::Command::new(&compiler);
    compile
        .current_dir(&dir)
        .args(["-g2012", "-s", "tb", "-o", "test.vvp", "rop_leaf.v", "tb.v"]);
    bounded(compile, &dir, "compile", Duration::from_secs(120));
    let mut run = std::process::Command::new(&runtime);
    run.current_dir(&dir).arg("test.vvp");
    bounded(run, &dir, "run", Duration::from_secs(120));
    let stdout = std::fs::read_to_string(dir.join("run.out")).unwrap();
    assert!(
        stdout.contains("PASS edges="),
        "vvp did not pass: {stdout}\n{}",
        std::fs::read_to_string(dir.join("run.err")).unwrap()
    );
    println!("{stdout}");
}
