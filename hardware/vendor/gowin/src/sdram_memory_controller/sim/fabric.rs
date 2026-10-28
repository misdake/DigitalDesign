//! Functional seven-client/stream-port reference with abstract memory service.
//! It models accepted ownership and actual reference writes, not SDRAM pin timing.
use super::{
    super::{arbiter, ports::*, streaming_port::StreamingSdramPortInputValue as Port},
    endpoint,
};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
struct Job {
    address: usize,
    write: bool,
    words: u8,
    mask: u8,
    beat: u8,
}
pub struct Fabric {
    arbiter: arbiter::CpuV3MemoryArbiterState,
    port: endpoint::State,
    pub image: OracleImage,
    jobs: VecDeque<Job>,
    init: u32,
    init_left: u32,
    read: Option<u64>,
    done: bool,
    pub cycle: u64,
}
impl Fabric {
    pub fn new(image: OracleImage, init_cycles: u32) -> Result<Self, String> {
        if init_cycles == 0 || init_cycles > 65535 {
            return Err("reference initialization bound".into());
        }
        Ok(Self {
            arbiter: Default::default(),
            port: Default::default(),
            image,
            jobs: VecDeque::new(),
            init: init_cycles.div_ceil(2),
            init_left: init_cycles.div_ceil(2),
            read: None,
            done: false,
            cycle: 0,
        })
    }
    pub fn initialized(&self) -> bool {
        self.init_left == 0
    }
    pub fn contains(&self, address: u64, bytes: usize) -> bool {
        address >= self.image.base()
            && address
                .checked_add(bytes as u64)
                .is_some_and(|end| end <= self.image.base() + self.image.bytes().len() as u64)
    }
    fn connect(
        &self,
        input: &arbiter::CpuV3MemoryArbiterInputValue,
    ) -> (arbiter::CpuV3MemoryArbiterInputValue, Port) {
        let mut a = input.clone();
        let mut p = Port {
            reset: a.reset,
            cpu_request_valid: false,
            cpu_write: false,
            cpu_line: false,
            cpu_address: 0,
            cpu_line_count_minus_1: 0,
            cpu_chain_class: 0,
            cpu_write_data: 0,
            cpu_response_ready: false,
            controller_read_data: self.read.unwrap_or(0),
            controller_read_valid: self.read.is_some(),
            controller_init_done: self.initialized(),
            controller_request_ready: self.jobs.len() < 2,
            controller_stream_active: !self.jobs.is_empty(),
            controller_write_done: self.done,
            controller_write_data_ready: self.jobs.front().is_some_and(|j| j.write),
        };
        for _ in 0..3 {
            let o = self.port.output(&p);
            a.lookahead_enable = o.cpu_lookahead_window;
            a.memory_request_ready = o.cpu_request_ready;
            a.memory_write_data_ready = o.cpu_write_data_ready;
            a.memory_response_valid = o.cpu_response_valid;
            a.memory_read_data = o.cpu_read_data;
            a.memory_response_last = o.cpu_response_last;
            a.memory_error = o.cpu_error;
            let ao = arbiter::compute_output(&self.arbiter, &a);
            p.cpu_request_valid = ao.memory_request_valid;
            p.cpu_write = ao.memory_write;
            p.cpu_line = ao.memory_line;
            p.cpu_address = ao.memory_address;
            p.cpu_line_count_minus_1 = ao.memory_line_count_minus_1;
            p.cpu_write_data = ao.memory_write_data;
            p.cpu_response_ready = ao.memory_response_ready;
            p.cpu_chain_class = if a.gpu_ro_request_valid && ao.gpu_ro_request_ready {
                1
            } else if a.gpu_fb_w_request_valid && ao.gpu_fb_w_request_ready {
                2
            } else if (a.gpu_fb_r_request_valid && ao.gpu_fb_r_request_ready)
                || (a.data_request_valid && ao.data_request_ready && a.data_write && a.data_line)
            {
                3
            } else {
                0
            };
        }
        (a, p)
    }
    pub fn output(
        &self,
        i: &arbiter::CpuV3MemoryArbiterInputValue,
    ) -> arbiter::CpuV3MemoryArbiterOutputValue {
        let (a, _) = self.connect(i);
        arbiter::compute_output(&self.arbiter, &a)
    }
    pub fn tick(&mut self, i: &arbiter::CpuV3MemoryArbiterInputValue) -> Result<(), String> {
        let (a, p) = self.connect(i);
        let o = self.port.output(&p);
        arbiter::advance_state(&mut self.arbiter, &a);
        self.port.clock(&p);
        self.read = None;
        self.done = false;
        if i.reset {
            self.jobs.clear();
            self.init_left = self.init;
        } else if self.init_left != 0 {
            self.init_left -= 1;
        } else {
            if let Some(job) = self.jobs.front_mut() {
                let bytes = if job.words == 1 { 4 } else { 8 };
                let offset = job.address + usize::from(job.beat) * 8;
                if !job.write {
                    let source = self
                        .image
                        .bytes()
                        .get(offset..offset + bytes)
                        .ok_or("functional read outside image")?;
                    let mut data = [0; 8];
                    data[..bytes].copy_from_slice(source);
                    self.read = Some(u64::from_le_bytes(data));
                    job.beat += 1;
                } else if o.controller_write_data_valid && p.controller_write_data_ready {
                    if offset + bytes > self.image.bytes().len() {
                        return Err("functional write outside image".into());
                    }
                    let mut data = [0; 8];
                    data.copy_from_slice(&o.controller_write_data.to_le_bytes());
                    let enables = if bytes == 4 { (!job.mask) & 15 } else { 255 };
                    self.image.write(
                        offset,
                        &[OracleWord::from_memory(u64::from_le_bytes(data))],
                        &[enables],
                    );
                    job.beat += 1;
                }
                if job.beat == job.words.div_ceil(2) {
                    self.done = job.write;
                    self.jobs.pop_front();
                }
            }
            if o.controller_request_valid && p.controller_request_ready {
                let absolute = o.controller_address * 4;
                let offset = absolute
                    .checked_sub(self.image.base())
                    .ok_or("functional request before image")?
                    as usize;
                self.jobs.push_back(Job {
                    address: offset,
                    write: o.controller_write,
                    words: o.controller_words as u8,
                    mask: o.controller_write_mask as u8,
                    beat: 0,
                });
            }
        }
        self.cycle += 1;
        Ok(())
    }
}
