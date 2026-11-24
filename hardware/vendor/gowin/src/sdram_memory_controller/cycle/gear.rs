//! Edge mirror of TangNano20KSdramNativeBridge108M54M, registered-return profile.
//! This is the bridge only: controller feedback is supplied by its caller.
use super::Edges;

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub request_valid: bool,
    pub continuation: bool,
    pub writing: bool,
    pub address: u32,
    pub words: u8,
    pub write_mask: u8,
    pub write_data: u64,
    pub write_data_valid: bool,
    pub next_valid: bool,
    pub next_address: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Feedback {
    pub ready: bool,
    pub done: bool,
    pub initialized: bool,
    pub read_valid: bool,
    pub read_data: u32,
    pub write_index: u8,
    pub phase: u8,
    pub chain_accept: bool,
    pub read_chain_accept: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogicOutput {
    pub request_ready: bool,
    pub stream_active: bool,
    pub write_data_ready: bool,
    pub read_data: u64,
    pub read_valid: bool,
    pub write_done: bool,
    pub initialized: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ControllerInput {
    pub reset: bool,
    pub request: bool,
    pub writing: bool,
    pub address: u32,
    pub words: u8,
    pub write_slot: bool,
    pub next_valid: bool,
    pub next_address: u32,
    pub next_words: u8,
    pub next_chainable: bool,
    pub next_write_ready: bool,
    pub next_generation: bool,
    pub write_data: u32,
    pub write_mask: u8,
}

#[derive(Clone, Copy, Debug, Default)]
struct Descriptor {
    address: u32,
    words: u8,
    mask: u8,
    writing: bool,
    continuation: bool,
}

/// Numeric state is independent of Circuit/Verilog and has no backing memory.
#[derive(Clone, Copy, Debug)]
pub struct Gear {
    cold_start_only: bool,
    mailbox: Descriptor,
    request_toggle: bool,
    request_seen_native: bool,
    request_seen_logic: bool,
    pair_banks: [u64; 2],
    pair_read: u8,
    pair_count: u8,
    initialized_q: bool,
    stream_q: bool,
    offer: Descriptor,
    offer_valid: bool,
    hint_valid: bool,
    active_write: bool,
    active_scalar: bool,
    next_write_ready_q: bool,
    offer_generation: bool,
    pop: bool,
    core_write_data: u32,
    write_retire_delay: bool,
    write_event: bool,
    write_visible: bool,
    write_seen: bool,
    write_done: bool,
    read_half: bool,
    read_low: u32,
    read_pair: u64,
    read_event: bool,
    read_visible: bool,
    read_seen: bool,
    stable_read_pair: u64,
    payload: u64,
    valid: bool,
    cpu_edge: bool,
    cpu_edge_seen: bool,
    slot_detect: bool,
    slot_pipe: bool,
    write_slot: bool,
}

impl Default for Gear {
    fn default() -> Self {
        Self::new(true)
    }
}

impl Gear {
    /// `true` matches the fitted cold-only transport. The controller still sees reset.
    pub fn new(cold_start_only: bool) -> Self {
        Self {
            cold_start_only,
            mailbox: Descriptor::default(),
            request_toggle: false,
            request_seen_native: false,
            request_seen_logic: false,
            pair_banks: [0; 2],
            pair_read: 0,
            pair_count: 0,
            initialized_q: false,
            stream_q: false,
            offer: Descriptor::default(),
            offer_valid: false,
            hint_valid: false,
            active_write: false,
            active_scalar: false,
            next_write_ready_q: false,
            offer_generation: false,
            pop: false,
            core_write_data: 0,
            write_retire_delay: false,
            write_event: false,
            write_visible: false,
            write_seen: false,
            write_done: false,
            read_half: false,
            read_low: 0,
            read_pair: 0,
            read_event: false,
            read_visible: false,
            read_seen: false,
            stable_read_pair: 0,
            payload: 0,
            valid: false,
            cpu_edge: false,
            cpu_edge_seen: false,
            slot_detect: false,
            slot_pipe: false,
            write_slot: false,
        }
    }

    pub(super) fn transport_reset(&self, i: &Input) -> bool {
        i.reset && !self.cold_start_only
    }

    pub fn logic_output(&self, i: &Input) -> LogicOutput {
        let reset = self.transport_reset(i);
        LogicOutput {
            request_ready: self.initialized_q
                && self.request_toggle == self.request_seen_logic
                && !reset,
            stream_active: self.stream_q,
            write_data_ready: self.pair_count < 2 && !reset,
            read_data: self.payload,
            read_valid: self.valid && !reset,
            write_done: self.write_done,
            initialized: self.initialized_q,
        }
    }

    pub fn controller_input(&self, i: &Input) -> ControllerInput {
        ControllerInput {
            reset: i.reset,
            request: self.offer_valid
                && (!self.offer.writing
                    || self.pair_count >= if self.offer.words == 1 { 1 } else { 2 }),
            writing: self.offer.writing,
            address: self.offer.address,
            words: self.offer.words,
            write_slot: self.write_slot,
            next_valid: self.offer_valid || self.hint_valid,
            next_address: self.offer.address,
            next_words: self.offer.words,
            next_chainable: self.offer_valid && self.offer.continuation,
            next_write_ready: self.next_write_ready_q,
            next_generation: self.offer_generation,
            write_data: self.core_write_data,
            write_mask: self.offer.mask,
        }
    }

    /// Every selected edge reads `old`, including coincident logic/native rises.
    /// Source-specific register reset/hold rules are preserved, not bulk-cleared.
    pub fn clock(&mut self, edges: Edges, i: &Input, n: &Feedback) {
        assert!(!(edges.native_rise && edges.native_fall));
        let old = *self;
        let reset = old.transport_reset(i);
        let logic = old.logic_output(i);
        let native = old.controller_input(i);
        let raw_read_valid = old.read_visible != old.read_seen && !reset;
        if edges.logic_rise {
            if reset {
                self.payload = 0;
                self.valid = false;
                self.request_toggle = false;
                self.request_seen_logic = false;
                self.pair_count = 0;
                self.pair_banks = [0; 2];
                self.pair_read = 0;
                self.write_done = false;
                self.write_seen = false;
                self.initialized_q = false;
                self.stream_q = false;
                self.read_seen = false;
                self.cpu_edge = false;
            } else {
                self.payload = old.stable_read_pair;
                self.valid = raw_read_valid;
                self.cpu_edge = !old.cpu_edge;
                self.initialized_q = n.initialized;
                self.request_seen_logic = old.request_seen_native;
                self.stream_q = matches!(n.phase & 31, 15 | 18);
                self.write_done = false;
                if i.request_valid && logic.request_ready {
                    self.mailbox = Descriptor {
                        address: i.address & 0x1fffff,
                        words: i.words & 63,
                        mask: i.write_mask & 15,
                        writing: i.writing,
                        continuation: i.continuation,
                    };
                    self.request_toggle = !old.request_toggle;
                }
                let push = i.write_data_valid && logic.write_data_ready;
                if push {
                    let bank = old.pair_read ^ (old.pair_count & 1);
                    self.pair_banks[usize::from(bank)] = i.write_data;
                }
                if old.pop {
                    self.pair_read = old.pair_read ^ 1;
                }
                match (push, old.pop) {
                    (true, false) => {
                        self.pair_count = old.pair_count.wrapping_add(1) & 3;
                    }
                    (false, true) => {
                        self.pair_count = old.pair_count.wrapping_sub(1) & 3;
                    }
                    _ => {}
                }
                if raw_read_valid {
                    self.read_seen = old.read_visible;
                }
                if old.write_seen != old.write_visible {
                    self.write_done = true;
                    self.write_seen = old.write_visible;
                    if old.active_scalar {
                        self.pair_count = 0;
                    }
                }
            }
        }
        if edges.native_rise {
            if reset {
                self.next_write_ready_q = false;
                self.offer_generation = false;
                self.request_seen_native = false;
                self.offer_valid = false;
                self.hint_valid = false;
                self.active_write = false;
                self.active_scalar = false;
                self.offer.continuation = false;
                self.read_half = false;
                self.read_event = false;
                self.write_event = false;
                self.write_retire_delay = false;
                self.slot_pipe = false;
                self.write_slot = false;
            } else {
                self.next_write_ready_q = old.pair_count != 0;
                self.slot_pipe = old.slot_detect;
                self.write_slot = old.slot_pipe;
                if !old.offer_valid && old.request_seen_native != old.request_toggle {
                    if !old.mailbox.continuation || !old.hint_valid {
                        self.offer_generation = !old.offer_generation;
                    }
                    self.offer = old.mailbox;
                    self.offer_valid = true;
                    self.hint_valid = false;
                    self.request_seen_native = old.request_toggle;
                }
                if !old.offer_valid && old.request_seen_native == old.request_toggle {
                    self.hint_valid = i.next_valid;
                    if i.next_valid && !old.hint_valid {
                        self.offer_generation = !old.offer_generation;
                        self.offer.address = i.next_address & 0x1fffff;
                    }
                }
                if (native.request && n.ready) || n.chain_accept || n.read_chain_accept {
                    self.offer_valid = false;
                    self.active_write = old.offer.writing;
                    self.active_scalar = old.offer.words == 1;
                }
                self.write_retire_delay = n.chain_accept;
                if old.write_retire_delay || (n.done && old.active_write) {
                    self.write_event = !old.write_event;
                }
                if n.read_valid {
                    if old.active_scalar {
                        self.read_pair = u64::from(n.read_data);
                        self.read_event = !old.read_event;
                    } else if !old.read_half {
                        self.read_low = n.read_data;
                        self.read_half = true;
                    } else {
                        self.read_pair = u64::from(n.read_data) << 32 | u64::from(old.read_low);
                        self.read_event = !old.read_event;
                        self.read_half = false;
                    }
                }
            }
        }
        if edges.native_fall {
            // These three registers have no transport-reset branch in the RTL.
            self.pop = n.phase & 31 == 15 && n.write_index & 1 != 0 && old.pair_count != 0;
            let head = old.pair_banks[usize::from(old.pair_read)];
            self.core_write_data = if n.write_index & 1 != 0 {
                (head >> 32) as u32
            } else {
                head as u32
            };
            self.stable_read_pair = old.read_pair;
            if reset {
                self.cpu_edge_seen = false;
                self.slot_detect = false;
                self.write_visible = false;
                self.read_visible = false;
            } else {
                self.slot_detect = old.cpu_edge != old.cpu_edge_seen;
                self.cpu_edge_seen = old.cpu_edge;
                self.write_visible = old.write_event;
                self.read_visible = old.read_event;
            }
        }
    }
}
