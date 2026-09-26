//! Transaction-level trace for GPU emulator-vs-RTL differential testing.
//!
//! Both the Rust model (`GpuCore` + `HostGpuMemory`) and the Icarus RTL run
//! (`gpu.v` + `gpu_tb.v`) emit the same ordered transaction trace for the same
//! scenario sequence; [`compare_traces`] reports the first divergence.
//!
//! The two memory responders have intentionally different cycle timing
//! (recovery cycles on the Rust side, write-data backpressure in the
//! testbench), so the comparison never looks at cycle timing or cross-port
//! interleaving. It compares the ordered event sequence of each memory port
//! (`ro`, `fb_r`, `fb_w`) plus the globally ordered completion points (`DONE`)
//! within each `SCENE`:
//!
//! ```text
//! GPU <seq> SCENE <name>
//! GPU <seq> REQ <port> <R|W> <addr-6hex> <lines>
//! GPU <seq> RDAT <port> <beat> <w0> <w1> <w2> <w3>   (16-bit hex, low first)
//! GPU <seq> WDAT <port> <beat> <w0> <w1> <w2> <w3>
//! GPU <seq> RESP <port> <error 0|1>
//! GPU <seq> DONE submission <executed_count>
//! GPU <seq> DONE draw <tile_count>
//! ```
//!
//! `REQ` is recorded on the `request_valid && request_ready` accepting edge,
//! `RDAT` on every read response beat, `WDAT` on every accepted write beat
//! (`write_data_ready` for every beat of a long write), `RESP` on
//! `response_valid && response_last`, `DONE submission` on an
//! `executed_count` change, and `DONE draw` when a nonempty draw hands control
//! back from tile stepping to decode. The RTL side prints the identical lines
//! with `$display` from the testbench responder and `dut` state monitors.

use super::{GpuCore, GpuDeviceBus, GpuMemoryBus, GpuOutputs, HostGpuMemory, Phase};
use crate::gpu_device::{
    GPU_CMD_BASE_HIGH, GPU_CMD_BASE_LOW, GPU_CMD_WORDS_HIGH, GPU_CMD_WORDS_LOW, GPU_CONTROL,
    GPU_CONTROL_CLEAR_ERRORS, GPU_DEVICE, GPU_DRAW_FLAG_GRADIENT_XY, GPU_FAKE_DRAW_QWORDS,
    GPU_LOAD_OP_CLEAR, GPU_LOAD_OP_LOAD, GPU_OPCODE_END, GPU_OPCODE_FAKE_DRAW,
    GPU_OPCODE_SET_TARGET, GPU_STATUS, GPU_STATUS_COMMAND_ERROR, GPU_SUBMIT,
};
use crate::layout::{FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD, FRAMEBUFFER_WORDS};

/// One of the three GPU memory masters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TracePort {
    Ro,
    FbR,
    FbW,
}

impl TracePort {
    fn name(self) -> &'static str {
        match self {
            Self::Ro => "ro",
            Self::FbR => "fb_r",
            Self::FbW => "fb_w",
        }
    }
}

/// One transaction-level trace event; the text rendering is the wire format
/// shared with the RTL testbench.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpuTraceEvent {
    Scene(String),
    Req {
        port: TracePort,
        write: bool,
        address: u32,
        lines: u8,
    },
    Rdat {
        port: TracePort,
        beat: u8,
        value: u64,
    },
    Wdat {
        port: TracePort,
        beat: u8,
        value: u64,
    },
    Resp {
        port: TracePort,
        error: bool,
    },
    DoneSubmission {
        executed_count: u16,
    },
    DoneDraw {
        tile_count: u16,
    },
    RasterRetire {
        epoch: u16,
        triangle: u32,
    },
}

impl GpuTraceEvent {
    /// The event body without the `GPU <seq>` prefix; this is the normalized
    /// form the comparator works on.
    pub(crate) fn body(&self) -> String {
        match self {
            Self::Scene(name) => format!("SCENE {name}"),
            Self::RasterRetire { epoch, triangle } => format!("RACK {epoch} {triangle}"),
            Self::Req {
                port,
                write,
                address,
                lines,
            } => {
                let kind = if *write { "W" } else { "R" };
                format!("REQ {} {} {:06x} {}", port.name(), kind, address, lines)
            }
            Self::Rdat { port, beat, value } | Self::Wdat { port, beat, value } => {
                let kind = if matches!(self, Self::Rdat { .. }) {
                    "RDAT"
                } else {
                    "WDAT"
                };
                format!(
                    "{} {} {} {:04x} {:04x} {:04x} {:04x}",
                    kind,
                    port.name(),
                    beat,
                    value & 0xffff,
                    (value >> 16) & 0xffff,
                    (value >> 32) & 0xffff,
                    (value >> 48) & 0xffff
                )
            }
            Self::Resp { port, error } => {
                format!("RESP {} {}", port.name(), u8::from(*error))
            }
            Self::DoneSubmission { executed_count } => {
                format!("DONE submission {executed_count}")
            }
            Self::DoneDraw { tile_count } => format!("DONE draw {tile_count}"),
        }
    }
}

/// Renders events to the shared text format with a global sequence number.
fn render_trace(events: &[GpuTraceEvent]) -> Vec<String> {
    events
        .iter()
        .enumerate()
        .map(|(seq, event)| format!("GPU {seq} {}", event.body()))
        .collect()
}

/// Cycle-hook trace collector for the Rust side. `on_cycle` must see the same
/// `(mem, outputs)` pair the core consumes, before the clock edge.
#[derive(Default)]
pub(crate) struct GpuTraceCollector {
    events: Vec<GpuTraceEvent>,
    ro_beat: u8,
    fb_r_beat: u8,
    fb_w_active: bool,
    fb_w_beat: u8,
    fb_w_beats: u8,
    prev_executed_count: u16,
    prev_was_tile_step: bool,
}

impl GpuTraceCollector {
    #[cfg(test)]
    pub(crate) fn events(&self) -> &[GpuTraceEvent] {
        &self.events
    }

