//! Logic stream endpoint connected to native edges; backing remains caller-owned.
use super::{gear, memory::Memory};
use crate::sdram_memory_controller::{
    sim::endpoint,
    streaming_port::{
        StreamingSdramPortInputValue as PortInput, StreamingSdramPortOutputValue as Output,
    },
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub reset: bool,
    pub request_valid: bool,
    pub writing: bool,
    pub line: bool,
    /// Logical 16-bit-word address, truncated to the fitted 22-bit port.
    pub address: u32,
    pub line_count_minus_1: u8,
    pub chain_class: u8,
    pub write_data: u64,
    pub response_ready: bool,
}

#[derive(Clone)]
pub struct Port {
    endpoint: endpoint::State,
    memory: Memory,
}
impl Default for Port {
    fn default() -> Self {
        Self::new(21600)
    }
}
impl Port {
    pub fn new(init_cycles: u16) -> Self {
        Self {
            endpoint: endpoint::State::default(),
            memory: Memory::new(true, init_cycles),
        }
    }
    pub fn initialized(&self) -> bool {
        self.memory.output(&gear::Input::default()).initialized
    }
    pub fn native_cycle(&self) -> u64 {
        self.memory.native_cycle()
    }
    fn connect(&self, i: &Input) -> PortInput {
        let native = self.memory.output(&gear::Input {
            reset: i.reset,
            ..Default::default()
        });
        PortInput {
            reset: i.reset,
            cpu_request_valid: i.request_valid,
            cpu_write: i.writing,
            cpu_line: i.line,
            cpu_address: u64::from(i.address & 0x3fffff),
            cpu_line_count_minus_1: u64::from(i.line_count_minus_1 & 3),
            cpu_chain_class: u64::from(i.chain_class & 3),
            cpu_write_data: i.write_data,
            cpu_response_ready: i.response_ready,
            controller_read_data: native.read_data,
            controller_read_valid: native.read_valid,
            controller_init_done: native.initialized,
            controller_request_ready: native.request_ready,
            controller_stream_active: native.stream_active,
            controller_write_done: native.write_done,
            controller_write_data_ready: native.write_data_ready,
        }
    }
    pub fn output(&self, i: &Input) -> Output {
        self.endpoint.output(&self.connect(i))
    }
    pub fn clock(&mut self, i: &Input, backing: &mut [u16]) -> Result<(), super::memory::Error> {
        let port_input = self.connect(i);
        let o = self.endpoint.output(&port_input);
        let drive = gear::Input {
            reset: i.reset,
            request_valid: o.controller_request_valid,
            continuation: o.controller_request_continuation,
            writing: o.controller_write,
            address: o.controller_address as u32,
            words: o.controller_words as u8,
            write_mask: o.controller_write_mask as u8,
            write_data: o.controller_write_data,
            write_data_valid: o.controller_write_data_valid,
            next_valid: o.controller_next_valid,
            next_address: o.controller_next_address as u32,
        };
        // Both logic owners consume the same old feedback before native edges advance.
        self.endpoint.clock(&port_input);
        self.memory.clock_logic(&drive, backing)
    }
}
