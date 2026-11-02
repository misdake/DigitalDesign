//! Native rising-edge mirror of the fixed fitted CL3 controller profile.
//! DQ capture is an explicit input; memory and physical delay belong to the caller.
use super::gear::{ControllerInput, Feedback};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub feedback: Feedback,
    pub read_known: u32,
    pub read_chain_pending: bool,
    pub stream_stats: u32,
    pub command: u8,
    pub cke: bool,
    pub address: u16,
    pub bank: u8,
    pub dqm: u8,
    pub dq_oe_n: u32,
    pub dq_out: u32,
}

/// Four-state input boundary: unowned/floating bits must not become owned data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample32 {
    pub value: u32,
    pub known: u32,
}
impl From<u32> for Sample32 {
    fn from(value: u32) -> Self {
        Self {
            value,
            known: u32::MAX,
        }
    }
}

const POWER: u8 = 0;
const PRECHARGE: u8 = 1;
const WAIT: u8 = 2;
const REFRESH1: u8 = 3;
const REFRESH2: u8 = 4;
const MODE: u8 = 5;
const INIT_DONE: u8 = 6;
const IDLE: u8 = 10;
const REFRESH: u8 = 11;
const ACTIVATE: u8 = 12;
const WRITE_LAUNCH: u8 = 14;
const WRITE: u8 = 15;
const WRITE_END: u8 = 16;
const READ_LAUNCH: u8 = 17;
const READ: u8 = 18;
const COMPLETE: u8 = 20;
const RESOLVE: u8 = 21;

fn mapped(address: u32) -> u32 {
    let address = address & 0x1f_ffff;
    ((address >> 5) & 3) << 19 | ((address & 31) | ((address >> 7) << 5))
}
fn bank(address: u32) -> usize {
    ((address >> 19) & 3) as usize
}
fn row(address: u32) -> u16 {
    ((address >> 8) & 0x7ff) as u16
}