    pub(crate) fn scene(&mut self, name: &str) {
        self.events.push(GpuTraceEvent::Scene(name.to_string()));
    }

    /// A DUT reset clears `executed_count` and every in-flight edge tracker.
    pub(crate) fn on_reset(&mut self) {
        self.ro_beat = 0;
        self.fb_r_beat = 0;
        self.fb_w_active = false;
        self.fb_w_beat = 0;
        self.fb_w_beats = 0;
        self.prev_executed_count = 0;
        self.prev_was_tile_step = false;
    }

    pub(crate) fn on_cycle(&mut self, mem: &GpuMemoryBus, out: &GpuOutputs, core: &GpuCore) {
        // `gpu_ro`: 32-byte command/tile-list reads.
        if out.ro_request_valid && mem.ro_request_ready {
            self.events.push(GpuTraceEvent::Req {
                port: TracePort::Ro,
                write: out.ro_write,
                address: out.ro_address,
                lines: out.ro_line_count_minus_1 + 1,
            });
            self.ro_beat = 0;
        }
        if mem.ro_response_valid {
            self.events.push(GpuTraceEvent::Rdat {
                port: TracePort::Ro,
                beat: self.ro_beat,
                value: mem.ro_read_data,
            });
            self.ro_beat += 1;
            if mem.ro_response_last {
                self.events.push(GpuTraceEvent::Resp {
                    port: TracePort::Ro,
                    error: mem.ro_error,
                });
            }
        }

        // `gpu_fb_r`: 128-byte tile refills.
        if out.fb_r_request_valid && mem.fb_r_request_ready {
            self.events.push(GpuTraceEvent::Req {
                port: TracePort::FbR,
                write: out.fb_r_write,
                address: out.fb_r_address,
                lines: out.fb_r_line_count_minus_1 + 1,
            });
            self.fb_r_beat = 0;
        }
        if mem.fb_r_response_valid {
            self.events.push(GpuTraceEvent::Rdat {
                port: TracePort::FbR,
                beat: self.fb_r_beat,
                value: mem.fb_r_read_data,
            });
            self.fb_r_beat += 1;
            if mem.fb_r_response_last {
                self.events.push(GpuTraceEvent::Resp {
                    port: TracePort::FbR,
                    error: mem.fb_r_error,
                });
            }
        }

        // `gpu_fb_w`: long tile cleans consume every beat on data-ready.
        // Only legacy fixed-line writes capture beat zero on acceptance.
        if out.fb_w_request_valid && mem.fb_w_request_ready {
            self.events.push(GpuTraceEvent::Req {
                port: TracePort::FbW,
                write: out.fb_w_write,
                address: out.fb_w_address,
                lines: out.fb_w_line_count_minus_1 + 1,
            });
            if out.fb_w_line_count_minus_1 == 0 {
                self.events.push(GpuTraceEvent::Wdat {
                    port: TracePort::FbW,
                    beat: 0,
                    value: out.fb_w_write_data,
                });
            }
            self.fb_w_active = true;
            self.fb_w_beats = (u32::from(out.fb_w_line_count_minus_1) + 1) as u8 * 4;
            self.fb_w_beat = if out.fb_w_line_count_minus_1 == 0 {
                1
            } else {
                0
            };
        }
        if self.fb_w_active && self.fb_w_beat < self.fb_w_beats && mem.fb_w_write_data_ready {
            self.events.push(GpuTraceEvent::Wdat {
                port: TracePort::FbW,
                beat: self.fb_w_beat,
                value: out.fb_w_write_data,
            });
            self.fb_w_beat += 1;
        }
        if mem.fb_w_response_valid && mem.fb_w_response_last {
            self.events.push(GpuTraceEvent::Resp {
                port: TracePort::FbW,
                error: mem.fb_w_error,
            });
            self.fb_w_active = false;
        }

        // Marker ACK precedes END's external-memory visibility fence.
        if core.raster_retire_ack {
            self.events.push(GpuTraceEvent::RasterRetire {
                epoch: core.draw_epoch,
                triangle: 0,
            });
        }
        // Completion points.
        let executed_count = core.executed_count();
        if executed_count != self.prev_executed_count {
            self.events
                .push(GpuTraceEvent::DoneSubmission { executed_count });
            self.prev_executed_count = executed_count;
        }
        // A nonempty draw completes when tile stepping hands control back to
        // decode; zero-tile draws never enter tile stepping on either side.
        let tile_step = core.phase == Phase::TileStep;
        if self.prev_was_tile_step && core.phase == Phase::Decode {
            self.events.push(GpuTraceEvent::DoneDraw {
                tile_count: core.draw_tile_count,
            });
        }
        self.prev_was_tile_step = tile_step;
    }
}

// ---------------------------------------------------------------------------
// Scenario driver: replays the exact `gpu_tb.v` scenario sequence (A through
// G, including the error cases) against the Rust model.
// ---------------------------------------------------------------------------

const CMD_BASE: u32 = 0x100;
const LIST_BASE: u32 = 0x400;
const LIST_STRIDE: u32 = 16;
/// Matches `MEM_WORDS` in `gpu_tb.v`.
const MEMORY_WORDS: usize = 0x240000;
const FB_GUARD: u32 = FRAMEBUFFER_A_BASE_WORD + FRAMEBUFFER_WORDS;
/// Per-submission run bound; the RTL side bounds the whole run at 4M cycles.
const MAX_SCENARIO_CYCLES: usize = 500_000;

fn set_target(base: u32) -> u64 {
    u64::from(GPU_OPCODE_SET_TARGET) | (1u64 << 8) | (u64::from(base) << 32)
}

