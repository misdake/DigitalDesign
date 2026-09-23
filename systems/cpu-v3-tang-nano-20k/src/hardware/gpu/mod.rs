//! System-owned GPU device, temporary command processor, and 8-entry
//! direct-mapped tile framebuffer cache.
//!
//! This is the Phase-1 framebuffer-cache milestone: a fixed 32-byte command /
//! tile-list read master (`gpu_ro`), a 128-byte framebuffer read master
//! (`gpu_fb_r`) and a 128-byte framebuffer write master (`gpu_fb_w`) are driven
//! by one serial command FSM. A draw fetches a list of `u16` tile indices from
//! `gpu_ro`, brings each tile into the cache (LOAD refills from `gpu_fb_r`,
//! CLEAR initializes locally), writes the selected tile rows from the draw
//! color and marks the entry dirty. A dirty victim is cleaned with four
//! 128-byte `gpu_fb_w` transactions before it is reused, and `END` drains every
//! dirty entry before the submission retires.
//!
//! The cache entry array is two synchronous 512x32 arrays addressed by
//! `{entry, beat}`; `gpu.v` mirrors it with the same one-write/one-read port
//! structure.
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
    gpu_dummy_beat, gpu_gradient_beat, gpu_target_slot, GPU_CMD_BASE_HIGH, GPU_CMD_BASE_LOW,
    GPU_CMD_WORDS_HIGH, GPU_CMD_WORDS_LOW, GPU_CONTROL, GPU_CONTROL_CLEAR_ERRORS,
    GPU_CONTROL_RESET, GPU_DEVICE, GPU_EXECUTED_COUNT, GPU_FAKE_DRAW_QWORDS, GPU_FIFO_DEPTH,
    GPU_LOAD_OP_CLEAR, GPU_OPCODE_END, GPU_OPCODE_FAKE_DRAW, GPU_OPCODE_SET_TARGET,
    GPU_QUEUE_LEVEL, GPU_RECEIVED_COUNT, GPU_STATUS, GPU_STATUS_BUSY, GPU_STATUS_COMMAND_ERROR,
    GPU_STATUS_FIFO_FULL, GPU_STATUS_SUBMIT_REJECTED, GPU_SUBMIT, GPU_TILE_TOTAL,
};
use digital_design_circuit::{CircuitWires, Wire, Wires};
use digital_design_hardware::{
    Hardware, Module, ModuleIo, ResourceAmount, ResourceKind, TargetComponent,
    TargetResourceRequest,
};

mod host;

pub(crate) use host::HostGpuMemory;

/// Direct-mapped cache geometry: eight entries of one 16x16 RGB565 tile.
const CACHE_ENTRIES: usize = 8;
/// One tile is a 256-pixel / 512-byte / 64-u64-beat block.
const TILE_BEATS: usize = 64;
const CACHE_BEATS: usize = CACHE_ENTRIES * TILE_BEATS;
/// A 32-byte line carries four u64 beats; a 128-byte transaction carries sixteen.
const BEATS_PER_LINE: usize = 16;
const LINES_PER_TILE: usize = TILE_BEATS / BEATS_PER_LINE;
/// Tile word addressing: 256 words per tile, 64 words per 128-byte line.
const WORDS_PER_TILE: u32 = 256;
const WORDS_PER_LINE: u32 = 64;
/// Tile indices here are u16 entries; every index must be below this bound.
const TILE_INDEX_LIMIT: u16 = GPU_TILE_TOTAL as u16;
/// One 32-byte tile-list read carries sixteen u16 entries.
const LIST_ENTRIES_PER_LINE: u16 = 16;

/// Physical resources inferred by the handwritten GPU leaf.
///
/// A leaf gets one hierarchical allocation label, so resources of different
/// kinds must be grouped into one component instead of emitted as two requests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct GpuResources;

impl TargetComponent for GpuResources {
    fn component_name(&self) -> &'static str {
        "gpu"
    }

    fn resource_requirements(&self) -> Vec<ResourceAmount> {
        vec![
            // The three small command/list/payload arrays occupy 24 RAM16
            // primitives in the fitted system. Account the physical granularity,
            // not just their logical payload bits.
            ResourceAmount::new(ResourceKind::SsramBit, 24 * 64),
            ResourceAmount::new(ResourceKind::Bsram18K, 2),
        ]
    }
}

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

