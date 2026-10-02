//! Independent edge-based execution of the actual controller/gearbox/adapter.
pub mod adapter;
pub mod bridge;
pub mod native;
pub mod pins;
pub mod service;
use super::{arbiter, shared_port::SharedSdramPortInputValue};
pub struct Combination {
    arbiter: arbiter::CpuV3MemoryArbiterState,
    adapter: adapter::State,
    pub bridge: bridge::Engine,
    pub cycle: u64,
    early_grant: bool,
}
impl Combination {
    pub fn new(image: super::ports::OracleImage, init_cycles: u32) -> Result<Self, String> {
        Self::with_early_grant(image, init_cycles, false)
    }
    pub fn with_early_grant(
        image: super::ports::OracleImage,
        init_cycles: u32,
        early_grant: bool,
    ) -> Result<Self, String> {
        Self::with_options(image, init_cycles, early_grant, false)
    }
    /// An opt-in 512-byte GPU group is one irrevocable arbiter transaction,
    /// with four native commands and a single group LAST/ack.
    pub fn with_options(
        image: super::ports::OracleImage,
        init_cycles: u32,
        early_grant: bool,
        chained_groups: bool,
    ) -> Result<Self, String> {
        Ok(Self {
            arbiter: Default::default(),
            adapter: adapter::State::with_options(early_grant, chained_groups),
            bridge: bridge::Engine::with_options(image, init_cycles, early_grant, chained_groups)?,
            early_grant,
            cycle: 0,
        })
    }
    fn connect(
        &self,
        input: &arbiter::CpuV3MemoryArbiterInputValue,
    ) -> (
        arbiter::CpuV3MemoryArbiterInputValue,
        SharedSdramPortInputValue,
    ) {
        let b = self.bridge.output(input.reset);
        let mut a = input.clone();
        // Response registers do not depend on the live request. Ready depends
        // on that descriptor; resolve it once more after arbitration selects.
        let mut p = SharedSdramPortInputValue {
            reset: a.reset,
            cpu_request_valid: false,
            cpu_write: false,
            cpu_line: false,
            cpu_address: 0,
            cpu_line_count_minus_1: 0,
            cpu_write_data: 0,
            cpu_response_ready: false,
            controller_read_data: b.read_data,
            controller_read_valid: b.read_valid,
            controller_init_done: b.initialized,
            controller_request_ready: b.ready,
            controller_stream_active: b.stream_active,
            controller_done: b.done,
            controller_write_data_ready: b.data_ready,
        };
        for _ in 0..2 {
            let o = self.adapter.output(&p);
            a.lookahead_enable = self.early_grant && o.cpu_lookahead_window;
            a.memory_request_ready = o.cpu_request_ready;
            a.memory_write_data_ready = o.cpu_write_data_ready;
            a.memory_response_valid = o.cpu_response_valid;
            a.memory_read_data = o.cpu_read_data;
            a.memory_response_last = o.cpu_response_last;
            a.memory_error = o.cpu_error;
            let aout = arbiter::compute_output(&self.arbiter, &a);
            p.cpu_request_valid = aout.memory_request_valid;
            p.cpu_write = aout.memory_write;
            p.cpu_line = aout.memory_line;
            p.cpu_address = aout.memory_address;
            p.cpu_line_count_minus_1 = aout.memory_line_count_minus_1;
            p.cpu_write_data = aout.memory_write_data;
            p.cpu_response_ready = aout.memory_response_ready;
        }
        (a, p)
    }
    /// Snapshot immediately before a logic rising edge. Intermediate line
    /// responses require an always-ready sink, as in the production interface.
    pub fn output(
        &self,
        i: &arbiter::CpuV3MemoryArbiterInputValue,
    ) -> arbiter::CpuV3MemoryArbiterOutputValue {
        let (a, _) = self.connect(i);
        arbiter::compute_output(&self.arbiter, &a)
    }
    pub fn tick(&mut self, i: &arbiter::CpuV3MemoryArbiterInputValue) -> Result<(), String> {
        let (a, p) = self.connect(i);
        let o = self.adapter.output(&p);
        arbiter::advance_state(&mut self.arbiter, &a);
        self.adapter.clock(&p);
        self.bridge.tick(bridge::Input {
            reset: i.reset,
            valid: o.controller_request_valid,
            next_valid: o.controller_next_valid,
            next_address: o.controller_next_address as u32,
            writing: o.controller_write,
            address: o.controller_address as u32,
            words: o.controller_words as u8,
            mask: o.controller_write_mask as u8,
            data: o.controller_write_data,
            data_valid: o.controller_write_data_valid,
        })?;
        self.cycle += 1;
        Ok(())
    }
}

/// All clients idle; memory feedback is overwritten by Combination.
pub fn idle_inputs() -> arbiter::CpuV3MemoryArbiterInputValue {
    arbiter::CpuV3MemoryArbiterInputValue {
        reset: false,
        lookahead_enable: false,
        instruction_request_valid: false,
        instruction_address: 0,
        instruction_response_ready: false,
        data_request_valid: false,
        data_write: false,
        data_line: false,
        data_address: 0,
        data_write_data: 0,
        data_response_ready: false,
        dma_request_valid: false,
        dma_write: false,
        dma_address: 0,
        dma_write_data: 0,
        dma_response_ready: false,
        display_request_valid: false,
        display_address: 0,
        display_response_ready: false,
        gpu_ro_request_valid: false,
        gpu_ro_write: false,
        gpu_ro_address: 0,
        gpu_ro_line_count_minus_1: 0,
        gpu_ro_write_data: 0,
        gpu_fb_r_request_valid: false,
        gpu_fb_r_write: false,
        gpu_fb_r_address: 0,
        gpu_fb_r_line_count_minus_1: 0,
        gpu_fb_r_write_data: 0,
        gpu_fb_w_request_valid: false,
        gpu_fb_w_write: false,
        gpu_fb_w_address: 0,
        gpu_fb_w_line_count_minus_1: 0,
        gpu_fb_w_write_data: 0,
        memory_request_ready: false,
        memory_write_data_ready: false,
        memory_response_valid: false,
        memory_read_data: 0,
        memory_response_last: false,
        memory_error: false,
    }
}
