//! Bounded host facade over actual cycle handshakes, not an event schedule.
use super::{idle_inputs, Combination};
use crate::sdram_memory_controller::{arbiter::*, ports::*};
use std::collections::VecDeque;
#[derive(Clone, Debug)]
pub struct Config {
    pub init_cycles: u32,
    pub early_grant: bool,
    /// Hold ownership for one aligned GPU 512-byte group. Defaults off.
    pub chained_groups: bool,
    pub max_cycles: u64,
    pub max_requests: usize,
    pub max_queued: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            init_cycles: 21600,
            early_grant: false,
            chained_groups: false,
            max_cycles: 1_000_000,
            max_requests: 4096,
            max_queued: 256,
        }
    }
}
struct Job {
    grouped: bool,
    id: u64,
    client: Client,
    request: Request,
    arrival: u64,
    offset: usize,
    accepted: bool,
    next_accepted: bool,
    fed: usize,
    read: usize,
    started: bool,
    scalar: u64,
}
pub struct Memory {
    pub combination: Combination,
    config: Config,
    queues: [VecDeque<Job>; 7],
    next_id: u64,
    pending: usize,
}
impl Memory {
    pub fn new(image: OracleImage, config: Config) -> Result<Self, String> {
        if config.max_cycles == 0 || config.max_requests == 0 || config.max_queued == 0 {
            return Err("cycle service bounds".into());
        }
        Ok(Self {
            combination: Combination::with_options(
                image,
                config.init_cycles,
                config.early_grant,
                config.chained_groups,
            )?,
            config,
            queues: std::array::from_fn(|_| VecDeque::new()),
            next_id: 0,
            pending: 0,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        self.combination.bridge.pins.bytes()
    }
    /// Abort all queued/active host IDs and protocol state. The physical image
    /// survives. Already clocked writes survive, unclocked bytes do not commit.
    pub fn reset(&mut self) -> Result<(), String> {
        self.queues = std::array::from_fn(|_| VecDeque::new());
        self.pending = 0;
        let mut input = idle_inputs();
        input.reset = true;
        self.combination.tick(&input)
    }
    fn sector(job: &Job) -> usize {
        if job.grouped {
            512
        } else if job.client == Client::Dma {
            2
        } else if matches!(
            job.client,
            Client::Display | Client::Instruction | Client::Data
        ) {
            32
        } else {
            job.request.burst().unwrap().bytes.min(128)
        }
    }
    fn drive(job: &Job, early_grant: bool, input: &mut CpuV3MemoryArbiterInputValue) {
        let burst = job.request.burst().unwrap();
        let bytes = Self::sector(job);
        // Descriptor lookahead never advances the active write payload/read
        // index. Only completion promotes the accepted successor segment.
        let successor =
            early_grant && job.accepted && !job.next_accepted && job.offset + bytes < burst.bytes;
        let descriptor_offset = job.offset + if successor { bytes } else { 0 };
        let address = (burst.address + descriptor_offset as u64) / 2;
        let writing = burst.access == Access::Write;
        let data = if let Request::Write { data, .. } = &job.request {
            if bytes == 2 {
                data[job.offset / 8].bits() >> (job.offset % 8 * 8) & 65535
            } else {
                data.get(job.offset / 8 + job.fed).map_or(0, |v| v.bits())
            }
        } else {
            0
        };
        let valid = !job.accepted || successor;
        let length = if job.grouped {
            2
        } else {
            (bytes as u64 / 32).saturating_sub(1)
        };
        match job.client {
            Client::Display => {
                input.display_request_valid = valid;
                input.display_address = address;
            }
            Client::Instruction => {
                input.instruction_request_valid = valid;
                input.instruction_address = address;
            }
            Client::Data => {
                input.data_request_valid = valid;
                input.data_address = address;
                input.data_line = true;
                input.data_write = writing;
                input.data_write_data = data;
            }
            Client::Dma => {
                input.dma_request_valid = valid;
                input.dma_address = address;
                input.dma_write = writing;
                input.dma_write_data = data;
            }
            Client::GpuReadOnly => {
                input.gpu_ro_request_valid = valid;
                input.gpu_ro_address = address;
                input.gpu_ro_write = writing;
                input.gpu_ro_line_count_minus_1 = length;
                input.gpu_ro_write_data = data;
            }
            Client::FramebufferRead => {
                input.gpu_fb_r_request_valid = valid;
                input.gpu_fb_r_address = address;
                input.gpu_fb_r_write = writing;
                input.gpu_fb_r_line_count_minus_1 = length;
                input.gpu_fb_r_write_data = data;
            }
            Client::FramebufferWrite => {
                input.gpu_fb_w_request_valid = valid;
                input.gpu_fb_w_address = address;
                input.gpu_fb_w_write = writing;
                input.gpu_fb_w_line_count_minus_1 = length;
                input.gpu_fb_w_write_data = data;
            }
        }
    }
}
impl Service for Memory {
    fn submit(&mut self, client: Client, request: Request) -> Result<u64, String> {
        let b = request.burst()?;
        let grouped = self.config.chained_groups && b.bytes == 512 && client.index() >= 4;
        if grouped && b.address & 511 != 0 {
            return Err("chained group requires 512-byte alignment".into());
        }
        if !self.combination.bridge.pins.contains(b.address, b.bytes) {
            return Err("cycle request outside image".into());
        }
        if let Request::Write { enables, .. } = &request {
            if matches!(client, Client::Display | Client::Instruction) {
                return Err("read-only hardware client".into());
            }
            if enables.iter().any(|&v| v != 255) {
                return Err("cycle facade requires full burst enables; use scalar low-level ports for halfword masks".into());
            }
        }
        if self.pending >= self.config.max_queued || self.next_id >= self.config.max_requests as u64
        {
            return Err("cycle service queue/request bound".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.pending += 1;
        self.queues[client.index()].push_back(Job {
            grouped,
            id,
            client,
            request,
            arrival: self.cycle(),
            offset: 0,
            accepted: false,
            next_accepted: false,
            fed: 0,
            read: 0,
            started: false,
            scalar: 0,
        });
        Ok(id)
    }
    fn step(&mut self) -> Result<Vec<Event>, String> {
        if self.cycle() >= self.config.max_cycles {
            return Err("cycle service watchdog".into());
        }
        let mut input = idle_inputs();
        input.display_response_ready = true;
        input.instruction_response_ready = true;
        input.data_response_ready = true;
        input.dma_response_ready = true;
        for queue in &self.queues {
            if let Some(job) = queue.front() {
                Self::drive(job, self.config.early_grant, &mut input);
            }
        }
        let o = self.combination.output(&input);
        let cycle = self.cycle();
        let mut events = Vec::new();
        for (index, queue) in self.queues.iter_mut().enumerate() {
            let Some(job) = queue.front_mut() else {
                continue;
            };
            let (ready, fed, valid, data, last, error) = match job.client {
                Client::Display => (
                    o.display_request_ready,
                    false,
                    o.display_response_valid,
                    o.display_read_data,
                    o.display_response_last,
                    o.display_error,
                ),
                Client::Instruction => (
                    o.instruction_request_ready,
                    false,
                    o.instruction_response_valid,
                    o.instruction_read_data,
                    job.read == 3,
                    o.instruction_error,
                ),
                Client::Data => (
                    o.data_request_ready,
                    o.data_write_data_ready,
                    o.data_response_valid,
                    o.data_read_data,
                    job.request.burst()?.access == Access::Write || job.read == 3,
                    o.data_error,
                ),
                Client::Dma => (
                    o.dma_request_ready,
                    false,
                    o.dma_response_valid,
                    o.dma_read_data,
                    true,
                    o.dma_error,
                ),
                Client::GpuReadOnly => (
                    o.gpu_ro_request_ready,
                    o.gpu_ro_write_data_ready,
                    o.gpu_ro_response_valid,
                    o.gpu_ro_read_data,
                    o.gpu_ro_response_last,
                    o.gpu_ro_error,
                ),
                Client::FramebufferRead => (
                    o.gpu_fb_r_request_ready,
                    o.gpu_fb_r_write_data_ready,
                    o.gpu_fb_r_response_valid,
                    o.gpu_fb_r_read_data,
                    o.gpu_fb_r_response_last,
                    o.gpu_fb_r_error,
                ),
                Client::FramebufferWrite => (
                    o.gpu_fb_w_request_ready,
                    o.gpu_fb_w_write_data_ready,
                    o.gpu_fb_w_response_valid,
                    o.gpu_fb_w_read_data,
                    o.gpu_fb_w_response_last,
                    o.gpu_fb_w_error,
                ),
            };
            if ready {
                if job.accepted {
                    job.next_accepted = true;
                } else {
                    job.accepted = true;
                }
                if !job.started {
                    events.push(Event::Started {
                        id: job.id,
                        cycle,
                        queued_cycles: cycle - job.arrival,
                    });
                    job.started = true;
                }
            }
            if fed {
                job.fed += 1;
            }
            if valid {
                if error {
                    return Err(format!("cycle hardware error for client {index}"));
                }
                let burst = job.request.burst()?;
                let bytes = Self::sector(job);
                if burst.access == Access::Read {
                    let beat = job.offset / 8 + job.read;
                    if bytes == 2 {
                        job.scalar |= data << (job.offset % 8 * 8);
                        if job.offset % 8 == 6 {
                            events.push(Event::ReadBeat {
                                id: job.id,
                                cycle,
                                index: beat,
                                data: OracleWord::from_memory(job.scalar),
                                last: beat == burst.bytes / 8 - 1,
                            });
                            job.scalar = 0;
                        }
                    } else {
                        events.push(Event::ReadBeat {
                            id: job.id,
                            cycle,
                            index: beat,
                            data: OracleWord::from_memory(data),
                            last: beat == burst.bytes / 8 - 1,
                        });
                        job.read += 1;
                    }
                }
                if last {
                    job.offset += bytes;
                    job.accepted = job.next_accepted;
                    job.next_accepted = false;
                    job.fed = 0;
                    job.read = 0;
                    if job.offset == burst.bytes {
                        events.push(Event::Complete { id: job.id, cycle });
                        queue.pop_front();
                        self.pending -= 1;
                    }
                }
            }
        }
        self.combination.tick(&input)?;
        Ok(events)
    }
    fn cycle(&self) -> u64 {
        self.combination.cycle
    }
    fn idle(&self) -> bool {
        self.pending == 0
    }
}
