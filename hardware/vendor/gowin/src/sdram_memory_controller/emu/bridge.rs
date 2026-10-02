//! Related-clock edge transport: two rising core edges per logic edge.
use super::{native, pins::Pins};
use crate::sdram_memory_controller::ports::OracleImage;
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub valid: bool,
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
    pub data_ready: bool,
    pub read_data: u64,
    pub read_valid: bool,
    pub done: bool,
    pub initialized: bool,
}
#[derive(Clone, Debug, Default)]
struct State {
    occupied: bool,
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
    fn output(&self, reset: bool) -> Output {
        Output {
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
        native::Input {
            request: self.request_toggle != self.request_seen
                && (!self.writing || self.queued != 0),
            writing: self.writing,
            address: self.address,
            words: self.words,
            mask: self.mask,
            slot: self.slot,
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
            self.cpu_toggle = false;
            self.done = false;
            self.read_valid = false;
            return;
        }
        self.cpu_toggle = !o.cpu_toggle;
        self.init = c.initialized;
        self.done = false;
        self.read_valid = false;
        if i.valid && output.ready {
            self.occupied = true;
            self.request_toggle = !o.request_toggle;
            self.writing = i.writing;
            self.address = i.address;
            self.words = i.words;
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
            (true, true) => self.head = i.data,
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
    fn core_rise(&mut self, o: &Self, c: &native::Controller, reset: bool) {
        if reset {
            self.request_seen = false;
            self.pipe = false;
            self.slot = false;
            self.done_event = false;
            self.read_event = false;
            self.half = false;
            return;
        }
        self.pipe = o.detect;
        self.slot = o.pipe;
        if o.core_input().request && c.ready() {
            self.request_seen = o.request_toggle;
            self.half = false;
        }
        if c.done {
            self.done_event = !o.done_event;
        }
        if c.read_valid {
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
}
impl Engine {
    pub fn new(image: OracleImage, init_cycles: u32) -> Result<Self, String> {
        if !(1..=65535).contains(&init_cycles) {
            return Err("init cycles must be 1..65535".into());
        }
        Ok(Self {
            state: State::default(),
            controller: native::Controller::new(init_cycles, false),
            pins: Pins::new(image),
            dq: 0,
            pending_dq: None,
            core_cycle: 0,
        })
    }
    pub fn output(&self, reset: bool) -> Output {
        self.state.output(reset)
    }
    pub fn tick(&mut self, i: Input) -> Result<(), String> {
        for phase in 0..2 {
            let old = self.state.clone();
            let core = self.controller.clone();
            let mut native = old.core_input();
            native.reset = i.reset;
            native.data = if core.write_index & 1 != 0 {
                (old.head >> 32) as u32
            } else {
                old.head as u32
            };
            // A native write stream cannot pause. Missing source supply is a
            // contract error, never a fabricated successful write.
            if !i.reset && core.state == 15 && core.write_index < old.words && old.queued == 0 {
                return Err("native write source underrun".into());
            }
            if phase == 0 {
                self.state.logic(i, &core);
            }
            self.state.core_rise(&old, &core, i.reset);
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
