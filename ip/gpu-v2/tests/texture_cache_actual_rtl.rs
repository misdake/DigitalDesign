//! Actual Icarus per-edge co-simulation of the demand-only texture cache RTL
//! against the live register emulator.
//!
//! Each scenario runs the emulator as the reference and replays the recorded
//! per-edge stimulus (CE, offered packet, output ready, abort) into a generated
//! testbench. The pre-edge handshake, the retained output, the cache memory
//! request, the numbered beats and the terminal acknowledgement are compared on
//! every edge. The behavioral memory reads the actual RAW565 byte image; no
//! precomputed RTL answer and no golden trace is embedded.
//!
//! The memory adapter and its Verilog twin implement one shared acknowledgement
//! policy: `ack_delay == 0` shares the last-beat edge, `ack_delay >= 1` issues
//! the terminal on a later edge with no beat, and `same_edge_first` returns beat
//! zero on the request-acceptance edge.
//!
//! Both tool invocations run under a wall-clock watchdog that kills and reaps
//! the child, and their stdout/stderr go to files (never pipes) so a large
//! diagnostic cannot fill a pipe. The generated bench keeps its own finite HDL
//! watchdog. The Icarus test is `#[ignore]` so `cargo test` stays tool-free; run
//! it with `-- --ignored` when `IVERILOG_EXE`/`VVP_EXE` are available.

#[path = "support/texture.rs"]
#[allow(dead_code)]
mod support;

use gpu_v2::memory::ports::{MemoryPort, Request, Response, BURST_BEATS};
use gpu_v2::texture::emu::cache::{CacheEmu, CacheState, CacheTick};
use gpu_v2::texture::ports::{Group4, Slot, TileKey};
use gpu_v2::texture::rtl::cache as rtl;
use std::fs::File;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Behavioral memory mirroring the generated Verilog twin edge for edge.
struct HostMemory {
    bytes: Vec<u8>,
    base: u64,
    ready_period: u64,
    ack_delay: u8,
    same_edge_first: bool,
    busy: bool,
    waiting: bool,
    addr: u64,
    count: u8,
    wait_edges: u8,
    edge: u64,
    beats: usize,
}

impl HostMemory {
    fn new(bytes: Vec<u8>, ready_period: u64, ack_delay: u8, same_edge_first: bool) -> Self {
        Self {
            bytes,
            base: u64::from(support::BASE),
            ready_period,
            ack_delay,
            same_edge_first,
            busy: false,
            waiting: false,
            addr: 0,
            count: 0,
            wait_edges: 0,
            edge: 0,
            beats: 0,
        }
    }

    fn beats(&self) -> usize {
        self.beats
    }

    fn word(&self, index: usize) -> u64 {
        let offset = usize::try_from(self.addr - self.base).unwrap() + index * 8;
        u64::from_le_bytes(self.bytes[offset..offset + 8].try_into().unwrap())
    }
}

impl MemoryPort for HostMemory {
    fn cycle(&mut self, request: Option<Request>, _write: Option<u64>) -> Result<Response, String> {
        let mut response = Response::default();
        let ready = self.ready_period == 0 || !self.edge.is_multiple_of(self.ready_period);
        if self.busy {
            if self.waiting {
                self.wait_edges += 1;
                if self.wait_edges >= self.ack_delay {
                    response.complete = Some(true);
                    self.busy = false;
                    self.waiting = false;
                }
            } else {
                let index = self.count;
                response.read = Some((index, self.word(usize::from(index))));
                self.beats += 1;
                if index + 1 == BURST_BEATS as u8 {
                    if self.ack_delay == 0 {
                        response.complete = Some(true);
                        self.busy = false;
                    } else {
                        self.waiting = true;
                        self.wait_edges = 0;
                    }
                } else {
                    self.count += 1;
                }
            }
        } else if let Some(request) = request {
            if ready && !request.write {
                self.busy = true;
                self.addr = request.address_bytes;
                self.count = 0;
                response.accepted = true;
                if self.same_edge_first {
                    response.read = Some((0, self.word(0)));
                    self.count = 1;
                    self.beats += 1;
                }
            }
        }
        self.edge += 1;
        Ok(response)
    }
}

