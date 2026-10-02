//! Edge-driven native controller. Deliberately independent of sim::controller.
//! Only the integrated rising-capture CL2/RCD2/RP2/RFC9 profile is supported.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub request: bool,
    pub writing: bool,
    pub address: u32,
    pub words: u8,
    pub mask: u8,
    pub data: u32,
    pub slot: bool,
    pub next: Option<u32>,
    pub read_boundary: bool,
}
#[derive(Clone, Debug)]
pub struct Controller {
    pub state: u8,
    after: u8,
    init: u32,
    init_cycles: u32,
    wait: u8,
    pub initialized: bool,
    pub done: bool,
    pub read_valid: bool,
    pub read_data: u32,
    pub write_index: u8,
    pub command: u8,
    pub pin_address: u16,
    pub pin_bank: usize,
    pub dqm: u8,
    pub drive: bool,
    pub dq: u32,
    pub cke: bool,
    pub refresh_age: u16,
    rows: [Option<u16>; 4],
    saved: u32,
    words: u8,
    last: u8,
    writing: bool,
    mask: u8,
    read_active: bool,
    read_age: u8,
    command_age: u8,
    limit: u8,
    finish: u8,
    request_matches: [bool; 4],
    next_match: bool,
    next_identity: u32,
    slot_q: bool,
    extra: u8,
    eligible: bool,
    prep: Option<u32>,
    prep_open: bool,
    prep_wait: u8,
    prep_rp: u8,
    pub read_chain_pending: bool,
    pub chain: bool,
    prepare_next: bool,
    prep_refresh_ok: bool,
}
pub fn mapped(address: u32) -> u32 {
    ((address >> 5 & 3) << 19) | (address & 31) | ((address >> 7) << 5)
}
impl Controller {
    pub fn new(init_cycles: u32, chain: bool) -> Self {
        Self {
            state: 0,
            after: 0,
            init: 0,
            init_cycles,
            wait: 0,
            initialized: false,
            done: false,
            read_valid: false,
            read_data: 0,
            write_index: 0,
            command: 7,
            pin_address: 0,
            pin_bank: 0,
            dqm: 15,
            drive: false,
            dq: 0,
            cke: false,
            refresh_age: 0,
            rows: [None; 4],
            saved: 0,
            words: 0,
            last: 0,
            writing: false,
            mask: 0,
            read_active: false,
            read_age: 0,
            command_age: 0,
            limit: 0,
            finish: 0,
            request_matches: [false; 4],
            next_match: false,
            next_identity: 0,
            slot_q: false,
            extra: 0,
            eligible: false,
            prep: None,
            prep_open: false,
            prep_wait: 0,
            prep_rp: 0,
            read_chain_pending: false,
            chain,
            prepare_next: false,
            prep_refresh_ok: true,
        }
    }
    pub fn with_prepare_next(init_cycles: u32, prepare_next: bool) -> Self {
        let mut controller = Self::new(init_cycles, false);
        controller.prepare_next = prepare_next;
        controller
    }
    pub fn ready(&self) -> bool {
        self.state == 10
            && self.initialized
            && self.refresh_age < 1100
            && self.prep_wait == 0
            && self.prep_rp == 0
    }
    pub fn phase(&self) -> u8 {
        self.command << 5 | self.state
    }
    pub fn chain_accept(&self, i: &Input) -> bool {
        self.chain
            && self.state == 15
            && self.write_index == self.last
            && self.eligible
            && self.next_ok(i)
            && self.refresh_age < 1000
            && self.extra != 3
    }
    fn next_ok(&self, i: &Input) -> bool {
        i.next
            .is_some_and(|a| self.next_match && self.next_identity == mapped(a) >> 8)
            && self.prep_wait == 0
    }
    pub fn read_chain_issue(&self, i: &Input) -> bool {
        self.chain && self.slot_q && self.next_ok(i)
    }
    fn timing(&mut self, clocks: u8, after: u8) {
        self.wait = clocks - 1;
        self.after = after;
        self.state = 2;
    }
    fn activate(&mut self, address: u32) {
        self.command = 3;
        self.pin_bank = (address >> 19) as usize;
        self.pin_address = (address >> 8 & 2047) as u16;
        self.rows[self.pin_bank] = Some(self.pin_address);
    }
    pub fn rise(&mut self, i: Input, sampled_dq: u32) {
        let o = self.clone();
        self.command = 7;
        self.done = false;
        self.read_valid = o.read_active && o.read_age >= 2 && o.read_age < o.limit;
        self.read_data = sampled_dq;
        let a = mapped(i.address);
        let next = mapped(i.next.unwrap_or(0));
        self.request_matches = std::array::from_fn(|b| o.rows[b] == Some((a >> 8 & 2047) as u16));
        self.next_match = o.rows[(next >> 19) as usize] == Some((next >> 8 & 2047) as u16);
        self.next_identity = next >> 8;
        self.finish = o.limit.wrapping_add(if o.words == 1 { 2 } else { 0 });
        self.slot_q = o.state == 18
            && o.command_age == o.last.wrapping_sub(1)
            && o.words == 32
            && o.refresh_age < 999
            && o.extra != 3;
        self.refresh_age = (o.refresh_age + 1).min(4095);
        self.prep_refresh_ok = o.refresh_age < 989;
        if o.read_active {
            self.read_age = o.read_age.wrapping_add(1);
            self.command_age = o.command_age.wrapping_add(1);
        }
        if i.read_boundary {
            self.read_chain_pending = false;
        }
        self.prep_wait = o.prep_wait.saturating_sub(1);
        self.prep_rp = o.prep_rp.saturating_sub(1);
        match o.state {
            0 => {
                if o.init == o.init_cycles - 1 {
                    self.cke = true;
                    self.state = 1;
                } else {
                    self.init = o.init + 1;
                }
            }
            1 => {
                self.command = 2;
                self.pin_address = 1024;
                self.timing(2, 3);
            }
            2 => {
                if o.wait == 1 {
                    self.state = o.after;
                } else {
                    self.wait = o.wait.wrapping_sub(1);
                }
            }
            3 => {
                self.command = 1;
                self.timing(9, 4);
            }
            4 => {
                self.command = 1;
                self.timing(9, 5);
            }
            5 => {
                self.command = 0;
                self.pin_bank = 0;
                self.pin_address = 39;
                self.wait = 1;
                self.after = 6;
                self.state = 2;
            }
            6 => {
                self.initialized = true;
                self.refresh_age = 0;
                self.state = 10;
            }
            10 => {
                if o.prep_wait != 0 || o.prep_rp != 0 {
                    // Finish bank timing before dispatch/refresh.
                } else if o.refresh_age >= 1100 {
                    if o.rows.iter().any(Option::is_some) {
                        self.command = 2;
                        self.pin_address = 1024;
                        self.rows = [None; 4];
                        self.timing(2, 11);
                    } else {
                        self.state = 11;
                    }
                } else if i.request && o.ready() {
                    self.eligible = i.words & 56 != 0;
                    self.extra = 0;
                    self.saved = a;
                    self.words = i.words;
                    self.last = i.words.wrapping_sub(1) & 63;
                    self.writing = i.writing;
                    self.mask = i.mask;
                    self.write_index = 0;
                    self.limit = 2 + i.words;
                    self.pin_bank = (a >> 19) as usize;
                    self.state = 21;
                }
            }
            11 => {
                self.command = 1;
                self.refresh_age = 0;
                self.timing(9, 10);
            }
            12 => {
                self.activate(o.saved);
                self.timing(2, if o.writing { 14 } else { 17 });
            }
            14 => {
                if i.slot {
                    self.command = 4;
                    self.pin_bank = (o.saved >> 19) as usize;
                    self.pin_address = (o.saved & 255) as u16;
                    self.dqm = o.mask;
                    self.drive = true;
                    self.dq = i.data;
                    self.write_index = 1;
                    self.state = 15;
                    self.prep = None;
                }
            }
            15 => {
                if o.chain && o.eligible && o.write_index == 1 {
                    self.prep = i
                        .next
                        .filter(|_| next >> 19 != o.saved >> 19 && !self.next_match)
                        .map(mapped);
                    self.prep_open = o.rows[(next >> 19) as usize].is_some();
                }
                if o.prepare_next
                    && i.next.is_some()
                    && next >> 19 != o.saved >> 19
                    && o.rows[(next >> 19) as usize].is_none()
                    && o.prep.is_none()
                    && o.prep_refresh_ok
                    && o.write_index < o.words
                {
                    self.prep = Some(next);
                    self.prep_open = false;
                }
                if (o.chain || o.prepare_next)
                    && o.prep.is_some()
                    && o.prep_rp == 0
                    && o.write_index < o.words
                {
                    self.prepare(&o);
                }
                if o.write_index < o.words {
                    self.dq = i.data;
                    self.write_index = (o.write_index + 1) & 63;
                    if o.chain_accept(&i) {
                        self.saved = next;
                        self.write_index = 0;
                        self.state = 14;
                        self.extra = o.extra + 1;
                    }
                } else {
                    self.command = 6;
                    self.state = 16;
                }
            }
            16 => {
                if o.prepare_next {
                    self.prep = None;
                }
                self.drive = false;
                self.done = true;
                self.state = 10;
            }
            17 => {
                self.command = 5;
                self.pin_address = (o.saved & 255) as u16;
                self.dqm = 0;
                self.read_active = true;
                self.read_age = 0;
                self.command_age = 0;
                self.state = 18;
                self.prep = None;
            }
            18 => {
                if o.chain && o.words == 32 && o.command_age == 8 {
                    self.prep = i
                        .next
                        .filter(|_| next >> 19 != o.saved >> 19 && !self.next_match)
                        .map(mapped);
                    self.prep_open = o.rows[(next >> 19) as usize].is_some();
                }
                if o.prepare_next
                    && i.next.is_some()
                    && next >> 19 != o.saved >> 19
                    && o.rows[(next >> 19) as usize].is_none()
                    && o.prep.is_none()
                    && o.prep_refresh_ok
                    && o.command_age < o.last
                {
                    self.prep = Some(next);
                    self.prep_open = false;
                }
                if (o.chain || o.prepare_next) && o.prep.is_some() && o.prep_rp == 0 {
                    self.prepare(&o);
                }
                if o.command_age == o.words + if o.words == 1 { 2 } else { 0 } {
                    self.command = 6;
                }
                if o.read_age == o.finish {
                    if o.prepare_next {
                        self.prep = None;
                    }
                    self.read_active = false;
                    self.done = true;
                    self.state = 10;
                }
            }
            21 => {
                let b = (o.saved >> 19) as usize;
                if o.rows[b].is_some() {
                    if o.request_matches[b] {
                        self.state = if o.writing { 14 } else { 17 };
                    } else {
                        self.command = 2;
                        self.pin_address = 0;
                        self.rows[b] = None;
                        self.timing(2, 12);
                    }
                } else {
                    self.activate(o.saved);
                    self.timing(2, if o.writing { 14 } else { 17 });
                }
            }
            _ => self.state = 0,
        }
        if o.read_chain_issue(&i) {
            self.extra = o.extra + 1;
            self.command = 5;
            self.pin_bank = (next >> 19) as usize;
            self.pin_address = (next & 255) as u16;
            self.saved = next;
            self.command_age = 0;
            self.limit = o.limit.wrapping_add(o.words);
            self.read_chain_pending = true;
        }
        if i.reset {
            *self = Self::new(o.init_cycles, o.chain);
            self.prepare_next = o.prepare_next;
        }
    }
    fn prepare(&mut self, o: &Self) {
        let a = o.prep.unwrap();
        self.pin_bank = (a >> 19) as usize;
        if o.prep_open {
            self.command = 2;
            self.pin_address = 0;
            self.rows[self.pin_bank] = None;
            self.prep_rp = 1;
            self.prep_open = false;
        } else {
            self.activate(a);
            self.prep_wait = 1;
            self.prep = None;
        }
    }
}
