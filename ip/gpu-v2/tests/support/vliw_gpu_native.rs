//! One production arbiter/endpoint/gear/MC/pin owner over one full backing.
//! Observation between native half edges checks when bytes become visible.
use digital_design_circuit::{build_circuit, Circuit};
use digital_design_hardware_gowin::sdram_memory_controller::{
    arbiter::{
        CpuV3MemoryArbiter, CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput,
        CpuV3MemoryArbiterOutputValue,
    },
    cycle::{gear, memory::Memory as NativeMemory, port::Input as MemoryInput},
    sim::endpoint,
    streaming_port::StreamingSdramPortInputValue as PortInput,
};
use digital_design_hardware_gowin::{Module, ModuleIo};
use gpu_v2::system::vliw_cycle::{router, Physical, Response};
pub struct Memory {
    circuit: Circuit,
    input: CpuV3MemoryArbiterInput,
    output: CpuV3MemoryArbiterOutput,
    endpoint: endpoint::State,
    memory: NativeMemory,
    sampled: MemoryInput,
    backing: Vec<u16>,
    // Visibility observations live from physical acceptance through terminal.
    // Keeping every historical CLEAR sector would make each half edge rescan
    // an ever-growing framebuffer, without checking any additional owner.
    pending: std::collections::VecDeque<(usize, usize, bool)>,
    pub changed_half_edges: usize,
    pub changed_halfwords: usize,
    pub write_accepts: usize,
    pub logic_edges: usize,
}
impl Memory {
    pub fn new(backing: Vec<u16>) -> Self {
        assert_eq!(backing.len(), 0x400000);
        let (circuit, (input, output)) = build_circuit(|| {
            let i = CpuV3MemoryArbiterInput::allocate();
            let o = CpuV3MemoryArbiter::emu(&i);
            (i, o)
        });
        Self {
            circuit,
            input,
            output,
            endpoint: Default::default(),
            memory: Default::default(),
            sampled: Default::default(),
            backing,
            pending: Default::default(),
            changed_half_edges: 0,
            changed_halfwords: 0,
            write_accepts: 0,
            logic_edges: 0,
        }
    }
    fn port_input(&self, i: MemoryInput) -> PortInput {
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
    fn memory_input(o: &CpuV3MemoryArbiterOutputValue, reset: bool) -> MemoryInput {
        MemoryInput {
            reset,
            request_valid: o.memory_request_valid,
            writing: o.memory_write,
            line: o.memory_line,
            address: o.memory_address as u32,
            line_count_minus_1: o.memory_line_count_minus_1 as u8,
            chain_class: 0,
            write_data: o.memory_write_data,
            response_ready: o.memory_response_ready,
        }
    }
    pub fn feedback(&mut self, gpu: router::Output, reset: bool) -> [Physical; 3] {
        let mut previous = None;
        for _ in 0..16 {
            let mo = self.endpoint.output(&self.port_input(self.sampled));
            let mut a = self.input.sample(&self.circuit);
            a.reset = reset;
            a.lookahead_enable = mo.cpu_lookahead_window;
            a.instruction_request_valid = false;
            a.data_request_valid = false;
            a.dma_request_valid = false;
            a.display_request_valid = false;
            a.instruction_response_ready = true;
            a.data_response_ready = true;
            a.dma_response_ready = true;
            a.display_response_ready = true;
            a.memory_request_ready = mo.cpu_request_ready;
            a.memory_write_data_ready = mo.cpu_write_data_ready;
            a.memory_response_valid = mo.cpu_response_valid;
            a.memory_read_data = mo.cpu_read_data;
            a.memory_response_last = mo.cpu_response_last;
            a.memory_error = mo.cpu_error;
            a.gpu_ro_request_valid = gpu.physical[0].valid;
            a.gpu_ro_address = u64::from(gpu.physical[0].address_halfword);
            a.gpu_ro_line_count_minus_1 = u64::from(gpu.physical[0].count);
            a.gpu_fb_r_request_valid = gpu.physical[1].valid;
            a.gpu_fb_r_address = u64::from(gpu.physical[1].address_halfword);
            a.gpu_fb_r_line_count_minus_1 = u64::from(gpu.physical[1].count);
            a.gpu_fb_w_request_valid = gpu.physical[2].valid;
            a.gpu_fb_w_write = true;
            a.gpu_fb_w_address = u64::from(gpu.physical[2].address_halfword);
            a.gpu_fb_w_line_count_minus_1 = u64::from(gpu.physical[2].count);
            a.gpu_fb_w_write_data = gpu.write_data;
            self.input.drive(&mut self.circuit, &a);
            self.circuit.execute_gates();
            let o = self.output.sample(&self.circuit);
            let mut mi = Self::memory_input(&o, reset);
            mi.chain_class = if gpu.physical[0].valid && o.gpu_ro_request_ready {
                1
            } else if gpu.physical[2].valid && o.gpu_fb_w_request_ready {
                2
            } else if gpu.physical[1].valid && o.gpu_fb_r_request_ready {
                3
            } else {
                0
            };
            let key = |v: MemoryInput| {
                (
                    v.reset,
                    v.request_valid,
                    v.writing,
                    v.line,
                    v.address,
                    v.line_count_minus_1,
                    v.chain_class,
                    v.write_data,
                    v.response_ready,
                )
            };
            let stable = previous.as_ref() == Some(&o) && key(self.sampled) == key(mi);
            self.sampled = mi;
            previous = Some(o.clone());
            if stable {
                return [
                    Physical {
                        request_ready: o.gpu_ro_request_ready,
                        write_ready: o.gpu_ro_write_data_ready,
                        response: Response {
                            valid: o.gpu_ro_response_valid,
                            last: o.gpu_ro_response_last,
                            error: o.gpu_ro_error,
                            data: o.gpu_ro_read_data,
                        },
                    },
                    Physical {
                        request_ready: o.gpu_fb_r_request_ready,
                        write_ready: o.gpu_fb_r_write_data_ready,
                        response: Response {
                            valid: o.gpu_fb_r_response_valid,
                            last: o.gpu_fb_r_response_last,
                            error: o.gpu_fb_r_error,
                            data: o.gpu_fb_r_read_data,
                        },
                    },
                    Physical {
                        request_ready: o.gpu_fb_w_request_ready,
                        write_ready: o.gpu_fb_w_write_data_ready,
                        response: Response {
                            valid: o.gpu_fb_w_response_valid,
                            last: o.gpu_fb_w_response_last,
                            error: o.gpu_fb_w_error,
                            data: o.gpu_fb_w_read_data,
                        },
                    },
                ];
            }
        }
        panic!("GPU/native arbiter feedback exceeded16 settle passes");
    }
    pub fn clock(&mut self) {
        let i = self.port_input(self.sampled);
        let o = self.endpoint.output(&i);
        let accepted = if !i.reset && i.cpu_request_valid && o.cpu_request_ready {
            let address = i.cpu_address as usize;
            let words = if i.cpu_line {
                16 * (i.cpu_line_count_minus_1 as usize + 1)
            } else {
                4
            };
            self.pending.push_back((address, words, i.cpu_write));
            if i.cpu_write {
                self.write_accepts += 1;
                Some((address, self.backing[address..address + words].to_vec()))
            } else {
                None
            }
        } else {
            None
        };
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
        self.circuit.clock_tick();
        self.endpoint.clock(&i);
        let mut regions = Vec::new();
        for &(a, n, w) in &self.pending {
            if w && !regions.contains(&(a, n)) {
                regions.push((a, n));
            }
        }
        for half in 0..4 {
            let before: Vec<_> = regions
                .iter()
                .map(|&(a, n)| self.backing[a..a + n].to_vec())
                .collect();
            self.memory.clock_half(&drive, &mut self.backing).unwrap();
            let changed: usize = regions
                .iter()
                .zip(before)
                .map(|(&(a, n), old)| {
                    self.backing[a..a + n]
                        .iter()
                        .zip(old)
                        .filter(|(x, y)| **x != *y)
                        .count()
                })
                .sum();
            assert!(
                changed <= 2,
                "one native falling edge changed more than32bits"
            );
            if changed != 0 {
                assert!(
                    half == 1 || half == 3,
                    "memory changed on a native rising edge"
                );
                self.changed_half_edges += 1;
                self.changed_halfwords += changed;
            }
        }
        if let Some((a, old)) = accepted {
            assert_eq!(
                self.backing[a..a + old.len()],
                old,
                "request acceptance atomically copied a sector"
            );
        }
        if !i.reset && o.cpu_response_valid && o.cpu_response_last && i.cpu_response_ready {
            assert!(
                self.pending.pop_front().is_some(),
                "native response without accepted observer descriptor"
            );
        }
        if i.reset {
            self.pending.clear();
        }
        self.logic_edges += 1;
    }
    pub fn backing(&self) -> &[u16] {
        &self.backing
    }
    pub fn native_cycles(&self) -> u64 {
        self.memory.native_cycle()
    }
}
