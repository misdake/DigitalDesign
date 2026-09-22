//! System-owned GPU device, temporary command processor, and dummy
//! framebuffer writer.
//!
//! This is the A3/A4 bring-up milestone: a fixed 32-byte command read master
//! (`gpu_ro`), a fixed 32-byte framebuffer write master (`gpu_fb_w`), and the
//! GPU device register file are driven by one serial command FSM. `gpu_fb_r`
//! exists for the next milestone and is tied idle here.
//!
//! The same FSM contract is implemented by two backends:
//!
//! * [`GpuCore`] is the cycle model used by the host transaction model in
//!   `crate::gpu_device`; and
//! * [`CpuV3Gpu`] wraps the fitted handwritten Verilog FSM.
//!
//! Both must reach the same outputs for the same inputs and command buffer;
//! the co-simulation test drives a real command buffer through both.

use crate::gpu_device::{
    gpu_dummy_beat, gpu_dummy_pixel, gpu_target_slot, GPU_CMD_BASE_HIGH, GPU_CMD_BASE_LOW,
    GPU_CMD_WORDS_HIGH, GPU_CMD_WORDS_LOW, GPU_COLOR_MODE_ZERO, GPU_CONTROL,
    GPU_CONTROL_CLEAR_ERRORS, GPU_CONTROL_RESET, GPU_DEVICE, GPU_EXECUTED_COUNT, GPU_FIFO_DEPTH,
    GPU_OPCODE_END, GPU_OPCODE_FAKE_DRAW, GPU_OPCODE_SET_TARGET, GPU_QUEUE_LEVEL,
    GPU_RECEIVED_COUNT, GPU_STATUS, GPU_STATUS_BUSY, GPU_STATUS_COMMAND_ERROR,
    GPU_STATUS_FIFO_FULL, GPU_STATUS_SUBMIT_REJECTED, GPU_SUBMIT, GPU_TILE_TOTAL,
};
use crate::layout::FRAMEBUFFER_TILE_COLUMNS;
use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    resources::components::SsramBits, Hardware, Module, ModuleIo, TargetResourceRequest,
};

mod host;

pub(crate) use host::HostGpuMemory;

/// One device-port access sampled from the input wires or driven directly by
/// the host model.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpuDeviceBus {
    pub index: u8,
    pub channel: u8,
    pub read_enable: bool,
    pub write_enable: bool,
    pub write_data: u16,
}

/// The two GPU memory masters' handshake inputs for one cycle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpuMemoryBus {
    pub ro_request_ready: bool,
    pub ro_response_valid: bool,
    pub ro_read_data: u64,
    pub ro_response_last: bool,
    pub ro_error: bool,
    pub fb_w_request_ready: bool,
    pub fb_w_response_valid: bool,
    pub fb_w_response_last: bool,
    pub fb_w_error: bool,
}

/// The GPU's combinational outputs for one cycle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpuOutputs {
    pub read_data: u16,
    pub ro_request_valid: bool,
    pub ro_write: bool,
    pub ro_address: u32,
    pub ro_write_data: u64,
    pub fb_w_request_valid: bool,
    pub fb_w_write: bool,
    pub fb_w_address: u32,
    pub fb_w_write_data: u64,
    pub fb_r_request_valid: bool,
    pub fb_r_write: bool,
    pub fb_r_address: u32,
    pub fb_r_write_data: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Phase {
    #[default]
    Idle,
    Fetch,
    Receive,
    Decode,
    DrawRequest,
    DrawWait,
    Retire,
    ErrorRetire,
}

