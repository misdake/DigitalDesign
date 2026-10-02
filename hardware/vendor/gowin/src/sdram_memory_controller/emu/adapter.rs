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
}
impl State {
    fn legal(i: &Input) -> bool {
        !i.cpu_line
            || (i.cpu_line_count_minus_1 != 2
                && i.cpu_address & ((16 * (i.cpu_line_count_minus_1 + 1)) - 1) == 0)
    }
    fn active(&self) -> bool {
        self.state == 1 && self.writing && self.fed < if self.line { self.total } else { 1 }
    }
    pub fn output(&self, i: &Input) -> Output {
        Output {
            cpu_request_ready: self.state == 0
                && i.controller_init_done
                && (!i.cpu_request_valid || !Self::legal(i) || i.controller_request_ready),
            cpu_write_data_ready: self.active() && self.line && i.controller_write_data_ready,
            cpu_response_valid: self.valid,
            cpu_read_data: self.data,
            cpu_response_last: self.last,
            cpu_error: self.error,
            controller_request_valid: self.state == 0
                && i.cpu_request_valid
                && i.controller_init_done
                && Self::legal(i),
            controller_write: i.cpu_write,
            controller_address: i.cpu_address >> 1,
            controller_write_mask: if i.cpu_write && !i.cpu_line {
                if i.cpu_address & 1 != 0 {
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
            controller_write_data_valid: self.active(),
            controller_words: if !i.cpu_line {
                1
            } else {
                8 * (i.cpu_line_count_minus_1 + 1)
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
            return;
        }
        match o.state {
            0 => {
                if i.cpu_request_valid && output.cpu_request_ready {
                    self.writing = i.cpu_write;
                    self.line = i.cpu_line;
                    self.lane = i.cpu_address & 1 != 0;
                    self.total = (4 * (i.cpu_line_count_minus_1 + 1)) as u8;
                    self.fed = 0;
                    self.beats = 0;
                    self.read_seen = false;
                    self.done_seen = false;
                    self.timeout = 0;
                    self.valid = false;
                    self.last = false;
                    self.error = !Self::legal(i);
                    if self.error {
                        self.data = 0;
                        self.valid = true;
                        self.last = true;
                        self.state = 4;
                    } else {
                        self.scalar_write =
                            (i.cpu_write_data & 65535) << if self.lane { 16 } else { 0 };
                        self.state = 1;
                    }
                }
            }
            1 => {
                self.timeout = (o.timeout + 1) & 0xfffff;
                if o.active() && i.controller_write_data_ready {
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
    }
}
