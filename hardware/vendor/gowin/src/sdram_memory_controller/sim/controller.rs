//! Event-level 2229 controller timing, including four-sector lookahead groups.
//! Derived from the controller RTL and its independently checked diagram offsets.
//! No pin/DQ simulator or FPGA resource accounting is provided here.
use super::{super::ports::*, calibration::bank_row};

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub request_transport: u64,
    pub read_transport: u64,
    pub write_ack_transport: u64,
    pub refresh_interval: u64,
    pub continuation_age: u64,
    pub max_core_cycles: u64,
}
impl Default for Config {
    fn default() -> Self {
        // 108-MHz core cycles. Read/ack transports are coarse 54/108 bridge
        // estimates, chosen to retain the old calibrated 32-B CPU-port baseline.
        Self {
            request_transport: 4,
            read_transport: 2,
            write_ack_transport: 4,
            refresh_interval: 1100,
            continuation_age: 1000,
            max_core_cycles: 200_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Hit,
    Closed,
    Conflict,
}
#[derive(Clone, Debug)]
pub struct Sector {
    pub address: u64,
    pub bank: usize,
    pub row: u64,
    pub row_state: RowState,
    pub accept_core: u64,
    pub launch_core: u64,
    pub first_system: u64,
    pub last_system: u64,
    pub chained: bool,
    pub bank_changed: bool,
    pub hidden_prepare_core: u64,
}
#[derive(Clone, Debug)]
pub struct Run {
    pub sectors: Vec<Sector>,
    pub done_core: u64,
    pub done_system: u64,
    pub refresh_wait_core: u64,
    pub refresh_break: bool,
    pub priority_break: bool,
    pub bank_break: bool,
}
pub struct Controller {
    pub config: Config,
    rows: [Option<u64>; 4],
    available: u64,
    refreshed: u64,
    previous_bank: Option<usize>,
}
impl Controller {
    pub fn new(config: Config) -> Result<Self, String> {
        if config.request_transport > 64
            || config.read_transport > 64
            || config.write_ack_transport > 64
            || config.refresh_interval < 32
            || config.refresh_interval > 1_000_000
            || config.continuation_age == 0
            || config.continuation_age > config.refresh_interval
            || config.max_core_cycles == 0
            || config.max_core_cycles > 200_000_000
        {
            return Err("controller timing bounds".into());
        }
        Ok(Self {
            config,
            rows: [None; 4],
            available: 0,
            refreshed: 0,
            previous_bank: None,
        })
    }
    fn row_state(&self, address: u64) -> RowState {
        let (bank, row) = bank_row(address);
        match self.rows[bank] {
            Some(r) if r == row => RowState::Hit,
            None => RowState::Closed,
            Some(_) => RowState::Conflict,
        }
    }
    fn admit(&mut self, earliest: u64) -> (u64, u64) {
        let initial = earliest.max(self.available);
        let due = self.refreshed + self.config.refresh_interval;
        if initial < due {
            return (initial, 0);
        }
        // Idle refresh continues autonomously; under load it waits for completion.
        let refresh_start = if self.available > due {
            self.available
        } else {
            due + (initial - due) / self.config.refresh_interval * self.config.refresh_interval
        };
        let ref_edge = refresh_start
            + if self.rows.iter().any(Option::is_some) {
                2
            } else {
                1
            };
        self.rows = [None; 4];
        self.refreshed = ref_edge;
        let accepted = initial.max(ref_edge + 9);
        (accepted, accepted - initial)
    }
    /// `continue_at` receives a 108-MHz command-commit edge. A false result
    /// withdraws lookahead before that edge; an issued READ is never cancelled.
    pub fn serve<F>(
        &mut self,
        sectors: &[Burst],
        grant_system: u64,
        chain: bool,
        mut continue_at: F,
    ) -> Result<Run, String>
    where
        F: FnMut(u64) -> bool,
    {
        if sectors.is_empty() || sectors.len() > 4 {
            return Err("controller group length".into());
        }
        for sector in sectors {
            sector.class()?;
            if sector.bytes > 128
                || sector.bytes != sectors[0].bytes
                || sector.access != sectors[0].access
            {
                return Err(
                    "controller group sectors must have equal native length/direction".into(),
                );
            }
        }
        let earliest = grant_system
            .checked_mul(2)
            .and_then(|v| v.checked_add(self.config.request_transport))
            .filter(|&v| v <= self.config.max_core_cycles)
            .ok_or("controller cycle bound")?;
        let (accept, refresh_wait_core) = self.admit(earliest);
        let access = sectors[0].access;
        let words = sectors[0].bytes as u64 / 4;
        let initial_state = self.row_state(sectors[0].address);
        // REQUEST_PIPELINE: handshake->column launch hit2/closed3/conflict5.
        let mut launch = accept
            + match initial_state {
                RowState::Hit => 2,
                RowState::Closed => 3,
                RowState::Conflict => 5,
            };
        // Coarse bridge phase: WRITE launch waits for the next system feed slot.
        if access == Access::Write {
            launch = launch.div_ceil(2) * 2;
        }
        let mut accepted = Vec::new();
        let mut priority_break = false;
        let mut refresh_break = false;
        let mut bank_break = false;
        let mut row_state = initial_state;
        let mut hidden_prepare = 0;
        for (index, sector) in sectors.iter().enumerate() {
            let (bank, row) = bank_row(sector.address);
            let bank_changed = self.previous_bank.is_some_and(|b| b != bank);
            self.rows[bank] = Some(row);
            self.previous_bank = Some(bank);
            let (first, last) = if access == Access::Read {
                // Native D0 consumed launch+4; first 64-bit beat consumes D1.
                (
                    (launch + 5 + self.config.read_transport).div_ceil(2),
                    (launch + words + 3 + self.config.read_transport).div_ceil(2),
                )
            } else {
                let done = (launch + words + 1 + self.config.write_ack_transport).div_ceil(2);
                (done, done)
            };
            accepted.push(Sector {
                address: sector.address,
                bank,
                row,
                row_state,
                accept_core: accept,
                launch_core: launch,
                first_system: first,
                last_system: last,
                chained: index != 0,
                bank_changed,
                hidden_prepare_core: hidden_prepare,
            });
            if index + 1 == sectors.len() || !chain {
                break;
            }
            let next = sectors[index + 1];
            let (next_bank, next_row) = bank_row(next.address);
            row_state = self.row_state(next.address);
            let eligible =
                access == Access::Read && words == 32 || access == Access::Write && words >= 8;
            if !eligible {
                break;
            }
            // Reads commit at launch+32; writes accept at launch+31 and then
            // launch next at+32. Preparation begins at read+10 / write+2.
            let boundary = launch + words - u64::from(access == Access::Write);
            if boundary.saturating_sub(self.refreshed) >= self.config.continuation_age {
                refresh_break = true;
                break;
            }
            let preparation = launch + if access == Access::Read { 10 } else { 2 };
            let mut ready = launch;
            hidden_prepare = 0;
            if row_state != RowState::Hit && bank != next_bank {
                // Other-bank PRE/ACT overlaps current DQ. Speculative rows may
                // remain open even when priority cancels the later command.
                if continue_at(preparation - 1) {
                    hidden_prepare = match row_state {
                        RowState::Closed => 2,
                        RowState::Conflict => 4,
                        RowState::Hit => 0,
                    };
                    ready = preparation + hidden_prepare;
                    self.rows[next_bank] = Some(next_row);
                } else {
                    ready = u64::MAX;
                }
            } else if row_state != RowState::Hit {
                ready = u64::MAX;
            }
            if !continue_at(boundary) {
                priority_break = true;
                break;
            }
            if ready > boundary {
                bank_break = true;
                break;
            }
            launch += words;
        }
        let done_core = launch + words + if access == Access::Read { 3 } else { 1 };
        self.available = done_core + 1;
        if self.available > self.config.max_core_cycles {
            return Err("controller completion watchdog".into());
        }
        let done_system = accepted.last().unwrap().last_system;
        Ok(Run {
            sectors: accepted,
            done_core,
            done_system,
            refresh_wait_core,
            refresh_break,
            priority_break,
            bank_break,
        })
    }
}
