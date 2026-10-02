//! Related-clock edge transport: two rising core edges per logic edge.
use super::{native, pins::Pins};
use crate::sdram_memory_controller::ports::OracleImage;
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub valid: bool,
    pub next_valid: bool,
    pub next_address: u32,
    pub writing: bool,
    pub address: u32,
    pub words: u8,
    pub mask: u8,
    pub data: u64,
    pub data_valid: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub ready: bool,
    pub stream_active: bool,
    pub data_ready: bool,
    pub read_data: u64,
    pub read_valid: bool,
    pub done: bool,
    pub initialized: bool,
}
#[derive(Clone, Debug, Default)]
struct State {
    chained_groups: bool,
    segment: u8,
    restart: bool,
    group_next_valid: bool,
    read_word: u8,
    descriptor_toggle: bool,
    group_descriptor: bool,
    descriptor_writing: bool,
    descriptor_address: u32,
    descriptor_words: u8,
    descriptor_mask: u8,
    occupied: bool,
    stream_active: bool,
    next_valid: bool,
    next_address: u32,
    request_toggle: bool,
    writing: bool,
    address: u32,
    words: u8,
    mask: u8,
    fed: u8,
    head: u64,
    next: u64,
    queued: u8,
    done_seen: bool,
    read_seen: bool,
    init: bool,
    cpu_toggle: bool,
    request_seen: bool,
    done_event: bool,
    done_toggle: bool,
    read_event: bool,
    read_toggle: bool,
    cpu_seen: bool,
    detect: bool,
    pipe: bool,
    slot: bool,
    read_low: u32,
    pair: u64,
    half: bool,
    read_data: u64,
    read_valid: bool,
    done: bool,
}
impl State {
    fn group(&self) -> bool {
        self.chained_groups && self.group_descriptor
    }
    fn group_next(&self) -> u32 {
        self.descriptor_address + u32::from((self.segment + 1) & 3) * 32
    }
    fn output(&self, reset: bool) -> Output {
        Output {
            stream_active: self.stream_active,
            ready: self.init && !self.occupied && !reset,
            data_ready: self.occupied
                && self.writing
                && self.fed < self.words.div_ceil(2)
                && self.queued < 2,
            read_data: self.read_data,
            read_valid: self.read_valid,
            done: self.done,
            initialized: self.init,
        }
    }
    fn core_input(&self) -> native::Input {
        let toggle = if self.chained_groups {
            self.descriptor_toggle
        } else {
            self.request_toggle
        };
        let writing = if self.chained_groups {
            self.descriptor_writing
        } else {
            self.writing
        };
        native::Input {
            request: (toggle != self.request_seen || self.restart)
                && (!writing || self.queued != 0),
            writing,
            address: if self.restart {
                self.group_next()
            } else {
                if self.chained_groups {
                    self.descriptor_address
                } else {
                    self.address
                }
            },
            words: if self.chained_groups {
                self.descriptor_words
            } else {
                self.words
            },
            mask: if self.chained_groups {
                self.descriptor_mask
            } else {
                self.mask
            },
            slot: self.slot,
            next: if self.group() {
                self.group_next_valid.then(|| self.group_next())
            } else {
                (self.next_valid && !self.chained_groups).then_some(self.next_address)
            },
            read_boundary: self.group() && self.read_word == 31,
            ..Default::default()
        }
    }
    fn logic(&mut self, i: Input, c: &native::Controller) {
        let o = self.clone();
        let output = o.output(i.reset);
        if i.reset {
            self.occupied = false;
            self.request_toggle = false;
            self.fed = 0;
            self.queued = 0;
            self.done_seen = false;
            self.read_seen = false;
            self.init = false;
            self.stream_active = false;
            self.cpu_toggle = false;
            self.done = false;
            self.read_valid = false;
            return;
        }
        self.cpu_toggle = !o.cpu_toggle;
        self.init = c.initialized;
        self.stream_active = o.occupied && matches!(c.state, 15 | 18);
        self.done = false;
        self.read_valid = false;
        if i.valid && output.ready {
            self.occupied = true;
            self.request_toggle = !o.request_toggle;
            self.writing = i.writing;
            self.address = i.address;
            self.words = if o.chained_groups && i.words == 0 {
                128
            } else {
                i.words
            };
            self.mask = i.mask;
            self.fed = 0;
            self.queued = 0;
        }
        let push = i.data_valid && output.data_ready;
        let pop =
            o.occupied && o.writing && c.state == 15 && c.write_index & 1 != 0 && o.queued != 0;
        if push {
            self.fed = o.fed + 1;
        }
        match (pop, push) {
            (false, true) => {
                if o.queued == 0 {
                    self.head = i.data;
                } else {
                    self.next = i.data;
                }
                self.queued = o.queued + 1;
            }
            (true, false) => {
                self.head = o.next;
                self.queued = o.queued - 1;
            }
            (true, true) => {
                if o.queued == 2 {
                    self.head = o.next;
                    self.next = i.data;
                } else {
                    self.head = i.data;
                }
            }
            _ => {}
        }
        if o.read_toggle != o.read_seen {
            self.read_seen = o.read_toggle;
            self.read_data = o.pair;
            self.read_valid = true;
        }
        if o.done_toggle != o.done_seen {
            self.done_seen = o.done_toggle;
            self.occupied = false;
            self.queued = 0;
            self.done = true;
        }
    }
    fn core_rise(&mut self, o: &Self, c: &native::Controller, i: Input) {
        if i.reset {
            self.next_valid = false;
            self.request_seen = false;
            self.pipe = false;
            self.slot = false;
            self.done_event = false;
            self.read_event = false;
            self.half = false;
            self.segment = 0;
            self.restart = false;
            self.group_next_valid = false;
            self.read_word = 0;
            self.descriptor_toggle = false;
            self.group_descriptor = false;
            self.descriptor_writing = false;
            self.descriptor_address = 0;
            self.descriptor_words = 0;
            self.descriptor_mask = 0;
            return;
        }
        self.next_valid = i.next_valid;
        self.next_address = i.next_address;
        self.pipe = o.detect;
        self.slot = o.pipe;
        if o.chained_groups && o.request_toggle != o.descriptor_toggle {
            self.descriptor_toggle = o.request_toggle;
            self.group_descriptor = o.words == 128;
            self.descriptor_writing = o.writing;
            self.descriptor_address = o.address;
            self.descriptor_words = if o.words == 128 { 32 } else { o.words };
            self.descriptor_mask = o.mask;
        }
        if o.core_input().request && c.ready() {
            self.request_seen = if o.chained_groups {
                o.descriptor_toggle
            } else {
                o.request_toggle
            };
            self.half = false;
            self.read_word = 0;
            self.segment = if o.restart { o.segment + 1 } else { 0 };
            self.restart = false;
            self.group_next_valid = o.group() && (!o.restart || o.segment != 2);
        }
        if c.chain_accept(&o.core_input()) || c.read_chain_issue(&o.core_input()) {
            self.segment = o.segment + 1;
            if o.segment == 2 {
                self.group_next_valid = false;
            }
        }
        if c.done {
            self.group_next_valid = false;
            if o.group() && o.segment != 3 {
                self.restart = true;
            } else {
                self.done_event = !o.done_event;
            }
        }
        if c.read_valid {
            self.read_word = (o.read_word + 1) & 31;
            if o.words == 1 {
                self.pair = u64::from(c.read_data);
                self.read_event = !o.read_event;
            } else if !o.half {
                self.read_low = c.read_data;
                self.half = true;
            } else {
                self.pair = (u64::from(c.read_data) << 32) | u64::from(o.read_low);
                self.read_event = !o.read_event;
                self.half = false;
            }
        }
    }
    fn fall(&mut self, reset: bool) {
        if reset {
            self.cpu_seen = false;
            self.detect = false;
            self.done_toggle = false;
            self.read_toggle = false;
        } else {
            self.detect = self.cpu_toggle != self.cpu_seen;
            self.cpu_seen = self.cpu_toggle;
            self.done_toggle = self.done_event;
            self.read_toggle = self.read_event;
        }
    }
}
pub struct Engine {
    state: State,
    pub controller: native::Controller,
    pub pins: Pins,
    dq: u32,
    pending_dq: Option<u32>,
    pub core_cycle: u64,
    pub read_chains: u64,
    pub write_chains: u64,
    pub group_restarts: u64,
}
impl Engine {
    pub fn new(image: OracleImage, init_cycles: u32) -> Result<Self, String> {
        Self::with_early_grant(image, init_cycles, false)
    }
    pub fn with_early_grant(
        image: OracleImage,
        init_cycles: u32,
        early_grant: bool,
    ) -> Result<Self, String> {
        Self::with_options(image, init_cycles, early_grant, false)
    }
    pub fn with_options(
        image: OracleImage,
        init_cycles: u32,
        early_grant: bool,
        chained_groups: bool,
    ) -> Result<Self, String> {
        if !(1..=65535).contains(&init_cycles) {
            return Err("init cycles must be 1..65535".into());
        }
        Ok(Self {
            state: State {
                chained_groups,
                ..State::default()
            },
            controller: {
                let mut controller =
                    native::Controller::with_prepare_next(init_cycles, early_grant);
                controller.chain = chained_groups;
                controller
            },
            pins: Pins::new(image),
            dq: 0,
            pending_dq: None,
            core_cycle: 0,
            read_chains: 0,
            write_chains: 0,
            group_restarts: 0,
        })
    }
    pub fn output(&self, reset: bool) -> Output {
        self.state.output(reset)
    }
    pub fn tick(&mut self, i: Input) -> Result<(), String> {
        if !i.reset
            && self.state.chained_groups
            && i.valid
            && self.output(false).ready
            && i.words == 0
            && i.address & 127 != 0
        {
            return Err("native four-sector group must be 512-byte aligned".into());
        }
        for phase in 0..2 {
            let old = self.state.clone();
            let core = self.controller.clone();
            let mut native = old.core_input();
            native.reset = i.reset;
            native.read_boundary &= core.read_valid;
            native.data = if core.write_index & 1 != 0 {
                (old.head >> 32) as u32
            } else {
                old.head as u32
            };
            // A native write stream cannot pause. Missing source supply is a
            // contract error, never a fabricated successful write.
            if !i.reset && core.state == 15 && core.write_index < native.words && old.queued == 0 {
                return Err("native write source underrun".into());
            }
            if !i.reset {
                self.read_chains += u64::from(core.read_chain_issue(&native));
                self.write_chains += u64::from(core.chain_accept(&native));
                self.group_restarts += u64::from(old.restart && native.request && core.ready());
            }
            if phase == 0 {
                self.state.logic(i, &core);
            }
            self.state.core_rise(&old, &core, i);
            self.controller.rise(native, self.dq);
            self.state.fall(i.reset);
            let next = self.pins.edge(&self.controller, i.reset)?;
            if let Some(d) = self.pending_dq {
                self.dq = d;
            }
            self.pending_dq = next;
            self.core_cycle += 1;
        }
        Ok(())
    }
}
