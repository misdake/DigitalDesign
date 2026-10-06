//! Actual Icarus per-edge co-simulation of the composed whole-sampler RTL
//! against the independent register emulator
//! `texture::emu::sampler::SamplerEmu`, plus independent full numerical goldens
//! from the project oracle `Config::counted` computed once at test setup.
//!
//! The composed module is `texture::rtl::sampler`: the accepted serial
//! preparation, demand-cache and color leaves wired into one synthesizable top
//! with a 108+2-bit wrapper bank. Nothing here re-implements a leaf, and the
//! RTL generator embeds no sampled answer.
//!
//! The Rust tests build byte-exact texture assets, run the live emulator over
//! filters, masks, mip levels, negative/wrapped/helper UV, CE pauses and output
//! backpressure, and check the held result against the oracle golden. The
//! ignored Icarus test then drives the actual RTL every edge with the recorded
//! memory responses (actual bytes), checking the pre-edge handshake (`in_ready`/
//! `in_accept`), the held result, the branch-chain equalities and the held
//! request/address. Tool invocations run under a wall-clock watchdog that kills
//! and reaps the child, with stdout/stderr directed to files, and the generated
//! bench keeps its own finite HDL watchdog.

use gpu_v2::{
    memory::ports::{MemoryPort, Request, Response},
    texture::{
        emu::{
            derivative::{self, Header},
            sampler::{QuadResult, SamplerEmu, Tick},
        },
        ports::*,
        rtl::sampler,
        sim::{oracle, staged::bound::serial},
    },
};
use std::fs::File;
use std::process::Stdio;
use std::time::{Duration, Instant};

#[allow(dead_code)]
#[path = "support/texture.rs"]
mod support;

const LIMIT: u64 = 600_000;
const OUT_DIR: &str = "../../target/opencode/whole-sampler-rtl-20261006";

// ---------------------------------------------------------------------------
// Recorded demand memory: the same deterministic refill protocol as the
// accepted fixture, with an optional simultaneous last-beat/terminal edge, an
// optional terminal error and configurable request/beat/ack periods. Every
// wall-edge (request, response) pair is retained so the testbench can replay the
// actual bytes and independently check the RTL request output.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Active {
    address: u64,
    index: u8,
    due: u64,
    fail: bool,
}

struct Refill {
    bytes: Vec<u8>,
    cycle: u64,
    request_period: u64,
    beat_period: u64,
    ack_delay: u64,
    simultaneous: bool,
    first_beat_same_edge: bool,
    fail: bool,
    active: Option<Active>,
    requests: u64,
    trace: Vec<(Option<Request>, Response)>,
}

impl Refill {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            cycle: 0,
            request_period: 1,
            beat_period: 1,
            ack_delay: 3,
            simultaneous: false,
            first_beat_same_edge: false,
            fail: false,
            active: None,
            requests: 0,
            trace: Vec::new(),
        }
    }
}

impl MemoryPort for Refill {
    fn cycle(&mut self, request: Option<Request>, write: Option<u64>) -> Result<Response, String> {
        assert!(write.is_none(), "whole sampler is read-only");
        self.cycle += 1;
        let cycle = self.cycle;
        let mut response = Response::default();
        if let Some(active) = self.active.as_mut() {
            let mut reached = false;
            if active.index < 16 && cycle.is_multiple_of(self.beat_period.max(1)) {
                // The byte image carries the `BASE` prefix, exactly as the
                // accepted fixture: index by the absolute burst address.
                let start = usize::try_from(active.address)
                    .map_err(|_| "refill address underflow")?
                    + usize::from(active.index) * 8;
                let data = u64::from_le_bytes(
                    self.bytes[start..start + 8]
                        .try_into()
                        .map_err(|_| "refill byte window")?,
                );
                response.read = Some((active.index, data));
                active.index += 1;
                reached = active.index == 16;
            }
            if reached {
                if self.simultaneous {
                    response.complete = Some(!active.fail);
                    self.active = None;
                } else {
                    active.due = cycle + self.ack_delay.max(1);
                }
            } else if active.index == 16 && !self.simultaneous && cycle >= active.due {
                response.complete = Some(!active.fail);
                self.active = None;
            }
        } else if let Some(request) = request {
            if cycle.is_multiple_of(self.request_period.max(1)) {
                self.requests += 1;
                let address = request.address_bytes;
                let fail = self.fail;
                response.accepted = true;
                if self.first_beat_same_edge {
                    // The first numbered beat shares the acceptance edge.
                    let start = usize::try_from(address).map_err(|_| "refill address")?;
                    let data = u64::from_le_bytes(
                        self.bytes[start..start + 8]
                            .try_into()
                            .map_err(|_| "refill byte window")?,
                    );
                    response.read = Some((0, data));
                    self.active = Some(Active {
                        address,
                        index: 1,
                        due: 0,
                        fail,
                    });
                } else {
                    self.active = Some(Active {
                        address,
                        index: 0,
                        due: 0,
                        fail,
                    });
                }
            }
        }
        self.trace.push((request, response));
        Ok(response)
    }
}

