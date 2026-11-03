//! Selected Group35/H15 cache, sole weight RAM and borrowed texel-bank DO.
//! Tags are wall-clock speculative reads. A paid line pin ends at DO capture;
//! the DO itself remains borrowed by Color until its later source ACK.
use super::{
    super::{require, vertex::Bits, ContractError},
    word::Word,
};
fn or(a: Bits, b: Bits) -> Bits {
    let ones = (a.value & a.known) | (b.value & b.known);
    Bits {
        value: ones & 1,
        known: (ones | (!a.value & a.known & !b.value & b.known)) & 1,
    }
}
fn and(a: Bits, b: Bits) -> Bits {
    let zeros = (!a.value & a.known) | (!b.value & b.known);
    Bits {
        value: (a.value & b.value) & 1,
        known: (zeros | (a.value & a.known & b.value & b.known)) & 1,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub proposal_ce: bool,
    pub commit_allow: bool,
    pub abort: bool,
    pub valid: bool,
    pub payload: Word,
    pub texture_invalidate: bool,
    pub weight_write: bool,
    pub weight_address: u8,
    pub weight_value: Bits,
    pub next_valid: bool,
    pub next_key: u16,
    pub out_ready: bool,
    pub mem_req_ready: bool,
    pub mem_resp_valid: bool,
    pub mem_resp_index: u8,
    pub mem_resp_data: u64,
    pub mem_resp_complete: bool,
    pub mem_resp_ok: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub proposal_ready: bool,
    pub in_ready: bool,
    pub advance: bool,
    pub valid: bool,
    pub payload: Word,
    pub texels: [Bits; 4],
    pub mem_req_valid: bool,
    pub mem_req_addr: u32,
    pub drained: bool,
    pub fault: bool,
    pub hit_ready: bool,
    pub look_filling: bool,
    pub hit_bits: Bits,
    pub filling_bits: Bits,
    pub hit_line_bits: Bits,
    pub hit_line: u8,
    pub victim_line: u8,
    pub read_issue: bool,
    pub tag_read: bool,
    pub tag_write: bool,
    pub tag_set: u8,
    pub state_set: u8,
    pub state_write: bool,
    pub state_next: u8,
    pub plru_write: bool,
    pub plru_next: Bits,
    pub result_pop: bool,
    pub return_capture: bool,
    pub weight_ra: u8,
    pub data_ra: [u16; 4],
    pub data_write: bool,
    pub data_waddr: u16,
}
#[derive(Clone, Debug)]
pub struct Cache {
    pub state_rows: [Bits; 8],
    pub plru_rows: [Bits; 8],
    pub state_initialized: u8,
    pub plru_initialized: u8,
    pub tags: [Bits; 32],
    pub tag_q: [Bits; 4],
    pub tag_valid: bool,
    pub weights: [Bits; 128],
    pub weight_q: Bits,
    pub banks: [Vec<Bits>; 4],
    pub data_q: [Bits; 4],
    pub res_payload: Word,
    pub paid_payload: Word,
    pub res_weights: Bits,
    pub res_valid: bool,
    pub rd_valid: bool,
    pub pin_line: Bits,
    pub desc_valid: bool,
    pub desc_started: bool,
    pub desc_presented: bool,
    pub desc_line: Bits,
    pub desc_next: u8,
    pub desc_sector: bool,
    pub fault: bool,
    descriptor_was_valid: bool,
    descriptor_address: u32,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            state_rows: [Bits::default(); 8],
            plru_rows: [Bits::default(); 8],
            state_initialized: 0,
            plru_initialized: 0,
            tags: [Bits::default(); 32],
            tag_q: [Bits::default(); 4],
            tag_valid: false,
            weights: [Bits::default(); 128],
            weight_q: Bits::new(0, 36),
            banks: std::array::from_fn(|_| vec![Bits::default(); 1024]),
            data_q: [Bits::new(0, 16); 4],
            res_payload: Word::default(),
            paid_payload: Word::default(),
            res_weights: Bits::default(),
            res_valid: false,
            rd_valid: false,
            pin_line: Bits::default(),
            desc_valid: false,
            desc_started: false,
            desc_presented: false,
            desc_line: Bits::default(),
            desc_next: 0,
            desc_sector: false,
            fault: false,
            descriptor_was_valid: false,
            descriptor_address: 0,
        }
    }
}
impl Cache {
    pub fn native() -> Self {
        Self {
            tags: [Bits::new(0, 18); 32],
            tag_q: [Bits::new(0, 18); 4],
            weights: [Bits::new(0, 36); 128],
            banks: std::array::from_fn(|_| vec![Bits::new(0, 16); 1024]),
            ..Self::default()
        }
    }
    pub fn outputs(&self, i: Input) -> Output {
        let h = i.payload.slice(0, 15).value as u16;
        let set = (h & 7) as u8;
        let state_set = if self.desc_valid {
            self.desc_line.value as u8 >> 2
        } else {
            set
        };
        let state = if self.state_initialized & (1 << state_set) != 0 {
            self.state_rows[state_set as usize].value as u8
        } else {
            0
        };
        let plru = if self.plru_initialized & (1 << state_set) != 0 {
            self.plru_rows[state_set as usize].value as u8
        } else {
            0
        };
        let mut ready = [Bits::new(0, 1); 4];
        let mut filling = ready;
        for w in 0..4 {
            let s = state >> (w * 2) & 3;
            let tag = self.tag_q[w];
            let compare = if (tag.value ^ u64::from(h >> 3)) & tag.known & 0x3ffff != 0 {
                Bits::new(0, 1)
            } else if tag.is_known(18) {
                Bits::new(1, 1)
            } else {
                Bits::default()
            };
            if s == 2 {
                ready[w] = compare;
            }
            if s != 0 && s != 2 {
                filling[w] = compare;
            }
        }
        let hit_bits = ready.into_iter().fold(Bits::new(0, 1), or);
        let filling_bits = filling.into_iter().fold(Bits::new(0, 1), or);
        let high = or(ready[3], ready[2]);
        let low = or(
            ready[3],
            and(
                ready[1],
                Bits {
                    value: ready[2].value ^ 1,
                    known: ready[2].known,
                },
            ),
        );
        let hit_line_bits =
            Bits::concat(Bits::new(u64::from(set), 3), Bits::concat(high, low, 1), 2);
        let hit_way = hit_line_bits.value as u8 & 3;
        let hit_ready = hit_bits.value != 0;
        let look_filling = filling_bits.value != 0;
        let pinned = |line: u8| self.rd_valid && self.pin_line.value == u64::from(line);
        let invalid = (0..4).find(|&w| state >> (w * 2) & 3 == 0 && !pinned(set * 4 + w));
        let first = ((plru & 1) << 1)
            | if plru & 1 != 0 {
                plru >> 2 & 1
            } else {
                plru >> 1 & 1
            };
        let victim_line = set * 4
            + invalid.unwrap_or_else(|| {
                if pinned(set * 4 + first) {
                    first ^ 1
                } else {
                    first
                }
            });
        let advance = i.proposal_ce && !i.reset && !self.fault && !i.abort && i.commit_allow;
        let proposal_ready = !i.reset
            && !self.fault
            && !i.abort
            && i.valid
            && self.tag_valid
            && !self.desc_valid
            && !self.rd_valid
            && hit_ready;
        let read_issue = advance && proposal_ready;
        let tag_write =
            advance && i.valid && self.tag_valid && !self.desc_valid && !hit_ready && !look_filling;
        let fault_eff = self.fault || i.abort;
        let tag_read = !i.reset && !fault_eff && i.valid && !tag_write;
        let result_pop = advance && i.out_ready && self.res_valid;
        let capture = self.rd_valid && (!self.res_valid || result_pop);
        let publish =
            i.mem_resp_complete && self.desc_sector && i.mem_resp_ok && !i.reset && !fault_eff;
        let state_write = tag_write
            || (self.desc_valid
                && ((i.mem_resp_complete && (self.desc_sector || !i.mem_resp_ok)) || fault_eff));
        let way = if tag_write {
            victim_line & 3
        } else {
            self.desc_line.value as u8 & 3
        };
        let value = if tag_write {
            1
        } else if publish {
            2
        } else {
            0
        };
        let state_next = (state & !(3 << (way * 2))) | (value << (way * 2));
        let used = if publish {
            self.desc_line.value as u8 & 3
        } else {
            hit_way
        };
        let used_known = if publish {
            3
        } else {
            hit_line_bits.known as u8 & 3
        };
        let mut plru_next = Bits::new(0, 3);
        let mut first_value: Option<u8> = None;
        for candidate in 0..4 {
            if (candidate ^ used) & used_known == 0 {
                let value = if candidate & 2 != 0 {
                    ((candidate & 1) ^ 1) << 2 | (plru & 2)
                } else {
                    (plru & 4) | ((candidate & 1) ^ 1) << 1 | 1
                };
                if let Some(first) = first_value {
                    plru_next.known &= u64::from(!(first ^ value) & 7);
                } else {
                    first_value = Some(value);
                    plru_next = Bits::new(u64::from(value), 3);
                }
            }
        }
        let x = self.paid_payload.slice(15, 4).value as u8;
        let y = self.paid_payload.slice(19, 3).value as u8;
        let mut data_ra = [0; 4];
        for (b, ra) in data_ra.iter_mut().enumerate() {
            let bx = if x & 1 == b as u8 & 1 {
                x
            } else {
                (x + 1) & 15
            };
            let by = if y & 1 == ((bx >> 1 & 1) ^ (b as u8 >> 1)) {
                y
            } else {
                (y + 1) & 7
            };
            *ra = (self.pin_line.value as u16) * 32 + u16::from(by * 4 + bx / 4);
        }
        let mut masked = [Bits::new(0, 9); 4];
        for (t, v) in masked.iter_mut().enumerate() {
            if self.res_payload.slice(31 + t as u32, 1).value != 0 {
                *v = self.res_weights.slice(t as u32 * 9, 9);
            }
        }
        let rx = self.res_payload.slice(15, 4).value as usize;
        let ry = self.res_payload.slice(19, 3).value as usize;
        let mut payload = Word::new(0, 72);
        payload.set(64, 8, self.res_payload.slice(22, 8));
        for (t, v) in masked.into_iter().enumerate() {
            let tx = (rx + (t & 1)) & 15;
            let ty = (ry + (t >> 1)) & 7;
            let bank = (tx & 1) | (((ty & 1) ^ ((tx >> 1) & 1)) << 1);
            payload.set(28 + bank as u32 * 9, 9, v);
        }
        if !self.res_payload.slice(15, 20).is_known(20) {
            payload.set(28, 36, Bits::default());
        }
        let present = !i.reset
            && self.desc_valid
            && !self.desc_started
            && (!self.fault || self.desc_presented);
        Output {
            proposal_ready,
            in_ready: read_issue,
            advance,
            valid: !i.reset && !fault_eff && self.res_valid,
            payload,
            texels: self.data_q,
            mem_req_valid: present && (!fault_eff || self.desc_presented),
            mem_req_addr: u32::from(h) * 256 + u32::from(self.desc_sector) * 128,
            drained: !self.desc_valid,
            fault: self.fault,
            hit_ready,
            look_filling,
            hit_bits,
            filling_bits,
            hit_line_bits,
            hit_line: set * 4 + hit_way,
            victim_line,
            read_issue,
            tag_read,
            tag_write,
            tag_set: if read_issue && i.next_valid {
                i.next_key as u8 & 7
            } else {
                set
            },
            state_set,
            state_write,
            state_next,
            plru_write: publish || read_issue,
            plru_next,
            result_pop,
            return_capture: capture,
            weight_ra: ((i.payload.value >> 24 & 15) << 3
                | (i.payload.value >> 28 & 3) << 1
                | (i.payload.value >> 30 & 1)) as u8,
            data_ra,
            data_write: !i.reset && !fault_eff && i.mem_resp_valid,
            data_waddr: (self.desc_line.value as u16) * 32
                + u16::from(self.desc_sector) * 16
                + u16::from(i.mem_resp_index & 15),
        }
    }
    pub fn clock(&mut self, i: Input) -> Result<(), ContractError> {
        let o = self.outputs(i);
        let accepted = o.mem_req_valid && i.mem_req_ready;
        let req_present = !i.reset
            && self.desc_valid
            && !self.desc_started
            && (!self.fault || self.desc_presented);
        let started = self.desc_valid && (self.desc_started || (req_present && i.mem_req_ready));
        let count = self.desc_next + u8::from(i.mem_resp_valid);
        let address = u32::from(i.payload.slice(0, 15).value as u16) * 256;
        let stable = require(
            i.reset || !self.descriptor_was_valid || address == self.descriptor_address,
            "Group cache",
            "Group sector changed before its descriptor drained",
        );
        let framing = require(
            i.reset
                || i.abort
                || !((i.mem_resp_valid
                    && (!started || self.desc_next >= 16 || i.mem_resp_index != self.desc_next))
                    || (i.mem_resp_complete && (!started || count != 16))),
            "Group cache",
            "GPU contract: texture return framing",
        );
        let terminal = require(
            i.reset || !i.mem_resp_complete || i.mem_resp_ok,
            "Group cache",
            "GPU contract: mem_resp_ok",
        );
        let old_desc = self.desc_valid;
        // Synchronous RAM reads see the old rows on a simultaneous write.
        let read_weights = self.weights[usize::from(o.weight_ra)];
        if i.weight_write {
            self.weights[usize::from(i.weight_address & 127)] = i.weight_value;
        }
        if i.reset {
            self.weight_q = Bits::new(0, 36);
            self.data_q = [Bits::new(0, 16); 4];
        } else {
            if o.return_capture {
                self.res_payload = self.paid_payload;
                self.res_weights = self.weight_q;
                // Each bank has its own independently permuted paid address.
                for (b, q) in self.data_q.iter_mut().enumerate() {
                    *q = self.banks[b][o.data_ra[b] as usize];
                }
            }
            if o.read_issue {
                self.weight_q = read_weights;
            }
        }
        if o.data_write {
            let swap = usize::from(i.mem_resp_index & 4 != 0) * 2;
            for b in 0..4 {
                self.banks[b][o.data_waddr as usize] =
                    Bits::new(i.mem_resp_data >> ((b ^ swap) * 16) & 65535, 16);
            }
        }
        if o.tag_read {
            for w in 0..4 {
                self.tag_q[w] = self.tags[usize::from(o.tag_set) * 4 + w];
            }
        }
        if o.tag_write {
            self.tags[usize::from(o.victim_line)] = Bits::new(u64::from(address >> 11), 18);
        }
        if i.reset || i.texture_invalidate {
            self.state_initialized = 0;
            self.plru_initialized = 0;
        } else {
            if o.state_write {
                self.state_rows[o.state_set as usize] = Bits::new(u64::from(o.state_next), 8);
                self.state_initialized |= 1 << o.state_set;
            }
            if o.plru_write {
                self.plru_rows[o.state_set as usize] = o.plru_next;
                self.plru_initialized |= 1 << o.state_set;
            }
        }
        if i.reset {
            self.res_valid = false;
            self.rd_valid = false;
            self.tag_valid = false;
            self.desc_valid = false;
            self.desc_started = false;
            self.desc_presented = false;
            self.desc_next = 0;
            self.desc_sector = false;
            self.fault = false;
            self.descriptor_was_valid = false;
        } else {
            if i.texture_invalidate || !i.valid || o.tag_write {
                self.tag_valid = false;
            } else if o.read_issue {
                self.tag_valid = i.next_valid;
            } else if o.tag_read {
                self.tag_valid = true;
            }
            if o.return_capture {
                self.res_valid = true;
                self.rd_valid = false;
            }
            if o.mem_req_valid {
                self.desc_presented = true;
            }
            if accepted {
                self.desc_started = true;
            }
            if i.mem_resp_valid
                && old_desc
                && started
                && self.desc_next < 16
                && i.mem_resp_index == self.desc_next
            {
                self.desc_next = count;
            }
            if i.mem_resp_complete {
                if !self.desc_sector && i.mem_resp_ok && !self.fault && !i.abort {
                    self.desc_sector = true;
                    self.desc_started = false;
                    self.desc_presented = false;
                    self.desc_next = 0;
                } else {
                    self.desc_valid = false;
                }
            }
            if (self.fault || i.abort) && old_desc && !self.desc_presented && !o.mem_req_valid {
                self.desc_valid = false;
            }
            if o.advance {
                if o.result_pop && !o.return_capture {
                    self.res_valid = false;
                }
                if o.read_issue {
                    self.paid_payload = i.payload;
                    self.rd_valid = true;
                    self.pin_line = Bits::new(u64::from(o.hit_line), 5);
                }
                if o.tag_write {
                    self.desc_line = Bits::new(u64::from(o.victim_line), 5);
                    self.desc_sector = false;
                    self.desc_started = false;
                    self.desc_presented = false;
                    self.desc_next = 0;
                    self.desc_valid = true;
                }
            }
            if i.abort {
                self.fault = true;
            }
            self.descriptor_was_valid = old_desc;
            self.descriptor_address = address;
        }
        stable?;
        framing?;
        terminal?;
        require(
            i.reset || i.abort || !o.fault,
            "Group cache",
            "GPU contract: fault",
        )
    }
}
