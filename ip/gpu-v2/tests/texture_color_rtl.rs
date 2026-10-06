//! Actual Icarus per-edge co-simulation of the captured-texel color RTL against
//! the independent register emulator `texture::emu::color::ColorEmu`.
//!
//! The Rust tests below first establish the semantics with an independent
//! integer-division golden (`(sum+255)/511`) over distinct texels and weights,
//! then the ignored Icarus tests drive the emitted module every edge and compare
//! the pre-edge handshake and held output with the emulator's live result. No
//! expected RTL answer is embedded in the module; the testbench is generated
//! from emulator steps at run time.
//!
//! Both tool invocations run under a wall-clock watchdog that kills and reaps
//! the child, and their stdout/stderr go to files rather than pipes so a large
//! diagnostic can never fill a pipe. The generated bench also keeps its own
//! finite HDL watchdog (`#2000000; $fatal`). The Icarus tests are `#[ignore]`
//! so `cargo test` stays tool-free.

use gpu_v2::texture::{
    emu::color::{ColorEmu, Event, Input, Output, Tick, LATENCY, RESULT_CAPACITY},
    ports::{Group4, TileKey},
    rtl::color,
};
use std::fs::File;
use std::process::Stdio;
use std::time::{Duration, Instant};

const OUT_DIR: &str = "../../target/opencode/color-rtl-20261006";

struct Stim {
    ce: bool,
    ready: bool,
    input: Option<Input>,
    input_ready: bool,
    output: Option<Output>,
}

fn bit(value: bool) -> u8 {
    u8::from(value)
}

/// Build one canonical Group4 packet with already-expanded texels.
///
/// `n` selects the mip level so a sequence crosses fine and coarse tile
/// boundaries; `slot/x/y/top_left_local` stay at their canonical zero.
fn group(key: u8, first: bool, last: bool, weights: [u32; 4], texels: [u16; 4], n: u8) -> Input {
    let g = Group4 {
        key: TileKey {
            slot: 0,
            n,
            x: 0,
            y: 0,
        },
        top_left_local: [0, 0],
        coefficients: weights,
        first,
        last,
        quad_id: key >> 2,
        lane: key & 3,
    };
    Input {
        payload: g.pack72().unwrap() as i128,
        texels,
    }
}

/// Independent RAW565 bit-replication expansion.
fn expand(word: u16) -> [u32; 3] {
    let r = u32::from(word >> 11);
    let g = u32::from((word >> 5) & 63);
    let b = u32::from(word & 31);
    [r * 8 + r / 4, g * 4 + g / 16, b * 8 + b / 4]
}

/// Per-group weighted contribution read straight from packet bits, using
/// ordinary integer products.
fn contribution(p: Input) -> [u32; 3] {
    let mut sums = [0_u32; 3];
    for t in 0..4 {
        let w = ((p.payload >> (28 + 9 * t)) & 511) as u32;
        let rgb = expand(p.texels[t]);
        for c in 0..3 {
            sums[c] += rgb[c] * w;
        }
    }
    sums
}

/// Independent semantic golden: accumulate groups in stream order and divide
/// once on `last` with exact nearest `(sum+255)/511`.
fn golden(input: &[Input]) -> Vec<Output> {
    let mut out = Vec::new();
    let mut sum = [0_u32; 3];
    for &p in input {
        if p.payload >> 64 & 1 != 0 {
            sum = [0; 3];
        }
        let value = contribution(p);
        for c in 0..3 {
            sum[c] += value[c];
        }
        if p.payload >> 65 & 1 != 0 {
            out.push(Output {
                key: (((p.payload >> 66) & 15) * 4 + ((p.payload >> 70) & 3)) as u8,
                rgb: sum.map(|v| ((v + 255) / 511) as u8),
            });
        }
    }
    out
}

