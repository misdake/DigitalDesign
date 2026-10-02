//! Fixed-average runtime. Background wait is already in the calibrated profile;
//! only this runtime's own ordered queue is added. No resource ledger is used.
use super::{super::ports::*, calibration};
use std::collections::VecDeque;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub read_first: [u64; 4],
    pub write_complete: [u64; 4],
    /// Four first-beat offsets for a 512 B read group; includes interruption gaps.
    pub read_sector_first: [u64; 4],
    pub recovery: u64,
}
impl Profile {
    pub fn from_calibration(report: &calibration::Report) -> Result<Self, String> {
        if report.classes.iter().any(|s| s.samples == 0) {
            return Err("profile requires all eight request classes".into());
        }
        let profile = Self {
            read_first: std::array::from_fn(|i| {
                report.classes[i]
                    .sum_first
                    .div_ceil(report.classes[i].samples)
            }),
            write_complete: std::array::from_fn(|i| {
                report.classes[i + 4]
                    .sum_complete
                    .div_ceil(report.classes[i + 4].samples)
            }),
            read_sector_first: std::array::from_fn(|i| {
                report.classes[3].sector_first[i].div_ceil(report.classes[3].samples)
            }),
            recovery: report.config.recovery,
        };
        profile.validate()?;
        Ok(profile)
    }
    pub fn gpu_default() -> Result<Self, String> {
        Self::from_calibration(&calibration::analyze(
            &calibration::representative_trace(128)?,
            Default::default(),
        )?)
    }
    /// Calibrate early admission with actual cycle handshakes and the requested
    /// deterministic background load, then retain only stable average offsets.
    pub fn gpu_early_grant(load: super::traffic::Load) -> Result<Self, String> {
        Self::gpu_cycle_profile(load, false)
    }
    pub fn gpu_chained_groups(load: super::traffic::Load) -> Result<Self, String> {
        Self::gpu_cycle_profile(load, true)
    }
    fn gpu_cycle_profile(load: super::traffic::Load, chained_groups: bool) -> Result<Self, String> {
        let config = super::cycle_calibration::Config {
            service: crate::sdram_memory_controller::emu::service::Config {
                early_grant: true,
                chained_groups,
                ..Default::default()
            },
            load,
            ..Default::default()
        };
        super::cycle_calibration::analyze(&calibration::representative_trace(128)?, config)?
            .profile()
    }
    pub fn validate(self) -> Result<(), String> {
        if self
            .read_first
            .into_iter()
            .chain(self.write_complete)
            .chain(self.read_sector_first)
            .chain([self.recovery])
            .any(|v| v == 0 || v > 100_000_000)
            || self.read_first[3] != self.read_sector_first[0]
            || self.read_sector_first.windows(2).any(|v| v[1] < v[0] + 16)
        {
            return Err("SDRAM average timing bounds/sector order".into());
        }
        Ok(())
    }
    pub fn read_due(self, burst: Burst, beat: usize) -> Result<u64, String> {
        let class = burst.class()?;
        if class >= 4 || beat >= burst.bytes / 8 {
            return Err("SDRAM read beat range".into());
        }
        Ok(if burst.bytes == 512 {
            self.read_sector_first[beat / 16] + (beat % 16) as u64
        } else {
            self.read_first[class] + beat as u64
        })
    }
    pub fn completion(self, burst: Burst) -> Result<u64, String> {
        self.validate()?;
        let class = burst.class()?;
        if class < 4 {
            self.read_due(burst, burst.bytes / 8 - 1)
        } else {
            Ok(self.write_complete[class - 4])
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_cycles: u64,
    pub max_requests: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_cycles: 1_000_000,
            max_requests: 65536,
        }
    }
}
impl Limits {
    pub fn validate(self) -> Result<(), String> {
        if self.max_cycles == 0
            || self.max_cycles > 100_000_000
            || self.max_requests == 0
            || self.max_requests > 65536
        {
            return Err("SDRAM service limits".into());
        }
        Ok(())
    }
}
struct Pending {
    id: u64,
    submitted: u64,
    request: Request,
}
struct Active {
    pending: Pending,
    started: u64,
    next_beat: usize,
}
pub struct Memory {
    image: OracleImage,
    profile: Profile,
    limits: Limits,
    cycle: u64,
    next_id: u64,
    available: u64,
    pending: VecDeque<Pending>,
    active: Option<Active>,
}
impl Memory {
    pub fn new(image: OracleImage, profile: Profile, limits: Limits) -> Result<Self, String> {
        profile.validate()?;
        limits.validate()?;
        Ok(Self {
            image,
            profile,
            limits,
            cycle: 0,
            next_id: 0,
            available: 0,
            pending: VecDeque::new(),
            active: None,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        self.image.bytes()
    }
}
impl Service for Memory {
    fn cycle(&self) -> u64 {
        self.cycle
    }
    fn idle(&self) -> bool {
        self.active.is_none() && self.pending.is_empty()
    }
    fn submit(&mut self, _client: Client, request: Request) -> Result<u64, String> {
        self.image.offset(request.burst()?)?;
        if self.next_id >= self.limits.max_requests as u64 || self.cycle >= self.limits.max_cycles {
            return Err("SDRAM request/cycle bound".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.pending.push_back(Pending {
            id,
            submitted: self.cycle,
            request,
        });
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<Event>, String> {
        let cycle = self
            .cycle
            .checked_add(1)
            .filter(|&v| v <= self.limits.max_cycles)
            .ok_or("SDRAM cycle watchdog")?;
        if self.active.is_none() && cycle >= self.available {
            if let Some(front) = self.pending.front() {
                if cycle
                    .checked_add(self.profile.completion(front.request.burst()?)?)
                    .is_none_or(|v| v > self.limits.max_cycles)
                {
                    return Err("SDRAM completion beyond watchdog".into());
                }
            }
        }
        self.cycle = cycle;
        let mut events = vec![];
        if self.active.is_none() && cycle >= self.available {
            if let Some(pending) = self.pending.pop_front() {
                events.push(Event::Started {
                    id: pending.id,
                    cycle,
                    queued_cycles: cycle - pending.submitted,
                });
                self.active = Some(Active {
                    pending,
                    started: cycle,
                    next_beat: 0,
                });
            }
        }
        if let Some(active) = &mut self.active {
            let burst = active.pending.request.burst()?;
            let offset = self.image.offset(burst)?;
            let elapsed = cycle - active.started;
            if burst.access == Access::Read
                && active.next_beat < burst.bytes / 8
                && elapsed == self.profile.read_due(burst, active.next_beat)?
            {
                let index = active.next_beat;
                events.push(Event::ReadBeat {
                    id: active.pending.id,
                    cycle,
                    index,
                    data: self.image.read(offset + index * 8),
                    last: index + 1 == burst.bytes / 8,
                });
                active.next_beat += 1;
            }
            if elapsed == self.profile.completion(burst)? {
                if let Request::Write { data, enables, .. } = &active.pending.request {
                    self.image.write(offset, data, enables);
                }
                events.push(Event::Complete {
                    id: active.pending.id,
                    cycle,
                });
                self.available = cycle + self.profile.recovery;
                self.active = None;
            }
        }
        Ok(events)
    }
}
