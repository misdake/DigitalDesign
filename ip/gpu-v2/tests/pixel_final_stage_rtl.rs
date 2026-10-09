//! Actual Icarus per-edge co-simulation of the `final_stage` RTL against the
//! independent register emulator. The testbench drives one input edge at a time
//! and compares the pre-edge handshake and payload with the emulator's live
//! result; it never embeds a precomputed RTL answer.
//!
//! Both tool invocations run under a wall-clock watchdog that kills and reaps
//! the child, and their stdout/stderr go to files rather than pipes so a large
//! diagnostic can never fill a pipe. The generated bench also keeps its own
//! finite HDL watchdog (`#200000; $fatal`).
//!
//! The Icarus test is `#[ignore]` so `cargo test` stays tool-free; run it with
//! `-- --ignored` when `IVERILOG_EXE`/`VVP_EXE` are available.

use gpu_v2::system::pixel::final_stage::{emu::FinalEmu, rtl, Input, Tick};
use std::fs::File;
use std::process::Stdio;
use std::time::{Duration, Instant};

struct Stim {
    ce: bool,
    ready: bool,
    input: Option<Input>,
    input_ready: bool,
    output: Option<gpu_v2::system::pixel::final_stage::Output>,
}

fn bit(value: bool) -> u8 {
    u8::from(value)
}

fn pixels() -> Vec<Input> {
    let mut state = 0x1234_5678_9abc_def0u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u32
    };
    (0..48)
        .map(|i| Input {
            key: (i % 4) as u8,
            tint: [((next() % 4) * 85) as u8, next() as u8, (255 - i) as u8],
            texture: [next() as u8, ((next() % 3) * 127) as u8, 255],
            g: match i % 7 {
                0 => 0,
                1 => 511,
                2 => 256,
                3 => 1,
                _ => (next() % 512) as u16,
            },
            h: match i % 5 {
                0 => 0,
                1 => 256,
                2 => 128,
                _ => (next() % 257) as u16,
            },
            specular: [next() as u8, 0, 255],
        })
        .collect()
}

/// Drive the emulator exactly as the testbench will drive the RTL.
fn stimulus(max_edges: usize) -> Vec<Stim> {
    let pixels = pixels();
    let mut emu = FinalEmu::new(500_000).unwrap();
    let mut ptr = 0;
    let mut edges = Vec::new();
    for wall in 0..max_edges {
        // Bursts of full throughput, then a CE pause, then output backpressure.
        let ce = !(wall % 17 == 13 || wall % 17 == 14);
        let ready = wall % 11 != 5 && !(wall % 29 >= 22 && wall % 29 <= 25);
        let input = pixels.get(ptr).copied();
        let step = emu
            .tick(Tick {
                ce,
                input,
                output_ready: ready,
            })
            .unwrap();
        if step.accepted {
            ptr += 1;
        }
        edges.push(Stim {
            ce,
            ready,
            input,
            input_ready: step.input_ready,
            output: step.output,
        });
    }
    assert_eq!(ptr, pixels.len(), "stimulus did not offer every pixel");
    assert!(emu.idle(), "stimulus did not drain");
    edges
}