/// The three GPU memory masters' handshake inputs for one cycle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpuMemoryBus {
    pub ro_request_ready: bool,
    pub ro_write_data_ready: bool,
    pub ro_response_valid: bool,
    pub ro_read_data: u64,
    pub ro_response_last: bool,
    pub ro_error: bool,
    pub fb_w_request_ready: bool,
    pub fb_w_write_data_ready: bool,
    pub fb_w_response_valid: bool,
    pub fb_w_response_last: bool,
    pub fb_w_error: bool,
    pub fb_r_request_ready: bool,
    pub fb_r_write_data_ready: bool,
    pub fb_r_response_valid: bool,
    pub fb_r_read_data: u64,
    pub fb_r_response_last: bool,
    pub fb_r_error: bool,
}

/// The GPU's combinational outputs for one cycle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpuOutputs {
    pub read_data: u16,
    pub ro_request_valid: bool,
    pub ro_write: bool,
    pub ro_address: u32,
    pub ro_line_count_minus_1: u8,
    pub ro_write_data: u64,
    pub fb_w_request_valid: bool,
    pub fb_w_write: bool,
    pub fb_w_address: u32,
    pub fb_w_line_count_minus_1: u8,
    pub fb_w_write_data: u64,
    pub fb_r_request_valid: bool,
    pub fb_r_write: bool,
    pub fb_r_address: u32,
    pub fb_r_line_count_minus_1: u8,
    pub fb_r_write_data: u64,
}