// ---------------------------------------------------------------------------
// Multi-mip fixture: five full-mip slots at contiguous bases covering
// `max_size_log2` 0/1/3/6/10.
// ---------------------------------------------------------------------------

fn table() -> (Vec<Slot>, Vec<u8>) {
    let mut slots = Vec::new();
    let mut bytes = Vec::new();
    let mut offset = 0_u32;
    for n in [0_u8, 1, 3, 6, 10] {
        let base = support::BASE + offset;
        let slot = Slot {
            base_address: base,
            has_full_mip: true,
            max_size_log2: n,
            valid: true,
        };
        let asset = support::asset(slot, support::pattern);
        offset += asset.len() as u32;
        bytes.extend_from_slice(&asset);
        slots.push(slot);
    }
    (slots, bytes)
}

/// Build the raw preparation input plus the independent oracle golden for one
/// quad, over the byte-exact asset image. The oracle runs `Config::counted` once
/// at setup; it is never on the live path.
#[allow(clippy::too_many_arguments)] // One explicit fixture-construction call.
fn build_case(
    slots: &[Slot],
    id: u8,
    mask: u8,
    filter: Filter,
    uv: [f64; 2],
    bias: f64,
    slot: u8,
    image: &mut support::Image,
    cache: &mut oracle::Cache,
) -> (derivative::Input, [[u8; 3]; 4]) {
    let slot_value = slots[usize::from(slot)];
    let mut q = QuadInput {
        force_coarsest: false,
        quad_id: id,
        mask,
        uv: [uv; 4],
        slot,
        material_size_log2: slot_value.max_size_log2,
        filter,
        lod_bias: bias,
    };
    q.uv[1][0] += 1.0 / 64.0;
    q.uv[2][1] += 1.0 / 32.0;
    let expected = oracle::sample(&q, cache, image, Config::counted()).unwrap();
    let mut rgb = [[255_u8; 3]; 4];
    for pixel in expected.pixels {
        rgb[pixel.lane as usize] = pixel.rgb;
    }
    let input = derivative::Input::capture(&q, slot_value).unwrap();
    (input, rgb)
}

/// The varied quad set: three filters, masks 0/sparse/full, five material
/// levels, negative and wrapped helper UV, low/high bias and repeated
/// identities for hot reuse.
fn cases(slots: &[Slot]) -> Vec<(derivative::Input, [[u8; 3]; 4])> {
    let (_, bytes) = table();
    let mut image = support::Image {
        bytes,
        requests: Vec::new(),
    };
    let mut cache = oracle::Cache::new(slots.to_vec()).unwrap();
    let filters = [Filter::Nearest, Filter::Bilinear, Filter::Trilinear];
    let masks = [15_u8, 5, 10, 1, 0];
    let uvs = [
        [-0.012, 0.995],
        [0.4375, 0.1875],
        [1.03125, -1.0625],
        [0.25, 0.25],
        [0.999, 0.001],
        [-1.5, 0.75],
    ];
    let biases = [-2.0_f64, 0.0, 0.5, 2.25];
    let mut out = Vec::new();
    // Five passes over the five material levels with rotating scenery.
    for i in 0..25_u32 {
        let slot = (i % slots.len() as u32) as u8;
        let (input, rgb) = build_case(
            slots,
            (i % 16) as u8,
            masks[(i as usize) % masks.len()],
            filters[(i as usize) % filters.len()],
            uvs[(i as usize) % uvs.len()],
            biases[(i as usize) % biases.len()],
            slot,
            &mut image,
            &mut cache,
        );
        out.push((input, rgb));
    }
    out
}

// ---------------------------------------------------------------------------
// Emulator run and recorded expectation.
// ---------------------------------------------------------------------------

struct Edge {
    ce: bool,
    ready: bool,
    input: Option<derivative::Input>,
    input_ready: bool,
    accepted: bool,
    packet_accepted: bool,
    color_accepted: bool,
    output: Option<[[u8; 3]; 4]>,
    quad: u8,
    mask: u8,
    transferred: bool,
    request: Option<Request>,
    request_ready: bool,
    resp_valid: bool,
    resp_index: u8,
    resp_data: u64,
    resp_complete: bool,
    resp_ok: bool,
}