fn testbench(edges: &[Stim]) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,out_ready=0;\n");
    tb.push_str("reg [5:0] in_key=0;\n");
    tb.push_str("reg [7:0] in_tint_r=0,in_tint_g=0,in_tint_b=0;\n");
    tb.push_str("reg [7:0] in_tex_r=0,in_tex_g=0,in_tex_b=0;\n");
    tb.push_str("reg [7:0] in_spec_r=0,in_spec_g=0,in_spec_b=0;\n");
    tb.push_str("reg [8:0] in_g=0,in_h=0;\n");
    tb.push_str("wire in_ready,out_valid; wire [5:0] out_key; wire [7:0] out_r,out_g,out_b;\n");
    tb.push_str("gpu_v2_final_stage dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_key(in_key),.in_tint_r(in_tint_r),.in_tint_g(in_tint_g),.in_tint_b(in_tint_b),.in_tex_r(in_tex_r),.in_tex_g(in_tex_g),.in_tex_b(in_tex_b),.in_g(in_g),.in_h(in_h),.in_spec_r(in_spec_r),.in_spec_g(in_spec_g),.in_spec_b(in_spec_b),.in_ready(in_ready),.out_ready(out_ready),.out_valid(out_valid),.out_key(out_key),.out_r(out_r),.out_g(out_g),.out_b(out_b));\n");
    tb.push_str("initial begin #200000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\n");
    tb.push_str("clk=0;reset=1;ce=0;in_valid=0;out_ready=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    for (edge, stim) in edges.iter().enumerate() {
        tb.push_str(&format!(
            "ce={};out_ready={};in_valid={};\n",
            bit(stim.ce),
            bit(stim.ready),
            bit(stim.input.is_some())
        ));
        if let Some(input) = stim.input {
            tb.push_str(&format!(
                "in_key=6'd{};in_g=9'd{};in_h=9'd{};\n",
                input.key, input.g, input.h
            ));
            tb.push_str(&format!(
                "in_tint_r=8'd{};in_tint_g=8'd{};in_tint_b=8'd{};\n",
                input.tint[0], input.tint[1], input.tint[2]
            ));
            tb.push_str(&format!(
                "in_tex_r=8'd{};in_tex_g=8'd{};in_tex_b=8'd{};\n",
                input.texture[0], input.texture[1], input.texture[2]
            ));
            tb.push_str(&format!(
                "in_spec_r=8'd{};in_spec_g=8'd{};in_spec_b=8'd{};\n",
                input.specular[0], input.specular[1], input.specular[2]
            ));
        }
        tb.push_str("#1;\n");
        tb.push_str(&format!(
            "if (in_ready !== 1'b{}) $fatal(1,\"in_ready edge {edge}\");\n",
            bit(stim.input_ready)
        ));
        tb.push_str(&format!(
            "if (out_valid !== 1'b{}) $fatal(1,\"out_valid edge {edge}\");\n",
            bit(stim.output.is_some())
        ));
        if let Some(output) = stim.output {
            tb.push_str(&format!(
                "if (out_key !== 6'd{} || out_r !== 8'd{} || out_g !== 8'd{} || out_b !== 8'd{}) $fatal(1,\"payload edge {edge}: got key=%0d r=%0d g=%0d b=%0d want key={} r={} g={} b={}\",out_key,out_r,out_g,out_b);\n",
                output.key, output.rgb[0], output.rgb[1], output.rgb[2],
                output.key, output.rgb[0], output.rgb[1], output.rgb[2]
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
fn rtl_declares_the_emulated_pipeline() {
    assert!(rtl::source().contains("module gpu_v2_final_stage"));
    assert_eq!(rtl::latency(), 9);
    assert_eq!(rtl::initiation_interval(), 1);
    // The emitted RTL encodes the fixed stage register calendar certified by
    // `sim::timed::StageCalendar`.
    let source = rtl::source();
    for pattern in [
        "s1_m1[i] <= tin[i] * tex[i];",              // stage 1: tint*texture
        "s1_m2[i] <= spe[i] * in_h;",                // stage 1: specular*h
        "s2_t[i]   <= n_t[i];",                      // stage 2: +128
        "s3_b[i]   <= n_b[i];",                      // stage 3: +(t >> 8)
        "s4_bg[i]  <= n_base[i] * s3_g[i];",         // stage 4: base*g
        "n_sum[c]  = s4_bg[c] + s4_m2[c];",          // stage 5: sum
        "n_r1[c]   = s5_sum[c] + 18'd127;",          // stage 6: +127
        "n_r2[c]   = s6_r1[c] + {17'd0, s6_bit[c]}", // stage 7: +bit
        "n_ov[c]   = (n_rd[c] > 10'd255);",          // stage 8: compare
        "n_col[c]  = s8_ov[c] ? 8'd255 : s8_rd[c][7:0];", // stage 9: select
    ] {
        assert!(
            source.contains(pattern),
            "missing fixed-calendar RTL: {pattern}"
        );
    }
    // Four credits, pre-edge readiness and the finite HDL watchdog stay in place.
    assert!(source.contains("assign in_ready  = ce && (credits < 3'd4);"));
    assert!(source.contains("assign out_valid = (f_count != 3'd0);"));
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
                "{name} failed: {} {}",
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
    assert_eq!(edges.len(), 600);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/opencode/final-stage-rtl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("gpu_v2_final_stage.v"), rtl::source()).unwrap();
    std::fs::write(dir.join("tb.v"), testbench(&edges)).unwrap();

    let compiler = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let runtime = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let mut compile = std::process::Command::new(&compiler);
    compile.current_dir(&dir).args([
        "-g2012",
        "-s",
        "tb",
        "-o",
        "test.vvp",
        "gpu_v2_final_stage.v",
        "tb.v",
    ]);
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