#[allow(clippy::too_many_arguments)]
fn fake_draw(
    list_addr: u32,
    tile_count: u16,
    load_op: u16,
    clear_color: u16,
    draw_color: u16,
    row_mask: u16,
    flags: u16,
) -> [u64; 3] {
    let arg0 = u64::from(tile_count) | (u64::from(load_op) << 16);
    let payload1 = u64::from(clear_color)
        | (u64::from(draw_color) << 16)
        | (u64::from(row_mask) << 32)
        | (u64::from(flags) << 48);
    [
        u64::from(GPU_OPCODE_FAKE_DRAW) | (u64::from(GPU_FAKE_DRAW_QWORDS) << 8) | (arg0 << 32),
        u64::from(list_addr),
        payload1,
    ]
}

const END: u64 = GPU_OPCODE_END as u64 | (1u64 << 8);

struct TraceHarness {
    core: GpuCore,
    memory: HostGpuMemory,
    words: Vec<u16>,
    trace: GpuTraceCollector,
}

impl TraceHarness {
    fn new() -> Self {
        Self {
            core: GpuCore::default(),
            memory: HostGpuMemory::default(),
            words: vec![0u16; MEMORY_WORDS],
            trace: GpuTraceCollector::default(),
        }
    }

    /// Mirrors the testbench `do_reset`: the DUT and the responder reset, the
    /// memory contents persist.
    fn reset_dut(&mut self) {
        self.core = GpuCore::default();
        self.memory = HostGpuMemory::default();
        self.trace.on_reset();
    }

    fn cycle(&mut self, read: Option<u8>, write: Option<(u8, u16)>) -> u16 {
        let (read_enable, channel, write_enable, write_data) = match (read, write) {
            (Some(channel), None) => (true, channel, false, 0),
            (None, Some((channel, data))) => (false, channel, true, data),
            _ => (false, 0, false, 0),
        };
        let dev = GpuDeviceBus {
            index: GPU_DEVICE,
            channel,
            read_enable,
            write_enable,
            write_data,
        };
        let mem = self.memory.inputs(&self.words);
        let outputs = self.core.combine(dev, mem);
        self.trace.on_cycle(&mem, &outputs, &self.core);
        self.core.advance(false, dev, mem);
        self.memory.advance(&outputs, &mut self.words);
        outputs.read_data
    }

    fn write(&mut self, channel: u8, value: u16) {
        self.cycle(None, Some((channel, value)));
    }

    fn read(&mut self, channel: u8) -> u16 {
        self.cycle(Some(channel), None)
    }

    fn set_qwords(&mut self, base: u32, qwords: &[u64]) {
        let mut cursor = base as usize;
        for qword in qwords {
            self.words[cursor] = *qword as u16;
            self.words[cursor + 1] = (*qword >> 16) as u16;
            self.words[cursor + 2] = (*qword >> 32) as u16;
            self.words[cursor + 3] = (*qword >> 48) as u16;
            cursor += 4;
        }
    }

    fn set_words(&mut self, base: u32, values: &[u16]) {
        for (offset, value) in values.iter().enumerate() {
            self.words[base as usize + offset] = *value;
        }
    }

    fn submit(&mut self, words: u16) {
        self.write(GPU_CMD_BASE_LOW, CMD_BASE as u16);
        self.write(GPU_CMD_BASE_HIGH, (CMD_BASE >> 16) as u16);
        self.write(GPU_CMD_WORDS_LOW, words);
        self.write(GPU_CMD_WORDS_HIGH, 0);
        self.write(GPU_SUBMIT, 0);
    }

    /// Mirrors `run_ok_case`: submit, wait for the execution count, require no
    /// command error.
    fn run_ok_case(&mut self, expected_executed: u16, words: u16) {
        self.submit(words);
        self.wait_executed(expected_executed);
        assert_eq!(
            self.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR,
            0,
            "unexpected command error in submission {expected_executed}"
        );
    }

    /// Mirrors `run_error_case`: submit, wait, require a command error, then
    /// clear the sticky error like the testbench does.
    fn run_error_case(&mut self, expected_executed: u16, words: u16) {
        self.submit(words);
        self.wait_executed(expected_executed);
        assert_ne!(
            self.read(GPU_STATUS) & GPU_STATUS_COMMAND_ERROR,
            0,
            "expected command error in submission {expected_executed}"
        );
        self.write(GPU_CONTROL, GPU_CONTROL_CLEAR_ERRORS);
    }

    fn wait_executed(&mut self, expected: u16) {
        let mut cycles = 0usize;
        while self.core.executed_count() != expected {
            assert!(
                cycles < MAX_SCENARIO_CYCLES,
                "scenario did not reach executed_count {expected} within {MAX_SCENARIO_CYCLES} cycles"
            );
            self.cycle(Some(GPU_STATUS), None);
            cycles += 1;
        }
        // The completion edge is visible to the collector one cycle after the
        // retire, so run a couple of idle cycles before moving on.
        self.cycle(None, None);
        self.cycle(None, None);
    }
}

/// Runs the full `gpu_tb.v` scenario sequence against the Rust model and
/// returns the rendered trace lines (the expected side of the differential).
pub fn rust_cosim_trace() -> Vec<String> {
    render_trace(&run_cosim_scenarios())
}

/// Same inline viewport triangle and initial tile contents as the RTL raster
/// scenario in `gpu_tb.v`.
pub fn rust_raster_trace() -> Vec<String> {
    let mut h = TraceHarness::new();
    h.trace.scene("RASTER");
    h.reset_dut();
    let fb = FRAMEBUFFER_A_BASE_WORD as usize;
    for y in 0..32 {
        for x in 0..32 {
            let tile = (y / 16) * 25 + x / 16;
            h.words[fb + tile * 256 + (y % 16) * 16 + x % 16] = 0x5a5a;
        }
    }
    h.words[FB_GUARD as usize] = 0xbeef;
    h.set_qwords(
        CMD_BASE,
        &[
            set_target(FRAMEBUFFER_A_BASE_WORD),
            u64::from(crate::gpu_device::GPU_OPCODE_TRIANGLE)
                | (u64::from(crate::gpu_device::GPU_TRIANGLE_QWORDS) << 8),
            0,
            0x0000_0200,
            0x0200_0000,
            END,
        ],
    );
    h.run_ok_case(1, 24);
    let tile_word = |tile: usize, row: usize, col: usize| fb + tile * 256 + row * 16 + col;
    assert_eq!(h.words[tile_word(0, 1, 1)], 0x0000);
    assert_eq!(h.words[tile_word(0, 4, 8)], 0x0820);
    assert_eq!(h.words[tile_word(1, 4, 8)], 0x1821);
    assert_eq!(h.words[tile_word(1, 15, 15)], 0x5a5a);
    assert_eq!(h.words[tile_word(25, 0, 0)], 0x0080);
    assert_eq!(h.words[tile_word(26, 0, 0)], 0x5a5a);
    assert_eq!(h.words[FB_GUARD as usize], 0xbeef);
    render_trace(&h.trace.events)
}

