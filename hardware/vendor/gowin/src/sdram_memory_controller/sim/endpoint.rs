//! Current stream endpoint's logic-edge reference. No native-clock or pad timing.
use crate::sdram_memory_controller::streaming_port::{
    StreamingSdramPortInputValue as Input, StreamingSdramPortOutputValue as Output,
};

#[derive(Clone, Copy, Default)]
struct Descriptor {
    address: u64,
    class: u64,
    count: u64,
    write: bool,
    line: bool,
    error: bool,
    supplied: bool,
}

#[derive(Clone, Default)]
pub struct State {
    queue: [Descriptor; 2],
    put: u8,
    head: u8,
    sent: u8,
    rx: u8,
    tx_beat: u8,
    rx_beat: u8,
    scalar: u16,
    valid: bool,
    last: bool,
    error: bool,
    data: u64,
}

impl State {
    fn used(&self) -> u8 {
        self.put.wrapping_sub(self.head) & 3
    }
    fn same_next(&self) -> bool {
        let a = self.queue[(self.head & 1) as usize];
        let b = self.queue[(!self.head & 1) as usize];
        self.used() == 2
            && a.class != 0
            && a.class == b.class
            && a.write == b.write
            && a.line
            && b.line
    }
    fn legal(i: &Input) -> bool {
        !i.cpu_line
            || match i.cpu_line_count_minus_1 {
                0 => i.cpu_address & 15 == 0,
                1 => i.cpu_address & 31 == 0,
                3 => i.cpu_address & 63 == 0,
                _ => false,
            }
    }
    fn words(d: Descriptor) -> u8 {
        if d.line {
            ((d.count + 1) * 4) as u8
        } else {
            1
        }
    }
    fn tx_slot(&self) -> usize {
        let h = (self.head & 1) as usize;
        if (self.sent.wrapping_sub(self.head) & 3) == 2
            && self.same_next()
            && self.queue[h].write
            && self.queue[h].supplied
        {
            h ^ 1
        } else {
            h
        }
    }
    pub fn output(&self, i: &Input) -> Output {
        let sent = self.queue[(self.sent & 1) as usize];
        let tx = self.queue[self.tx_slot()];
        let forwarded = self.sent.wrapping_sub(self.head) & 3;
        let feeding = self.used() != 0 && forwarded != 0 && tx.write && !tx.supplied && !i.reset;
        Output {
            cpu_request_ready: i.controller_init_done
                && self.used() < 2
                && (Self::legal(i) || self.used() == 0)
                && (!i.cpu_write || i.cpu_line || self.used() == 0)
                && !i.reset,
            cpu_lookahead_window: self.used() == 1
                && self.queue[(self.head & 1) as usize].line
                && forwarded != 0
                && i.controller_stream_active,
            cpu_write_data_ready: feeding && i.controller_write_data_ready && tx.line,
            cpu_response_valid: self.valid,
            cpu_read_data: self.data,
            cpu_response_last: self.last,
            cpu_error: self.error,
            controller_request_valid: self.sent != self.put
                && (self.sent == self.head || self.same_next())
                && !sent.error
                && !i.reset,
            controller_next_valid: self.used() == 2,
            controller_next_address: self.queue[(!self.head & 1) as usize].address >> 1,
            controller_request_continuation: self.sent != self.head,
            controller_write: sent.write,
            controller_address: sent.address >> 1,
            controller_write_mask: if sent.write && !sent.line {
                if sent.address & 1 != 0 {
                    3
                } else {
                    12
                }
            } else {
                0
            },
            controller_write_data: if tx.line {
                i.cpu_write_data
            } else if tx.address & 1 != 0 {
                u64::from(self.scalar) << 16
            } else {
                u64::from(self.scalar)
            },
            controller_write_data_valid: feeding,
            controller_words: if sent.line { (sent.count + 1) * 8 } else { 1 },
        }
    }
    pub fn clock(&mut self, i: &Input) {
        if i.reset {
            *self = Self::default();
            return;
        }
        let old = self.clone();
        let o = old.output(i);
        if i.cpu_request_valid && o.cpu_request_ready {
            self.queue[(old.put & 1) as usize] = Descriptor {
                address: i.cpu_address,
                class: i.cpu_chain_class,
                count: i.cpu_line_count_minus_1,
                write: i.cpu_write,
                line: i.cpu_line,
                error: !Self::legal(i),
                supplied: false,
            };
            if i.cpu_write && !i.cpu_line {
                self.scalar = i.cpu_write_data as u16;
            }
            self.put = (old.put + 1) & 3;
        }
        if o.controller_request_valid && i.controller_request_ready {
            self.sent = (old.sent + 1) & 3;
        }
        let rx = old.queue[(old.rx & 1) as usize];
        if !old.valid || i.cpu_response_ready {
            self.data = if rx.line {
                i.controller_read_data
            } else {
                (i.controller_read_data >> ((rx.address & 1) * 16)) & 65535
            };
        }
        if o.controller_write_data_valid && i.controller_write_data_ready {
            if old.tx_beat == Self::words(old.queue[old.tx_slot()]) - 1 {
                self.queue[old.tx_slot()].supplied = true;
                self.tx_beat = 0;
            } else {
                self.tx_beat = old.tx_beat + 1;
            }
        }
        if i.cpu_response_ready {
            self.valid = false;
            self.last = false;
            self.error = false;
        }
        if i.controller_read_valid {
            assert!(
                !old.valid || i.cpu_response_ready,
                "stream read sink stalled"
            );
            self.valid = true;
            self.last = old.rx_beat == Self::words(rx) - 1;
            if self.last {
                self.rx_beat = 0;
                self.rx = (old.rx + 1) & 3;
            } else {
                self.rx_beat = old.rx_beat + 1;
            }
        }
        if i.controller_write_done {
            self.valid = true;
            self.last = true;
            self.data = 0;
            self.rx = (old.rx + 1) & 3;
        }
        if old.used() != 0 && old.queue[(old.head & 1) as usize].error && !old.valid {
            self.valid = true;
            self.last = true;
            self.error = true;
        }
        if old.valid && old.last && i.cpu_response_ready {
            self.head = (old.head + 1) & 3;
            if old.queue[(old.head & 1) as usize].error {
                self.sent = self.head;
                self.rx = self.head;
            }
        }
    }
}