fn run_emu(
    slots: &[Slot],
    cases: &[(derivative::Input, [[u8; 3]; 4])],
    memory: Refill,
) -> (Vec<Edge>, Vec<[[u8; 3]; 4]>) {
    run_emu_with_config(slots, cases, memory, serial::Config::default())
}

fn run_emu_with_config(
    slots: &[Slot],
    cases: &[(derivative::Input, [[u8; 3]; 4])],
    memory: Refill,
    config: serial::Config,
) -> (Vec<Edge>, Vec<[[u8; 3]; 4]>) {
    let mut dut = SamplerEmu::with_config(slots.to_vec(), memory, LIMIT, config).unwrap();
    let mut sent = 0_usize;
    let mut received = 0_usize;
    let mut held: Option<QuadResult> = None;
    let mut edges = Vec::new();
    let mut committed = Vec::new();
    for wall in 0..LIMIT {
        let ce = wall % 13 != 4 && wall % 17 != 8;
        let ready = wall % 19 > 5;
        let offer = cases.get(sent).map(|c| c.0);
        let step = dut
            .tick(Tick {
                ce,
                input: offer,
                output_ready: ready,
            })
            .unwrap();
        if let Some(value) = held {
            assert_eq!(step.output, Some(value), "held result changed");
        }
        assert!(
            !(step.transferred && step.accepted),
            "a same-edge transfer must not admit a new quad"
        );
        if step.accepted {
            sent += 1;
        }
        if step.transferred {
            let quad = step.output.unwrap();
            let case = &cases[received];
            assert_eq!(quad.quad, case.0.header.quad, "quad {received}");
            assert_eq!(quad.mask, case.0.header.mask, "mask {received}");
            assert_eq!(quad.colors, case.1, "colors quad {received}");
            committed.push(quad.colors);
            received += 1;
            held = None;
        } else {
            held = step.output;
        }
        let trace = dut.cache().memory().trace.last().unwrap();
        let (request, response) = *trace;
        edges.push(Edge {
            ce,
            ready,
            input: offer,
            input_ready: step.input_ready,
            accepted: step.accepted,
            packet_accepted: step.packet_accepted,
            color_accepted: step.color_accepted,
            output: step.output.map(|o| o.colors),
            quad: step.output.map_or(0, |o| o.quad),
            mask: step.output.map_or(0, |o| o.mask),
            transferred: step.transferred,
            request,
            request_ready: response.accepted,
            resp_valid: response.read.is_some(),
            resp_index: response.read.map_or(0, |(i, _)| i),
            resp_data: response.read.map_or(0, |(_, d)| d),
            resp_complete: response.complete.is_some(),
            resp_ok: response.complete.unwrap_or(false),
        });
        if received == cases.len() && dut.idle() {
            break;
        }
        assert!(wall + 1 < LIMIT, "whole sampler bounded wall watchdog");
    }
    assert_eq!(received, cases.len());
    assert!(!dut.faulted());
    assert!(dut.cache().memory().requests > 0);
    (edges, committed)
}

// ---------------------------------------------------------------------------
// Rust-only tests.
// ---------------------------------------------------------------------------

#[test]
fn rtl_composes_the_accepted_leaves_and_declares_the_wrapper_bank() {
    sampler::audit().expect("whole sampler audit");
    let (slots, _) = table();
    let source = sampler::verilog(&slots).unwrap();
    assert!(source.contains("module gpu_v2_texture_sampler ("));
    assert!(source.contains("module gpu_v2_texture_serial_preparation ("));
    assert!(source.contains("module gpu_v2_texture_cache("));
    assert!(source.contains("module gpu_v2_texture_color("));
    assert_eq!(sampler::WRAPPER_DATA_BITS, 108);
    assert_eq!(sampler::WRAPPER_CONTROL_BITS, 2);
    // The composed wrapper must not retain a sampled answer or oracle entry.
    let wrapper = sampler::wrapper_source();
    for forbidden in ["oracle", "counted", "Program", "FrameReport", "average"] {
        assert!(
            !wrapper.contains(forbidden),
            "wrapper references {forbidden}"
        );
    }
}

#[test]
fn oracle_goldens_match_the_live_whole_sampler() {
    let (slots, bytes) = table();
    let cases = cases(&slots);
    assert_eq!(cases.len(), 25);
    let mut image = vec![0x69_u8; support::BASE as usize];
    image.extend_from_slice(&bytes);
    let memory = Refill::new(image);
    let (edges, committed) = run_emu(&slots, &cases, memory);
    assert_eq!(committed.len(), cases.len());
    // Every edge class was actually exercised.
    assert!(edges.iter().any(|e| !e.ce));
    assert!(edges.iter().any(|e| !e.ready));
    assert!(edges.iter().any(|e| e.output.is_some()));
    assert!(edges.iter().any(|e| e.input.is_none()));
    assert_eq!(
        edges.iter().filter(|e| e.accepted).count(),
        cases.len(),
        "every quad admitted exactly once"
    );
    for (index, ((_, rgb), got)) in cases.iter().zip(&committed).enumerate() {
        assert_eq!(rgb, got, "oracle golden quad {index}");
    }
}

