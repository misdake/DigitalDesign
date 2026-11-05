//! Command profile of the selected eight-line framebuffer TDP cache.
//! Four real dual-port banks: A follows compute CE, B advances on wall edges.
//! Runtime bases are in 512-byte units; UI/copy reuse the same physical banks.
mod evaluate;
mod sink;
pub mod storage;
mod update;
use super::{
    pixel_math::{Rop, RopInput, RopOutput},
    require,
    vertex::Bits,
    ContractError,
};
pub use sink::{Sink, SinkInput, SinkOutput};
use storage::{Bank, Port};
#[derive(Clone, Copy, Debug, Default)]
pub struct Serial {
    pub active: bool,
    pub command_valid: bool,
    pub writing: bool,
    pub lower_ready: bool,
    pub tile: u16,
    pub copy: bool,
    pub copy_address: Bits,
    pub pixel_valid: bool,
    pub pixel_half: bool,
    pub pixel_word: u8,
    pub pixels: Bits,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub serial: Serial,
    pub reset: bool,
    pub ce: bool,
    pub commit_allow: bool,
    pub shade_phase: bool,
    pub shared_rop_product: Bits,
    pub row_valid: bool,
    pub tile: u16,
    pub x: u16,
    pub y: u8,
    pub mask: u8,
    pub row_index: u8,
    pub data: Bits,
    pub alpha: Bits,
    pub depth_func: u8,
    pub depth_write: bool,
    pub blend: bool,
    pub color_base: u16,
    pub depth_base: u16,
    pub flush_request: bool,
    pub prefetch_abort: bool,
    pub prefetch_valid: bool,
    pub prefetch_tile: u16,
    pub mem_req_ready: bool,
    pub mem_read_data: u64,
    pub mem_read_valid: bool,
    pub mem_write_ready: bool,
    pub mem_complete_valid: bool,
    pub mem_complete_ok: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub serial_ready: bool,
    pub serial_done: bool,
    pub mem_req_address_known: u32,
    pub row_ready: bool,
    pub row_accept: bool,
    pub commit_valid: bool,
    pub commit_x: Bits,
    pub commit_y: Bits,
    pub commit_mask: Bits,
    pub flush_complete: bool,
    pub idle: bool,
    pub mem_req_valid: bool,
    pub mem_req_write: bool,
    pub mem_req_address: u32,
    pub mem_write_valid: bool,
    pub mem_write_data: Bits,
    pub shared_rop_offer: bool,
    pub shared_rop_ce: bool,
    pub shared_rop_a: Bits,
    pub shared_rop_b: Bits,
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub mt_active: bool,
    pub mt_tvalid: bool,
    pub mt_write: bool,
    pub mt_line: u8,
    pub mt_target: u16,
    pub mt_remaining: u8,
    pub mt_issue: u8,
    pub mt_retire: u8,
    pub mt_data: u8,
    pub wr_beat_valid: bool,
    pub flushing: bool,
    pub mt_cancel: bool,
    pub mt_request: bool,
    pub tag: [u16; 8],
    pub valid: [bool; 8],
    pub dirty_color: [bool; 8],
    pub dirty_depth: [bool; 8],
    pub victim: u8,
    pub hints: [u16; 2],
    pub hint_count: u8,
    pub mt_prefetch: bool,
    pub protect_valid: bool,
    pub protect_line: u8,
    pub cal: u8,
    pub fill_row: u8,
    pub ctx_valid: [bool; 2],
    pub ctx_line: [u8; 2],
    pub ctx_x: [u16; 2],
    pub ctx_y: [u8; 2],
    pub ctx_mask: [Bits; 2],
    pub ctx_line_mask: [u8; 2],
    pub tail: bool,
    pub result_owner: bool,
    pub return_key: u8,
    pub source_rgb: Bits,
    pub compare_bank: u8,
    pub depth_pending: bool,
    pub color_pending: bool,
    pub depth_data: Bits,
    pub color_data: Bits,
    pub result_address: u16,
    pub result_bank: u8,
    pub result_depth_flag: Bits,
    pub result_color_flag: Bits,
    pub result_last: bool,
    pub result_context: bool,
    pub commit_valid: bool,
    pub commit_x: Bits,
    pub commit_y: Bits,
    pub commit_mask: Bits,
    pub flush_complete: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Cache {
    pub state: State,
    pub rop: Rop,
    pub banks: [Bank; 4],
}
#[derive(Clone, Debug, Default)]
struct Evaluation {
    o: Output,
    ri: RopInput,
    ro: RopOutput,
    a: [Port; 4],
    b: [Port; 4],
    a_read: [bool; 4],
    a_write_d: [bool; 4],
    a_write_c: [bool; 4],
    mt_end: u8,
    data_end: u8,
    row_key: u16,
    hits: u8,
    hit: bool,
    hit_line: u8,
    ingress_error: bool,
    extra_read: bool,
    return_bad: bool,
    req_accept: bool,
    color_fire: bool,
    depth_input_fire: bool,
    row0_fire: bool,
    leaf_ce: bool,
    return_fire: bool,
    depth_fire: bool,
    color_fire_write: bool,
    incoming_bank: u8,
    rbeat: bool,
    wbeat: bool,
    read_issue: bool,
    data_last: bool,
    retire_last: bool,
    issue_last: bool,
    pinned: u8,
    result_line_mask: u8,
    dirty_found: Option<u8>,
    any_dirty: bool,
    select_line: u8,
    sel_line: u8,
    sel_color: bool,
    sel_depth: bool,
    sel_dirty: bool,
    sel_plane: bool,
    hint_start: bool,
    hint_unsafe: bool,
    hint_pop: bool,
    hint_push: bool,
}
impl Cache {
    pub(crate) fn observe(&self, i: Input, trace: &mut super::trace::Trace, quad_slot: u8) {
        let e = self.evaluate(i);
        let s = &self.state;
        trace.cache = Some(super::trace::Cache {
            valid: s.valid,
            tags: s.tag,
            color_base: i.color_base,
            depth_base: i.depth_base,
        });
        trace.load[14] = s.ctx_valid.into_iter().map(u64::from).sum();
        trace.load[30] = u64::from(s.hint_count);
        trace.load[31] = u64::from(s.mt_active);
        if i.reset {
            return;
        }
        if e.row0_fire {
            trace.event("rop_begin", [u64::from(s.tail), u64::from(quad_slot)]);
        }
        if !i.serial.active {
            for (bank, a) in e.a.into_iter().enumerate() {
                if a.en && a.we {
                    trace.event(
                        "cache_write",
                        [
                            bank as u64,
                            u64::from(a.address),
                            a.data.value & 65535,
                            a.data.known & 65535,
                            u64::from(s.result_context),
                        ],
                    );
                }
            }
        }
        if e.color_fire_write && s.result_last {
            trace.event("q_commit", [u64::from(s.result_context)]);
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.evaluate(i).o
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        self.update(i)
    }
}
