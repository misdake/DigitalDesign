//! Candidate cycle model: row-zero owns the sole full canonical description.
//! Eight-row ingress and independent branch source queues.
//!
//! This unit owns reservation/publication, not result completion or join. A
//! global slot remains occupied until the external ROP row-seven acceptance.
//! A branch entry expires on its own last source-row handshake. Publication
//! exports a compact descriptor to the downstream status owner exactly once;
//! it does not allocate another global descriptor array here.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Begin {
    pub mask: u8,
    pub draw: bool,
    pub unlit: bool,
    pub untextured: bool,
    pub force_coarsest: bool,
    /// Packed quad origin, owned downstream after publication.
    pub origin: u16,
}

impl Begin {
    pub fn packed(self) -> u32 {
        u32::from(self.mask)
            | (u32::from(self.draw) << 4)
            | (u32::from(self.unlit) << 5)
            | (u32::from(self.untextured) << 6)
            | (u32::from(self.force_coarsest) << 7)
            | (u32::from(self.origin) << 8)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Beat {
    /// Consulted only at row zero; the row cursor is implicit.
    pub begin: Begin,
    pub basic: u32,
    pub lighting: u64,
    /// Consulted only on odd rows, including uncovered helper lanes.
    pub uv: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub reset: bool,
    pub ce: bool,
    pub input: Option<Beat>,
    pub light_ready: bool,
    pub sample_ready: bool,
    /// Already accepted, ordered external ROP row-seven event.
    pub rop_row7: Option<u8>,
    /// Join must reserve the return sink before supplying this address.
    pub basic_read: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Descriptor {
    pub slot: u8,
    pub mask: u8,
    pub draw: bool,
    pub force_coarsest: bool,
}

impl Descriptor {
    pub fn packed(self) -> u16 {
        u16::from(self.slot)
            | (u16::from(self.mask) << 5)
            | (u16::from(self.draw) << 9)
            | (u16::from(self.force_coarsest) << 10)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub descriptor: Descriptor,
    pub row: u8,
    pub last: bool,
    pub data: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    /// Shared commit enable; attached consumers must use this, not raw CE.
    pub advance: bool,
    pub input_ready: bool,
    pub input_row: u8,
    pub light: Option<Row>,
    pub sample: Option<Row>,
    pub basic_return: Option<u32>,
    pub basic_collision: bool,
    pub global_used: u8,
    pub light_used: u8,
    pub sample_used: u8,
    pub retire_slot: u8,
    pub fault: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step {
    pub signals: Signals,
    pub accepted: bool,
    pub published: Option<(u8, Begin)>,
    pub basic_write: Option<(u8, u32)>,
    pub light_accepted: bool,
    pub sample_accepted: bool,
    pub light_returned: bool,
    pub sample_returned: bool,
}

/// Cycle model of the reused SPSC registered-read variant. The pending hard DO
/// owns its payload during CE0 and may move into an already paid head on a wall
/// edge. No new memory request, issue credit or row consumption occurs at CE0.
struct Rows {
    entries: u8,
    rows: u8,
    ram: [u64; 32],
    insert: u8,
    consume: u8,
    issue: u8,
    write_row: u8,
    issue_row: u8,
    consume_row: u8,
    pending: Option<u64>,
    heads: [u64; 2],
    head_read: usize,
    head_write: usize,
    head_count: usize,
}

impl Rows {
    fn new(entries: u8, rows: u8) -> Self {
        Self {
            entries,
            rows,
            ram: [0; 32],
            insert: 0,
            consume: 0,
            issue: 0,
            write_row: 0,
            issue_row: 0,
            consume_row: 0,
            pending: None,
            heads: [0; 2],
            head_read: 0,
            head_write: 0,
            head_count: 0,
        }
    }

    fn output(&self) -> Option<(u64, u8, bool)> {
        let data = if self.head_count != 0 {
            Some(self.heads[self.head_read])
        } else {
            self.pending
        };
        data.map(|d| (d, self.consume_row, self.consume_row == self.rows - 1))
    }

    fn tick(&mut self, ce: bool, input: Option<u64>, ready: bool) -> bool {
        let output = self.output();
        let pop = ce && ready && output.is_some();
        let pop_pending = pop && self.head_count == 0;
        let pop_head = pop && self.head_count != 0;
        let returned = self.pending.is_some() && self.head_count < 2 && !pop_pending;
        let published = input.is_some() && self.write_row == self.rows - 1;
        let request = ce
            && self.head_count < 2
            && (self.issue != self.insert || (published && self.issue_row < self.write_row));
        let read_address = usize::from(self.issue % self.entries) * usize::from(self.rows)
            + usize::from(self.issue_row);
        let write_address = usize::from(self.insert % self.entries) * usize::from(self.rows)
            + usize::from(self.write_row);
        assert!(!request || input.is_none() || read_address != write_address);
        let read_data = request.then_some(self.ram[read_address]);
        if let Some(data) = input {
            assert!(ce);
            assert!(
                self.write_row != 0
                    || (self.insert.wrapping_sub(self.consume) & (2 * self.entries - 1))
                        < self.entries
            );
            self.ram[write_address] = data;
            if published {
                self.insert = (self.insert + 1) & (2 * self.entries - 1);
                self.write_row = 0;
            } else {
                self.write_row += 1;
            }
        }
        if pop_head {
            self.head_read ^= 1;
        }
        if pop {
            if self.consume_row == self.rows - 1 {
                self.consume = (self.consume + 1) & (2 * self.entries - 1);
                self.consume_row = 0;
            } else {
                self.consume_row += 1;
            }
        }
        if returned {
            self.heads[self.head_write] = self.pending.unwrap();
            self.head_write ^= 1;
        }
        self.head_count = self.head_count + usize::from(returned) - usize::from(pop_head);
        if request {
            self.pending = read_data;
            if self.issue_row == self.rows - 1 {
                self.issue = (self.issue + 1) & (2 * self.entries - 1);
                self.issue_row = 0;
            } else {
                self.issue_row += 1;
            }
        } else if returned || pop_pending {
            self.pending = None;
        }
        returned
    }
}

/// Bounded register controller; no host Ticket, generation, or transaction list.
pub struct CanonicalIngress {
    row: u8,
    held_control: Begin,
    canonical: [Begin; 32],
    alloc: u8,
    published: u8,
    retire: u8,
    light_alloc: u8,
    light_consume: u8,
    sample_alloc: u8,
    sample_consume: u8,
    light_desc: [Descriptor; 2],
    sample_desc: [Descriptor; 8],
    basic: [u32; 256],
    basic_return: Option<u32>,
    light: Rows,
    sample: Rows,
    fault: bool,
    wall: u64,
    max_wall: u64,
}

impl CanonicalIngress {
    pub fn new(max_wall: u64) -> Self {
        assert!(max_wall > 0);
        Self {
            row: 0,
            held_control: Begin::default(),
            canonical: [Begin::default(); 32],
            alloc: 0,
            published: 0,
            retire: 0,
            light_alloc: 0,
            light_consume: 0,
            sample_alloc: 0,
            sample_consume: 0,
            light_desc: [Descriptor::default(); 2],
            sample_desc: [Descriptor::default(); 8],
            basic: [0; 256],
            basic_return: None,
            light: Rows::new(2, 8),
            sample: Rows::new(8, 4),
            fault: false,
            wall: 0,
            max_wall,
        }
    }

    pub fn signals(&self, tick: Tick) -> Signals {
        let begin = if self.row == 0 {
            tick.input.unwrap_or_default().begin
        } else {
            self.held_control
        };
        let global_used = self.alloc.wrapping_sub(self.retire) & 63;
        let light_used = self.light_alloc.wrapping_sub(self.light_consume) & 3;
        let sample_used = self.sample_alloc.wrapping_sub(self.sample_consume) & 15;
        let room = begin.mask == 0
            || (global_used < 32
                && (begin.unlit || light_used < 2)
                && (begin.untextured || sample_used < 8));
        let proposed_ready = !tick.reset && !self.fault && tick.ce && (self.row != 0 || room);
        let proposed_push = proposed_ready && tick.input.is_some();
        let slot = if self.row == 0 {
            self.alloc & 31
        } else {
            self.alloc.wrapping_sub(1) & 31
        };
        let basic_collision = tick.ce
            && proposed_push
            && begin.mask != 0
            && tick.basic_read == Some((slot << 3) | self.row);
        let bad_depth = proposed_push
            && begin.mask != 0
            && self.row & 1 != 0
            && tick.input.unwrap().basic >> 16 != 0;
        let bad_retire = tick.rop_row7.is_some_and(|slot| {
            !tick.ce || self.retire == self.published || slot != (self.retire & 31)
        });
        let advance =
            !tick.reset && tick.ce && !self.fault && !basic_collision && !bad_depth && !bad_retire;
        let row = |queue: &Rows, descriptor| {
            queue.output().map(|(data, row, last)| Row {
                descriptor,
                row,
                last,
                data,
            })
        };
        Signals {
            advance,
            input_ready: proposed_ready && advance,
            input_row: self.row,
            light: (!tick.reset)
                .then(|| {
                    row(
                        &self.light,
                        self.light_desc[usize::from(self.light_consume & 1)],
                    )
                })
                .flatten(),
            sample: (!tick.reset)
                .then(|| {
                    row(
                        &self.sample,
                        self.sample_desc[usize::from(self.sample_consume & 7)],
                    )
                })
                .flatten(),
            basic_return: (!tick.reset).then_some(self.basic_return).flatten(),
            basic_collision,
            global_used,
            light_used,
            sample_used,
            retire_slot: self.retire & 31,
            fault: self.fault,
        }
    }

    pub fn tick(&mut self, tick: Tick) -> Result<Step, String> {
        self.wall += 1;
        if self.wall > self.max_wall {
            return Err("quad ingress wall watchdog".into());
        }
        let signals = self.signals(tick);
        if tick.reset {
            let wall = self.wall;
            *self = Self::new(self.max_wall);
            self.wall = wall;
            return Ok(Step {
                signals,
                ..Step::default()
            });
        }
        let accepted = signals.input_ready && tick.input.is_some();
        let advance = signals.advance;
        if (!self.fault && tick.ce && !advance) || (tick.rop_row7.is_some() && !advance) {
            self.fault = true;
        }
        let begin = if self.row == 0 {
            tick.input.unwrap_or_default().begin
        } else {
            self.held_control
        };
        let live = begin.mask != 0;
        let slot = if self.row == 0 {
            self.alloc & 31
        } else {
            self.alloc.wrapping_sub(1) & 31
        };
        let beat = tick.input.unwrap_or_default();
        if accepted
            && (begin.mask > 15
                || begin.origin > 0x7fff
                || beat.lighting >> 36 != 0
                || beat.uv >> 36 != 0)
        {
            return Err("quad ingress input width".into());
        }
        let basic_write =
            (accepted && live).then_some(((slot << 3) | self.row, beat.basic & 0xffffff));
        let light_accepted = advance && tick.light_ready && signals.light.is_some();
        let sample_accepted = advance && tick.sample_ready && signals.sample.is_some();
        let light_returned = self.light.tick(
            advance,
            (accepted && live && !begin.unlit).then_some(beat.lighting),
            tick.light_ready,
        );
        let sample_returned = self.sample.tick(
            advance,
            (accepted && live && !begin.untextured && self.row & 1 != 0).then_some(beat.uv),
            tick.sample_ready,
        );
        // Basic hard DO is a reserved return and progresses independently of CE.
        self.basic_return = tick
            .basic_read
            .filter(|_| advance)
            .map(|a| self.basic[usize::from(a)]);
        if let Some((a, data)) = basic_write {
            self.basic[usize::from(a)] = data;
        }
        if light_accepted && signals.light.unwrap().last {
            self.light_consume = (self.light_consume + 1) & 3;
        }
        if sample_accepted && signals.sample.unwrap().last {
            self.sample_consume = (self.sample_consume + 1) & 15;
        }
        if advance && tick.rop_row7.is_some() {
            self.retire = (self.retire + 1) & 63;
        }
        let published = (accepted && live && self.row == 7)
            .then_some((slot, self.canonical[usize::from(slot)]));
        if accepted {
            if self.row == 0 {
                self.held_control = Begin {
                    origin: 0,
                    force_coarsest: false,
                    ..begin
                };
                if live {
                    self.canonical[usize::from(slot)] = begin;
                }
                if live {
                    self.alloc = (self.alloc + 1) & 63;
                    let descriptor = Descriptor {
                        slot,
                        mask: begin.mask,
                        draw: begin.draw,
                        force_coarsest: begin.force_coarsest,
                    };
                    if !begin.unlit {
                        self.light_desc[usize::from(self.light_alloc & 1)] = Descriptor {
                            force_coarsest: false,
                            ..descriptor
                        };
                        self.light_alloc = (self.light_alloc + 1) & 3;
                    }
                    if !begin.untextured {
                        self.sample_desc[usize::from(self.sample_alloc & 7)] = descriptor;
                        self.sample_alloc = (self.sample_alloc + 1) & 15;
                    }
                }
            }
            if self.row == 7 {
                self.row = 0;
                if live {
                    self.published = (self.published + 1) & 63;
                }
            } else {
                self.row += 1;
            }
        }
        Ok(Step {
            signals,
            accepted,
            published,
            basic_write,
            light_accepted,
            sample_accepted,
            light_returned,
            sample_returned,
        })
    }
}
