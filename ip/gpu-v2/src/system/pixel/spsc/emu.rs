//! Independent old-state synchronous-memory emulator; no RTL or golden calls.

use super::{Config, ReadTiming, Signals, Step, Tick, Word};
use std::collections::VecDeque;

pub struct Queue {
    config: Config,
    ram: Vec<u64>,
    insert: usize,
    consume: usize,
    issue: usize,
    write_row: usize,
    issue_row: usize,
    pending: Option<Word>,
    heads: VecDeque<Word>,
    wall: u64,
}

impl Queue {
    pub fn new(config: Config) -> Result<Self, String> {
        config.validate()?;
        Ok(Self {
            config,
            // Deliberately nonzero stale payload; reset does not clear it.
            ram: (0..config.entries * config.rows)
                .map(|i| (0xbad0_0000_1234_5678 ^ i as u64) & config.mask())
                .collect(),
            insert: 0,
            consume: 0,
            issue: 0,
            write_row: 0,
            issue_row: 0,
            pending: None,
            heads: VecDeque::with_capacity(2),
            wall: 0,
        })
    }

    fn distance(&self, tail: usize, head: usize) -> usize {
        tail.wrapping_sub(head) & (2 * self.config.entries - 1)
    }

    fn advance(&self, pointer: usize) -> usize {
        (pointer + 1) & (2 * self.config.entries - 1)
    }

    pub fn occupied_entries(&self) -> usize {
        self.distance(self.insert, self.consume) + usize::from(self.write_row != 0)
    }

    pub fn idle(&self) -> bool {
        self.insert == self.consume
            && self.write_row == 0
            && self.pending.is_none()
            && self.heads.is_empty()
    }

    pub fn signals(&self, ce: bool) -> Signals {
        Signals {
            input_ready: ce
                && (self.write_row != 0
                    || self.distance(self.insert, self.consume) < self.config.entries),
            output: self.heads.front().copied(),
        }
    }

    pub fn tick(&mut self, tick: Tick) -> Result<Step, String> {
        if self.wall >= self.config.max_wall {
            return Err("SPSC wall watchdog".into());
        }
        self.wall += 1;
        let signals = if tick.reset {
            Signals {
                input_ready: false,
                output: None,
            }
        } else {
            self.signals(tick.ce)
        };
        let accepted = signals.input_ready && tick.input.is_some();
        if accepted && tick.input.unwrap() & !self.config.mask() != 0 {
            return Err("SPSC input exceeds row width".into());
        }
        let consumed = tick.ce && tick.output_ready && signals.output.is_some();
        let published = accepted && self.write_row + 1 == self.config.rows;
        let pending_return =
            !tick.reset && tick.ce && self.heads.len() < 2 && self.pending.is_some();
        // No use of a head position released by this edge's consume.
        let read_address = (!tick.reset
            && tick.ce
            && self.heads.len() < 2
            && (self.issue != self.insert
                || (published && self.config.rows > 1 && self.issue_row < self.write_row)))
            .then_some((self.issue % self.config.entries) * self.config.rows + self.issue_row);
        let write_address = accepted
            .then_some((self.insert % self.config.entries) * self.config.rows + self.write_row);
        let returned = pending_return
            || (self.config.read_timing == ReadTiming::Capture && read_address.is_some());
        if tick.reset {
            self.insert = 0;
            self.consume = 0;
            self.issue = 0;
            self.write_row = 0;
            self.issue_row = 0;
            self.pending = None;
            self.heads.clear();
        } else if tick.ce {
            let next_pending = read_address.map(|address| Word {
                data: self.ram[address],
                row: self.issue_row,
                last: self.issue_row + 1 == self.config.rows,
            });
            if consumed {
                let word = self.heads.pop_front().unwrap();
                if word.last {
                    self.consume = self.advance(self.consume);
                }
            }
            if pending_return {
                self.heads.push_back(self.pending.take().unwrap());
            }
            if let Some(word) = next_pending {
                match self.config.read_timing {
                    ReadTiming::Capture => self.heads.push_back(word),
                    ReadTiming::Registered => self.pending = Some(word),
                }
                self.issue_row += 1;
                if self.issue_row == self.config.rows {
                    self.issue_row = 0;
                    self.issue = self.advance(self.issue);
                }
            }
            if let Some(address) = write_address {
                if read_address == Some(address) {
                    return Err("SPSC RAM read/write collision".into());
                }
                self.ram[address] = tick.input.unwrap();
                self.write_row += 1;
                if published {
                    self.write_row = 0;
                    self.insert = self.advance(self.insert);
                }
            }
        }
        if self.heads.len() > 2 || self.occupied_entries() > self.config.entries {
            return Err("SPSC finite storage overflow".into());
        }
        Ok(Step {
            signals,
            accepted,
            published,
            consumed,
            write_address,
            read_address,
            returned,
            occupied_entries: self.occupied_entries(),
        })
    }
}