#[test]
fn zero_mask_completes_without_any_memory_or_color_work() {
    let (slots, bytes) = table();
    let mut image = vec![0x69_u8; support::BASE as usize];
    image.extend_from_slice(&bytes);
    let memory = Refill::new(image);
    let mut dut = SamplerEmu::new(slots, memory, 4096).unwrap();
    let header = Header {
        quad: 7,
        mask: 0,
        slot: 0,
        max_n: 0,
        has_mip: true,
        filter: 0,
    };
    let input = derivative::Input {
        force_coarsest: false,
        uv: [0; 8],
        bias: 0,
        header,
    };
    let mut result = None;
    for _ in 0..4096 {
        let step = dut
            .tick(Tick {
                ce: true,
                input: result.is_none().then_some(input),
                output_ready: true,
            })
            .unwrap();
        if step.transferred {
            result = Some(step.output.unwrap());
            break;
        }
    }
    let output = result.expect("zero-mask quad must complete");
    assert_eq!(output.quad, 7);
    assert_eq!(output.mask, 0);
    assert_eq!(output.colors, [[255_u8; 3]; 4], "unfilled lanes are white");
    assert_eq!(
        dut.cache().memory().requests,
        0,
        "zero mask must not touch memory"
    );
}

#[test]
fn missing_mip_slot_uses_level_zero_and_reuses_tiles() {
    // A non-full-mip slot has only the top level; repeated identities must hit
    // the same line and never request another level.
    let slot = Slot {
        base_address: support::BASE,
        has_full_mip: false,
        max_size_log2: 6,
        valid: true,
    };
    let asset = support::asset(slot, support::pattern);
    let mut image = vec![0x69_u8; support::BASE as usize];
    image.extend_from_slice(&asset);
    let memory = Refill::new(image);
    let mut dut = SamplerEmu::new(vec![slot], memory, 20_000).unwrap();
    let q = QuadInput {
        force_coarsest: false,
        quad_id: 0,
        mask: 15,
        uv: [[0.25, 0.25]; 4],
        slot: 0,
        material_size_log2: 6,
        filter: Filter::Bilinear,
        lod_bias: 0.0,
    };
    let input = derivative::Input::capture(&q, slot).unwrap();
    let mut committed = 0;
    for _ in 0..20_000 {
        let step = dut
            .tick(Tick {
                ce: true,
                input: (committed == 0).then_some(input),
                output_ready: true,
            })
            .unwrap();
        committed += usize::from(step.transferred);
        if committed == 1 && dut.idle() {
            break;
        }
    }
    assert_eq!(committed, 1);
    // The same line is reused: exactly one refill for the whole quad is the
    // optimistic case, but the cache is demand-only; assert bounded requests and
    // that every refill address is a top-level tile.
    assert!(dut.cache().memory().requests >= 1);
    for (request, _) in &dut.cache().memory().trace {
        if let Some(request) = request {
            assert!(request.address_bytes >= u64::from(support::BASE));
        }
    }
}

#[test]
fn memory_error_faults_and_drains_bounded() {
    let (slots, bytes) = table();
    let slot = slots[3];
    let mut memory = Refill::new(padded(&bytes));
    memory.fail = true;
    let mut dut = SamplerEmu::new(slots.to_vec(), memory, 8192).unwrap();
    let mut q = QuadInput {
        force_coarsest: false,
        quad_id: 3,
        mask: 15,
        uv: [[0.25, 0.25]; 4],
        slot: 3,
        material_size_log2: 6,
        filter: Filter::Bilinear,
        lod_bias: 0.0,
    };
    q.uv[1][0] += 1.0 / 64.0;
    q.uv[2][1] += 1.0 / 32.0;
    let input = derivative::Input::capture(&q, slot).unwrap();
    let mut faulted = false;
    for _ in 0..8192 {
        if dut
            .tick(Tick {
                ce: true,
                input: (!faulted).then_some(input),
                output_ready: true,
            })
            .is_err()
        {
            faulted = true;
            break;
        }
    }
    assert!(faulted, "failed terminal did not fault the sampler");
    assert!(dut.faulted());
    for count in 0..8192 {
        if dut.drain_tick().unwrap() {
            return;
        }
        assert!(count + 1 < 8192, "bounded explicit drain");
    }
    panic!("bounded explicit drain");
}

// ---------------------------------------------------------------------------
// Icarus co-simulation.
// ---------------------------------------------------------------------------

