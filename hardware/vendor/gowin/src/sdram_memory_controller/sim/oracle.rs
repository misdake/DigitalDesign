//! Independent byte-data oracle with deliberately synthetic, configurable delays.
//! No SDRAM commands, banks, bridge schedule, refresh or physical performance model.
use super::super::ports::*;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub read_latency: u64,
    pub write_latency: u64,
    pub beat_interval: u64,
    pub max_cycles: u64,
    pub max_requests: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            read_latency: 3,
            write_latency: 5,
            beat_interval: 1,
            max_cycles: 1_000_000,
            max_requests: 4096,
        }
    }
}
struct Job {
    id: u64,
    arrival: u64,
    request: Request,
    beat: usize,
    due: u64,
}
pub struct Memory {
    image: OracleImage,
    config: Config,
    queue: VecDeque<Job>,
    cycle: u64,
    next_id: u64,
    active: bool,
}
impl Memory {
    pub fn new(image: OracleImage, config: Config) -> Result<Self, String> {
        if config.read_latency == 0
            || config.write_latency == 0
            || config.beat_interval == 0
            || config.read_latency > 100_000
            || config.write_latency > 100_000
            || config.beat_interval > 100_000
            || config.max_cycles == 0
            || config.max_cycles > 100_000_000
            || config.max_requests == 0
            || config.max_requests > 65536
        {
            return Err("data oracle bounds".into());
        }
        Ok(Self {
            image,
            config,
            queue: VecDeque::new(),
            cycle: 0,
            next_id: 0,
            active: false,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        self.image.bytes()
    }
}
impl Service for Memory {
    fn submit(&mut self, _client: Client, request: Request) -> Result<u64, String> {
        self.image.offset(request.burst()?)?;
        if self.next_id >= self.config.max_requests as u64 || self.cycle >= self.config.max_cycles {
            return Err("data oracle request/cycle bound".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.queue.push_back(Job {
            id,
            arrival: self.cycle,
            request,
            beat: 0,
            due: 0,
        });
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<Event>, String> {
        if self.cycle >= self.config.max_cycles {
            return Err("data oracle watchdog".into());
        }
        let mut events = Vec::new();
        if let Some(job) = self.queue.front_mut() {
            if !self.active {
                self.active = true;
                job.due = self.cycle
                    + match job.request {
                        Request::Read { .. } => self.config.read_latency,
                        Request::Write { .. } => self.config.write_latency,
                    };
                events.push(Event::Started {
                    id: job.id,
                    cycle: self.cycle,
                    queued_cycles: self.cycle - job.arrival,
                });
            }
            if self.cycle >= job.due {
                let offset = self.image.offset(job.request.burst()?)?;
                let last = match &job.request {
                    Request::Read { bytes, .. } => {
                        let last = job.beat + 1 == bytes / 8;
                        events.push(Event::ReadBeat {
                            id: job.id,
                            cycle: self.cycle,
                            index: job.beat,
                            data: self.image.read(offset + job.beat * 8),
                            last,
                        });
                        last
                    }
                    Request::Write { data, enables, .. } => {
                        self.image.write(
                            offset + job.beat * 8,
                            &data[job.beat..=job.beat],
                            &enables[job.beat..=job.beat],
                        );
                        job.beat + 1 == data.len()
                    }
                };
                job.beat += 1;
                job.due += self.config.beat_interval;
                if last {
                    events.push(Event::Complete {
                        id: job.id,
                        cycle: self.cycle,
                    });
                    self.queue.pop_front();
                    self.active = false;
                }
            }
        }
        self.cycle += 1;
        Ok(events)
    }
    fn cycle(&self) -> u64 {
        self.cycle
    }
    fn idle(&self) -> bool {
        self.queue.is_empty()
    }
}
