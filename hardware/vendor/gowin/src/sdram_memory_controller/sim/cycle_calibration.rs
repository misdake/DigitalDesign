//! Measure real cycle handshakes once, then reuse their means in the functional
//! average oracle. Background traffic uses real clients and isolated pin memory.
use super::{average::Profile, calibration::Sample, traffic::Load};
use crate::sdram_memory_controller::{emu::service, ports::*};

#[derive(Clone, Debug)]
pub struct Config {
    pub service: service::Config,
    pub load: Load,
    pub max_samples: usize,
    /// Idle recovery between foreground jobs; never charged as background wait.
    pub recovery: u64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            service: service::Config::default(),
            load: Load::solo(),
            max_samples: 65536,
            recovery: 1,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub samples: u64,
    pub sum_first: u64,
    pub sum_complete: u64,
    pub sum_grant_wait: u64,
    pub sum_self_queue: u64,
    pub sector_first: [u64; 4],
    pub completions: Vec<u64>,
}
impl Stats {
    pub fn mean_first(&self) -> f64 {
        self.sum_first as f64 / self.samples.max(1) as f64
    }
    pub fn mean_complete(&self) -> f64 {
        self.sum_complete as f64 / self.samples.max(1) as f64
    }
    pub fn p95(&self) -> u64 {
        let mut values = self.completions.clone();
        values.sort_unstable();
        if values.is_empty() {
            0
        } else {
            values[(values.len() * 95).div_ceil(100) - 1]
        }
    }
}
#[derive(Clone, Debug)]
pub struct Report {
    pub config: Config,
    pub classes: [Stats; 8],
    pub background_submitted: u64,
    pub background_completed: u64,
    pub elapsed: u64,
}
impl Report {
    pub fn profile(&self) -> Result<Profile, String> {
        if self.classes.iter().any(|s| s.samples == 0) {
            return Err("cycle profile requires all eight classes".into());
        }
        let profile = Profile {
            read_first: std::array::from_fn(|i| {
                self.classes[i].sum_first.div_ceil(self.classes[i].samples)
            }),
            write_complete: std::array::from_fn(|i| {
                self.classes[i + 4]
                    .sum_complete
                    .div_ceil(self.classes[i + 4].samples)
            }),
            read_sector_first: std::array::from_fn(|i| {
                self.classes[3].sector_first[i].div_ceil(self.classes[3].samples)
            }),
            recovery: self.config.recovery,
        };
        profile.validate()?;
        Ok(profile)
    }
}
struct Foreground {
    id: u64,
    index: usize,
    eligible: u64,
    first: Option<u64>,
    firsts: [u64; 4],
}
fn request(burst: Burst) -> Request {
    if burst.access == Access::Read {
        Request::Read {
            address: burst.address,
            bytes: burst.bytes,
        }
    } else {
        Request::Write {
            address: burst.address,
            data: vec![OracleWord::constant::<0>(); burst.bytes / 8],
            enables: vec![255; burst.bytes / 8],
        }
    }
}
/// One foreground job at a time. Its own backlog is reported separately and
/// excluded from the profile, which already contains background wait/refresh.
/// Initialization is excluded; idle refresh and row state continue afterwards.
pub fn analyze(trace: &[Sample], config: Config) -> Result<Report, String> {
    config.load.validate()?;
    if trace.is_empty()
        || trace.len() > config.max_samples
        || config.max_samples > 65536
        || config.recovery == 0
        || config.recovery > 1000
        || trace.windows(2).any(|s| s[0].arrival > s[1].arrival)
    {
        return Err("cycle calibration bounds/order".into());
    }
    let mut image_end = 0;
    for sample in trace {
        sample.burst.class()?;
        if sample.arrival >= config.service.max_cycles {
            return Err("cycle sample arrival bound".into());
        }
        image_end = image_end.max(sample.burst.address + sample.burst.bytes as u64);
        for stream in &config.load.streams {
            if sample.burst.address < stream.base + stream.span
                && stream.base < sample.burst.address + sample.burst.bytes as u64
            {
                return Err("cycle background must use an isolated address region".into());
            }
        }
    }
    for stream in &config.load.streams {
        image_end = image_end.max(stream.base + stream.span);
    }
    let mut memory = service::Memory::new(
        OracleImage::filled::<0>(0, image_end as usize)?,
        config.service.clone(),
    )?;
    // No stimulus until initialization completes; its cycles are not profile cost.
    while !memory.combination.bridge.output(false).initialized {
        memory.step()?;
    }
    let origin = memory.cycle();
    let mut report = Report {
        config: config.clone(),
        classes: std::array::from_fn(|_| Stats::default()),
        background_submitted: 0,
        background_completed: 0,
        elapsed: 0,
    };
    let mut streams = vec![0; config.load.streams.len()];
    let mut next = 0;
    let mut foreground: Option<Foreground> = None;
    let mut available = 0;
    loop {
        let now = memory.cycle() - origin;
        for (stream, index) in config.load.streams.iter().zip(&mut streams) {
            while stream.arrival(*index).is_some_and(|t| t <= now) {
                memory.submit(stream.client, request(stream.burst(*index)))?;
                report.background_submitted += 1;
                *index += 1;
            }
        }
        if foreground.is_none()
            && next < trace.len()
            && trace[next].arrival <= now
            && available <= now
        {
            let id = memory.submit(trace[next].client, request(trace[next].burst))?;
            foreground = Some(Foreground {
                id,
                index: next,
                eligible: now,
                first: None,
                firsts: [0; 4],
            });
            next += 1;
        }
        for event in memory.step()? {
            let Some(Foreground {
                id,
                index,
                eligible,
                first,
                firsts,
            }) = foreground.as_mut()
            else {
                if matches!(event, Event::Complete { .. }) {
                    report.background_completed += 1;
                }
                continue;
            };
            let s = &mut report.classes[trace[*index].burst.class()?];
            match event {
                Event::Started { id: who, cycle, .. } if who == *id => {
                    s.sum_grant_wait += cycle - origin - *eligible
                }
                Event::ReadBeat {
                    id: who,
                    cycle,
                    index: beat,
                    ..
                } if who == *id => {
                    let delay = cycle - origin - *eligible;
                    if first.is_none() {
                        *first = Some(delay);
                    }
                    if trace[*index].burst.bytes == 512 && beat % 16 == 0 {
                        firsts[beat / 16] = delay;
                    }
                }
                Event::Complete { id: who, cycle } if who == *id => {
                    let delay = cycle - origin - *eligible;
                    s.samples += 1;
                    s.sum_first += first.unwrap_or(delay);
                    s.sum_complete += delay;
                    s.sum_self_queue += *eligible - trace[*index].arrival;
                    s.completions.push(delay);
                    for (sum, value) in s.sector_first.iter_mut().zip(*firsts) {
                        *sum += value;
                    }
                    available = cycle - origin + config.recovery;
                    foreground = None;
                }
                Event::Complete { .. } => report.background_completed += 1,
                _ => {}
            }
        }
        if next == trace.len() && foreground.is_none() {
            report.elapsed = memory.cycle() - origin;
            return Ok(report);
        }
    }
}