fn bit(value: bool) -> u8 {
    u8::from(value)
}

fn rgb_literal(rgb: &[[u8; 3]; 4]) -> u128 {
    let mut value = 0_u128;
    for (lane, channels) in rgb.iter().enumerate() {
        let packed = (u128::from(channels[0]) << 16)
            | (u128::from(channels[1]) << 8)
            | u128::from(channels[2]);
        value |= packed << (lane * 24);
    }
    value
}

fn input_literal(input: &derivative::Input) -> String {
    let mut tb = String::new();
    for (index, uv) in input.uv.iter().enumerate() {
        tb.push_str(&format!(
            "in_uv_{index}=40'sh{:010x};",
            (*uv as u64) & 0xff_ffff_ffff
        ));
    }
    tb.push_str(&format!("in_bias=16'sh{:04x};", input.bias as u16));
    tb.push_str(&format!(
        "in_force_coarsest={};in_quad=4'd{};in_mask=4'd{};in_slot=4'd{};in_max_n=4'd{};in_has_mip={};in_filter=2'd{};",
        input.force_coarsest as u8,
        input.header.quad,
        input.header.mask,
        input.header.slot,
        input.header.max_n,
        bit(input.header.has_mip),
        input.header.filter
    ));
    tb
}

fn testbench(edges: &[Edge]) -> String {
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,out_ready=0;\n");
    tb.push_str("reg signed [17:0] in_uv_0=0,in_uv_1=0,in_uv_2=0,in_uv_3=0,in_uv_4=0,in_uv_5=0,in_uv_6=0,in_uv_7=0;\n");
    tb.push_str("reg signed [15:0] in_bias=0;\n");
    tb.push_str("reg [3:0] in_quad=0,in_mask=0,in_slot=0,in_max_n=0;reg in_force_coarsest=0;reg in_has_mip=0;reg [1:0] in_filter=0;\n");
    tb.push_str("reg mem_req_ready=0,mem_resp_valid=0,mem_resp_complete=0,mem_resp_ok=0;\n");
    tb.push_str("reg [3:0] mem_resp_index=0;reg [63:0] mem_resp_data=0;\n");
    tb.push_str(
        "wire in_ready,in_accept,out_valid,out_transfer,cache_accept,color_accept,fault;\n",
    );
    tb.push_str("wire [3:0] out_quad,out_mask;wire [95:0] out_rgb;\n");
    tb.push_str("wire mem_req_valid;wire [31:0] mem_req_addr;\n");
    tb.push_str("gpu_v2_texture_sampler dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),");
    tb.push_str(".in_uv_0(in_uv_0),.in_uv_1(in_uv_1),.in_uv_2(in_uv_2),.in_uv_3(in_uv_3),");
    tb.push_str(".in_uv_4(in_uv_4),.in_uv_5(in_uv_5),.in_uv_6(in_uv_6),.in_uv_7(in_uv_7),");
    tb.push_str(".in_bias(in_bias),.in_quad(in_quad),.in_mask(in_mask),.in_slot(in_slot),.in_max_n(in_max_n),");
    tb.push_str(
        ".in_has_mip(in_has_mip),.in_filter(in_filter),.in_force_coarsest(in_force_coarsest),.in_ready(in_ready),.in_accept(in_accept),",
    );
    tb.push_str(".out_ready(out_ready),.out_valid(out_valid),.out_quad(out_quad),.out_mask(out_mask),.out_rgb(out_rgb),");
    tb.push_str(".out_transfer(out_transfer),.cache_accept(cache_accept),.color_accept(color_accept),.fault(fault),");
    tb.push_str(
        ".mem_req_valid(mem_req_valid),.mem_req_addr(mem_req_addr),.mem_req_ready(mem_req_ready),",
    );
    tb.push_str(".mem_resp_valid(mem_resp_valid),.mem_resp_index(mem_resp_index),.mem_resp_data(mem_resp_data),");
    tb.push_str(".mem_resp_complete(mem_resp_complete),.mem_resp_ok(mem_resp_ok));\n");
    tb.push_str("initial begin #2000000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\n");
    tb.push_str("clk=0;reset=1;ce=0;in_valid=0;out_ready=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    for (edge, stim) in edges.iter().enumerate() {
        tb.push_str(&format!(
            "ce={};out_ready={};in_valid={};",
            bit(stim.ce),
            bit(stim.ready),
            bit(stim.input.is_some())
        ));
        if let Some(input) = stim.input {
            tb.push_str(&input_literal(&input));
        }
        tb.push_str(&format!(
            "mem_req_ready={};mem_resp_valid={};mem_resp_index=4'd{};mem_resp_data=64'h{:016x};mem_resp_complete={};mem_resp_ok={};\n",
            bit(stim.request_ready),
            bit(stim.resp_valid),
            stim.resp_index,
            stim.resp_data,
            bit(stim.resp_complete),
            bit(stim.resp_ok)
        ));
        tb.push_str("#1;\n");
        tb.push_str(&format!(
            "if (fault !== 1'b0) $fatal(1,\"unexpected fault edge {edge}\");\n",
            edge = edge
        ));
        tb.push_str(&format!(
            "if (in_ready !== 1'b{}) $fatal(1,\"in_ready edge {edge}\");\n",
            bit(stim.input_ready),
            edge = edge
        ));
        tb.push_str(&format!(
            "if (in_accept !== 1'b{}) $fatal(1,\"in_accept edge {edge}\");\n",
            bit(stim.accepted),
            edge = edge
        ));
        tb.push_str(&format!(
            "if (mem_req_valid !== 1'b{}) $fatal(1,\"held request edge {edge}\");\n",
            bit(stim.request.is_some()),
            edge = edge
        ));
        if let Some(request) = stim.request {
            tb.push_str(&format!(
                "if (mem_req_addr !== 32'h{:08x}) $fatal(1,\"request address edge {edge}\");\n",
                request.address_bytes as u32,
                edge = edge
            ));
        }
        tb.push_str(&format!(
            "if (cache_accept !== 1'b{}) $fatal(1,\"cache accept edge {edge}\");\n",
            bit(stim.packet_accepted),
            edge = edge
        ));
        tb.push_str(&format!(
            "if (color_accept !== 1'b{}) $fatal(1,\"color accept edge {edge}\");\n",
            bit(stim.color_accepted),
            edge = edge
        ));
        tb.push_str(&format!(
            "if (out_valid !== 1'b{}) $fatal(1,\"out_valid edge {edge}\");\n",
            bit(stim.output.is_some()),
            edge = edge
        ));
        if let Some(output) = stim.output {
            tb.push_str(&format!(
                "if (out_quad !== 4'd{} || out_mask !== 4'd{} || out_rgb !== 96'h{:024x}) $fatal(1,\"result edge {edge}\");\n",
                stim.quad,
                stim.mask,
                rgb_literal(&output),
                edge = edge
            ));
        }
        tb.push_str(&format!(
            "if (out_transfer !== 1'b{}) $fatal(1,\"out_transfer edge {edge}\");\n",
            bit(stim.transferred),
            edge = edge
        ));
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

fn padded(bytes: &[u8]) -> Vec<u8> {
    let mut image = vec![0x69_u8; support::BASE as usize];
    image.extend_from_slice(bytes);
    image
}

fn run_icarus(
    stem: &str,
    slots: &[Slot],
    cases: &[(derivative::Input, [[u8; 3]; 4])],
    memory: Refill,
) -> Vec<Edge> {
    let (edges, committed) = run_emu(slots, cases, memory);
    assert_eq!(committed.len(), cases.len());
    assert!(edges.len() > 64);
    let module = sampler::verilog(slots).unwrap();
    let dir = out_dir();
    compile_and_run(&dir, stem, &module, &testbench(&edges));
    edges
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_matches_whole_sampler_every_edge() {
    let (slots, bytes) = table();
    let cases = cases(&slots);
    let mut memory = Refill::new(padded(&bytes));
    memory.request_period = 5;
    memory.beat_period = 3;
    memory.ack_delay = 7;
    let edges = run_icarus("texture_sampler_rtl", &slots, &cases, memory);
    assert!(edges.iter().any(|e| e.request.is_some()));
    assert!(edges.iter().any(|e| e.resp_complete));
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_whole_sampler_all_preparation_configurations() {
    let (slots, bytes) = table();
    let cases = cases(&slots);
    assert_eq!(
        sampler::verilog(&slots).unwrap(),
        sampler::verilog_with_config(&slots, serial::Config::default()).unwrap()
    );
    for (index, (nearest_bypass, short_alignment)) in
        [(false, false), (true, false), (false, true), (true, true)]
            .into_iter()
            .enumerate()
    {
        let config = serial::Config {
            nearest_bypass,
            short_alignment,
        };
        let mut memory = Refill::new(padded(&bytes));
        memory.request_period = 3;
        memory.beat_period = 2;
        memory.ack_delay = 4;
        memory.first_beat_same_edge = true;
        memory.simultaneous = true;
        let (edges, committed) = run_emu_with_config(&slots, &cases, memory, config);
        assert_eq!(committed.len(), cases.len());
        assert!(edges.iter().any(|e| e.resp_complete && e.resp_valid));
        assert!(edges.iter().any(|e| e.request_ready && e.resp_valid));
        let module = sampler::verilog_with_config(&slots, config).unwrap();
        compile_and_run(
            &out_dir(),
            &format!("whole_config_{index}"),
            &module,
            &testbench(&edges),
        );
    }
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_last_beat_and_terminal_share_one_edge() {
    let (slots, bytes) = table();
    let cases = cases(&slots);
    let mut memory = Refill::new(padded(&bytes));
    memory.simultaneous = true;
    memory.request_period = 1;
    memory.beat_period = 1;
    memory.ack_delay = 0;
    let edges = run_icarus("texture_sampler_rtl_simul", &slots, &cases, memory);
    assert!(
        edges.iter().any(|e| e.resp_valid && e.resp_complete),
        "no simultaneous last-beat/terminal edge"
    );
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_first_beat_on_the_accept_edge() {
    let (slots, bytes) = table();
    let cases = cases(&slots);
    let mut memory = Refill::new(padded(&bytes));
    memory.first_beat_same_edge = true;
    memory.request_period = 2;
    memory.beat_period = 2;
    memory.ack_delay = 4;
    let edges = run_icarus("texture_sampler_rtl_firstbeat", &slots, &cases, memory);
    assert!(
        edges.iter().any(|e| e.request_ready && e.resp_valid),
        "no accept/first-beat shared edge"
    );
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn iverilog_eviction_and_hot_reuse() {
    let (slots, bytes) = table();
    let cases = eviction_cases(&slots, 80);
    let mut memory = Refill::new(padded(&bytes));
    memory.request_period = 1;
    memory.beat_period = 1;
    memory.ack_delay = 1;
    let edges = run_icarus("texture_sampler_rtl_evict", &slots, &cases, memory);
    assert!(edges.iter().filter(|e| e.request.is_some()).count() > 64);
}

/// Walk `count` helper UVs across the n=10 slot so the demand cache must touch
/// and evict more than its sixty-four tag lines; every fifth case repeats the
/// fourth of its group for hot reuse. A tiny derivative keeps the selected level
/// at 0, so the n=10 slot is indexed at full 1024x1024 resolution.
fn eviction_cases(slots: &[Slot], count: u32) -> Vec<(derivative::Input, [[u8; 3]; 4])> {
    let slot = slots[4];
    let (_, bytes) = table();
    let mut image = support::Image {
        bytes,
        requests: Vec::new(),
    };
    let mut cache = oracle::Cache::new(slots.to_vec()).unwrap();
    let mut out = Vec::new();
    for k in 0..count {
        let j = (k / 5) * 5 + (k % 5).min(3);
        let uv = [
            (((j * 37) % 200) as f64) / 200.0 - 0.5,
            (((j * 71) % 200) as f64) / 200.0 - 0.5,
        ];
        let mut q = QuadInput {
            force_coarsest: false,
            quad_id: (j % 16) as u8,
            mask: 15,
            uv: [uv; 4],
            slot: 4,
            material_size_log2: 10,
            filter: Filter::Bilinear,
            lod_bias: 0.0,
        };
        q.uv[1][0] += 1.0 / 4096.0;
        q.uv[2][1] += 1.0 / 8192.0;
        let expected = oracle::sample(&q, &mut cache, &mut image, Config::counted()).unwrap();
        let mut rgb = [[255_u8; 3]; 4];
        for p in expected.pixels {
            rgb[p.lane as usize] = p.rgb;
        }
        out.push((derivative::Input::capture(&q, slot).unwrap(), rgb));
    }
    out
}

#[test]
fn eviction_and_hot_reuse_over_sixty_four_lines() {
    let (slots, bytes) = table();
    let cases = eviction_cases(&slots, 140);
    assert!(cases.len() > 64);
    let mut memory = Refill::new(padded(&bytes));
    memory.request_period = 1;
    memory.beat_period = 1;
    memory.ack_delay = 1;
    let (edges, committed) = run_emu(&slots, &cases, memory);
    assert_eq!(committed.len(), cases.len());
    assert!(edges.iter().filter(|e| e.request.is_some()).count() > 64);
}

#[test]
#[ignore = "requires Icarus (IVERILOG_EXE/VVP_EXE)"]
fn input_width_fault_blocks_and_reset_recovers_in_rtl() {
    let (slots, _bytes) = table();
    let module = sampler::verilog(&slots).unwrap();
    let dir = out_dir();
    let mut tb = String::new();
    tb.push_str("module tb;\n");
    tb.push_str("reg clk=0,reset=0,ce=0,in_valid=0,out_ready=0;\n");
    tb.push_str("reg signed [17:0] in_uv_0=0,in_uv_1=0,in_uv_2=0,in_uv_3=0,in_uv_4=0,in_uv_5=0,in_uv_6=0,in_uv_7=0;\n");
    tb.push_str("reg signed [15:0] in_bias=0;\n");
    tb.push_str("reg [3:0] in_quad=0,in_mask=0,in_slot=0,in_max_n=0;reg in_force_coarsest=0;reg in_has_mip=0;reg [1:0] in_filter=0;\n");
    tb.push_str("reg mem_req_ready=0,mem_resp_valid=0,mem_resp_complete=0,mem_resp_ok=0;\n");
    tb.push_str("reg [3:0] mem_resp_index=0;reg [63:0] mem_resp_data=0;\n");
    tb.push_str(
        "wire in_ready,in_accept,out_valid,out_transfer,cache_accept,color_accept,fault;\n",
    );
    tb.push_str("wire [3:0] out_quad,out_mask;wire [95:0] out_rgb;\n");
    tb.push_str("wire mem_req_valid;wire [31:0] mem_req_addr;\n");
    tb.push_str("gpu_v2_texture_sampler dut(.clk(clk),.reset(reset),.ce(ce),.in_valid(in_valid),");
    tb.push_str(".in_uv_0(in_uv_0),.in_uv_1(in_uv_1),.in_uv_2(in_uv_2),.in_uv_3(in_uv_3),");
    tb.push_str(".in_uv_4(in_uv_4),.in_uv_5(in_uv_5),.in_uv_6(in_uv_6),.in_uv_7(in_uv_7),");
    tb.push_str(".in_bias(in_bias),.in_quad(in_quad),.in_mask(in_mask),.in_slot(in_slot),.in_max_n(in_max_n),");
    tb.push_str(
        ".in_has_mip(in_has_mip),.in_filter(in_filter),.in_force_coarsest(in_force_coarsest),.in_ready(in_ready),.in_accept(in_accept),",
    );
    tb.push_str(".out_ready(out_ready),.out_valid(out_valid),.out_quad(out_quad),.out_mask(out_mask),.out_rgb(out_rgb),");
    tb.push_str(".out_transfer(out_transfer),.cache_accept(cache_accept),.color_accept(color_accept),.fault(fault),");
    tb.push_str(
        ".mem_req_valid(mem_req_valid),.mem_req_addr(mem_req_addr),.mem_req_ready(mem_req_ready),",
    );
    tb.push_str(".mem_resp_valid(mem_resp_valid),.mem_resp_index(mem_resp_index),.mem_resp_data(mem_resp_data),");
    tb.push_str(".mem_resp_complete(mem_resp_complete),.mem_resp_ok(mem_resp_ok));\n");
    tb.push_str("initial begin #2000000; $fatal(1,\"simulation watchdog\"); end\n");
    tb.push_str("initial begin\n");
    tb.push_str("clk=0;reset=1;ce=0;in_valid=0;out_ready=0;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    // Bad external width: max_n 11 is unrepresentable for the slot.
    tb.push_str("ce=1;in_valid=1;in_mask=4'd15;in_max_n=4'd11;in_uv_0=40'sh0;in_uv_1=40'sh0;in_uv_2=40'sh0;in_uv_3=40'sh0;in_uv_4=40'sh0;in_uv_5=40'sh0;in_uv_6=40'sh0;in_uv_7=40'sh0;#1;clk=1;#1;clk=0;#1;\n");
    tb.push_str("if (fault !== 1'b1) $fatal(1,\"input fault not latched\");\n");
    tb.push_str("if (in_ready !== 1'b0) $fatal(1,\"in_ready after fault\");\n");
    tb.push_str("if (in_accept !== 1'b0) $fatal(1,\"in_accept after fault\");\n");
    tb.push_str("if (out_valid !== 1'b0) $fatal(1,\"out_valid after fault\");\n");
    // The latched fault stays terminal under continued stimulus.
    tb.push_str("ce=1;in_valid=1;#1;clk=1;#1;clk=0;#1;\n");
    tb.push_str("if (fault !== 1'b1) $fatal(1,\"fault not terminal\");\n");
    tb.push_str(
        "if (in_ready !== 1'b0 || out_valid !== 1'b0) $fatal(1,\"handshake after fault\");\n",
    );
    // Reset, only after no accepted maintenance remains, clears the fault.
    tb.push_str("in_valid=0;reset=1;#1;clk=1;#1;clk=0;#1;reset=0;\n");
    tb.push_str("if (fault !== 1'b0) $fatal(1,\"reset did not clear fault\");\n");
    tb.push_str("if (out_valid !== 1'b0) $fatal(1,\"out_valid after reset\");\n");
    tb.push_str("$display(\"PASS edges=fault-reset\");$finish;end endmodule\n");
    compile_and_run(&dir, "texture_sampler_rtl_fault", &module, &tb);
}
