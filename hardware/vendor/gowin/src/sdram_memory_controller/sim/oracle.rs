//! Functional oracle with configured background traffic and event-level timing.
//! This is not the independent cycle emu or a pin-level controller model.
use super::{
    super::ports::*,
    average::Limits,
    controller::{self, Run},
    traffic::{ChainPolicy, Load},
};
use std::collections::VecDeque;
#[derive(Clone, Debug)]
pub struct Config {
    pub controller: controller::Config,
    pub load: Load,
    pub chain: ChainPolicy,
    pub limits: Limits,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            controller: Default::default(),
            load: Load::solo(),
            chain: ChainPolicy::ExistingUnchained,
            limits: Default::default(),
        }
    }
}
struct Job {
    id: Option<u64>,
    client: Client,
    submitted: u64,
    burst: Burst,
    request: Option<Request>,
    next_sector: usize,
}
struct Active {
    job: Job,
    run: Run,
    first_sector: usize,
    beat: usize,
}
pub struct Memory {
    image: OracleImage,
    config: Config,
    controller: controller::Controller,
    cycle: u64,
    next_id: u64,
    background_count: usize,
    available: u64,
    cursor: usize,
    streams: Vec<u64>,
    queues: [VecDeque<Job>; 7],
    active: Option<Active>,
    pub runs: Vec<Run>,
}
impl Memory {
    pub fn new(image: OracleImage, config: Config) -> Result<Self, String> {
        config.limits.validate()?;
        config.load.validate()?;
        let image_end = image.base() + image.bytes().len() as u64;
        for stream in &config.load.streams {
            if stream.base < image_end && image.base() < stream.base + stream.span {
                return Err("background traffic must use an isolated address region".into());
            }
        }
        let streams = vec![0; config.load.streams.len()];
        let controller = controller::Controller::new(config.controller)?;
        Ok(Self {
            image,
            config,
            controller,
            cycle: 0,
            next_id: 0,
            background_count: 0,
            available: 0,
            cursor: 1,
            streams,
            queues: std::array::from_fn(|_| VecDeque::new()),
            active: None,
            runs: vec![],
        })
    }
    pub fn bytes(&self) -> &[u8] {
        self.image.bytes()
    }
    pub fn background_requests(&self) -> usize {
        self.background_count
    }
}
impl Service for Memory {
    fn cycle(&self) -> u64 {
        self.cycle
    }
    /// Experiment idle means all foreground work completed. Background is periodic.
    fn idle(&self) -> bool {
        self.active.as_ref().is_none_or(|a| a.job.id.is_none())
            && self.queues.iter().all(|q| q.iter().all(|j| j.id.is_none()))
    }
    fn submit(&mut self, client: Client, request: Request) -> Result<u64, String> {
        let burst = request.burst()?;
        self.image.offset(burst)?;
        if self.next_id as usize + self.background_count >= self.config.limits.max_requests
            || self.cycle >= self.config.limits.max_cycles
        {
            return Err("SDRAM oracle request/cycle bound".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.queues[client.index()].push_back(Job {
            id: Some(id),
            client,
            submitted: self.cycle,
            burst,
            request: Some(request),
            next_sector: 0,
        });
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<Event>, String> {
        if self.cycle >= self.config.limits.max_cycles {
            return Err("SDRAM oracle cycle watchdog".into());
        }
        self.cycle += 1;
        let cycle = self.cycle;
        let mut events = vec![];
        for (stream, index) in self.config.load.streams.iter().zip(&mut self.streams) {
            while stream
                .arrival(*index)
                .is_some_and(|arrival| arrival <= cycle)
            {
                if self.next_id as usize + self.background_count >= self.config.limits.max_requests
                {
                    return Err("SDRAM oracle background queue bound".into());
                }
                self.queues[stream.client.index()].push_back(Job {
                    id: None,
                    client: stream.client,
                    submitted: stream.arrival(*index).unwrap(),
                    burst: stream.burst(*index),
                    request: None,
                    next_sector: 0,
                });
                *index += 1;
                self.background_count += 1;
            }
        }
        if self.active.is_none() && cycle >= self.available {
            let chosen = if !self.queues[0].is_empty() {
                Some(0)
            } else {
                (0..6)
                    .map(|n| 1 + (self.cursor - 1 + n) % 6)
                    .find(|&c| !self.queues[c].is_empty())
            };
            if let Some(client) = chosen {
                let job = self.queues[client].pop_front().unwrap();
                if client != 0 {
                    self.cursor = 1 + client % 6;
                }
                let next_display = self.queues[0]
                    .front()
                    .map(|j| j.submitted)
                    .into_iter()
                    .chain(
                        self.config
                            .load
                            .streams
                            .iter()
                            .zip(&self.streams)
                            .filter(|(s, _)| s.client == Client::Display)
                            .filter_map(|(s, &i)| s.arrival(i)),
                    )
                    .min();
                let sectors = job.burst.sectors()?;
                let run = self.controller.serve(
                    &sectors[job.next_sector..],
                    cycle,
                    self.config.chain == ChainPolicy::ChainedCandidate,
                    |edge| client == 0 || next_display.is_none_or(|arrival| arrival * 2 > edge),
                )?;
                if run.done_system > self.config.limits.max_cycles {
                    return Err("SDRAM oracle completion watchdog".into());
                }
                if job.next_sector == 0 {
                    if let Some(id) = job.id {
                        events.push(Event::Started {
                            id,
                            cycle,
                            queued_cycles: cycle - job.submitted,
                        });
                    }
                }
                let first_sector = job.next_sector;
                self.runs.push(run.clone());
                self.active = Some(Active {
                    job,
                    run,
                    first_sector,
                    beat: 0,
                });
            }
        }
        if let Some(active) = &mut self.active {
            let sector_bytes = if active.job.burst.bytes == 512 {
                128
            } else {
                active.job.burst.bytes
            };
            let per_sector = sector_bytes / 8;
            if active.job.burst.access == Access::Read
                && active.beat < active.run.sectors.len() * per_sector
            {
                let sector = active.beat / per_sector;
                let within = active.beat % per_sector;
                if cycle == active.run.sectors[sector].first_system + within as u64 {
                    if let Some(id) = active.job.id {
                        let index = active.first_sector * per_sector + active.beat;
                        let offset = self.image.offset(active.job.burst)?;
                        events.push(Event::ReadBeat {
                            id,
                            cycle,
                            index,
                            data: self.image.read(offset + index * 8),
                            last: index + 1 == active.job.burst.bytes / 8,
                        });
                    }
                    active.beat += 1;
                }
            }
            if cycle == active.run.done_system {
                if let Some(Request::Write { data, enables, .. }) = &active.job.request {
                    let first = active.first_sector * per_sector;
                    let end = first + active.run.sectors.len() * per_sector;
                    let offset = self.image.offset(active.job.burst)?;
                    self.image
                        .write(offset + first * 8, &data[first..end], &enables[first..end]);
                }
                let mut active = self.active.take().unwrap();
                active.job.next_sector += active.run.sectors.len();
                self.available = cycle + 1;
                if active.job.next_sector == active.job.burst.sectors()?.len() {
                    if let Some(id) = active.job.id {
                        events.push(Event::Complete { id, cycle });
                    }
                } else {
                    self.queues[active.job.client.index()].push_front(active.job);
                }
            }
        }
        Ok(events)
    }
}