pub(crate) fn run_cosim_scenarios() -> Vec<GpuTraceEvent> {
    let mut h = TraceHarness::new();
    let fb_a = FRAMEBUFFER_A_BASE_WORD;
    let fb_b = FRAMEBUFFER_B_BASE_WORD;

    // Scenario A: partial-row LOAD preservation and CLEAR initialization.
    h.trace.scene("A");
    h.reset_dut();
    for offset in 0..256usize {
        h.words[(fb_a + 512) as usize + offset] = 0x1000 + offset as u16;
    }
    h.words[FB_GUARD as usize] = 0xbeef;
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(
        LIST_BASE,
        1,
        GPU_LOAD_OP_LOAD,
        0xffff,
        0x1234,
        0x000f,
        0,
    ));
    program.extend(fake_draw(
        LIST_BASE + LIST_STRIDE,
        1,
        GPU_LOAD_OP_CLEAR,
        0x0abc,
        0x5678,
        0x00f0,
        0,
    ));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[2]);
    h.set_words(LIST_BASE + LIST_STRIDE, &[5]);
    h.run_ok_case(1, 32);

    // Scenario B: duplicate/unordered indices, then dirty victim eviction.
    h.trace.scene("B_UNORDERED");
    h.reset_dut();
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(
        LIST_BASE,
        4,
        GPU_LOAD_OP_CLEAR,
        0,
        0x1111,
        0xffff,
        0,
    ));
    program.extend(fake_draw(
        LIST_BASE + LIST_STRIDE,
        2,
        GPU_LOAD_OP_LOAD,
        0,
        0x2222,
        0x0001,
        0,
    ));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[5, 2, 5, 2]);
    h.set_words(LIST_BASE + LIST_STRIDE, &[2, 5]);
    h.run_ok_case(1, 32);

    h.trace.scene("B_EVICT");
    h.reset_dut();
    for offset in 0..256usize {
        h.words[(fb_a + 2048) as usize + offset] = 0x2000 + offset as u16;
    }
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(
        LIST_BASE,
        1,
        GPU_LOAD_OP_LOAD,
        0,
        0xaaaa,
        0xffff,
        0,
    ));
    program.extend(fake_draw(
        LIST_BASE + LIST_STRIDE,
        1,
        GPU_LOAD_OP_LOAD,
        0,
        0xbbbb,
        0xffff,
        0,
    ));
    program.extend(fake_draw(
        LIST_BASE + 2 * LIST_STRIDE,
        1,
        GPU_LOAD_OP_LOAD,
        0,
        0xcccc,
        0x0001,
        0,
    ));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[0]);
    h.set_words(LIST_BASE + LIST_STRIDE, &[8]);
    h.set_words(LIST_BASE + 2 * LIST_STRIDE, &[0]);
    h.run_ok_case(1, 44);

    // Scenario C: END retires only after every dirty entry is clean, then a
    // target switch without a device reset.
    h.trace.scene("C_DRAIN");
    h.reset_dut();
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(
        LIST_BASE,
        3,
        GPU_LOAD_OP_CLEAR,
        0,
        0x0f0f,
        0xffff,
        0,
    ));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[0, 1, 2]);
    h.run_ok_case(1, 20);

    h.trace.scene("C_GRADIENT");
    let mut program = vec![set_target(fb_b)];
    program.extend(fake_draw(
        LIST_BASE,
        1,
        GPU_LOAD_OP_CLEAR,
        0,
        0x8215,
        0xffff,
        GPU_DRAW_FLAG_GRADIENT_XY,
    ));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[7]);
    h.run_ok_case(2, 20);

    // Scenario D: tile-local XY gradient.
    h.trace.scene("D");
    h.reset_dut();
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(
        LIST_BASE,
        1,
        GPU_LOAD_OP_CLEAR,
        0,
        0x8215,
        0xffff,
        GPU_DRAW_FLAG_GRADIENT_XY,
    ));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[7]);
    h.run_ok_case(1, 20);

    // Scenario E: every invalid command class retires with command error,
    // followed by one legal empty-list draw.
    h.trace.scene("E1");
    h.reset_dut();
    // Wrong qword count for FAKE_DRAW (two instead of three).
    h.set_qwords(
        CMD_BASE,
        &[
            set_target(fb_a),
            u64::from(GPU_OPCODE_FAKE_DRAW) | (2u64 << 8),
            0,
        ],
    );
    h.run_error_case(1, 12);

    h.trace.scene("E2");
    // Reserved load op 2.
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(LIST_BASE, 0, 2, 0, 0, 0, 0));
    h.set_qwords(CMD_BASE, &program);
    h.run_error_case(2, 16);

    h.trace.scene("E3");
    // Nonzero reserved arg0 bit above the load op.
    let bad_arg0 = u64::from(GPU_OPCODE_FAKE_DRAW) | (3u64 << 8) | (0x0004_0000u64 << 32);
    h.set_qwords(CMD_BASE, &[set_target(fb_a), bad_arg0, 0, 0]);
    h.run_error_case(3, 16);

    h.trace.scene("E4");
    // Nonzero payload-0 high half.
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0, 0));
    program[2] = 1u64 << 32;
    h.set_qwords(CMD_BASE, &program);
    h.run_error_case(4, 16);

    h.trace.scene("E5");
    // Nonzero payload-1 reserved bits.
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0, 0));
    program[3] = 1u64 << 48;
    h.set_qwords(CMD_BASE, &program);
    h.run_error_case(5, 16);

    h.trace.scene("E6");
    // Unaligned tile list.
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(LIST_BASE + 1, 1, GPU_LOAD_OP_CLEAR, 0, 0, 0, 0));
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE + 1, &[0]);
    h.run_error_case(6, 16);

    h.trace.scene("E7");
    // Tile list that leaves 22-bit word memory.
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(0x3ffff0, 32, GPU_LOAD_OP_CLEAR, 0, 0, 0, 0));
    h.set_qwords(CMD_BASE, &program);
    h.run_error_case(7, 16);

    h.trace.scene("E8");
    // Tile index at the tile limit.
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(LIST_BASE, 1, GPU_LOAD_OP_CLEAR, 0, 0, 0, 0));
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[375]);
    h.run_error_case(8, 16);

    h.trace.scene("E9");
    // FAKE_DRAW before any SET_TARGET.
    let program = fake_draw(LIST_BASE, 0, GPU_LOAD_OP_CLEAR, 0, 0, 0, 0);
    h.set_qwords(CMD_BASE, &program);
    h.run_error_case(9, 12);

    h.trace.scene("E10");
    // A legal empty list performs no read and no error.
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(LIST_BASE, 0, GPU_LOAD_OP_LOAD, 0, 0, 0, 0));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.run_ok_case(10, 20);

    // Scenario X: a command that crosses a 32-byte line boundary.
    h.trace.scene("X");
    h.reset_dut();
    let mut program = vec![set_target(fb_a), set_target(fb_b), set_target(fb_a)];
    program.extend(fake_draw(
        LIST_BASE,
        1,
        GPU_LOAD_OP_CLEAR,
        0,
        0x2222,
        0xffff,
        0,
    ));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[0]);
    h.run_ok_case(1, 28);

    // Scenario F: one active plus two queued submissions, then a rejection.
    h.trace.scene("F");
    h.reset_dut();
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(
        LIST_BASE,
        3,
        GPU_LOAD_OP_CLEAR,
        0,
        0x0f0f,
        0xffff,
        0,
    ));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[0, 1, 2]);
    // The full two-deep FIFO rejects the fourth submission.
    for _ in 0..4 {
        h.submit(20);
    }
    h.wait_executed(3);

    // Scenario G: a target change with resident cache entries is illegal,
    // while re-selecting the same target is legal.
    h.trace.scene("G1");
    h.reset_dut();
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(
        LIST_BASE,
        1,
        GPU_LOAD_OP_CLEAR,
        0,
        0x1234,
        0xffff,
        0,
    ));
    program.push(set_target(fb_b));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[0]);
    h.run_error_case(1, 24);

    h.trace.scene("G2");
    h.reset_dut();
    let mut program = vec![set_target(fb_a)];
    program.extend(fake_draw(
        LIST_BASE,
        1,
        GPU_LOAD_OP_CLEAR,
        0,
        0x1234,
        0xffff,
        0,
    ));
    program.push(set_target(fb_a));
    program.push(END);
    h.set_qwords(CMD_BASE, &program);
    h.set_words(LIST_BASE, &[0]);
    h.run_ok_case(1, 24);

    h.trace.events
}