/// Backend-independent GPU command state machine.
///
/// `combine` computes the outputs visible on `phase` and the register state;
/// `advance` applies the clock edge. Both the circuit emulator and the host
/// model call exactly these two functions per cycle, so they cannot drift.
#[derive(Clone)]
pub struct GpuCore {
    // Device register file.
    staging_base: u32,
    staging_words: u16,
    base_low_written: bool,
    base_high_written: bool,
    words_low_written: bool,
    words_high_written: bool,
    staging_bad_bits: bool,
    received_count: u16,
    executed_count: u16,
    submit_rejected: bool,
    command_error: bool,
    // Exactly two queued submissions, plus the active one.
    fifo_base: [u32; GPU_FIFO_DEPTH],
    fifo_words: [u16; GPU_FIFO_DEPTH],
    fifo_head: u8,
    fifo_count: u8,
    // Active submission.
    active: bool,
    active_base: u32,
    active_words: u16,
    qword_index: u16,
    /// Global qword index of the line buffer's first qword; `0xffff` when the
    /// buffer holds no line for the current cursor.
    line_qword_base: u16,
    line_buffer: [u64; 4],
    recv_beat: u8,
    // Header held between the header qword and its payload qword.
    pending_opcode: u8,
    pending_qword_count: u8,
    pending_arg0: u32,
    have_pending: bool,
    // Per-submission draw state.
    target_set: bool,
    target_base: u32,
    tiles_done: u32,
    // Current tile.
    draw_tiles_left: u16,
    tile_x: u8,
    tile_y: u8,
    phase_bias: u16,
    r_bias: u16,
    g_bias: u16,
    b_bias: u16,
    pixel: u16,
    row: u8,
    draw_addr: u32,
    phase: Phase,
}

impl Default for GpuCore {
    fn default() -> Self {
        Self {
            staging_base: 0,
            staging_words: 0,
            base_low_written: false,
            base_high_written: false,
            words_low_written: false,
            words_high_written: false,
            staging_bad_bits: false,
            received_count: 0,
            executed_count: 0,
            submit_rejected: false,
            command_error: false,
            fifo_base: [0; GPU_FIFO_DEPTH],
            fifo_words: [0; GPU_FIFO_DEPTH],
            fifo_head: 0,
            fifo_count: 0,
            active: false,
            active_base: 0,
            active_words: 0,
            qword_index: 0,
            line_qword_base: 0xffff,
            line_buffer: [0; 4],
            recv_beat: 0,
            pending_opcode: 0,
            pending_qword_count: 0,
            pending_arg0: 0,
            have_pending: false,
            target_set: false,
            target_base: 0,
            tiles_done: 0,
            draw_tiles_left: 0,
            tile_x: 0,
            tile_y: 0,
            phase_bias: 0,
            r_bias: 0,
            g_bias: 0,
            b_bias: 0,
            pixel: 0,
            row: 0,
            draw_addr: 0,
            phase: Phase::Idle,
        }
    }
}

impl GpuCore {
    pub(crate) fn busy(&self) -> bool {
        self.active || self.fifo_count != 0
    }

    pub(crate) fn received_count(&self) -> u16 {
        self.received_count
    }

    pub(crate) fn executed_count(&self) -> u16 {
        self.executed_count
    }

    pub(crate) fn command_error(&self) -> bool {
        self.command_error
    }

    pub(crate) fn submit_rejected(&self) -> bool {
        self.submit_rejected
    }

    pub(crate) fn queue_level(&self) -> u16 {
        u16::from(self.fifo_count)
    }

    fn status(&self) -> u16 {
        let mut status = 0u16;
        if self.busy() {
            status |= GPU_STATUS_BUSY;
        }
        if self.fifo_count >= GPU_FIFO_DEPTH as u8 {
            status |= GPU_STATUS_FIFO_FULL;
        }
        if self.submit_rejected {
            status |= GPU_STATUS_SUBMIT_REJECTED;
        }
        if self.command_error {
            status |= GPU_STATUS_COMMAND_ERROR;
        }
        status
    }

    fn clear_staging(&mut self) {
        self.base_low_written = false;
        self.base_high_written = false;
        self.words_low_written = false;
        self.words_high_written = false;
        self.staging_bad_bits = false;
    }

    fn submit(&mut self) {
        let complete = self.base_low_written
            && self.base_high_written
            && self.words_low_written
            && self.words_high_written;
        let words = self.staging_words;
        // The command buffer base is 32-byte/16-word aligned, the length is
        // qword-sized, and the whole buffer must stay inside 22-bit memory.
        let valid = complete
            && !self.staging_bad_bits
            && self.staging_base & 0xf == 0
            && words != 0
            && words.is_multiple_of(4)
            && u64::from(self.staging_base) + u64::from(words) <= 1 << 22
            && self.fifo_count < GPU_FIFO_DEPTH as u8;
        if valid {
            let tail = ((self.fifo_head + self.fifo_count) % GPU_FIFO_DEPTH as u8) as usize;
            self.fifo_base[tail] = self.staging_base;
            self.fifo_words[tail] = words;
            self.fifo_count += 1;
            self.received_count = self.received_count.wrapping_add(1);
            self.clear_staging();
        } else {
            self.submit_rejected = true;
        }
    }

