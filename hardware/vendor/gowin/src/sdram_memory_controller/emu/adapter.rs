use crate::sdram_memory_controller::shared_port::{
    SharedSdramPortInputValue as Input, SharedSdramPortOutputValue as Output,
};
#[derive(Clone, Debug, Default)]
pub struct State {
    state: u8,
    writing: bool,
    line: bool,
    lane: bool,
    total: u8,
    fed: u8,
    beats: u8,
    scalar_write: u64,
    scalar_read: u32,
    read_seen: bool,
    done_seen: bool,
    timeout: u32,
    valid: bool,
    data: u64,
    last: bool,
    error: bool,
    queued: Option<Input>,
    write_hold: bool,
    early_grant: bool,
    chained_groups: bool,
}
impl State {
    pub fn with_early_grant(early_grant: bool) -> Self {
        Self::with_options(early_grant, false)
    }
    pub fn with_options(early_grant: bool, chained_groups: bool) -> Self {
        Self {
            early_grant,
            chained_groups,
            ..Self::default()
        }
    }
    fn legal(&self, i: &Input) -> bool {
        !i.cpu_line
            || (self.chained_groups && i.cpu_line_count_minus_1 == 2 && i.cpu_address & 255 == 0)
            || (i.cpu_line_count_minus_1 != 2
                && i.cpu_address & ((16 * (i.cpu_line_count_minus_1 + 1)) - 1) == 0)
    }
    fn active(&self) -> bool {
        self.state == 1 && self.writing && self.fed < if self.line { self.total } else { 1 }
    }
    fn window(&self, i: &Input) -> bool {
        self.early_grant
            && self.state == 1
            && i.controller_stream_active
            && self.line
            && self.queued.is_none()
            && u16::from(if self.writing { self.fed } else { self.beats }) + 4
                >= u16::from(self.total)
    }
    fn terminal(&self, i: &Input) -> bool {
        self.queued.is_some()
            && !self.error
            && i.cpu_response_ready
            && (self.state == 4 || self.state == 2 && (self.done_seen || i.controller_done))
    }
    pub fn output(&self, i: &Input) -> Output {
        let launch = self.queued.as_ref().unwrap_or(i);
        Output {
            cpu_request_ready: i.controller_init_done
                && self.queued.is_none()
                && if self.state == 0 {
                    !i.cpu_request_valid || !self.legal(launch) || i.controller_request_ready
                } else {
                    self.window(i)
                },
            cpu_lookahead_window: self.window(i),
            cpu_write_data_ready: self.active()
                && !self.write_hold
                && self.line
                && i.controller_write_data_ready,
            cpu_response_valid: self.valid,
            cpu_read_data: self.data,
            cpu_response_last: self.last,
            cpu_error: self.error,
            controller_request_valid: (self.state == 0 || self.terminal(i))
                && (self.queued.is_some() || i.cpu_request_valid)
                && i.controller_init_done
                && self.legal(launch),
            controller_next_valid: self.queued.as_ref().is_some_and(|q| self.legal(q))
                && self.state == 1,
            controller_next_address: self.queued.as_ref().map_or(0, |q| q.cpu_address >> 1),
            controller_write: launch.cpu_write,
            controller_address: launch.cpu_address >> 1,
            controller_write_mask: if launch.cpu_write && !launch.cpu_line {
                if launch.cpu_address & 1 != 0 {
                    3
                } else {
                    12
                }
            } else {
                0
            },
            controller_write_data: if self.line {
                i.cpu_write_data
            } else {
                self.scalar_write
            },
            controller_write_data_valid: self.active() && !self.write_hold,
            controller_words: if !launch.cpu_line {
                1
            } else if self.chained_groups && launch.cpu_line_count_minus_1 == 2 {
                0
            } else {
                8 * (launch.cpu_line_count_minus_1 + 1)
            },
        }
    }
    pub fn clock(&mut self, i: &Input) {
        let o = self.clone();
        let output = o.output(i);
        if i.reset || !i.controller_init_done {
            self.state = 0;
            self.valid = false;
            self.last = false;
            self.error = false;
            self.fed = 0;
            self.read_seen = false;
            self.done_seen = false;
            self.timeout = 0;
            self.queued = None;
            self.write_hold = false;
            return;
        }
        self.write_hold = false;
        if (o.early_grant || o.chained_groups) && o.state == 2 && i.controller_done {
            self.done_seen = true;
        }
        if o.state != 0 && i.cpu_request_valid && output.cpu_request_ready {
            self.queued = Some(i.clone());
        }
        match o.state {
            0 => {
                let launch = o.queued.as_ref().unwrap_or(i);
                if (o.queued.is_some() || i.cpu_request_valid)
                    && (!o.legal(launch) || i.controller_request_ready)
                {
                    self.launch(launch);
                }
            }

            1 => {
                self.timeout = (o.timeout + 1) & 0xfffff;
                if output.controller_write_data_valid && i.controller_write_data_ready {
                    self.fed = o.fed + 1;
                }
                if i.controller_done {
                    self.done_seen = true;
                }
                if !o.writing && i.controller_read_valid {
                    if o.line {
                        self.data = i.controller_read_data;
                        self.valid = true;
                        self.last = o.beats == o.total - 1;
                        self.beats = o.beats + 1;
                        if self.last {
                            self.state = 2;
                        }
                    } else {
                        self.scalar_read = i.controller_read_data as u32;
                        self.read_seen = true;
                    }
                } else if o.line && !o.writing {
                    self.valid = false;
                }
                if i.controller_done && o.writing {
                    self.data = 0;
                    self.valid = true;
                    self.last = true;
                    self.state = 4;
                } else if !o.writing
                    && !o.line
                    && (o.done_seen || i.controller_done)
                    && (o.read_seen || i.controller_read_valid)
                {
                    let d = if i.controller_read_valid {
                        i.controller_read_data as u32
                    } else {
                        o.scalar_read
                    };
                    self.data = u64::from(d >> if o.lane { 16 } else { 0 } & 65535);
                    self.valid = true;
                    self.last = true;
                    self.state = 4;
                } else if o.timeout == 0xfffff {
                    self.error = true;
                    self.valid = true;
                    self.last = true;
                    self.state = 4;
                }
            }
            2 => {
                if i.cpu_response_ready {
                    self.valid = false;
                    self.last = false;
                    self.timeout = 0;
                    self.state = if o.done_seen || i.controller_done {
                        0
                    } else {
                        3
                    };
                }
            }
            3 => {
                if i.controller_done {
                    self.state = 0;
                } else if o.timeout == 0xfffff {
                    self.error = true;
                    self.valid = true;
                    self.last = true;
                    self.state = 4;
                } else {
                    self.timeout = o.timeout + 1;
                }
            }
            4 => {
                if i.cpu_response_ready {
                    self.valid = false;
                    self.last = false;
                    self.error = false;
                    self.state = 0;
                }
            }
            _ => self.state = 0,
        }
        if o.terminal(i) {
            let launch = o.queued.as_ref().unwrap();
            if !o.legal(launch) || i.controller_request_ready {
                self.launch(launch);
                self.write_hold = launch.cpu_write && launch.cpu_line;
            }
        }
    }
    fn launch(&mut self, i: &Input) {
        self.queued = None;
        self.writing = i.cpu_write;
        self.line = i.cpu_line;
        self.lane = i.cpu_address & 1 != 0;
        self.total = if self.chained_groups && i.cpu_line_count_minus_1 == 2 {
            64
        } else {
            (4 * (i.cpu_line_count_minus_1 + 1)) as u8
        };
        self.fed = 0;
        self.beats = 0;
        self.read_seen = false;
        self.done_seen = false;
        self.timeout = 0;
        self.valid = false;
        self.last = false;
        self.error = !self.legal(i);
        if self.error {
            self.data = 0;
            self.valid = true;
            self.last = true;
            self.state = 4;
        } else {
            self.scalar_write = (i.cpu_write_data & 65535) << if self.lane { 16 } else { 0 };
            self.state = 1;
        }
    }
}