// ---------------------------------------------------------------------------
// Integration image fixtures. These are command inputs shared by the two
// harnesses; expected coverage is computed independently in gpu_trace_cosim.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct RasterSuiteScene {
    pub name: String,
    pub triangles: Vec<[[i16; 2]; 3]>,
    pub clear: Option<u16>,
    pub target: u32,
}

impl RasterSuiteScene {
    pub fn commands(&self) -> Vec<u64> {
        let mut words = vec![set_target(self.target)];
        if let Some(color) = self.clear {
            words.extend(fake_draw(LIST_BASE, 375, GPU_LOAD_OP_CLEAR, color, 0, 0, 0));
        }
        for vertices in &self.triangles {
            words.push(u64::from(crate::gpu_device::GPU_OPCODE_TRIANGLE) | (4 << 8));
            for [x, y] in vertices {
                words.push(u64::from(*x as u16) | (u64::from(*y as u16) << 16));
            }
        }
        words.push(END);
        words
    }
}

/// Deterministic snapped viewport inputs, including direct-map alias pressure.
pub fn raster_suite_scenes() -> Vec<RasterSuiteScene> {
    let mut result = Vec::new();
    let mut add = |name: &str, triangles: Vec<[[i16; 2]; 3]>, clear| {
        result.push(RasterSuiteScene {
            name: name.into(),
            triangles,
            clear,
            target: FRAMEBUFFER_A_BASE_WORD,
        });
    };
    let tri = |v: [[i16; 2]; 3]| v.map(|p| p.map(|n| n * 16));
    add(
        "shared_edge",
        vec![
            tri([[0, 0], [32, 0], [0, 32]]),
            tri([[32, 0], [32, 32], [0, 32]]),
        ],
        None,
    );
    add(
        "opposite_winding",
        vec![tri([[20, 20], [20, 80], [80, 20]])],
        None,
    );
    add(
        "zero_area",
        vec![
            tri([[30, 30], [30, 30], [30, 30]]),
            tri([[0, 0], [16, 16], [32, 32]]),
        ],
        None,
    );
    add(
        "subpixel",
        vec![
            [[160, 160], [161, 160], [160, 161]],
            [[168, 168], [184, 168], [168, 184]],
        ],
        None,
    );
    add(
        "screen_edges",
        vec![
            tri([[-16, -16], [48, -16], [-16, 48]]),
            tri([[384, -16], [416, -16], [384, 48]]),
            tri([[-16, 224], [48, 224], [-16, 256]]),
            tri([[384, 224], [416, 224], [384, 256]]),
        ],
        None,
    );
    add(
        "offscreen",
        vec![
            tri([[-80, 0], [-32, 0], [-80, 48]]),
            tri([[448, 0], [480, 0], [448, 48]]),
            tri([[0, -80], [48, -80], [0, -32]]),
            tri([[0, 272], [48, 272], [0, 320]]),
        ],
        None,
    );
    add(
        "alias_eviction",
        vec![
            tri([[0, 0], [176, 0], [0, 64]]),
            tri([[0, 0], [48, 0], [0, 48]]),
        ],
        None,
    );
    add(
        "clear_then_load",
        vec![
            tri([[0, 0], [48, 0], [0, 48]]),
            tri([[16, 16], [80, 16], [16, 80]]),
        ],
        Some(0x1234),
    );
    add(
        "wide_triangle",
        vec![tri([[0, 0], [400, 0], [0, 240]])],
        None,
    );
    add(
        "empty_draws_between",
        vec![
            tri([[8, 8], [72, 8], [8, 72]]),
            tri([[30, 30], [30, 30], [30, 30]]),
            tri([[480, 0], [512, 0], [480, 32]]),
            tri([[16, 16], [80, 16], [16, 80]]),
        ],
        None,
    );
    // Fixed seed is part of the fixture, with modest AABBs so replay is cheap.
    let mut seed = 0x91e1_0da5u32;
    for case in 0..16 {
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let x = (next() % 448) as i16 - 24;
        let y = (next() % 288) as i16 - 24;
        let w = (next() % 72 + 1) as i16;
        let h = (next() % 72 + 1) as i16;
        add(
            &format!("seeded_{case:02}"),
            vec![tri([[x, y], [x + w, y], [x, y + h]])],
            None,
        );
    }
    result.last_mut().unwrap().target = FRAMEBUFFER_B_BASE_WORD;
    result
}