    fn control(&mut self, value: u16) {
        if value & GPU_CONTROL_RESET != 0 && !self.busy() {
            // Soft reset is idle-only. It clears the staging registers, FIFO
            // and command state plus sticky errors, but preserves the event
            // counters.
            self.fifo_head = 0;
            self.fifo_count = 0;
            self.active = false;
            self.phase = Phase::Idle;
            self.line_qword_base = 0xffff;
            self.have_pending = false;
            self.clear_staging();
            self.submit_rejected = false;
            self.command_error = false;
        }
        if value & GPU_CONTROL_CLEAR_ERRORS != 0 {
            self.submit_rejected = false;
            self.command_error = false;
        }
    }

    fn device_write(&mut self, channel: u8, value: u16) {
        match channel {
            GPU_CMD_BASE_LOW => {
                self.staging_base = (self.staging_base & 0xffff_0000) | u32::from(value);
                self.base_low_written = true;
            }
            GPU_CMD_BASE_HIGH => {
                self.staging_base =
                    (self.staging_base & 0x0000_ffff) | (u32::from(value & 0x3f) << 16);
                self.base_high_written = true;
                if value & 0xffc0 != 0 {
                    self.staging_bad_bits = true;
                }
            }
            GPU_CMD_WORDS_LOW => {
                self.staging_words = value;
                self.words_low_written = true;
            }
            GPU_CMD_WORDS_HIGH => {
                self.words_high_written = true;
                if value != 0 {
                    self.staging_bad_bits = true;
                }
            }
            GPU_SUBMIT => self.submit(),
            GPU_CONTROL => self.control(value),
            _ => {}
        }
    }

    fn pop_submission(&mut self) {
        if self.fifo_count == 0 {
            return;
        }
        let head = self.fifo_head as usize;
        self.active = true;
        self.active_base = self.fifo_base[head];
        self.active_words = self.fifo_words[head];
        self.fifo_head = (self.fifo_head + 1) % GPU_FIFO_DEPTH as u8;
        self.fifo_count -= 1;
        self.qword_index = 0;
        self.line_qword_base = 0xffff;
        self.recv_beat = 0;
        self.have_pending = false;
        self.target_set = false;
        self.target_base = 0;
        self.tiles_done = 0;
        self.tile_x = 0;
        self.tile_y = 0;
        self.phase = Phase::Fetch;
    }

    fn enter_error(&mut self) {
        self.phase = Phase::ErrorRetire;
    }

    fn compute_pixel(&mut self) {
        self.pixel = gpu_dummy_pixel(
            u32::from(self.tile_x),
            u32::from(self.tile_y),
            self.phase_bias,
            self.r_bias,
            self.g_bias,
            self.b_bias,
        );
    }

    fn finish_row(&mut self) {
        self.row += 1;
        self.draw_addr = self.draw_addr.wrapping_add(16);
        if self.row == 16 {
            self.row = 0;
            self.tiles_done += 1;
            self.draw_tiles_left = self.draw_tiles_left.wrapping_sub(1);
            self.tile_x += 1;
            if u32::from(self.tile_x) == FRAMEBUFFER_TILE_COLUMNS {
                self.tile_x = 0;
                self.tile_y += 1;
            }
            if self.draw_tiles_left == 0 {
                self.phase = Phase::Decode;
                return;
            }
            self.compute_pixel();
        }
        self.phase = Phase::DrawRequest;
    }