/// Serial command/cache phases. At most one memory request is outstanding.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Phase {
    #[default]
    Idle,
    /// `gpu_ro` 32-byte command-line request.
    Fetch,
    /// `gpu_ro` command-line response; four beats.
    Receive,
    Decode,
    /// Execute a fully registered header and payload.
    Execute,
    /// `gpu_ro` 32-byte tile-list request.
    ListFetch,
    /// `gpu_ro` tile-list response; four beats / sixteen entries.
    ListReceive,
    /// Decide whether the current tile hits, misses, or needs eviction.
    TileStep,
    /// One-cycle synchronous cache-read prime before a clean request.
    CleanPrime,
    /// `gpu_fb_w` 128-byte request for a victim or END-drain clean.
    CleanRequest,
    /// `gpu_fb_w` clean response; sixteen beats per line.
    CleanWait,
    /// `gpu_fb_r` 128-byte refill request.
    RefillRequest,
    /// `gpu_fb_r` refill response; sixteen beats per line.
    RefillWait,
    /// Initialize one cache beat per clock from the draw clear color.
    ClearFill,
    /// Overwrite at most one row-mask-selected beat per clock.
    DrawApply,
    /// END drain: scan all entries and clean the dirty ones.
    EndScan,
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
    /// Global qword index of the command line buffer's first qword; `0xffff`
    /// when the buffer holds no line for the current cursor.
    line_qword_base: u16,
    line_buffer: [u64; 4],
    recv_beat: u8,
    // Header held between the header qword and its payload qwords.
    pending_opcode: u8,
    pending_qword_count: u8,
    pending_arg0: u32,
    pending_payload: [u64; 2],
    pending_payload_remaining: u8,
    have_pending: bool,
    // Per-submission target.
    target_set: bool,
    target_base: u32,
    // Framebuffer cache: valid/tag/dirty metadata plus the tile beat array.
    cache: [u64; CACHE_BEATS],
    cache_valid: [bool; CACHE_ENTRIES],
    cache_tag: [u16; CACHE_ENTRIES],
    cache_dirty: [bool; CACHE_ENTRIES],
    // Current draw's tile list and parameters.
    list_buffer: [u64; 4],
    list_beat: u8,
    list_fetch_start: u16,
    list_chunk_start: u16,
    list_chunk_valid: bool,
    draw_tile_count: u16,
    draw_tile_pos: u16,
    draw_list_addr: u32,
    draw_clear_color: u16,
    draw_color: u16,
    draw_row_mask: u16,
    draw_gradient: bool,
    draw_is_clear: bool,
    // Current tile access.
    cur_tile_index: u16,
    cur_entry: u8,
    cur_tag: u16,
    // Blocking clean/refill progress.
    transfer_entry: u8,
    transfer_line: u8,
    transfer_beat: u8,
    clean_for_end: bool,
    // END drain cursor.
    end_scan_index: u8,
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
            pending_payload: [0; 2],
            pending_payload_remaining: 0,
            have_pending: false,
            target_set: false,
            target_base: 0,
            cache: [0; CACHE_BEATS],
            cache_valid: [false; CACHE_ENTRIES],
            cache_tag: [0; CACHE_ENTRIES],
            cache_dirty: [false; CACHE_ENTRIES],
            list_buffer: [0; 4],
            list_beat: 0,
            list_fetch_start: 0,
            list_chunk_start: 0,
            list_chunk_valid: false,
            draw_tile_count: 0,
            draw_tile_pos: 0,
            draw_list_addr: 0,
            draw_clear_color: 0,
            draw_color: 0,
            draw_row_mask: 0,
            draw_gradient: false,
            draw_is_clear: false,
            cur_tile_index: 0,
            cur_entry: 0,
            cur_tag: 0,
            transfer_entry: 0,
            transfer_line: 0,
            transfer_beat: 0,
            clean_for_end: false,
            end_scan_index: 0,
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

    /// Drops every cache entry. A submission starts from a cold cache: `END`
    /// cleans all dirty entries before retiring, so the clean entries are safe
    /// to discard.
    fn invalidate_cache(&mut self) {
        self.cache_valid = [false; CACHE_ENTRIES];
        self.cache_dirty = [false; CACHE_ENTRIES];
    }

    /// Clears the per-submission command and draw state.
    fn reset_command_state(&mut self) {
        self.qword_index = 0;
        self.line_qword_base = 0xffff;
        self.recv_beat = 0;
        self.have_pending = false;
        self.pending_payload_remaining = 0;
        self.target_set = false;
        self.target_base = 0;
        self.draw_tile_count = 0;
        self.draw_tile_pos = 0;
        self.list_chunk_valid = false;
        self.list_chunk_start = 0;
        self.end_scan_index = 0;
        self.invalidate_cache();
    }

    fn control(&mut self, value: u16) {
        if value & GPU_CONTROL_RESET != 0 && !self.busy() {
            // Soft reset is idle-only. It clears the staging registers, FIFO
            // and command state plus sticky errors, but preserves the event
            // counters.
            self.fifo_head = 0;
            self.fifo_count = 0;
            self.active = false;
            self.reset_command_state();
            self.clear_staging();
            self.submit_rejected = false;
            self.command_error = false;
            self.phase = Phase::Idle;
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
        self.reset_command_state();
        self.phase = Phase::Fetch;
    }

    fn enter_error(&mut self) {
        self.phase = Phase::ErrorRetire;
    }

    /// Begins a blocking whole-tile clean of `entry`; `for_end` selects the END
    /// drain instead of a draw-time victim eviction.
    fn start_clean(&mut self, entry: u8, for_end: bool) {
        self.transfer_entry = entry;
        self.transfer_line = 0;
        self.transfer_beat = 0;
        self.clean_for_end = for_end;
        self.phase = Phase::CleanPrime;
    }

    fn clean_address(&self) -> u32 {
        match self.phase {
            Phase::CleanRequest | Phase::CleanWait => {
                let entry = self.transfer_entry;
                let index = (u32::from(self.cache_tag[entry as usize]) << 3) | u32::from(entry);
                self.target_base
                    + index * WORDS_PER_TILE
                    + u32::from(self.transfer_line) * WORDS_PER_LINE
            }
            _ => 0,
        }
    }

    fn clean_beat(&self) -> u64 {
        let base = self.transfer_entry as usize * TILE_BEATS
            + self.transfer_line as usize * BEATS_PER_LINE;
        let beat = (self.transfer_beat as usize).min(BEATS_PER_LINE - 1);
        self.cache[base + beat]
    }

    fn refill_address(&self) -> u32 {
        match self.phase {
            Phase::RefillRequest | Phase::RefillWait => {
                self.target_base
                    + u32::from(self.cur_tile_index) * WORDS_PER_TILE
                    + u32::from(self.transfer_line) * WORDS_PER_LINE
            }
            _ => 0,
        }
    }

    /// Executes a fully collected command. Sets `phase` and must be called from
    /// [`Phase::Decode`].
    fn execute_command(&mut self) {
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
                        // A cache tag is relative to one framebuffer target.
                        // Re-selecting the same target is harmless, and a
                        // target may change before the first tile is acquired.
                        // Switching with resident entries would make a later
                        // clean use the wrong base, so reject it explicitly.
                        if self.target_set
                            && self.target_base != base
                            && self.cache_valid.iter().any(|valid| *valid)
                        {
                            self.enter_error();
                            return;
                        }
                        self.target_set = true;
                        self.target_base = base;
                        self.phase = Phase::Decode;
                    }
                    None => self.enter_error(),
                }
            }
            GPU_OPCODE_FAKE_DRAW => {
                // Three qwords: header, tile-list address, color/row-mask.
                if count != GPU_FAKE_DRAW_QWORDS {
                    self.enter_error();
                    return;
                }
                let tile_count = (arg0 & 0xffff) as u16;
                let load_op = (arg0 >> 16) & 0x3;
                if arg0 >> 18 != 0 || load_op > 1 {
                    self.enter_error();
                    return;
                }
                if !self.target_set {
                    self.enter_error();
                    return;
                }
                let payload0 = self.pending_payload[0];
                let payload1 = self.pending_payload[1];
                if payload0 >> 32 != 0 || payload1 >> 49 != 0 {
                    self.enter_error();
                    return;
                }
                let list_addr = payload0 as u32;
                if tile_count != 0 {
                    // A nonempty list is 32-byte aligned and must stay inside
                    // 22-bit word memory.
                    if list_addr & 0xf != 0
                        || u64::from(list_addr) + u64::from(tile_count) > 1 << 22
                    {
                        self.enter_error();
                        return;
                    }
                }
                self.draw_tile_count = tile_count;
                self.draw_tile_pos = 0;
                self.draw_list_addr = list_addr;
                self.draw_clear_color = payload1 as u16;
                self.draw_color = (payload1 >> 16) as u16;
                self.draw_row_mask = (payload1 >> 32) as u16;
                self.draw_gradient = payload1 & (1u64 << 48) != 0;
                self.draw_is_clear = load_op == u32::from(GPU_LOAD_OP_CLEAR);
                self.list_chunk_valid = false;
                self.list_chunk_start = 0;
                self.phase = if tile_count == 0 {
                    Phase::Decode
                } else {
                    Phase::TileStep
                };
            }
            GPU_OPCODE_END => {
                if count != 1 || arg0 != 0 {
                    self.enter_error();
                    return;
                }
                if !self.target_set {
                    self.enter_error();
                    return;
                }
                self.end_scan_index = 0;
                self.phase = Phase::EndScan;
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
            let slot = (self.pending_qword_count - 1 - self.pending_payload_remaining) as usize;
            if slot < self.pending_payload.len() {
                self.pending_payload[slot] = word;
            }
            self.pending_payload_remaining -= 1;
            self.qword_index += 1;
            if self.pending_payload_remaining == 0 {
                self.have_pending = false;
                self.phase = Phase::Execute;
            }
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
            self.phase = Phase::Execute;
        } else {
            self.pending_payload_remaining = count - 1;
            self.have_pending = true;
        }
    }

    /// Extracts the tile index for `draw_tile_pos`, fetching the 32-byte list
    /// chunk first when the cursor entered a new one.
    fn tile_step(&mut self) {
        if self.draw_tile_pos >= self.draw_tile_count {
            self.phase = Phase::Decode;
            return;
        }
        let chunk = (self.draw_tile_pos / LIST_ENTRIES_PER_LINE) * LIST_ENTRIES_PER_LINE;
        if !self.list_chunk_valid || self.list_chunk_start != chunk {
            self.list_fetch_start = chunk;
            self.phase = Phase::ListFetch;
            return;
        }
        let offset = usize::from(self.draw_tile_pos - self.list_chunk_start);
        let beat = offset / 4;
        let shift = (offset % 4) * 16;
        let index = ((self.list_buffer[beat] >> shift) & 0xffff) as u16;
        if index >= TILE_INDEX_LIMIT {
            self.enter_error();
            return;
        }
        self.cur_tile_index = index;
        self.cur_entry = (index & (CACHE_ENTRIES as u16 - 1)) as u8;
        self.cur_tag = index >> 3;
        let entry = self.cur_entry as usize;
        if self.cache_valid[entry] && self.cache_tag[entry] == self.cur_tag {
            // Hit: LOAD keeps the line, CLEAR re-initializes it locally.
            if self.draw_is_clear {
                self.transfer_entry = self.cur_entry;
                self.transfer_beat = 0;
                self.phase = Phase::ClearFill;
            } else {
                self.transfer_beat = 0;
                self.phase = Phase::DrawApply;
            }
        } else if self.cache_valid[entry] && self.cache_dirty[entry] {
            // Clean the dirty victim first; TileStep re-runs against the now
            // invalid entry.
            self.start_clean(self.cur_entry, false);
        } else {
            self.cache_valid[entry] = false;
            self.cache_dirty[entry] = false;
            if self.draw_is_clear {
                self.transfer_entry = self.cur_entry;
                self.transfer_beat = 0;
                self.phase = Phase::ClearFill;
            } else {
                self.transfer_entry = self.cur_entry;
                self.transfer_line = 0;
                self.transfer_beat = 0;
                self.phase = Phase::RefillRequest;
            }
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
        let ro_address = match self.phase {
            Phase::Fetch => self
                .active_base
                .wrapping_add((u32::from(self.qword_index) >> 2) << 4),
            Phase::ListFetch => self
                .draw_list_addr
                .wrapping_add(u32::from(self.list_fetch_start)),
            _ => 0,
        };
        let line_count_minus_1 = (LINES_PER_TILE - 1) as u8;
        GpuOutputs {
            read_data,
            ro_request_valid: self.phase == Phase::Fetch || self.phase == Phase::ListFetch,
            ro_write: false,
            ro_address,
            // `gpu_ro` stays at the fixed 32-byte (one-line) command/list size.
            ro_line_count_minus_1: 0,
            ro_write_data: 0,
            fb_w_request_valid: self.phase == Phase::CleanRequest,
            fb_w_write: true,
            fb_w_address: self.clean_address(),
            fb_w_line_count_minus_1: line_count_minus_1,
            fb_w_write_data: self.clean_beat(),
            fb_r_request_valid: self.phase == Phase::RefillRequest,
            fb_r_write: false,
            fb_r_address: self.refill_address(),
            fb_r_line_count_minus_1: line_count_minus_1,
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
            Phase::Execute => self.execute_command(),
            Phase::ListFetch => {
                if mem.ro_request_ready {
                    self.phase = Phase::ListReceive;
                    self.list_beat = 0;
                }
            }
            Phase::ListReceive => {
                if mem.ro_response_valid {
                    if mem.ro_error {
                        self.enter_error();
                    } else {
                        self.list_buffer[self.list_beat as usize] = mem.ro_read_data;
                        if self.list_beat == 3 || mem.ro_response_last {
                            self.list_beat = 0;
                            self.list_chunk_start = self.list_fetch_start;
                            self.list_chunk_valid = true;
                            self.phase = Phase::TileStep;
                        } else {
                            self.list_beat += 1;
                        }
                    }
                }
            }
            Phase::TileStep => self.tile_step(),
            Phase::CleanPrime => {
                self.phase = Phase::CleanRequest;
            }
            Phase::CleanRequest => {
                if mem.fb_w_request_ready {
                    // Beat zero is captured on the accepting edge; present beat
                    // one next cycle.
                    self.transfer_beat = self.transfer_beat.wrapping_add(1);
                    self.phase = Phase::CleanWait;
                }
            }
            Phase::CleanWait => {
                if mem.fb_w_write_data_ready && self.transfer_beat < BEATS_PER_LINE as u8 {
                    self.transfer_beat += 1;
                }
                if mem.fb_w_response_valid {
                    if mem.fb_w_error {
                        self.enter_error();
                    } else if mem.fb_w_response_last {
                        let entry = self.transfer_entry as usize;
                        if self.transfer_line + 1 == LINES_PER_TILE as u8 {
                            self.cache_dirty[entry] = false;
                            if self.clean_for_end {
                                self.end_scan_index += 1;
                                self.phase = Phase::EndScan;
                            } else {
                                self.cache_valid[entry] = false;
                                self.phase = Phase::TileStep;
                            }
                        } else {
                            self.transfer_line += 1;
                            self.transfer_beat = 0;
                            self.phase = Phase::CleanPrime;
                        }
                    }
                }
            }
            Phase::RefillRequest => {
                if mem.fb_r_request_ready {
                    self.phase = Phase::RefillWait;
                    self.transfer_beat = 0;
                }
            }
            Phase::RefillWait => {
                if mem.fb_r_response_valid {
                    if mem.fb_r_error {
                        self.enter_error();
                    } else {
                        let slot = self.transfer_entry as usize * TILE_BEATS
                            + self.transfer_line as usize * BEATS_PER_LINE
                            + self.transfer_beat as usize;
                        self.cache[slot] = mem.fb_r_read_data;
                        if self.transfer_beat as usize + 1 == BEATS_PER_LINE
                            || mem.fb_r_response_last
                        {
                            self.transfer_beat = 0;
                            if self.transfer_line as usize + 1 == LINES_PER_TILE {
                                let entry = self.cur_entry as usize;
                                self.cache_valid[entry] = true;
                                self.cache_tag[entry] = self.cur_tag;
                                self.cache_dirty[entry] = false;
                                self.transfer_beat = 0;
                                self.phase = Phase::DrawApply;
                            } else {
                                self.transfer_line += 1;
                                self.phase = Phase::RefillRequest;
                            }
                        } else {
                            self.transfer_beat += 1;
                        }
                    }
                }
            }
            Phase::ClearFill => {
                let entry = self.transfer_entry as usize;
                let beat = self.transfer_beat as usize;
                self.cache[entry * TILE_BEATS + beat] = gpu_dummy_beat(self.draw_clear_color);
                if beat + 1 == TILE_BEATS {
                    self.cache_valid[entry] = true;
                    self.cache_tag[entry] = self.cur_tag;
                    self.cache_dirty[entry] = false;
                    self.transfer_beat = 0;
                    self.phase = Phase::DrawApply;
                } else {
                    self.transfer_beat += 1;
                }
            }
            Phase::DrawApply => {
                let entry = self.cur_entry as usize;
                let beat = self.transfer_beat as usize;
                let row = beat / (TILE_BEATS / 16);
                if self.draw_row_mask & (1u16 << row) != 0 {
                    self.cache[entry * TILE_BEATS + beat] = if self.draw_gradient {
                        gpu_gradient_beat(self.draw_color, beat)
                    } else {
                        gpu_dummy_beat(self.draw_color)
                    };
                }
                if beat + 1 == TILE_BEATS {
                    self.cache_dirty[entry] = true;
                    self.transfer_beat = 0;
                    self.draw_tile_pos += 1;
                    self.phase = Phase::TileStep;
                } else {
                    self.transfer_beat += 1;
                }
            }
            Phase::EndScan => {
                if self.end_scan_index as usize >= CACHE_ENTRIES {
                    self.phase = Phase::Retire;
                } else {
                    let entry = self.end_scan_index as usize;
                    if self.cache_valid[entry] && self.cache_dirty[entry] {
                        self.start_clean(self.end_scan_index, true);
                    } else {
                        self.end_scan_index += 1;
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
    pub gpu_ro_write_data_ready: Wire,
    pub gpu_ro_response_valid: Wire,
    pub gpu_ro_read_data: Wires<64>,
    pub gpu_ro_response_last: Wire,
    pub gpu_ro_error: Wire,

    pub gpu_fb_w_request_ready: Wire,
    pub gpu_fb_w_write_data_ready: Wire,
    pub gpu_fb_w_response_valid: Wire,
    pub gpu_fb_w_response_last: Wire,
    pub gpu_fb_w_error: Wire,

    pub gpu_fb_r_request_ready: Wire,
    pub gpu_fb_r_write_data_ready: Wire,
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
    pub gpu_ro_line_count_minus_1: Wires<2>,
    pub gpu_ro_write_data: Wires<64>,
    pub gpu_fb_w_request_valid: Wire,
    pub gpu_fb_w_write: Wire,
    pub gpu_fb_w_address: Wires<22>,
    pub gpu_fb_w_line_count_minus_1: Wires<2>,
    pub gpu_fb_w_write_data: Wires<64>,
    pub gpu_fb_r_request_valid: Wire,
    pub gpu_fb_r_write: Wire,
    pub gpu_fb_r_address: Wires<22>,
    pub gpu_fb_r_line_count_minus_1: Wires<2>,
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
        // seventeen 64-bit RAM16 leaves. The 512x64 framebuffer beat cache is
        // two inferred 512x32 synchronous 1R1W BSRAMs. Keep both parts in one
        // allocation because a target leaf has one hierarchical label.
        vec![TargetResourceRequest::new(GpuResources)]
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
            ro_write_data_ready: input.gpu_ro_write_data_ready.get(circuit) != 0,
            ro_response_valid: input.gpu_ro_response_valid.get(circuit) != 0,
            ro_read_data: sample_wires::<64>(&input.gpu_ro_read_data, circuit),
            ro_response_last: input.gpu_ro_response_last.get(circuit) != 0,
            ro_error: input.gpu_ro_error.get(circuit) != 0,
            fb_w_request_ready: input.gpu_fb_w_request_ready.get(circuit) != 0,
            fb_w_write_data_ready: input.gpu_fb_w_write_data_ready.get(circuit) != 0,
            fb_w_response_valid: input.gpu_fb_w_response_valid.get(circuit) != 0,
            fb_w_response_last: input.gpu_fb_w_response_last.get(circuit) != 0,
            fb_w_error: input.gpu_fb_w_error.get(circuit) != 0,
            fb_r_request_ready: input.gpu_fb_r_request_ready.get(circuit) != 0,
            fb_r_write_data_ready: input.gpu_fb_r_write_data_ready.get(circuit) != 0,
            fb_r_response_valid: input.gpu_fb_r_response_valid.get(circuit) != 0,
            fb_r_read_data: sample_wires::<64>(&input.gpu_fb_r_read_data, circuit),
            fb_r_response_last: input.gpu_fb_r_response_last.get(circuit) != 0,
            fb_r_error: input.gpu_fb_r_error.get(circuit) != 0,
        };
        let outputs = state.combine(dev, mem);
        output.drive(
            circuit,
            &CpuV3GpuOutputValue {
                device_read_data: u64::from(outputs.read_data),
                gpu_ro_request_valid: outputs.ro_request_valid,
                gpu_ro_write: outputs.ro_write,
                gpu_ro_address: u64::from(outputs.ro_address),
                gpu_ro_line_count_minus_1: u64::from(outputs.ro_line_count_minus_1),
                gpu_ro_write_data: outputs.ro_write_data,
                gpu_fb_w_request_valid: outputs.fb_w_request_valid,
                gpu_fb_w_write: outputs.fb_w_write,
                gpu_fb_w_address: u64::from(outputs.fb_w_address),
                gpu_fb_w_line_count_minus_1: u64::from(outputs.fb_w_line_count_minus_1),
                gpu_fb_w_write_data: outputs.fb_w_write_data,
                gpu_fb_r_request_valid: outputs.fb_r_request_valid,
                gpu_fb_r_write: outputs.fb_r_write,
                gpu_fb_r_address: u64::from(outputs.fb_r_address),
                gpu_fb_r_line_count_minus_1: u64::from(outputs.fb_r_line_count_minus_1),
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
            ro_write_data_ready: input.gpu_ro_write_data_ready.get(circuit) != 0,
            ro_response_valid: input.gpu_ro_response_valid.get(circuit) != 0,
            ro_read_data: sample_wires::<64>(&input.gpu_ro_read_data, circuit),
            ro_response_last: input.gpu_ro_response_last.get(circuit) != 0,
            ro_error: input.gpu_ro_error.get(circuit) != 0,
            fb_w_request_ready: input.gpu_fb_w_request_ready.get(circuit) != 0,
            fb_w_write_data_ready: input.gpu_fb_w_write_data_ready.get(circuit) != 0,
            fb_w_response_valid: input.gpu_fb_w_response_valid.get(circuit) != 0,
            fb_w_response_last: input.gpu_fb_w_response_last.get(circuit) != 0,
            fb_w_error: input.gpu_fb_w_error.get(circuit) != 0,
            fb_r_request_ready: input.gpu_fb_r_request_ready.get(circuit) != 0,
            fb_r_write_data_ready: input.gpu_fb_r_write_data_ready.get(circuit) != 0,
            fb_r_response_valid: input.gpu_fb_r_response_valid.get(circuit) != 0,
            fb_r_read_data: sample_wires::<64>(&input.gpu_fb_r_read_data, circuit),
            fb_r_response_last: input.gpu_fb_r_response_last.get(circuit) != 0,
            fb_r_error: input.gpu_fb_r_error.get(circuit) != 0,
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
