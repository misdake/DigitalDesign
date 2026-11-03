//! Selected production recursive Raster and its physically scheduled scanner.
mod evaluate;
mod storage;
mod update;
use super::{require, vertex::Bits, ContractError};
use std::sync::OnceLock;
use storage::{Origin, Rf, RfPorts, StepPorts, Steps};
const IDLE: u8 = 0;
const IMPORT: u8 = 1;
const EXEC: u8 = 2;
const BOUND: u8 = 3;
const OFF: u8 = 0;
const LOAD: u8 = 1;
const CHECK: u8 = 2;
const ALIGN: u8 = 3;
const SCAN: u8 = 4;
const S_LOAD: u8 = 0;
const S_ROW: u8 = 1;
const S_SCAN: u8 = 2;
const S_PREFETCH: u8 = 3;
const S_CHECK: u8 = 4;
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub ce: bool,
    pub phase: u8,
    pub bbox_done: bool,
    pub bbox_empty: bool,
    pub bbox_xlo: u8,
    pub bbox_xhi: u8,
    pub bbox_ylo: u8,
    pub bbox_yhi: u8,
    pub run_enable: bool,
    pub start: bool,
    pub geo_write: bool,
    pub geo_address: u8,
    pub geo_data: Bits,
    pub quad_ready: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub quad_valid: bool,
    pub quad_take: bool,
    pub quad_x: u8,
    pub quad_y: u8,
    pub sample_valid: bool,
    pub sample_lane: u8,
    pub sample_h: Bits,
    pub sample_cover: bool,
    pub sample_anchor: bool,
    pub quad_end_valid: bool,
    pub quad_mask: u8,
    pub busy: bool,
    pub done: bool,
    pub full_scan: bool,
    pub evt_import: bool,
    pub evt_prep: bool,
    pub evt_traverse: bool,
    pub evt_row: bool,
    pub evt_align: bool,
    pub evt_credit_wait: bool,
    pub evt_leaf: bool,
    pub evt_empty_quad: bool,
    pub evt_node: bool,
}
#[derive(Clone, Debug, Default)]
pub struct State {
    pub phase: u8,
    pub life: u8,
    pub scan_mode: u8,
    pub busy: bool,
    pub done: bool,
    pub tau: u8,
    pub w_pc: u8,
    pub w_edge: u8,
    pub s_edge: u8,
    pub prep_level: u8,
    pub level: u8,
    pub nx: u16,
    pub ny: u16,
    pub walk_done: bool,
    pub w_pass: u8,
    pub w_epoch: bool,
    pub row_first: bool,
    pub tile_head: u8,
    pub tile_tail: u8,
    pub tile_meta: [u32; 2],
    pub scan_slot: bool,
    pub scan_full: bool,
    pub scan_qvalid: bool,
    pub scan_inflight: bool,
    pub anchor_seen: bool,
    pub scan_bank: bool,
    pub prefetch_active: bool,
    pub scan_classify_pending: bool,
    pub check_mask: u8,
    pub origin_x: u16,
    pub sx: u16,
    pub sy: u16,
    pub end_x: u16,
    pub end_y: u16,
    pub lane: u8,
    pub cover_acc: bool,
    pub cover_mask: u8,
    pub rom_valid: bool,
    pub rom_pc: u8,
    pub r_valid: bool,
    pub r_sub: bool,
    pub r_edge: u8,
    pub r_tw: u8,
    pub r_uop: u64,
    pub r_pc: u8,
    pub r_epoch: bool,
    pub s_valid: bool,
    pub s_kind: u8,
    pub s_return_edge: u8,
    pub s_sub: bool,
    pub origin_valid: u8,
    pub origin_edge0: u8,
    pub origin_edge1: u8,
    pub check_tau: bool,
    pub sim_geo_count: u8,
    pub mail_written: [u8; 2],
}
#[derive(Clone, Debug)]
pub struct Raster {
    pub state: State,
    pub work: Rf<16>,
    pub mailbox: Rf<8>,
    pub scan: Rf<16>,
    pub nx: Rf<4>,
    pub ny: Rf<4>,
    pub steps: Steps,
    pub origin: Origin,
    pub uop: u64,
}
impl Default for Raster {
    fn default() -> Self {
        Self {
            state: State {
                level: 9,
                ..Default::default()
            },
            work: Rf::new(30),
            mailbox: Rf::new(30),
            scan: Rf::new(30),
            nx: Rf::new(18),
            ny: Rf::new(18),
            steps: Steps::default(),
            origin: Origin::default(),
            uop: 0,
        }
    }
}
fn program() -> &'static [u64; 128] {
    static WORDS: OnceLock<[u64; 128]> = OnceLock::new();
    WORDS.get_or_init(|| {
        let mut words = [0; 128];
        let mut seen = [false; 128];
        for line in include_str!("../../../frontend/vliw/raster_program.v").lines() {
            if let Some(tail) = line.trim().strip_prefix("mem[") {
                let (address, tail) = tail.split_once("]=36'h").expect("production raster word");
                let address: usize = address.parse().unwrap();
                words[address] = u64::from_str_radix(tail.trim_end_matches(';'), 16).unwrap();
                seen[address] = true;
            }
        }
        assert!(seen.into_iter().all(|v| v));
        words
    })
}
fn pass(h: Bits, tau: bool) -> bool {
    h.value & (1 << 29) == 0 && (!tau || h.value & 0x3fffffff != 0)
}
fn alu(a: Bits, b: Bits, sub: bool) -> Bits {
    if a.is_known(30) && b.is_known(30) {
        Bits::new(
            if sub {
                a.value.wrapping_sub(b.value)
            } else {
                a.value.wrapping_add(b.value)
            },
            30,
        )
    } else {
        Bits::default()
    }
}
struct Eval {
    o: Output,
    work: RfPorts,
    mailbox: RfPorts,
    scan: RfPorts,
    nx: RfPorts,
    ny: RfPorts,
    steps: StepPorts,
    packet_take: bool,
    input_take: bool,
    root_level: u8,
    bbox_origin: [u16; 2],
    tile_count: u8,
    tile_empty: bool,
    tile_full: bool,
    head_meta: u32,
    tile_end: [u16; 2],
    row_end: bool,
    scan_end: bool,
    check_finish: bool,
    check_accept: bool,
    reject_tail: bool,
    scanner_finishing: bool,
    rom_read: bool,
    rom_address: u8,
    origin_op: bool,
    xbit: bool,
    ybit: bool,
    return_side: u16,
    fast_w: bool,
    micro_issue: bool,
    micro_sub: bool,
    micro_tw: u8,
    w_retire: bool,
    w_cond_retire: bool,
    cond_pass: bool,
    w_static_tail: bool,
    w_decide: bool,
    origin_issue: bool,
    origin_retire: bool,
    start_tile: bool,
    preload_issue: bool,
    load_issue: bool,
    load_edge: u8,
    prefetch_issue: bool,
    fetch_check: bool,
    scan_read: bool,
    row_issue: bool,
    scan_start: bool,
    check_start: bool,
    scan_fetch0: bool,
    scan_rf_read: bool,
    scan_read_edge: u8,
    scan_cover_valid: bool,
    prefetch_handoff: bool,
    prefetch_start: bool,
    prefetch_vec: u8,
    s_load_retire: bool,
    predicate_write: bool,
    next_quad_mask: u8,
    threshold_read: bool,
    threshold_edge: u8,
    prep_last: u8,
    issue_next_pc: u8,
    branch_target: u8,
    branch_split: bool,
    branch_all_pass: bool,
    tile_publish: bool,
    mailbox_write: bool,
    origin_n: [Bits; 2],
    origin_xy: [u32; 2],
    origin_h: Bits,
}
impl Raster {
    pub fn outputs(&self, i: Input) -> Output {
        self.evaluate(i).o
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        self.update(i)
    }
}