    /// Executes a fully collected command. Sets `phase` and must be called from
    /// [`Phase::Decode`].
    fn execute_command(&mut self, payload: Option<u64>) {
        let opcode = self.pending_opcode;
        let count = self.pending_qword_count;
        let arg0 = self.pending_arg0;
        match opcode {
            GPU_OPCODE_SET_TARGET => {
                if count != 1 {
                    self.enter_error();
                    return;
                }
                match gpu_target_slot(arg0) {
                    Some(base) => {
                        self.target_set = true;
                        self.target_base = base;
                        self.phase = Phase::Decode;
                    }
                    None => self.enter_error(),
                }
            }
            GPU_OPCODE_FAKE_DRAW => {
                let Some(payload) = payload else {
                    self.enter_error();
                    return;
                };
                if count != 2 || arg0 >> 16 != GPU_COLOR_MODE_ZERO {
                    self.enter_error();
                    return;
                }
                if !self.target_set {
                    self.enter_error();
                    return;
                }
                let tile_count = (arg0 & 0xffff) as u16;
                if self.tiles_done + u32::from(tile_count) > GPU_TILE_TOTAL {
                    self.enter_error();
                    return;
                }
                self.phase_bias = payload as u16;
                self.r_bias = (payload >> 16) as u16;
                self.g_bias = (payload >> 32) as u16;
                self.b_bias = (payload >> 48) as u16;
                self.draw_tiles_left = tile_count;
                self.draw_addr = self.target_base.wrapping_add(self.tiles_done << 8);
                self.row = 0;
                if tile_count == 0 {
                    self.phase = Phase::Decode;
                } else {
                    self.compute_pixel();
                    self.phase = Phase::DrawRequest;
                }
            }
            GPU_OPCODE_END => {
                if count != 1 || arg0 != 0 {
                    self.enter_error();
                    return;
                }
                if !self.target_set || self.tiles_done != GPU_TILE_TOTAL {
                    self.enter_error();
                    return;
                }
                self.phase = Phase::Retire;
            }
            _ => self.enter_error(),
        }
    }

    fn decode_step(&mut self) {
        if self.line_qword_base != (self.qword_index & !3) {
            self.phase = Phase::Fetch;
            return;
        }
        let total_qwords = (self.active_words / 4) as u32;
        if u32::from(self.qword_index) >= total_qwords {
            // Ran off the declared length without seeing END.
            self.enter_error();
            return;
        }
        let word = self.line_buffer[(self.qword_index & 3) as usize];
        if self.have_pending {
            self.have_pending = false;
            self.qword_index += 1;
            self.execute_command(Some(word));
            return;
        }
        let opcode = (word & 0xff) as u8;
        let count = ((word >> 8) & 0xff) as u8;
        let flags = ((word >> 16) & 0xffff) as u32;
        let arg0 = (word >> 32) as u32;
        if count == 0 || flags != 0 {
            self.enter_error();
            return;
        }
        if u32::from(self.qword_index) + u32::from(count) > total_qwords {
            self.enter_error();
            return;
        }
        self.pending_opcode = opcode;
        self.pending_qword_count = count;
        self.pending_arg0 = arg0;
        self.qword_index += 1;
        if count == 1 {
            self.execute_command(None);
        } else {
            self.have_pending = true;
        }
    }

    pub(crate) fn combine(&self, dev: GpuDeviceBus, mem: GpuMemoryBus) -> GpuOutputs {
        let read_data = if dev.read_enable && dev.index == GPU_DEVICE {
            match dev.channel {
                GPU_RECEIVED_COUNT => self.received_count,
                GPU_EXECUTED_COUNT => self.executed_count,
                GPU_STATUS => self.status(),
                GPU_QUEUE_LEVEL => u16::from(self.fifo_count),
                _ => 0,
            }
        } else {
            0
        };
        let _ = mem;
        let ro_address = self
            .active_base
            .wrapping_add((u32::from(self.qword_index) >> 2) << 4);
        GpuOutputs {
            read_data,
            ro_request_valid: self.phase == Phase::Fetch,
            ro_write: false,
            ro_address,
            ro_write_data: 0,
            fb_w_request_valid: self.phase == Phase::DrawRequest || self.phase == Phase::DrawWait,
            fb_w_write: true,
            fb_w_address: self.draw_addr,
            fb_w_write_data: gpu_dummy_beat(self.pixel),
            fb_r_request_valid: false,
            fb_r_write: false,
            fb_r_address: 0,
            fb_r_write_data: 0,
        }
    }