/// Deterministic stream: sample sizes one to eight, repeated keys, zero-weight
/// interior groups, fine/coarse mip levels and distinct tap colors and weights.
fn pixels() -> Vec<Input> {
    let mut state = 0x1234_5678_9abc_def0_u64;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 32) as u32
    };
    let mut out = Vec::new();
    for sample in 0..64_u32 {
        let groups = sample % 8 + 1;
        let key = (sample % 6) as u8;
        let n = [5_u8, 5, 2, 2, 0, 5, 3, 5][(sample % 8) as usize];
        let base = 511 / groups;
        let rem = 511 % groups;
        for g in 0..groups {
            let first = g == 0;
            let last = g + 1 == groups;
            let mut weights = [0_u32; 4];
            if !(sample % 7 == 3 && !last) {
                let total = base + u32::from(g < rem);
                weights[0] = total / 2;
                weights[1] = total - total / 2;
            }
            let texels = std::array::from_fn(|t| {
                (next() ^ ((g + 1) * 131 + t as u32 * 977) ^ (sample * 17)) as u16
            });
            out.push(group(key, first, last, weights, texels, n));
        }
    }
    out
}

/// Run the emulator with CE pauses and output backpressure and collect commits.
/// Returns `(commits, saw_credit_block, saw_ce_freeze)`.
fn replay(input: &[Input]) -> (Vec<Output>, bool, bool) {
    let mut m = ColorEmu::new(50_000).unwrap();
    let mut cursor = 0;
    let mut out = Vec::new();
    let mut blocked = false;
    let mut frozen = false;
    for t in 0..50_000_u64 {
        let ce = !(t % 17 == 11 || t % 17 == 12);
        let ready = t > 120 && t % 13 > 5;
        let before = m.snapshot();
        let s = m
            .tick(Tick {
                ce,
                input: input.get(cursor).copied(),
                output_ready: ready,
            })
            .unwrap();
        cursor += usize::from(s.accepted);
        if ce && cursor < input.len() && !s.input_ready {
            blocked = true;
        }
        if !ce && before.pipeline.into_iter().any(|v| v) {
            assert!(s.events.is_empty() && !s.accepted && !s.input_ready);
            assert_eq!(before.pipeline, s.snapshot.pipeline);
            frozen = true;
        }
        for e in s.events {
            if let Event::Commit(value) = e {
                out.push(value);
            }
        }
        assert!(s.snapshot.result_credits <= RESULT_CAPACITY);
        if cursor == input.len() && m.idle() {
            return (out, blocked, frozen);
        }
    }
    panic!("bounded replay did not drain");
}

#[test]
fn rtl_declares_the_emulated_pipeline() {
    let source = color::verilog();
    assert!(source.contains("module gpu_v2_texture_color("));
    assert_eq!(color::latency(), color::LATENCY);
    assert_eq!(LATENCY, 8);
    assert_eq!(color::latency(), 8);
    assert_eq!(color::initiation_interval(), 1);
    // Decode, three product ages, tree, feedback and exact nearest /511 stay in
    // the emitted calendar; the readiness and credit checks are fixed logic.
    for pattern in [
        "assign prod[t][c] = d_w[t] * d_col[t][c];",
        "assign pair[i][c] = {1'b0, p2_val[2*i][c]} + {1'b0, p2_val[2*i+1][c]};",
        "assign part[c] = {1'b0, q_val[0][c]} + {1'b0, q_val[1][c]};",
        "assign sumv[c] = {1'b0, r_val[c]} + {2'b0, prev[c]};",
        "wire [8:0] hlow = {1'b0, s_val[c][17:9]} + {1'b0, s_val[c][7:0]};",
        "assign ninc[c] = s_val[c][8] | hlow[8];",
    ] {
        assert!(
            source.contains(pattern),
            "missing fixed-calendar RTL: {pattern}"
        );
    }
    // The leaf declares generic registered products, not a Gowin macro claim.
    let a = color::Allocation::default();
    assert_eq!(a.multiplier_lanes, 12);
    assert_eq!(a.registered_product_bits, 3 * 12 * 17);
}