#[derive(Clone, Copy)]
struct Edge {
    ce: bool,
    abort: bool,
    in_valid: bool,
    in_payload: i128,
    out_ready: bool,
    in_ready: bool,
    out_valid: bool,
    out_payload: i128,
    out_tex: [u16; 4],
    req_valid: bool,
    req_addr: u32,
    req_accepted: bool,
    resp_valid: bool,
    resp_index: u8,
    resp_data: u64,
    resp_complete: bool,
    resp_ok: bool,
    fault: bool,
}

impl Edge {
    fn from_step(
        ce: bool,
        out_ready: bool,
        step: &gpu_v2::texture::emu::cache::CacheStep,
        input: Option<i128>,
    ) -> Self {
        let output = step.output;
        Self {
            ce,
            abort: false,
            in_valid: input.is_some(),
            in_payload: input.unwrap_or(0),
            out_ready,
            in_ready: step.input_ready,
            out_valid: output.is_some(),
            out_payload: output.map_or(0, |o| o.payload),
            out_tex: output.map_or([0; 4], |o| o.texels),
            req_valid: step.request.is_some(),
            req_addr: step.request.map_or(0, |r| r.address_bytes as u32),
            req_accepted: step.response.accepted,
            resp_valid: step.response.read.is_some(),
            resp_index: step.response.read.map_or(0, |(i, _)| i),
            resp_data: step.response.read.map_or(0, |(_, d)| d),
            resp_complete: step.response.complete.is_some(),
            resp_ok: step.response.complete == Some(true),
            fault: false,
        }
    }

    fn from_drain(
        abort: bool,
        fault: bool,
        drain: &gpu_v2::texture::emu::cache::CacheDrain,
    ) -> Self {
        Self {
            ce: false,
            abort,
            in_valid: false,
            in_payload: 0,
            out_ready: false,
            in_ready: false,
            out_valid: false,
            out_payload: 0,
            out_tex: [0; 4],
            req_valid: drain.request.is_some(),
            req_addr: drain.request.map_or(0, |r| r.address_bytes as u32),
            req_accepted: drain.response.accepted,
            resp_valid: drain.response.read.is_some(),
            resp_index: drain.response.read.map_or(0, |(i, _)| i),
            resp_data: drain.response.read.map_or(0, |(_, d)| d),
            resp_complete: drain.response.complete.is_some(),
            resp_ok: drain.response.complete == Some(true),
            fault,
        }
    }
}

#[derive(Clone, Copy)]
enum Schedule {
    Always,
    EveryOther,
    PauseOnState,
}

struct Scenario {
    name: &'static str,
    slot: Slot,
    packets: Vec<i128>,
    ready_period: u64,
    ack_delay: u8,
    same_edge_first: bool,
    schedule: Schedule,
    /// Hold `out_ready` low on every fourth edge to create output backpressure.
    out_ready_hold: bool,
    abort_after_beats: Option<usize>,
}

fn packet(key: TileKey, top_left_local: [u8; 2], quad_id: u8) -> i128 {
    Group4 {
        key,
        top_left_local,
        coefficients: [0, 1, 2, 3],
        first: true,
        last: true,
        quad_id,
        lane: 0,
    }
    .pack72()
    .unwrap() as i128
}

fn ce_for(schedule: Schedule, edge: u64, state: &CacheState, paused: &mut u32) -> bool {
    match schedule {
        Schedule::Always => true,
        Schedule::EveryOther => edge.is_multiple_of(2),
        Schedule::PauseOnState => {
            let live = state.head || state.read_pending || state.result || state.descriptor;
            if live && *paused < 2 {
                *paused += 1;
                false
            } else {
                *paused = 0;
                true
            }
        }
    }
}