    pub(crate) fn advance(&mut self, reset: bool, dev: GpuDeviceBus, mem: GpuMemoryBus) {
        if reset {
            *self = Self::default();
            return;
        }
        if dev.write_enable && dev.index == GPU_DEVICE {
            self.device_write(dev.channel, dev.write_data);
        }
        match self.phase {
            Phase::Idle => {
                if !self.active && self.fifo_count != 0 {
                    self.pop_submission();
                }
            }
            Phase::Fetch => {
                if mem.ro_request_ready {
                    self.phase = Phase::Receive;
                    self.recv_beat = 0;
                }
            }
            Phase::Receive => {
                if mem.ro_response_valid {
                    if mem.ro_error {
                        self.enter_error();
                    } else {
                        self.line_buffer[self.recv_beat as usize] = mem.ro_read_data;
                        if self.recv_beat == 3 || mem.ro_response_last {
                            self.recv_beat = 0;
                            self.line_qword_base = self.qword_index & !3;
                            self.phase = Phase::Decode;
                        } else {
                            self.recv_beat += 1;
                        }
                    }
                }
            }
            Phase::Decode => self.decode_step(),
            Phase::DrawRequest => {
                if mem.fb_w_request_ready {
                    self.phase = Phase::DrawWait;
                }
            }
            Phase::DrawWait => {
                if mem.fb_w_response_valid {
                    if mem.fb_w_error {
                        self.enter_error();
                    } else if mem.fb_w_response_last {
                        self.finish_row();
                    }
                }
            }
            Phase::Retire => {
                self.executed_count = self.executed_count.wrapping_add(1);
                self.active = false;
                self.phase = Phase::Idle;
            }
            Phase::ErrorRetire => {
                self.command_error = true;
                self.executed_count = self.executed_count.wrapping_add(1);
                self.active = false;
                self.phase = Phase::Idle;
            }
        }
    }
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3GpuInput {
    pub reset: Wire,
    pub device_index: Wires<3>,
    pub device_channel: Wires<4>,
    pub device_read_enable: Wire,
    pub device_write_enable: Wire,
    pub device_write_data: Wires<16>,

    pub gpu_ro_request_ready: Wire,
    pub gpu_ro_response_valid: Wire,
    pub gpu_ro_read_data: Wires<64>,
    pub gpu_ro_response_last: Wire,
    pub gpu_ro_error: Wire,

    pub gpu_fb_w_request_ready: Wire,
    pub gpu_fb_w_response_valid: Wire,
    pub gpu_fb_w_response_last: Wire,
    pub gpu_fb_w_error: Wire,

    pub gpu_fb_r_request_ready: Wire,
    pub gpu_fb_r_response_valid: Wire,
    pub gpu_fb_r_read_data: Wires<64>,
    pub gpu_fb_r_response_last: Wire,
    pub gpu_fb_r_error: Wire,
}

#[derive(Clone, ModuleIo)]
pub struct CpuV3GpuOutput {
    pub device_read_data: Wires<16>,
    pub gpu_ro_request_valid: Wire,
    pub gpu_ro_write: Wire,
    pub gpu_ro_address: Wires<22>,
    pub gpu_ro_write_data: Wires<64>,
    pub gpu_fb_w_request_valid: Wire,
    pub gpu_fb_w_write: Wire,
    pub gpu_fb_w_address: Wires<22>,
    pub gpu_fb_w_write_data: Wires<64>,
    pub gpu_fb_r_request_valid: Wire,
    pub gpu_fb_r_write: Wire,
    pub gpu_fb_r_address: Wires<22>,
    pub gpu_fb_r_write_data: Wires<64>,
}

#[derive(Hardware)]
#[hardware(namespace = "systems/cpu_v3_tang_nano_20k/gpu", target_leaf)]
pub struct CpuV3Gpu;

impl Module for CpuV3Gpu {
    type Input = CpuV3GpuInput;
    type Output = CpuV3GpuOutput;
    type EmuState = GpuCore;

    const USES_MAIN_CLOCK: bool = true;

    fn target_resources() -> Vec<TargetResourceRequest> {
        // Gowin maps the small command-line and two-entry submission arrays to
        // seventeen 64-bit RAM16 leaves. Keep the physical audit explicit so
        // an inference change is caught instead of silently changing packing.
        vec![TargetResourceRequest::new(SsramBits::new(17 * 64))]
    }

    fn create_emu(_input: &Self::Input, _output: &Self::Output) -> Self::EmuState {
        GpuCore::default()
    }