#[test]
fn semantic_goldens_cover_groups_owners_and_reuse() {
    let input = pixels();
    let expected = golden(&input);
    assert!(expected.len() >= 64);
    let (commits, blocked, frozen) = replay(&input);
    assert_eq!(commits, expected);
    assert!(blocked && frozen, "CE/credit stalls must be exercised");
}

#[test]
fn full_credit_stall_returns_only_after_commit() {
    let mut input = Vec::new();
    for i in 0..RESULT_CAPACITY {
        let texels = std::array::from_fn(|t| ((i as u32 * 31 + t as u32 * 997) as u16) | 1);
        input.push(group(
            (i % 6) as u8,
            true,
            true,
            [200, 120, 120, 71],
            texels,
            5,
        ));
    }
    let mut m = ColorEmu::new(20_000).unwrap();
    for &p in &input {
        let s = m
            .tick(Tick {
                ce: true,
                input: Some(p),
                output_ready: false,
            })
            .unwrap();
        assert!(s.accepted);
    }
    assert_eq!(m.snapshot().result_credits, RESULT_CAPACITY);
    // A seventeenth last group is blocked while all sixteen credits are owned.
    let blocked = group(0, true, true, [511, 0, 0, 0], [0xffff; 4], 5);
    let s = m
        .tick(Tick {
            ce: true,
            input: Some(blocked),
            output_ready: false,
        })
        .unwrap();
    assert!(!s.accepted && !s.input_ready);
    // A commit in the same edge returns one credit, but the held packet cannot
    // consume it until the following edge.
    let mut commits = Vec::new();
    let s = m
        .tick(Tick {
            ce: true,
            input: Some(blocked),
            output_ready: true,
        })
        .unwrap();
    assert!(!s.accepted, "same-edge returned credit must not admit");
    assert_eq!(s.snapshot.result_credits, RESULT_CAPACITY - 1);
    for e in s.events {
        if let Event::Commit(value) = e {
            commits.push(value);
        }
    }
    let s = m
        .tick(Tick {
            ce: true,
            input: Some(blocked),
            output_ready: true,
        })
        .unwrap();
    assert!(s.accepted, "returned credit admits the held packet");
    for e in s.events {
        if let Event::Commit(value) = e {
            commits.push(value);
        }
    }

    let mut all = input;
    all.push(blocked);
    for _ in 0..2_000 {
        let s = m
            .tick(Tick {
                ce: true,
                input: None,
                output_ready: true,
            })
            .unwrap();
        for e in s.events {
            if let Event::Commit(value) = e {
                commits.push(value);
            }
        }
        if m.idle() {
            break;
        }
    }
    assert!(m.idle());
    assert_eq!(m.snapshot().result_credits, 0);
    assert_eq!(commits, golden(&all));
}

#[test]
fn owner_and_sum_violations_fault_the_emulator() {
    let mut m = ColorEmu::new(64).unwrap();
    m.tick(Tick {
        ce: true,
        input: Some(group(2, true, false, [255, 0, 0, 0], [0; 4], 5)),
        output_ready: true,
    })
    .unwrap();
    assert_eq!(
        m.tick(Tick {
            ce: true,
            input: Some(group(3, false, true, [256, 0, 0, 0], [0; 4], 5)),
            output_ready: true,
        })
        .unwrap_err(),
        "color non-first owner/order"
    );
    assert!(m.faulted());

    // [256,0,256,0] over 0xffff makes the four-tap partial 130560 > 130305.
    let mut m = ColorEmu::new(64).unwrap();
    m.tick(Tick {
        ce: true,
        input: Some(group(0, true, true, [256, 0, 256, 0], [0xffff; 4], 5)),
        output_ready: true,
    })
    .unwrap();
    let mut failure = None;
    for _ in 0..8 {
        if let Err(e) = m.tick(Tick {
            ce: true,
            input: None,
            output_ready: true,
        }) {
            failure = Some(e);
            break;
        }
    }
    assert_eq!(
        failure.as_deref(),
        Some("color partial outside legal domain")
    );
}