/// Run the emulator for one scenario and record every edge. When the scenario
/// requests an abort, the abort and drain edges are recorded with their flags.
fn record(scenario: &Scenario) -> (Vec<Edge>, usize) {
    let bytes = support::asset(scenario.slot, support::pattern);
    let mut emu = CacheEmu::new(
        vec![scenario.slot],
        HostMemory::new(
            bytes,
            scenario.ready_period,
            scenario.ack_delay,
            scenario.same_edge_first,
        ),
        2_000_000,
    )
    .unwrap();
    let mut ptr = 0;
    let mut paused = 0_u32;
    let mut edges = Vec::new();
    let mut aborted = false;
    let abort_at = scenario.abort_after_beats;
    for edge_index in 0..200_000_u64 {
        if !aborted {
            if let Some(target) = abort_at {
                let beats = emu.memory().beats();
                if beats >= target && emu.state().started {
                    emu.abort();
                    let drain = emu.drain_tick(false).unwrap();
                    // The RTL fault register is sampled pre-edge, so it still
                    // reads low on the abort edge and latches for the next one.
                    edges.push(Edge::from_drain(true, false, &drain));
                    aborted = true;
                    continue;
                }
            }
        }
        if aborted {
            let drain = emu.drain_tick(false).unwrap();
            let drained = drain.drained;
            let fault = emu.faulted();
            edges.push(Edge::from_drain(false, fault, &drain));
            if drained {
                break;
            }
            continue;
        }
        let ce = ce_for(scenario.schedule, edge_index, &emu.state(), &mut paused);
        let out_ready = !scenario.out_ready_hold || !edge_index.is_multiple_of(4);
        let input = scenario.packets.get(ptr).copied();
        let step = emu
            .tick(CacheTick {
                ce,
                input,
                output_ready: out_ready,
            })
            .unwrap();
        if step.accepted {
            ptr += 1;
        }
        edges.push(Edge::from_step(ce, out_ready, &step, input));
        if ptr == scenario.packets.len() && emu.idle() {
            break;
        }
    }
    if abort_at.is_none() {
        assert_eq!(
            ptr,
            scenario.packets.len(),
            "{}: stimulus did not offer every packet",
            scenario.name
        );
        assert!(emu.idle(), "{}: stimulus did not drain", scenario.name);
    } else {
        assert!(emu.drained(), "{}: abort did not drain", scenario.name);
    }
    let count = edges.len();
    (edges, count)
}

fn bit(value: bool) -> u8 {
    u8::from(value)
}