    fn execute_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        output: &Self::Output,
    ) {
        let dev = GpuDeviceBus {
            index: sample_wires::<3>(&input.device_index, circuit) as u8,
            channel: sample_wires::<4>(&input.device_channel, circuit) as u8,
            read_enable: input.device_read_enable.get(circuit) != 0,
            write_enable: input.device_write_enable.get(circuit) != 0,
            write_data: sample_wires::<16>(&input.device_write_data, circuit) as u16,
        };
        let mem = GpuMemoryBus {
            ro_request_ready: input.gpu_ro_request_ready.get(circuit) != 0,
            ro_response_valid: input.gpu_ro_response_valid.get(circuit) != 0,
            ro_read_data: sample_wires::<64>(&input.gpu_ro_read_data, circuit),
            ro_response_last: input.gpu_ro_response_last.get(circuit) != 0,
            ro_error: input.gpu_ro_error.get(circuit) != 0,
            fb_w_request_ready: input.gpu_fb_w_request_ready.get(circuit) != 0,
            fb_w_response_valid: input.gpu_fb_w_response_valid.get(circuit) != 0,
            fb_w_response_last: input.gpu_fb_w_response_last.get(circuit) != 0,
            fb_w_error: input.gpu_fb_w_error.get(circuit) != 0,
        };
        let outputs = state.combine(dev, mem);
        output.drive(
            circuit,
            &CpuV3GpuOutputValue {
                device_read_data: u64::from(outputs.read_data),
                gpu_ro_request_valid: outputs.ro_request_valid,
                gpu_ro_write: outputs.ro_write,
                gpu_ro_address: u64::from(outputs.ro_address),
                gpu_ro_write_data: outputs.ro_write_data,
                gpu_fb_w_request_valid: outputs.fb_w_request_valid,
                gpu_fb_w_write: outputs.fb_w_write,
                gpu_fb_w_address: u64::from(outputs.fb_w_address),
                gpu_fb_w_write_data: outputs.fb_w_write_data,
                gpu_fb_r_request_valid: outputs.fb_r_request_valid,
                gpu_fb_r_write: outputs.fb_r_write,
                gpu_fb_r_address: u64::from(outputs.fb_r_address),
                gpu_fb_r_write_data: outputs.fb_r_write_data,
            },
        );
    }

    fn clock_emu(
        state: &mut Self::EmuState,
        circuit: &mut CircuitWires,
        input: &Self::Input,
        _output: &Self::Output,
    ) {
        let dev = GpuDeviceBus {
            index: sample_wires::<3>(&input.device_index, circuit) as u8,
            channel: sample_wires::<4>(&input.device_channel, circuit) as u8,
            read_enable: input.device_read_enable.get(circuit) != 0,
            write_enable: input.device_write_enable.get(circuit) != 0,
            write_data: sample_wires::<16>(&input.device_write_data, circuit) as u16,
        };
        let mem = GpuMemoryBus {
            ro_request_ready: input.gpu_ro_request_ready.get(circuit) != 0,
            ro_response_valid: input.gpu_ro_response_valid.get(circuit) != 0,
            ro_read_data: sample_wires::<64>(&input.gpu_ro_read_data, circuit),
            ro_response_last: input.gpu_ro_response_last.get(circuit) != 0,
            ro_error: input.gpu_ro_error.get(circuit) != 0,
            fb_w_request_ready: input.gpu_fb_w_request_ready.get(circuit) != 0,
            fb_w_response_valid: input.gpu_fb_w_response_valid.get(circuit) != 0,
            fb_w_response_last: input.gpu_fb_w_response_last.get(circuit) != 0,
            fb_w_error: input.gpu_fb_w_error.get(circuit) != 0,
        };
        state.advance(input.reset.get(circuit) != 0, dev, mem);
    }

    fn verilog_source() -> Option<String> {
        Some(include_str!("gpu.v").to_string())
    }

    fn verilog_testbench() -> Option<String> {
        Some(include_str!("gpu_tb.v").to_string())
    }
}

fn sample_wires<const W: usize>(wires: &Wires<W>, circuit: &CircuitWires) -> u64 {
    let mut value = 0u64;
    for (bit, wire) in wires.wires.iter().enumerate() {
        if wire.get(circuit) != 0 {
            value |= 1 << bit;
        }
    }
    value
}

// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests;