/// Drive the emulator exactly as the testbench drives the RTL, stopping once the
/// stream has drained.
fn stimulus() -> Vec<Stim> {
    let pixels = pixels();
    let mut emu = ColorEmu::new(500_000).unwrap();
    let mut ptr = 0;
    let mut edges = Vec::new();
    for wall in 0..4_000 {
        let ce = !(wall % 19 == 5 || wall % 19 == 6 || wall % 19 == 7);
        let ready = !(wall < 64) && (wall % 11 != 2);
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
        if ptr == pixels.len() && emu.idle() {
            break;
        }
    }
    assert_eq!(ptr, pixels.len(), "stimulus did not offer every group");
    assert!(emu.idle(), "stimulus did not drain");
    edges
}

fn testbench(edges: &[Stim]) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,out_ready=0;\n");
    tb.push_str("reg [71:0] in_payload=0;\n");
    tb.push_str("reg [15:0] in_tex0=0,in_tex1=0,in_tex2=0,in_tex3=0;\n");
    tb.push_str(
        "wire in_ready,out_valid,fault; wire [5:0] out_key; wire [7:0] out_r,out_g,out_b;\n",
    );
    tb.push_str("gpu_v2_texture_color dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_payload(in_payload),.in_tex0(in_tex0),.in_tex1(in_tex1),.in_tex2(in_tex2),.in_tex3(in_tex3),.in_ready(in_ready),.out_ready(out_ready),.out_valid(out_valid),.out_key(out_key),.out_r(out_r),.out_g(out_g),.out_b(out_b),.fault(fault));\n");
    tb.push_str("initial begin #2000000; $fatal(1,\"simulation watchdog\"); end\n");
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
                "in_payload=72'h{:018x};in_tex0=16'd{};in_tex1=16'd{};in_tex2=16'd{};in_tex3=16'd{};\n",
                input.payload as u128,
                input.texels[0],
                input.texels[1],
                input.texels[2],
                input.texels[3]
            ));
        }
        tb.push_str("#1;\n");
        tb.push_str(&format!(
            "if (fault !== 1'b0) $fatal(1,\"unexpected fault edge {edge}\");\n"
        ));
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

