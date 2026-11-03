//! Single production arbiter/endpoint/gear/MC/pin owner. No client clocks it.
use super::request_slot::Slot;
use digital_design_circuit::{build_circuit, Circuit};
use digital_design_hardware::ModuleIo;
use digital_design_hardware_gowin::{
    sdram_memory_controller::{
        arbiter::{
            CpuV3MemoryArbiter, CpuV3MemoryArbiterInput, CpuV3MemoryArbiterInputValue,
            CpuV3MemoryArbiterOutput, CpuV3MemoryArbiterOutputValue,
        },
        cycle::{
            gear,
            memory::{Error, Memory},
        },
        sim::endpoint,
        streaming_port::{
            StreamingSdramPortInputValue as PortInput, StreamingSdramPortOutputValue as PortOutput,
        },
    },
    Module,
};
use gpu_v2::system::vliw_cycle::{Physical, Response};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheRequest {
    pub valid: bool,
    pub writing: bool,
    pub line: bool,
    pub address: u32,
    pub write_data: u64,
    pub response_ready: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clients {
    pub instruction: CacheRequest,
    pub data: CacheRequest,
    pub gpu: [Slot; 3],
    pub gpu_write_data: u64,
    pub display_valid: bool,
    pub display_address: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheFeedback {
    pub request_ready: bool,
    pub write_data_ready: bool,
    pub response_valid: bool,
    pub read_data: u64,
    pub error: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Feedback {
    pub instruction: CacheFeedback,
    pub data: CacheFeedback,
    pub gpu: [Physical; 3],
    pub display: Physical,
    pub initialized: bool,
}
/// A frozen logic-edge sample. Later commits never resample another owner's new state.
pub struct Sample {
    pub clients: Clients,
    pins: CpuV3MemoryArbiterInputValue,
    pub arbiter: CpuV3MemoryArbiterOutputValue,
    port_input: PortInput,
    pub endpoint: PortOutput,
    drive: gear::Input,
    pub feedback: Feedback,
}
pub struct SharedMemory {
    circuit: Circuit,
    input: CpuV3MemoryArbiterInput,
    output: CpuV3MemoryArbiterOutput,
    endpoint: endpoint::State,
    memory: Memory,
    drive: gear::Input,
    /// The only physical image; golden/test comparisons are outside the service.
    backing: Vec<u16>,
}
impl SharedMemory {
    pub fn new(backing: Vec<u16>) -> Self {
        assert_eq!(backing.len(), 1 << 22);
        let (circuit, (input, output)) = build_circuit(|| {
            let input = CpuV3MemoryArbiterInput::allocate();
            let output = CpuV3MemoryArbiter::emu(&input);
            (input, output)
        });
        Self {
            circuit,
            input,
            output,
            endpoint: Default::default(),
            memory: Default::default(),
            drive: Default::default(),
            backing,
        }
    }
    pub fn backing(&self) -> &[u16] {
        &self.backing
    }
    pub fn native_cycles(&self) -> u64 {
        self.memory.native_cycle()
    }
    pub fn initialized(&self) -> bool {
        self.memory.output(&self.drive).initialized
    }
    fn port_input(
        &self,
        reset: bool,
        a: Option<&CpuV3MemoryArbiterOutputValue>,
        class: u8,
    ) -> PortInput {
        let n = self.memory.output(&gear::Input {
            reset,
            ..self.drive
        });
        PortInput {
            reset,
            cpu_request_valid: a.is_some_and(|v| v.memory_request_valid),
            cpu_write: a.is_some_and(|v| v.memory_write),
            cpu_line: a.is_some_and(|v| v.memory_line),
            cpu_address: a.map_or(0, |v| v.memory_address),
            cpu_line_count_minus_1: a.map_or(0, |v| v.memory_line_count_minus_1),
            cpu_chain_class: class.into(),
            cpu_write_data: a.map_or(0, |v| v.memory_write_data),
            cpu_response_ready: a.is_some_and(|v| v.memory_response_ready),
            controller_read_data: n.read_data,
            controller_read_valid: n.read_valid,
            controller_init_done: n.initialized,
            controller_request_ready: n.request_ready,
            controller_stream_active: n.stream_active,
            controller_write_done: n.write_done,
            controller_write_data_ready: n.write_data_ready,
        }
    }
    pub fn sample(&mut self, c: Clients, reset: bool) -> Sample {
        let mut prior = None;
        let mut port = self.port_input(reset, None, 0);
        for _ in 0..16 {
            let e = self.endpoint.output(&port);
            let mut p = self.input.sample(&self.circuit);
            p.reset = reset;
            p.lookahead_enable = e.cpu_lookahead_window;
            p.instruction_request_valid = c.instruction.valid;
            p.instruction_address = c.instruction.address.into();
            p.instruction_response_ready = c.instruction.response_ready;
            p.data_request_valid = c.data.valid;
            p.data_write = c.data.writing;
            p.data_line = c.data.line;
            p.data_address = c.data.address.into();
            p.data_write_data = c.data.write_data;
            p.data_response_ready = c.data.response_ready;
            p.dma_request_valid = false;
            p.dma_write = false;
            p.dma_address = 0;
            p.dma_write_data = 0;
            p.dma_response_ready = true;
            p.display_request_valid = c.display_valid;
            p.display_address = c.display_address.into();
            p.display_response_ready = true;
            p.gpu_ro_request_valid = c.gpu[0].request.valid;
            p.gpu_ro_write = c.gpu[0].writing;
            p.gpu_ro_address = c.gpu[0].request.address_halfword.into();
            p.gpu_ro_line_count_minus_1 = c.gpu[0].request.count.into();
            p.gpu_ro_write_data = 0;
            p.gpu_fb_r_request_valid = c.gpu[1].request.valid;
            p.gpu_fb_r_write = c.gpu[1].writing;
            p.gpu_fb_r_address = c.gpu[1].request.address_halfword.into();
            p.gpu_fb_r_line_count_minus_1 = c.gpu[1].request.count.into();
            p.gpu_fb_r_write_data = 0;
            p.gpu_fb_w_request_valid = c.gpu[2].request.valid;
            p.gpu_fb_w_write = c.gpu[2].writing;
            p.gpu_fb_w_address = c.gpu[2].request.address_halfword.into();
            p.gpu_fb_w_line_count_minus_1 = c.gpu[2].request.count.into();
            p.gpu_fb_w_write_data = c.gpu_write_data;
            p.memory_request_ready = e.cpu_request_ready;
            p.memory_write_data_ready = e.cpu_write_data_ready;
            p.memory_response_valid = e.cpu_response_valid;
            p.memory_read_data = e.cpu_read_data;
            p.memory_response_last = e.cpu_response_last;
            p.memory_error = e.cpu_error;
            self.input.drive(&mut self.circuit, &p);
            self.circuit.execute_gates();
            let a = self.output.sample(&self.circuit);
            // Exact production grant priority; never classify a merely offered request.
            let class = if c.gpu[0].request.valid && a.gpu_ro_request_ready {
                1
            } else if c.gpu[2].request.valid && a.gpu_fb_w_request_ready {
                2
            } else if c.gpu[1].request.valid && a.gpu_fb_r_request_ready
                || c.data.valid && a.data_request_ready && c.data.writing
            {
                3
            } else {
                0
            };
            let next = self.port_input(reset, Some(&a), class);
            let feedback = Feedback {
                initialized: self.initialized(),
                instruction: CacheFeedback {
                    request_ready: a.instruction_request_ready,
                    response_valid: a.instruction_response_valid,
                    read_data: a.instruction_read_data,
                    error: a.instruction_error,
                    ..Default::default()
                },
                data: CacheFeedback {
                    request_ready: a.data_request_ready,
                    write_data_ready: a.data_write_data_ready,
                    response_valid: a.data_response_valid,
                    read_data: a.data_read_data,
                    error: a.data_error,
                },
                gpu: [
                    Physical {
                        request_ready: a.gpu_ro_request_ready,
                        write_ready: a.gpu_ro_write_data_ready,
                        response: Response {
                            valid: a.gpu_ro_response_valid,
                            last: a.gpu_ro_response_last,
                            error: a.gpu_ro_error,
                            data: a.gpu_ro_read_data,
                        },
                    },
                    Physical {
                        request_ready: a.gpu_fb_r_request_ready,
                        write_ready: a.gpu_fb_r_write_data_ready,
                        response: Response {
                            valid: a.gpu_fb_r_response_valid,
                            last: a.gpu_fb_r_response_last,
                            error: a.gpu_fb_r_error,
                            data: a.gpu_fb_r_read_data,
                        },
                    },
                    Physical {
                        request_ready: a.gpu_fb_w_request_ready,
                        write_ready: a.gpu_fb_w_write_data_ready,
                        response: Response {
                            valid: a.gpu_fb_w_response_valid,
                            last: a.gpu_fb_w_response_last,
                            error: a.gpu_fb_w_error,
                            data: a.gpu_fb_w_read_data,
                        },
                    },
                ],
                display: Physical {
                    request_ready: a.display_request_ready,
                    write_ready: false,
                    response: Response {
                        valid: a.display_response_valid,
                        last: a.display_response_last,
                        error: a.display_error,
                        data: a.display_read_data,
                    },
                },
            };
            if prior.as_ref() == Some(&a) && port == next {
                let drive = gear::Input {
                    reset,
                    request_valid: e.controller_request_valid,
                    continuation: e.controller_request_continuation,
                    writing: e.controller_write,
                    address: e.controller_address as u32,
                    words: e.controller_words as u8,
                    write_mask: e.controller_write_mask as u8,
                    write_data: e.controller_write_data,
                    write_data_valid: e.controller_write_data_valid,
                    next_valid: e.controller_next_valid,
                    next_address: e.controller_next_address as u32,
                };
                return Sample {
                    clients: c,
                    pins: p,
                    arbiter: a,
                    port_input: next,
                    endpoint: e,
                    drive,
                    feedback,
                };
            }
            prior = Some(a);
            port = next;
        }
        panic!("shared arbiter/endpoint exceeded16 pure settle passes")
    }
    pub fn commit_logic(&mut self, s: Sample) {
        self.input.drive(&mut self.circuit, &s.pins);
        self.circuit.clock_tick();
        self.endpoint.clock(&s.port_input);
        self.drive = s.drive;
    }
    pub fn clock_native_half(&mut self) -> Result<(), Error> {
        self.memory.clock_half(&self.drive, &mut self.backing)
    }
    pub fn clock_native_half_observing(
        &mut self,
    ) -> Result<
        digital_design_hardware_gowin::sdram_memory_controller::cycle::memory::Activity,
        Error,
    > {
        self.memory
            .clock_half_observing(&self.drive, &mut self.backing)
    }
}
