//! Closed command producer pool: 32 physical quad slots, active24/unpublished8.
mod evaluate;
mod storage;
mod update;
use super::{require, vertex::Bits, ContractError};
use storage::{Depth, Payload, Ports};
const L_READ: u8 = 0;
const L_X: u8 = 1;
const L_PIXEL: u8 = 2;
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub front_ce: bool,
    pub backend_ce: bool,
    pub phase: u8,
    pub draw_unlit: bool,
    pub quad_valid: bool,
    pub quad_x: u8,
    pub quad_y: u8,
    pub quad_rgb: bool,
    pub quad_color: u32,
    pub quad_end_valid: bool,
    pub quad_end_id: u8,
    pub quad_end_mask: u8,
    pub result_write: bool,
    pub result_address: u8,
    pub result_data: Bits,
    pub depth_valid: bool,
    pub depth_pid: u8,
    pub depth_z16: u16,
    pub depth_pass: bool,
    pub lod_valid: bool,
    pub lod_id: u8,
    pub lod_value: u8,
    pub publish_ready: bool,
    pub descriptor_issue_slot: u8,
    pub basic_read: bool,
    pub basic_address: u8,
    pub depth_read: bool,
    pub depth_address: u8,
    pub light_ready: bool,
    pub sample_ready: bool,
    pub retire_valid: bool,
    pub retire_id: u8,
    pub empty_skip: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub quad_ready: bool,
    pub quad_id: u8,
    pub prefetch_valid: bool,
    pub prefetch_tile: u16,
    pub published: bool,
    pub published_slot: u8,
    pub published_begin: Bits,
    pub descriptor_issue: Bits,
    pub descriptor_head: Bits,
    pub descriptor_next: Bits,
    pub issue_color: Bits,
    pub descriptor_head_tile: Bits,
    pub descriptor_next_tile: Bits,
    pub basic_data: Bits,
    pub basic_collision: bool,
    pub depth_data: Bits,
    pub light_valid: bool,
    pub light_normal: Bits,
    pub light_position: Bits,
    pub light_lane: u8,
    pub light_descriptor: u16,
    pub sample_valid: bool,
    pub sample_data: Bits,
    pub sample_row: u8,
    pub sample_last: Bits,
    pub sample_descriptor: u32,
    pub empty_head: bool,
    pub empty_skip_ready: bool,
    pub retire_slot: u8,
    pub global_used: u8,
    pub producer_used: u8,
    pub light_pending: u8,
    pub sample_pending: u8,
    pub busy: bool,
}
#[derive(Clone, Debug)]
pub struct State {
    pub allocated: u8,
    pub publish_ptr: u8,
    pub retired: u8,
    pub complete_tail: u8,
    pub light_cursor: u8,
    pub sample_cursor: u8,
    pub attr_pixel: u8,
    pub light_descriptor: u16,
    pub sample_descriptor: u32,
    pub normal_y_high: Bits,
    pub depth_mask: u8,
    pub light_state: u8,
    pub light_lane: u8,
    pub ndc_x: Bits,
    pub sample_loaded: bool,
    pub sample_lane: u8,
}
impl Default for State {
    fn default() -> Self {
        Self {
            allocated: 0,
            publish_ptr: 0,
            retired: 0,
            complete_tail: 0,
            light_cursor: 0,
            sample_cursor: 0,
            attr_pixel: 0,
            light_descriptor: 0,
            sample_descriptor: 0,
            normal_y_high: Bits::new(0, 6),
            depth_mask: 0,
            light_state: L_READ,
            light_lane: 0,
            ndc_x: Bits::new(0, 16),
            sample_loaded: false,
            sample_lane: 0,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts {
    pub rgb: bool,
    pub geometry: bool,
    pub lod: bool,
    pub depth: u8,
    pub attributes: u16,
}
#[derive(Clone, Debug, Default)]
pub struct Shadow {
    pub depth_pixel: u8,
    pub end_tail: u8,
    pub lod_tail: u8,
    pub attr_word: u8,
    pub attribute_rgb: bool,
    pub facts: [Option<Facts>; 32],
}
#[derive(Clone, Debug)]
pub struct Pool {
    pub state: State,
    pub shadow: Shadow,
    pub normal: Payload,
    pub uv: Payload,
    pub basic: Payload,
    pub depth: Depth,
    pub headers: [Bits; 32],
    pub masks: [Bits; 32],
    pub lods: [Bits; 32],
    pub colors: [Bits; 32],
    pub ndc_q: Bits,
}
impl Default for Pool {
    fn default() -> Self {
        Self {
            state: State::default(),
            shadow: Shadow::default(),
            normal: Payload::default(),
            uv: Payload::default(),
            basic: Payload::default(),
            depth: Depth::default(),
            headers: [Bits::default(); 32],
            masks: [Bits::default(); 32],
            lods: [Bits::default(); 32],
            colors: [Bits::default(); 32],
            ndc_q: Bits::new(0, 32),
        }
    }
}
fn distance(a: u8, b: u8) -> u8 {
    a.wrapping_sub(b) & 63
}
fn descriptor(header: Bits, mask: Bits) -> Bits {
    Bits::concat(header.slice(0, 19), mask.slice(0, 4), 4)
}
fn tile(header: Bits) -> Bits {
    Bits::concat(header.slice(15, 4), header.slice(7, 5), 5)
}
struct Eval {
    o: Output,
    allocate: bool,
    attr_event: bool,
    depth_event: bool,
    lod_event: bool,
    geom_event: bool,
    complete_event: bool,
    normal_write: bool,
    uv_write: bool,
    basic_write: bool,
    normal_data: Bits,
    uv_data: Bits,
    basic_data: Bits,
    normal_mask: u8,
    uv_mask: u8,
    basic_mask: u8,
    next_depth_mask: u8,
    mask_write: bool,
    header_write: Bits,
    light_read: bool,
    ndc_read: bool,
    ndc_address: usize,
    light_skip: bool,
    light_fetch: bool,
    light_take: bool,
    light_meta: Bits,
    light_mask: Bits,
    sample_meta: Bits,
    sample_mask: Bits,
    sample_lod: Bits,
    sample_read_lane: u8,
    sample_next_lane: u8,
    sample_skip: bool,
    sample_fetch: bool,
    sample_take: bool,
    basic_read: bool,
    depth_read: bool,
    skip_empty: bool,
    regular_retire: bool,
}
impl Pool {
    pub(crate) fn observe(&self, i: Input, trace: &mut super::trace::Trace) {
        let e = self.evaluate(i);
        let o = e.o;
        trace.load[6] = u64::from(o.global_used);
        trace.load[7] = u64::from(o.producer_used);
        trace.load[8] = u64::from(o.light_pending);
        trace.load[9] = u64::from(o.sample_pending);
        trace.load[15] = u64::from(e.allocate);
        if e.regular_retire || e.skip_empty {
            trace.event(
                "q_retire",
                [u64::from(o.retire_slot), u64::from(e.skip_empty)],
            );
        }
        if e.allocate {
            trace.event("q_alloc", [u64::from(self.state.allocated & 31)]);
        }
        if o.published {
            trace.event(
                "q_publish",
                [u64::from(o.published_slot), o.published_begin.value & 15],
            );
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        self.evaluate(i).o
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        self.update(i)
    }
}