pub fn rust_raster_suite_trace(scenes: &[RasterSuiteScene]) -> Vec<String> {
    let mut h = TraceHarness::new();
    for scene in scenes {
        h.trace.scene(&scene.name);
        h.reset_dut();
        let base = scene.target as usize;
        h.words[base..base + FRAMEBUFFER_WORDS as usize].fill(0x5a5a);
        h.words[base - 1] = 0xbeef;
        h.words[base + FRAMEBUFFER_WORDS as usize] = 0xbeef;
        h.set_words(LIST_BASE, &(0..375).collect::<Vec<_>>());
        let commands = scene.commands();
        h.set_qwords(CMD_BASE, &commands);
        h.run_ok_case(1, (commands.len() * 4) as u16);
        assert_eq!(h.words[base - 1], 0xbeef);
        assert_eq!(h.words[base + FRAMEBUFFER_WORDS as usize], 0xbeef);
        assert_eq!(h.core.cache_dirty, [false; 8], "END retained dirty entries");
    }
    render_trace(&h.trace.events)
}

// ---------------------------------------------------------------------------
// Trace comparison.
// ---------------------------------------------------------------------------

/// One scene's normalized event bodies, bucketed for comparison.
struct SceneTrace {
    name: String,
    ro: Vec<String>,
    fb_r: Vec<String>,
    fb_w: Vec<String>,
    done: Vec<String>,
}

/// Strips the `GPU <seq>` prefix; returns `None` for non-trace lines.
fn trace_body(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("GPU ")?;
    let body = rest.split_once(' ')?.1;
    Some(body.to_string())
}

fn split_scenes(lines: &[String]) -> Result<Vec<SceneTrace>, String> {
    let mut scenes: Vec<SceneTrace> = Vec::new();
    for line in lines {
        let Some(body) = trace_body(line) else {
            continue;
        };
        let mut tokens = body.split_whitespace();
        let kind = tokens.next().unwrap_or("");
        if kind == "SCENE" {
            scenes.push(SceneTrace {
                name: tokens.next().unwrap_or("?").to_string(),
                ro: Vec::new(),
                fb_r: Vec::new(),
                fb_w: Vec::new(),
                done: Vec::new(),
            });
            continue;
        }
        let Some(scene) = scenes.last_mut() else {
            return Err(format!("trace event before the first SCENE marker: {body}"));
        };
        match kind {
            "REQ" | "RDAT" | "WDAT" | "RESP" => match tokens.next() {
                Some("ro") => scene.ro.push(body),
                Some("fb_r") => scene.fb_r.push(body),
                Some("fb_w") => scene.fb_w.push(body),
                other => return Err(format!("unknown port in trace line: {other:?}")),
            },
            "DONE" | "RACK" => scene.done.push(body),
            _ => return Err(format!("unknown trace event kind: {body}")),
        }
    }
    Ok(scenes)
}

const CONTEXT: usize = 5;