/// Only the production profile is implemented; constructor changes init duration.
/// Dead registers of disabled parameter branches do not participate in the mirror.
#[derive(Clone, Copy, Debug)]
pub struct Controller {
    init_cycles: u16,
    state: u8,
    after_wait: u8,
    init_count: u16,
    wait_count: u8,
    command: u8,
    dq_drive: bool,
    dqm_state: u8,
    dq_oe_n: u32,
    dq_out: u32,
    dqm: u8,
    address: u16,
    bank: u8,
    cke: bool,
    done: bool,
    read_data: u32,
    read_known: u32,
    read_valid: bool,
    initialized: bool,
    write_index: u8,
    stream_stats: u32,
    command_age: u8,
    read_active: bool,
    stream_left: u8,
    stream_delay: u8,
    stream_tail: u8,
    refresh_age: u16,
    prep_refresh_ok: bool,
    refresh_due: bool,
    chain_refresh_ok: bool,
    row_valid: u8,
    rows: [u16; 4],
    saved_address: u32,
    saved_words: u8,
    read_stop_age: u8,
    write_chain_eligible: bool,
    saved_last: u8,
    saved_write: bool,
    saved_mask: u8,
    prep_wait: u8,
    prep_rp: u8,
    prep_needed: bool,
    prep_open: bool,
    prep_address: u32,
    request_matches_q: u8,
    next_matches_q: bool,
    next_generation_q: bool,
    next_identity_q: u16,
    read_chain_slot: bool,
    read_prepare_window: bool,
    write_prepare_window: bool,
    launch_age: u8,
    observed_chain: bool,
    read_chain_pending: bool,
}
impl Default for Controller {
    fn default() -> Self {
        Self::new(21_600)
    }
}
impl Controller {
    pub fn new(init_cycles: u16) -> Self {
        assert!(init_cycles > 0);
        Self {
            init_cycles,
            state: POWER,
            after_wait: POWER,
            init_count: 0,
            wait_count: 0,
            command: 7,
            dq_drive: false,
            dqm_state: 15,
            dq_oe_n: u32::MAX,
            dq_out: 0,
            dqm: 15,
            address: 0,
            bank: 0,
            cke: false,
            done: false,
            read_data: 0,
            read_known: u32::MAX,
            read_valid: false,
            initialized: false,
            write_index: 0,
            stream_stats: 0,
            command_age: 0,
            read_active: false,
            stream_left: 0,
            stream_delay: 0,
            stream_tail: 0,
            refresh_age: 0,
            prep_refresh_ok: true,
            refresh_due: false,
            chain_refresh_ok: true,
            row_valid: 0,
            rows: [0; 4],
            saved_address: 0,
            saved_words: 0,
            read_stop_age: 0,
            write_chain_eligible: false,
            saved_last: 0,
            saved_write: false,
            saved_mask: 0,
            prep_wait: 0,
            prep_rp: 0,
            prep_needed: false,
            prep_open: false,
            prep_address: 0,
            request_matches_q: 0,
            next_matches_q: false,
            next_generation_q: false,
            next_identity_q: 0,
            read_chain_slot: false,
            read_prepare_window: false,
            write_prepare_window: false,
            launch_age: 0,
            observed_chain: false,
            read_chain_pending: false,
        }
    }
    fn matches(&self, address: u32) -> bool {
        self.row_valid & (1 << bank(address)) != 0 && self.rows[bank(address)] == row(address)
    }
    pub fn output(&self, i: &ControllerInput) -> Output {
        let qualified = self.next_matches_q && self.next_generation_q == i.next_generation;
        let prepared = !self.prep_needed && self.prep_wait == 0;
        let read_chain =
            self.read_chain_slot && i.next_valid && i.next_chainable && qualified && prepared;
        let chain = self.state == WRITE
            && self.write_index == self.saved_last
            && self.write_chain_eligible
            && i.next_valid
            && i.next_chainable
            && i.next_write_ready
            && qualified
            && prepared
            && self.chain_refresh_ok;
        Output {
            feedback: Feedback {
                ready: self.state == IDLE
                    && self.initialized
                    && !self.refresh_due
                    && self.prep_wait == 0
                    && self.prep_rp == 0,
                done: self.done,
                initialized: self.initialized,
                read_valid: self.read_valid,
                read_data: self.read_data,
                write_index: self.write_index,
                phase: self.command << 5 | self.state,
                chain_accept: chain,
                read_chain_accept: read_chain,
            },
            read_known: self.read_known,
            read_chain_pending: self.read_chain_pending,
            stream_stats: self.stream_stats,
            command: self.command,
            cke: self.cke,
            address: self.address,
            bank: self.bank,
            dqm: self.dqm,
            dq_oe_n: self.dq_oe_n,
            dq_out: self.dq_out,
        }
    }
    fn activate(&mut self, address: u32) {
        self.command = 3;
        self.address = row(address);
        self.row_valid |= 1 << bank(address);
        self.rows[bank(address)] = row(address);
    }
    fn prepare(&mut self, old: &Self, i: &ControllerInput, next: u32, window: bool) {
        let needs =
            old.next_generation_q == i.next_generation && !old.next_matches_q && old.prep_wait == 0;
        if i.next_valid
            && bank(next) != bank(old.saved_address)
            && needs
            && !old.prep_needed
            && old.prep_refresh_ok
            && window
        {
            self.prep_needed = true;
            self.prep_open = old.row_valid & (1 << bank(next)) != 0;
            self.prep_address = next;
        }
        if old.prep_needed && old.prep_rp == 0 && window {
            self.bank = bank(old.prep_address) as u8;
            if old.prep_open {
                self.command = 2;
                self.address = 0;
                self.row_valid &= !(1 << bank(old.prep_address));
                self.prep_rp = 2;
                self.prep_open = false;
            } else {
                self.activate(old.prep_address);
                self.prep_wait = 2;
                self.prep_needed = false;
            }
        }
    }
    /// Commit one rising native edge; `dq_sample` is the pre-edge consumed input.
    pub fn clock(
        &mut self,
        i: &ControllerInput,
        read_boundary: bool,
        dq_sample: impl Into<Sample32>,
    ) {
        let dq_sample = dq_sample.into();
        let old = *self;
        let out = old.output(i);
        let address = mapped(i.address);
        let next = mapped(i.next_address);
        let words = i.words & 63;
        let next_words = i.next_words & 63;
        let capture = old.read_active && old.stream_delay == 0 && old.stream_left != 0;
        let column = old.read_chain_slot && i.next_valid && !old.prep_needed && old.prep_wait == 0;
        if !i.reset {
            assert!(
                !i.next_valid
                    || old.next_generation_q != i.next_generation
                    || old.next_identity_q == (next >> 8) as u16,
                "native next-generation/row identity mismatch"
            );
            if old.initialized {
                assert_eq!(old.refresh_due, old.refresh_age >= 1100);
                assert_eq!(old.chain_refresh_ok, old.refresh_age < 1000);
            }
        }
        self.dq_out = i.write_data;
        self.read_data = dq_sample.value;
        self.read_known = dq_sample.known;
        self.read_valid = capture && !i.reset;
        self.dqm = if i.reset {
            15
        } else if old.state == WRITE_LAUNCH {
            old.saved_mask
        } else if old.state == READ_LAUNCH {
            0
        } else {
            old.dqm_state
        };
        let oe = if i.reset {
            true
        } else if old.state == WRITE_LAUNCH {
            false
        } else if old.state == WRITE_END {
            true
        } else {
            !old.dq_drive
        };
        self.dq_oe_n = if oe { u32::MAX } else { 0 };
        self.command = 7;
        self.done = false;
        self.next_matches_q = old.matches(next);
        self.next_generation_q = i.next_generation;
        self.request_matches_q = (0..4).fold(0, |bits, b| {
            bits | u8::from(old.row_valid & (1 << b) != 0 && old.rows[b] == row(address)) << b
        });
        self.next_identity_q = (next >> 8) as u16;
        self.read_chain_slot = old.state == READ
            && old.command_age == old.saved_last.wrapping_sub(1)
            && old.saved_words >= 8
            && old.refresh_age < 999;
        if old.state == READ && old.command_age == old.saved_last.wrapping_sub(1) {
            self.read_prepare_window = false;
        }
        self.launch_age = old.launch_age.saturating_add(1);
        if out.feedback.chain_accept || out.feedback.read_chain_accept {
            self.observed_chain = true;
        }
        if matches!(old.command, 4 | 5) {
            self.launch_age = 1;
            if old.observed_chain {
                self.observed_chain = false;
                self.stream_stats = old.stream_stats.wrapping_add(256) & 0xff_ffff;
                if old.launch_age != old.saved_words {
                    self.stream_stats |= 128;
                }
                self.stream_stats = self.stream_stats & !127 | u32::from(old.launch_age & 127);
            }
        }
        self.refresh_age = old.refresh_age.saturating_add(1).min(4095);
        self.prep_refresh_ok = old.refresh_age < 989;
        self.refresh_due = old.refresh_age >= 1099;
        self.chain_refresh_ok = old.refresh_age < 999;
        if old.read_active {
            if old.stream_delay != 0 {
                self.stream_delay = old.stream_delay - 1;
            } else if old.stream_left != 0 {
                self.stream_left = old.stream_left - 1;
                if old.stream_left == 1 {
                    self.stream_tail = 2;
                }
            } else if old.stream_tail != 0 {
                self.stream_tail = old.stream_tail - 1;
            }
            self.command_age = old.command_age.wrapping_add(1);
        }
        if read_boundary {
            self.read_chain_pending = false;
        }
        if old.prep_wait != 0 {
            self.prep_wait = old.prep_wait - 1;
        }
        if old.prep_rp != 0 {
            self.prep_rp = old.prep_rp - 1;
        }
        match old.state {
            POWER => {
                if old.init_count == old.init_cycles - 1 {
                    self.cke = true;
                    self.state = PRECHARGE;
                } else {
                    self.init_count = old.init_count.wrapping_add(1);
                }
            }
            PRECHARGE => {
                self.command = 2;
                self.address = 0x400;
                self.wait_count = 2;
                self.after_wait = REFRESH1;
                self.state = WAIT;
            }
            WAIT => {
                if old.wait_count == 1 {
                    self.state = old.after_wait;
                } else {
                    self.wait_count = old.wait_count.wrapping_sub(1);
                }
            }
            REFRESH1 | REFRESH2 => {
                self.command = 1;
                self.wait_count = 8;
                self.after_wait = if old.state == REFRESH1 {
                    REFRESH2
                } else {
                    MODE
                };
                self.state = WAIT;
            }
            MODE => {
                self.command = 0;
                self.bank = 0;
                self.address = 55;
                self.wait_count = 1;
                self.after_wait = INIT_DONE;
                self.state = WAIT;
            }
            INIT_DONE => {
                self.initialized = true;
                self.refresh_age = 0;
                self.refresh_due = false;
                self.chain_refresh_ok = true;
                self.state = IDLE;
            }
            IDLE => {
                if old.prep_wait != 0 || old.prep_rp != 0 {
                } else if old.refresh_due {
                    if old.row_valid != 0 {
                        self.command = 2;
                        self.address = 0x400;
                        self.row_valid = 0;
                        self.wait_count = 2;
                        self.after_wait = REFRESH;
                        self.state = WAIT;
                    } else {
                        self.state = REFRESH;
                    }
                } else if i.request && out.feedback.ready {
                    self.write_chain_eligible = words & 56 != 0;
                    self.saved_address = address;
                    self.saved_words = words;
                    self.read_stop_age = words + if words == 1 { 2 } else { 0 };
                    self.saved_last = words.wrapping_sub(1) & 63;
                    self.saved_write = i.writing;
                    self.saved_mask = i.write_mask & 15;
                    self.write_index = 0;
                    self.bank = bank(address) as u8;
                    self.state = RESOLVE;
                }
            }
            REFRESH => {
                self.command = 1;
                self.refresh_age = 0;
                self.refresh_due = false;
                self.chain_refresh_ok = true;
                self.wait_count = 8;
                self.after_wait = IDLE;
                self.state = WAIT;
            }
            ACTIVATE => {
                self.activate(old.saved_address);
                self.wait_count = 2;
                self.after_wait = if old.saved_write {
                    WRITE_LAUNCH
                } else {
                    READ_LAUNCH
                };
                self.state = WAIT;
            }
            WRITE_LAUNCH => {
                if i.write_slot {
                    self.command = 4;
                    self.bank = bank(old.saved_address) as u8;
                    self.address = (old.saved_address & 255) as u16;
                    self.dqm_state = old.saved_mask;
                    self.dq_drive = true;
                    self.write_index = 1;
                    self.write_prepare_window = old.saved_words > 1;
                    self.state = WRITE;
                    self.prep_needed = false;
                }
            }
            WRITE => {
                if old.write_index == old.saved_last {
                    self.write_prepare_window = false;
                }
                self.prepare(&old, i, next, old.write_prepare_window);
                if old.write_index < old.saved_words {
                    self.write_index = old.write_index.wrapping_add(1) & 63;
                    if out.feedback.chain_accept {
                        self.saved_address = next;
                        self.write_index = 0;
                        self.saved_words = next_words;
                        self.saved_last = next_words.wrapping_sub(1) & 63;
                        self.state = WRITE_LAUNCH;
                    }
                } else {
                    self.command = 6;
                    self.state = WRITE_END;
                }
            }
            WRITE_END => {
                self.dq_drive = false;
                self.prep_needed = false;
                self.done = true;
                self.state = IDLE;
            }
            READ_LAUNCH => {
                self.read_prepare_window = old.saved_words > 1;
                self.command = 5;
                self.address = (old.saved_address & 255) as u16;
                self.dqm_state = 0;
                self.read_active = true;
                self.stream_left = old.saved_words;
                self.stream_delay = 3;
                self.stream_tail = 0;
                self.command_age = 0;
                self.state = READ;
                self.prep_needed = false;
            }
            READ => {
                self.prepare(&old, i, next, old.read_prepare_window);
                if old.command_age == old.read_stop_age {
                    self.command = 6;
                }
                if old.stream_left == 0 && old.stream_delay == 0 && old.stream_tail == 1 {
                    self.read_active = false;
                    self.prep_needed = false;
                    self.done = true;
                    self.state = IDLE;
                }
            }
            COMPLETE => {
                self.done = true;
                self.state = IDLE;
            }
            RESOLVE => {
                let b = bank(old.saved_address);
                if old.row_valid & (1 << b) != 0 {
                    if old.request_matches_q & (1 << b) != 0 {
                        self.state = if old.saved_write {
                            WRITE_LAUNCH
                        } else {
                            READ_LAUNCH
                        };
                    } else {
                        self.command = 2;
                        self.address = 0;
                        self.row_valid &= !(1 << b);
                        self.wait_count = 2;
                        self.after_wait = ACTIVATE;
                        self.state = WAIT;
                    }
                } else {
                    self.activate(old.saved_address);
                    self.wait_count = 2;
                    self.after_wait = if old.saved_write {
                        WRITE_LAUNCH
                    } else {
                        READ_LAUNCH
                    };
                    self.state = WAIT;
                }
            }
            _ => self.state = POWER,
        }
        if column {
            self.bank = bank(next) as u8;
            self.address = (next & 255) as u16;
        }
        if out.feedback.read_chain_accept {
            self.read_prepare_window = true;
            self.command = 5;
            self.saved_address = next;
            self.command_age = 0;
            self.stream_left = old
                .stream_left
                .wrapping_add(next_words)
                .wrapping_sub(u8::from(capture))
                & 127;
            self.saved_words = next_words;
            self.saved_last = next_words.wrapping_sub(1) & 63;
            self.read_stop_age = next_words;
            self.read_chain_pending = true;
        }
        if i.reset {
            self.state = POWER;
            self.command = 7;
            self.cke = false;
            self.dqm_state = 15;
            self.dq_drive = false;
            self.initialized = false;
            self.init_count = 0;
            self.refresh_age = 0;
            self.refresh_due = false;
            self.chain_refresh_ok = true;
            self.prep_refresh_ok = true;
            self.row_valid = 0;
            self.read_active = false;
            self.write_index = 0;
            self.done = false;
            self.prep_wait = 0;
            self.prep_rp = 0;
            self.launch_age = 0;
            self.observed_chain = false;
            self.stream_stats = 0;
            self.prep_needed = false;
            self.prep_open = false;
            self.command_age = 0;
            self.read_chain_pending = false;
            self.next_identity_q = 0;
            self.next_matches_q = false;
            self.next_generation_q = false;
            self.request_matches_q = 0;
            self.write_chain_eligible = false;
            self.read_chain_slot = false;
            self.stream_left = 0;
            self.stream_delay = 0;
            self.stream_tail = 0;
            self.read_prepare_window = false;
            self.write_prepare_window = false;
            self.read_stop_age = 0;
        }
    }
}