/// Generated behavioral memory. `ACK_DELAY`, `SAME_EDGE_FIRST` and
/// `READY_PERIOD` are literal so the twin matches [`HostMemory`] exactly.
fn memory_module(len: usize, ack_delay: u8, same_edge_first: bool, ready_period: u64) -> String {
    format!(
        r#"module gpu_v2_texture_cache_mem(
  input clk, input reset,
  input req_valid, input [31:0] req_addr, output req_ready,
  output resp_valid, output [3:0] resp_index, output [63:0] resp_data,
  output resp_complete, output resp_ok
);
  localparam integer B = {len};
  localparam [31:0] BASEADDR = 32'h{BASE:08x};
  localparam integer ACK_DELAY = {ack_delay};
  localparam integer SAME_EDGE_FIRST = {same_edge_first};
  localparam integer READY_PERIOD = {ready_period};
  reg [7:0] bytes [0:B-1];
  initial $readmemh("asset.hex", bytes);
  reg busy; reg waiting; reg [31:0] addr; reg [3:0] count;
  reg [2:0] wait_edges; reg [31:0] tick_count;
  wire ready = (READY_PERIOD == 0) || (tick_count % READY_PERIOD != 0);
  assign req_ready = ready && !busy && !reset;
  wire accept = req_valid && req_ready;
  wire [31:0] cur_addr = busy ? addr : req_addr;
  wire [31:0] cur_off = cur_addr - BASEADDR;
  wire [3:0]  cur_index = busy ? count : 4'd0;
  assign resp_valid = busy ? !waiting : (accept && SAME_EDGE_FIRST);
  assign resp_index = cur_index;
  assign resp_data = {{ bytes[cur_off+cur_index*8+7], bytes[cur_off+cur_index*8+6],
                       bytes[cur_off+cur_index*8+5], bytes[cur_off+cur_index*8+4],
                       bytes[cur_off+cur_index*8+3], bytes[cur_off+cur_index*8+2],
                       bytes[cur_off+cur_index*8+1], bytes[cur_off+cur_index*8+0] }};
  assign resp_complete = busy ? (waiting ? ((wait_edges + 3'd1) >= ACK_DELAY)
                                          : ((count == 4'd15) && (ACK_DELAY == 0)))
                              : 1'b0;
  assign resp_ok = 1'b1;
  always @(posedge clk) begin
    if (reset) begin
      busy <= 1'b0; waiting <= 1'b0; addr <= 32'd0; count <= 4'd0;
      wait_edges <= 3'd0; tick_count <= 32'd0;
    end else begin
      tick_count <= tick_count + 32'd1;
      if (busy) begin
        if (waiting) begin
          if ((wait_edges + 3'd1) >= ACK_DELAY) begin busy <= 1'b0; waiting <= 1'b0; end
          else wait_edges <= wait_edges + 3'd1;
        end else if (count == 4'd15) begin
          if (ACK_DELAY == 0) busy <= 1'b0;
          else begin waiting <= 1'b1; wait_edges <= 3'd0; end
        end else begin
          count <= count + 4'd1;
        end
      end else if (accept) begin
        busy <= 1'b1; addr <= req_addr; count <= SAME_EDGE_FIRST ? 4'd1 : 4'd0;
        waiting <= 1'b0; wait_edges <= 3'd0;
      end
    end
  end
endmodule
"#,
        BASE = support::BASE,
        ack_delay = ack_delay,
        same_edge_first = u8::from(same_edge_first),
        ready_period = ready_period,
    )
}

fn testbench(edges: &[Edge], counter: usize) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0, reset=0, ce=0, abort=0, in_valid=0, out_ready=0;\n");
    tb.push_str("reg [71:0] in_payload=0;\n");
    tb.push_str("wire in_ready, out_valid, mem_req_valid, mem_req_ready;\n");
    tb.push_str("wire [31:0] mem_req_addr;\n");
    tb.push_str("wire [71:0] out_payload;\n");
    tb.push_str("wire [15:0] out_tex0,out_tex1,out_tex2,out_tex3;\n");
    tb.push_str("wire mem_resp_valid, mem_resp_complete, mem_resp_ok;\n");
    tb.push_str("wire [3:0] mem_resp_index;\n");
    tb.push_str("wire [63:0] mem_resp_data;\n");
    tb.push_str("wire fault;\n");
    tb.push_str("gpu_v2_texture_cache dut(.clk(clk),.reset(reset),.ce(ce),.abort(abort),.in_valid(in_valid),.in_payload(in_payload),.in_ready(in_ready),.out_ready(out_ready),.out_valid(out_valid),.out_payload(out_payload),.out_tex0(out_tex0),.out_tex1(out_tex1),.out_tex2(out_tex2),.out_tex3(out_tex3),.mem_req_valid(mem_req_valid),.mem_req_addr(mem_req_addr),.mem_req_ready(mem_req_ready),.mem_resp_valid(mem_resp_valid),.mem_resp_index(mem_resp_index),.mem_resp_data(mem_resp_data),.mem_resp_complete(mem_resp_complete),.mem_resp_ok(mem_resp_ok),.fault(fault));\n");
    tb.push_str("gpu_v2_texture_cache_mem mem(.clk(clk),.reset(reset),.req_valid(mem_req_valid),.req_addr(mem_req_addr),.req_ready(mem_req_ready),.resp_valid(mem_resp_valid),.resp_index(mem_resp_index),.resp_data(mem_resp_data),.resp_complete(mem_resp_complete),.resp_ok(mem_resp_ok));\n");
    tb.push_str("initial begin #2000000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\n");
    tb.push_str("clk=0;reset=1;ce=0;abort=0;in_valid=0;out_ready=0;in_payload=0;\n");
    tb.push_str("#1;clk=1;#1;clk=0;#1;reset=0;\n");
    for (index, edge) in edges.iter().enumerate() {
        tb.push_str(&format!(
            "ce={};abort={};in_valid={};out_ready={};in_payload=72'h{:018x};\n",
            bit(edge.ce),
            bit(edge.abort),
            bit(edge.in_valid),
            bit(edge.out_ready),
            edge.in_payload as u128 & ((1_u128 << 72) - 1),
        ));
        tb.push_str("#1;\n");
        tb.push_str(&format!(
            "if (fault !== 1'b{}) $fatal(1,\"fault edge {index}\");\n",
            bit(edge.fault)
        ));
        tb.push_str(&format!(
            "if (in_ready !== 1'b{}) $fatal(1,\"in_ready edge {index}\");\n",
            bit(edge.in_ready)
        ));
        tb.push_str(&format!(
            "if (out_valid !== 1'b{}) $fatal(1,\"out_valid edge {index}\");\n",
            bit(edge.out_valid)
        ));
        tb.push_str(&format!(
            "if (mem_req_valid !== 1'b{}) $fatal(1,\"mem_req_valid edge {index}\");\n",
            bit(edge.req_valid)
        ));
        if edge.req_valid {
            tb.push_str(&format!(
                "if (mem_req_addr !== 32'h{:08x}) $fatal(1,\"mem_req_addr edge {index}: got %0h\",mem_req_addr);\n",
                edge.req_addr
            ));
        }
        tb.push_str(&format!(
            "if ((mem_req_valid && mem_req_ready) !== 1'b{}) $fatal(1,\"req handshake edge {index}\");\n",
            bit(edge.req_accepted)
        ));
        tb.push_str(&format!(
            "if (mem_resp_valid !== 1'b{}) $fatal(1,\"resp_valid edge {index}\");\n",
            bit(edge.resp_valid)
        ));
        if edge.resp_valid {
            tb.push_str(&format!(
                "if (mem_resp_index !== 4'd{}) $fatal(1,\"resp_index edge {index}\");\n",
                edge.resp_index
            ));
            tb.push_str(&format!(
                "if (mem_resp_data !== 64'h{:016x}) $fatal(1,\"resp_data edge {index}: got %0h\",mem_resp_data);\n",
                edge.resp_data
            ));
        }
        tb.push_str(&format!(
            "if (mem_resp_complete !== 1'b{}) $fatal(1,\"resp_complete edge {index}\");\n",
            bit(edge.resp_complete)
        ));
        if edge.resp_complete {
            tb.push_str(&format!(
                "if (mem_resp_ok !== 1'b{}) $fatal(1,\"resp_ok edge {index}\");\n",
                bit(edge.resp_ok)
            ));
        }
        if edge.out_valid {
            tb.push_str(&format!(
                "if (out_payload !== 72'h{:018x}) $fatal(1,\"out_payload edge {index}\");\n",
                edge.out_payload as u128 & ((1_u128 << 72) - 1)
            ));
            for (tex, value) in edge.out_tex.iter().enumerate() {
                tb.push_str(&format!(
                    "if (out_tex{tex} !== 16'h{value:04x}) $fatal(1,\"out_tex{tex} edge {index}\");\n"
                ));
            }
        }
        tb.push_str("clk=1;#1;clk=0;#1;\n");
    }
    tb.push_str(&format!(
        "$display(\"PASS edges={counter}\");$finish;end endmodule\n"
    ));
    tb
}

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

fn run_icarus(scenario: &Scenario, edges: &[Edge], counter: usize) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/opencode/texture-cache-round2")
        .join(scenario.name);
    std::fs::create_dir_all(&dir).unwrap();
    let bytes = support::asset(scenario.slot, support::pattern);
    std::fs::write(
        dir.join("gpu_v2_texture_cache.v"),
        rtl::verilog(&[scenario.slot]),
    )
    .unwrap();
    let mut tb = testbench(edges, counter);
    tb.push_str(&memory_module(
        bytes.len(),
        scenario.ack_delay,
        scenario.same_edge_first,
        scenario.ready_period,
    ));
    std::fs::write(dir.join("tb.v"), tb).unwrap();
    let mut hex = String::with_capacity(bytes.len() * 3);
    for byte in &bytes {
        hex.push_str(&format!("{byte:02x}\n"));
    }
    std::fs::write(dir.join("asset.hex"), hex).unwrap();

    let compiler = std::env::var_os("IVERILOG_EXE").unwrap_or_else(|| "iverilog".into());
    let runtime = std::env::var_os("VVP_EXE").unwrap_or_else(|| "vvp".into());
    let mut compile = std::process::Command::new(&compiler);
    compile.current_dir(&dir).args([
        "-g2012",
        "-s",
        "tb",
        "-o",
        "test.vvp",
        "gpu_v2_texture_cache.v",
        "tb.v",
    ]);
    bounded(compile, &dir, "compile", Duration::from_secs(120));
    let mut run = std::process::Command::new(&runtime);
    run.current_dir(&dir).arg("test.vvp");
    bounded(run, &dir, "run", Duration::from_secs(120));
    let stdout = std::fs::read_to_string(dir.join("run.out")).unwrap();
    assert!(
        stdout.contains("PASS edges="),
        "{}: vvp did not pass: {stdout}\n{}",
        scenario.name,
        std::fs::read_to_string(dir.join("run.err")).unwrap()
    );
    println!("{}: {stdout}", scenario.name);
}

fn seam_packets() -> Vec<i128> {
    let key = TileKey {
        slot: 0,
        n: 8,
        x: 2,
        y: 1,
    };
    (0..8_u8)
        .flat_map(|ly| (0..8_u8).map(move |lx| packet(key, [lx, ly], (lx ^ ly) & 15)))
        .collect()
}

fn scenarios() -> Vec<Scenario> {
    // Exercise replacement and revisits in every set. A truncated set index
    // otherwise leaves texel goldens correct but changes future miss identities.
    let all_sets: Vec<i128> = (0..16u8)
        .flat_map(|set| {
            [0, 1, 2, 3, 0, 4, 1, 2, 3, 0].into_iter().map(move |way| {
                packet(
                    TileKey {
                        slot: 0,
                        n: 8,
                        x: (set & 3) + way * 4,
                        y: set >> 2,
                    },
                    [7, 7],
                    set,
                )
            })
        })
        .collect();
    let varied: Vec<i128> = (0..20_u8)
        .map(|i| {
            let key = TileKey {
                slot: 0,
                n: 6,
                x: (i * 3) % 8,
                y: (i * 5) % 8,
            };
            packet(key, [(i * 7) % 8, (i * 3) % 8], i & 15)
        })
        .collect();
    let same_set: Vec<i128> = [(0_u8, 0_u8), (4, 0), (8, 0), (12, 0), (0, 4)]
        .iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            packet(
                TileKey {
                    slot: 0,
                    n: 7,
                    x,
                    y,
                },
                [0, 0],
                i as u8,
            )
        })
        .collect();
    let levels: Vec<i128> = (0..=10_u8)
        .map(|n| {
            packet(
                TileKey {
                    slot: 0,
                    n,
                    x: 0,
                    y: 0,
                },
                [3, 5],
                n,
            )
        })
        .collect();
    let partial: Vec<i128> = (0..4_u8)
        .map(|i| {
            packet(
                TileKey {
                    slot: 0,
                    n: 6,
                    x: i,
                    y: 0,
                },
                [i, 0],
                i,
            )
        })
        .collect();
    let first_beat: Vec<i128> = (0..4_u8)
        .map(|i| {
            packet(
                TileKey {
                    slot: 0,
                    n: 5,
                    x: i,
                    y: i,
                },
                [i, i],
                i,
            )
        })
        .collect();
    let over64: Vec<i128> = (0..80_usize)
        .map(|i| {
            packet(
                TileKey {
                    slot: 0,
                    n: 7,
                    x: (i % 16) as u8,
                    y: (i / 16) as u8,
                },
                [(i % 8) as u8, (i % 5) as u8],
                (i % 16) as u8,
            )
        })
        .collect();
    vec![
        Scenario {
            name: "all_sets_plru_revisit",
            slot: support::slot(8, true),
            packets: all_sets,
            ready_period: 3,
            ack_delay: 2,
            same_edge_first: false,
            schedule: Schedule::EveryOther,
            out_ready_hold: false,
            abort_after_beats: None,
        },
        Scenario {
            name: "varied_pause",
            slot: support::slot(6, true),
            packets: varied,
            ready_period: 5,
            ack_delay: 5,
            same_edge_first: false,
            schedule: Schedule::PauseOnState,
            out_ready_hold: false,
            abort_after_beats: None,
        },
        Scenario {
            name: "seams_always",
            slot: support::slot(8, true),
            packets: seam_packets(),
            ready_period: 0,
            ack_delay: 0,
            same_edge_first: false,
            schedule: Schedule::Always,
            out_ready_hold: false,
            abort_after_beats: None,
        },
        Scenario {
            name: "same_set_evict",
            slot: support::slot(7, true),
            packets: same_set,
            ready_period: 0,
            ack_delay: 1,
            same_edge_first: false,
            schedule: Schedule::EveryOther,
            out_ready_hold: false,
            abort_after_beats: None,
        },
        Scenario {
            name: "levels_full_mip",
            slot: support::slot(10, true),
            packets: levels,
            ready_period: 0,
            ack_delay: 2,
            same_edge_first: false,
            schedule: Schedule::Always,
            out_ready_hold: false,
            abort_after_beats: None,
        },
        Scenario {
            name: "partial_chain",
            slot: support::slot(6, false),
            packets: partial,
            ready_period: 0,
            ack_delay: 1,
            same_edge_first: false,
            schedule: Schedule::EveryOther,
            out_ready_hold: false,
            abort_after_beats: None,
        },
        Scenario {
            name: "same_edge_first_beat",
            slot: support::slot(5, true),
            packets: first_beat.clone(),
            ready_period: 0,
            ack_delay: 0,
            same_edge_first: true,
            schedule: Schedule::Always,
            out_ready_hold: false,
            abort_after_beats: None,
        },
        Scenario {
            name: "held_request",
            slot: support::slot(5, true),
            packets: first_beat,
            ready_period: 3,
            ack_delay: 0,
            same_edge_first: false,
            schedule: Schedule::Always,
            out_ready_hold: false,
            abort_after_beats: None,
        },
        Scenario {
            name: "over_64_tiles",
            slot: support::slot(7, true),
            packets: over64,
            ready_period: 0,
            ack_delay: 0,
            same_edge_first: false,
            schedule: Schedule::Always,
            out_ready_hold: false,
            abort_after_beats: None,
        },
        Scenario {
            name: "output_backpressure",
            slot: support::slot(6, true),
            packets: (0..12_u8)
                .map(|i| {
                    packet(
                        TileKey {
                            slot: 0,
                            n: 6,
                            x: i % 8,
                            y: i / 8,
                        },
                        [i % 8, 0],
                        i,
                    )
                })
                .collect(),
            ready_period: 0,
            ack_delay: 1,
            same_edge_first: false,
            schedule: Schedule::Always,
            out_ready_hold: true,
            abort_after_beats: None,
        },
    ]
}

#[test]
fn generated_module_is_slot_specialized() {
    let slot = support::slot(6, true);
    let source = rtl::verilog(&[slot]);
    assert!(source.contains("module gpu_v2_texture_cache("));
    assert!(source.contains("input  wire        abort,"));
    assert!(source.contains("slot_base_f = 32'h00001000;"));
    assert!(!source.contains("fill_hit"));
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_matches_emu_every_edge_and_every_memory_beat() {
    let mut total = 0;
    for scenario in scenarios() {
        let (edges, count) = record(&scenario);
        assert!(count > 0, "{}: empty stimulus", scenario.name);
        total += count;
        run_icarus(&scenario, &edges, count);
    }
    assert!(
        total > 194,
        "the co-simulation must exceed the old 194-edge bench"
    );
    println!("total co-simulated edges: {total}");
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_abort_drains_accepted_burst() {
    let scenario = Scenario {
        name: "abort_mid_burst",
        slot: support::slot(5, true),
        packets: (0..2_u8)
            .map(|i| {
                packet(
                    TileKey {
                        slot: 0,
                        n: 5,
                        x: i,
                        y: i,
                    },
                    [i, i],
                    i,
                )
            })
            .collect(),
        ready_period: 0,
        ack_delay: 1,
        same_edge_first: false,
        schedule: Schedule::Always,
        out_ready_hold: false,
        abort_after_beats: Some(4),
    };
    let (edges, count) = record(&scenario);
    assert!(count > 0);
    assert!(edges.iter().any(|e| e.abort), "abort edge must be recorded");
    assert!(
        edges
            .iter()
            .filter(|e| e.abort || e.fault)
            .all(|e| !e.out_valid),
        "no output may appear after abort"
    );
    run_icarus(&scenario, &edges, count);
}