fn compare_bucket(
    scene: &str,
    label: &str,
    expected: &[String],
    actual: &[String],
) -> Result<(), String> {
    let common = expected.len().max(actual.len());
    for index in 0..common {
        let matches = match (expected.get(index), actual.get(index)) {
            (Some(e), Some(a)) => e == a,
            _ => false,
        };
        if matches {
            continue;
        }
        let context = |lines: &[String]| {
            let lo = index.saturating_sub(CONTEXT);
            let hi = (index + CONTEXT + 1).min(lines.len());
            (lo..hi)
                .map(|i| {
                    let marker = if i == index { ">>" } else { "  " };
                    format!("  {marker} [{i}] {}", lines[i])
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        return Err(format!(
            "first mismatch: scene '{scene}' {label} event {index}\n  \
             expected (rust): {}\n  \
             actual   (rtl):  {}\n\
             expected context:\n{}\n\
             actual context:\n{}",
            expected.get(index).map_or("<none>", String::as_str),
            actual.get(index).map_or("<none>", String::as_str),
            context(expected),
            context(actual),
        ));
    }
    Ok(())
}

/// Compares a rendered Rust trace (expected) against the RTL trace lines
/// (actual). Per scene, the per-port event sequences and the global `DONE`
/// sequence must match exactly; cross-port interleaving is ignored.
pub fn compare_traces(expected: &[String], actual: &[String]) -> Result<(), String> {
    let expected_scenes = split_scenes(expected)?;
    let actual_scenes = split_scenes(actual)?;
    if expected_scenes.len() != actual_scenes.len() {
        return Err(format!(
            "scene count mismatch: expected {} scenes, RTL produced {}",
            expected_scenes.len(),
            actual_scenes.len()
        ));
    }
    for (expected_scene, actual_scene) in expected_scenes.iter().zip(&actual_scenes) {
        if expected_scene.name != actual_scene.name {
            return Err(format!(
                "scene order mismatch: expected '{}', RTL produced '{}'",
                expected_scene.name, actual_scene.name
            ));
        }
        let name = &expected_scene.name;
        compare_bucket(name, "ro", &expected_scene.ro, &actual_scene.ro)?;
        compare_bucket(name, "fb_r", &expected_scene.fb_r, &actual_scene.fb_r)?;
        compare_bucket(name, "fb_w", &expected_scene.fb_w, &actual_scene.fb_w)?;
        compare_bucket(name, "done", &expected_scene.done, &actual_scene.done)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Structural trace invariants checked by the unit tests (and available to the
// harness hook test): every request is answered, beat counts match the
// request length, and submission completions count up within a reset domain.
// ---------------------------------------------------------------------------
#[cfg(test)]
pub(crate) fn check_invariants(events: &[GpuTraceEvent]) -> Result<(), String> {
    // Per-port open-transaction state: expected remaining beats and kind.
    struct Open {
        remaining_beats: usize,
        write: bool,
    }
    let mut open: [Option<Open>; 3] = [None, None, None];
    let port_index = |port: TracePort| match port {
        TracePort::Ro => 0,
        TracePort::FbR => 1,
        TracePort::FbW => 2,
    };
    let mut last_done: Option<(String, u16)> = None;
    let mut scene = String::new();
    for (index, event) in events.iter().enumerate() {
        match event {
            GpuTraceEvent::Scene(name) => scene = name.clone(),
            GpuTraceEvent::Req {
                port, write, lines, ..
            } => {
                let slot = port_index(*port);
                if open[slot].is_some() {
                    return Err(format!(
                        "event {index}: REQ on {} while one is open",
                        port.name()
                    ));
                }
                open[slot] = Some(Open {
                    remaining_beats: usize::from(*lines) * 4,
                    write: *write,
                });
            }
            GpuTraceEvent::Rdat { port, .. } | GpuTraceEvent::Wdat { port, .. } => {
                let slot = port_index(*port);
                let Some(state) = &mut open[slot] else {
                    return Err(format!("event {index}: data beat without a request"));
                };
                let is_rdat = matches!(event, GpuTraceEvent::Rdat { .. });
                if state.write == is_rdat {
                    return Err(format!(
                        "event {index}: {} beat on a {} transaction",
                        if is_rdat { "RDAT" } else { "WDAT" },
                        if state.write { "write" } else { "read" }
                    ));
                }
                if state.remaining_beats == 0 {
                    return Err(format!("event {index}: more beats than the request length"));
                }
                state.remaining_beats -= 1;
            }
            GpuTraceEvent::Resp { port, .. } => {
                let slot = port_index(*port);
                let Some(state) = open[slot].take() else {
                    return Err(format!("event {index}: RESP without a request"));
                };
                if state.remaining_beats != 0 {
                    return Err(format!(
                        "event {index}: RESP with {} beats outstanding",
                        state.remaining_beats
                    ));
                }
            }
            GpuTraceEvent::DoneSubmission { executed_count } => {
                if let Some((last_scene, last_count)) = &last_done {
                    // The count either keeps counting up (a scene boundary
                    // without a reset, e.g. C_DRAIN -> C_GRADIENT) or restarts
                    // at one at a reset-separated scene.
                    let contiguous = *executed_count == last_count + 1;
                    let new_domain = *last_scene != scene && *executed_count == 1;
                    if !contiguous && !new_domain {
                        return Err(format!(
                            "event {index}: DONE submission count {executed_count} after \
                             {last_count} (scene {last_scene} -> {scene})"
                        ));
                    }
                }
                last_done = Some((scene.clone(), *executed_count));
            }
            GpuTraceEvent::DoneDraw { .. } | GpuTraceEvent::RasterRetire { .. } => {}
        }
    }
    for (slot, state) in open.iter().enumerate() {
        if state.is_some() {
            return Err(format!(
                "port {slot} has an unanswered request at end of trace"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bodies(events: &[GpuTraceEvent]) -> Vec<String> {
        events.iter().map(GpuTraceEvent::body).collect()
    }

    #[test]
    fn rendered_format_matches_the_rtl_wire_format() {
        let events = vec![
            GpuTraceEvent::Scene("A".to_string()),
            GpuTraceEvent::Req {
                port: TracePort::Ro,
                write: false,
                address: 0x100,
                lines: 1,
            },
            GpuTraceEvent::Rdat {
                port: TracePort::Ro,
                beat: 2,
                value: 0x0004_0003_0002_0001,
            },
            GpuTraceEvent::Resp {
                port: TracePort::Ro,
                error: false,
            },
            GpuTraceEvent::Req {
                port: TracePort::FbW,
                write: true,
                address: 0x200100,
                lines: 4,
            },
            GpuTraceEvent::Wdat {
                port: TracePort::FbW,
                beat: 15,
                value: 0xaaaa_bbbb_cccc_dddd,
            },
            GpuTraceEvent::Resp {
                port: TracePort::FbW,
                error: true,
            },
            GpuTraceEvent::DoneSubmission { executed_count: 7 },
            GpuTraceEvent::DoneDraw { tile_count: 3 },
        ];
        assert_eq!(
            bodies(&events),
            vec![
                "SCENE A",
                "REQ ro R 000100 1",
                "RDAT ro 2 0001 0002 0003 0004",
                "RESP ro 0",
                "REQ fb_w W 200100 4",
                "WDAT fb_w 15 dddd cccc bbbb aaaa",
                "RESP fb_w 1",
                "DONE submission 7",
                "DONE draw 3",
            ]
        );
        assert_eq!(
            render_trace(&events)[2],
            "GPU 2 RDAT ro 2 0001 0002 0003 0004"
        );
    }

    #[test]
    fn collector_records_a_full_ro_read_transaction() {
        let mut collector = GpuTraceCollector::default();
        let core = GpuCore::default();
        // Accepting edge.
        let out = GpuOutputs {
            ro_request_valid: true,
            ro_address: 0x400,
            ro_line_count_minus_1: 0,
            ..GpuOutputs::default()
        };
        let mem = GpuMemoryBus {
            ro_request_ready: true,
            ..GpuMemoryBus::default()
        };
        collector.on_cycle(&mem, &out, &core);
        // Four read beats.
        for beat in 0..4u8 {
            let mem = GpuMemoryBus {
                ro_response_valid: true,
                ro_read_data: u64::from(beat),
                ro_response_last: beat == 3,
                ..GpuMemoryBus::default()
            };
            collector.on_cycle(&mem, &GpuOutputs::default(), &core);
        }
        assert_eq!(
            bodies(collector.events()),
            vec![
                "REQ ro R 000400 1",
                "RDAT ro 0 0000 0000 0000 0000",
                "RDAT ro 1 0001 0000 0000 0000",
                "RDAT ro 2 0002 0000 0000 0000",
                "RDAT ro 3 0003 0000 0000 0000",
                "RESP ro 0",
            ]
        );
        check_invariants(collector.events()).unwrap();
    }

    #[test]
    fn collector_records_a_full_fb_w_write_transaction() {
        let mut collector = GpuTraceCollector::default();
        let core = GpuCore::default();
        let out = GpuOutputs {
            fb_w_request_valid: true,
            fb_w_write: true,
            fb_w_address: 0x200000,
            fb_w_line_count_minus_1: 3,
            fb_w_write_data: 0x11,
            ..GpuOutputs::default()
        };
        let mem = GpuMemoryBus {
            fb_w_request_ready: true,
            ..GpuMemoryBus::default()
        };
        collector.on_cycle(&mem, &out, &core);
        assert_eq!(
            collector.events().len(),
            1,
            "acceptance must not consume beat zero"
        );
        for beat in 0..16u8 {
            let out = GpuOutputs {
                fb_w_write_data: if beat == 0 {
                    0x11
                } else {
                    u64::from(beat) * 0x10
                },
                ..GpuOutputs::default()
            };
            let mem = GpuMemoryBus {
                fb_w_write_data_ready: true,
                ..GpuMemoryBus::default()
            };
            collector.on_cycle(&mem, &out, &core);
        }
        let mem = GpuMemoryBus {
            fb_w_response_valid: true,
            fb_w_response_last: true,
            ..GpuMemoryBus::default()
        };
        collector.on_cycle(&mem, &GpuOutputs::default(), &core);
        let bodies = bodies(collector.events());
        assert_eq!(bodies[0], "REQ fb_w W 200000 4");
        assert_eq!(bodies[1], "WDAT fb_w 0 0011 0000 0000 0000");
        assert_eq!(bodies[16], "WDAT fb_w 15 00f0 0000 0000 0000");
        assert_eq!(bodies[17], "RESP fb_w 0");
        assert_eq!(bodies.len(), 18);
        check_invariants(collector.events()).unwrap();
    }

    #[test]
    fn scenario_trace_satisfies_structural_invariants() {
        let events = run_cosim_scenarios();
        check_invariants(&events).unwrap();
        let submissions = events
            .iter()
            .filter(|event| matches!(event, GpuTraceEvent::DoneSubmission { .. }))
            .count();
        assert_eq!(submissions, 22, "the scenario set retires 22 submissions");
        let scenes = events
            .iter()
            .filter(|event| matches!(event, GpuTraceEvent::Scene(_)))
            .count();
        assert_eq!(scenes, 20, "A..G2 scene markers");
        // Every dirty END drain / eviction writes whole tiles: fb_w traffic
        // exists, and every fb_w request is a 4-line (128-byte) clean.
        assert!(events.iter().any(|event| matches!(
            event,
            GpuTraceEvent::Req {
                port: TracePort::FbW,
                lines: 4,
                ..
            }
        )));
    }

    #[test]
    fn comparator_accepts_identical_traces() {
        let trace = rust_cosim_trace();
        compare_traces(&trace, &trace.clone()).unwrap();
    }

    #[test]
    fn comparator_reports_the_first_divergence_with_context() {
        let rust = vec![
            "GPU 0 SCENE A".to_string(),
            "GPU 1 REQ ro R 000100 1".to_string(),
            "GPU 2 RDAT ro 0 0001 0002 0003 0004".to_string(),
            "GPU 3 RESP ro 0".to_string(),
            "GPU 4 DONE submission 1".to_string(),
        ];
        let mut rtl = rust.clone();
        rtl[3] = "GPU 3 RESP ro 1".to_string();
        let error = compare_traces(&rust, &rtl).unwrap_err();
        assert!(error.contains("scene 'A'"), "{error}");
        assert!(error.contains("ro event 2"), "{error}");
        assert!(error.contains("expected (rust): RESP ro 0"), "{error}");
        assert!(error.contains("actual   (rtl):  RESP ro 1"), "{error}");
        assert!(error.contains("context"), "{error}");
    }

    #[test]
    fn comparator_detects_missing_and_extra_events() {
        let rust = vec![
            "GPU 0 SCENE A".to_string(),
            "GPU 1 REQ ro R 000100 1".to_string(),
            "GPU 2 RESP ro 0".to_string(),
        ];
        let rtl = vec![
            "GPU 0 SCENE A".to_string(),
            "GPU 1 REQ ro R 000100 1".to_string(),
        ];
        let error = compare_traces(&rust, &rtl).unwrap_err();
        assert!(error.contains("ro event 1"), "{error}");
        assert!(error.contains("<none>"), "{error}");
    }
}