fn out_dir() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(OUT_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile_and_run(dir: &std::path::Path, stem: &str, module: &str, bench: &str) {
    std::fs::write(dir.join(format!("{stem}.v")), module).unwrap();
    std::fs::write(dir.join(format!("{stem}_tb.v")), bench).unwrap();
    let compiler = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let runtime = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let mut compile = std::process::Command::new(&compiler);
    compile.current_dir(dir).args([
        "-g2012",
        "-s",
        "tb",
        "-o",
        &format!("{stem}.vvp"),
        &format!("{stem}.v"),
        &format!("{stem}_tb.v"),
    ]);
    bounded(
        compile,
        dir,
        &format!("{stem}_compile"),
        Duration::from_secs(120),
    );
    let mut run = std::process::Command::new(&runtime);
    run.current_dir(dir).arg(format!("{stem}.vvp"));
    bounded(run, dir, &format!("{stem}_run"), Duration::from_secs(120));
    let stdout = std::fs::read_to_string(dir.join(format!("{stem}_run.out"))).unwrap();
    assert!(
        stdout.contains("PASS edges="),
        "vvp did not pass: {stdout}\n{}",
        std::fs::read_to_string(dir.join(format!("{stem}_run.err"))).unwrap()
    );
    println!("{stdout}");
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_matches_emu_every_edge() {
    let edges = stimulus();
    assert!(edges.len() > 64);
    let dir = out_dir();
    compile_and_run(
        &dir,
        "texture_color_rtl",
        color::verilog(),
        &testbench(&edges),
    );
}

/// Small raw edge list for the fault tests.
struct RawStim {
    ce: bool,
    ready: bool,
    input: Option<Input>,
}

fn raw_testbench(edges: &[RawStim], fault_from: usize) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,out_ready=0;\n");
    tb.push_str("reg [71:0] in_payload=0;\n");
    tb.push_str("reg [15:0] in_tex0=0,in_tex1=0,in_tex2=0,in_tex3=0;\n");
    tb.push_str(
        "wire in_ready,out_valid; wire [5:0] out_key; wire [7:0] out_r,out_g,out_b; wire fault;\n",
    );
    tb.push_str("gpu_v2_texture_color dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),.in_payload(in_payload),.in_tex0(in_tex0),.in_tex1(in_tex1),.in_tex2(in_tex2),.in_tex3(in_tex3),.in_ready(in_ready),.out_ready(out_ready),.out_valid(out_valid),.out_key(out_key),.out_r(out_r),.out_g(out_g),.out_b(out_b),.fault(fault));\n");
    tb.push_str("initial begin #2000000; $fatal(1,\"simulation watchdog\"); end\n");
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
                "in_payload=72'h{:018x};in_tex0=16'd{};in_tex1=16'd{};in_tex2=16'd{};in_tex3=16'd{};\n",
                input.payload as u128,
                input.texels[0],
                input.texels[1],
                input.texels[2],
                input.texels[3]
            ));
        }
        tb.push_str("#1;\n");
        if edge >= fault_from {
            tb.push_str(&format!(
                "if (fault !== 1'b1) $fatal(1,\"fault not latched by edge {edge}\");\n"
            ));
        } else {
            tb.push_str(&format!(
                "if (fault !== 1'b0) $fatal(1,\"unexpected fault before edge {edge}\");\n"
            ));
        }
        tb.push_str("clk=1;#1;clk=0;#1;\n");
    }
    // The failing edge has happened even if the vector ends at the boundary.
    // A zero-length suffix must not skip the terminal assertion.
    assert!(edges.len() >= fault_from);
    tb.push_str("if (fault !== 1'b1) $fatal(1,\"final fault not latched\");\n");
    tb.push_str("ce=1;in_valid=1;#1;if(in_ready !== 1'b0 || out_valid !== 1'b0) $fatal(1,\"terminal fault still handshakes\");\n");
    tb.push_str(&format!(
        "$display(\"PASS edges={}\");$finish;end endmodule\n",
        edges.len()
    ));
    tb
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_latches_owner_and_sum_faults() {
    let dir = out_dir();
    // Group4's four-bit mip field has only eleven canonical values.
    let mut invalid = group(0, true, true, [511, 0, 0, 0], [0; 4], 5);
    invalid.payload = (invalid.payload & !(15 << 4)) | (15 << 4);
    let mut emu = ColorEmu::new(32).unwrap();
    assert!(emu
        .tick(Tick {
            ce: true,
            input: Some(invalid),
            output_ready: true
        })
        .is_err());
    compile_and_run(
        &dir,
        "texture_color_rtl_mip_fault",
        color::verilog(),
        &raw_testbench(
            &[RawStim {
                ce: true,
                ready: true,
                input: Some(invalid),
            }],
            1,
        ),
    );
    // Owner/order: a non-first group with a different key faults at its accept.
    let owner = [
        RawStim {
            ce: true,
            ready: true,
            input: Some(group(2, true, false, [255, 0, 0, 0], [0; 4], 5)),
        },
        RawStim {
            ce: true,
            ready: true,
            input: Some(group(3, false, true, [256, 0, 0, 0], [0; 4], 5)),
        },
    ];
    compile_and_run(
        &dir,
        "texture_color_rtl_owner_fault",
        color::verilog(),
        &raw_testbench(&owner, 2),
    );

    // Sum domain: the four-tap partial 130560 exceeds 255*511 at age five.
    let mut sum = vec![RawStim {
        ce: true,
        ready: false,
        input: Some(group(0, true, true, [256, 0, 256, 0], [0xffff; 4], 5)),
    }];
    for _ in 0..8 {
        sum.push(RawStim {
            ce: true,
            ready: false,
            input: None,
        });
    }
    compile_and_run(
        &dir,
        "texture_color_rtl_sum_fault",
        color::verilog(),
        &raw_testbench(&sum, 6),
    );
}
