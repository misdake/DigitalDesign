//! Live GPU model -> production arbiter -> native endpoint/gear/MC/pins.
//! RTL replays this service's exact feedback and must reproduce every GPU pin.
use super::*;
use digital_design_circuit::{build_circuit, Circuit};
use digital_design_hardware_gowin::sdram_memory_controller::{
    arbiter::{
        CpuV3MemoryArbiter, CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput,
        CpuV3MemoryArbiterOutputValue,
    },
    cycle::port::{Input as MemoryInput, Port},
};
use digital_design_hardware_gowin::{Module, ModuleIo};

pub(crate) struct Memory {
    circuit: Circuit,
    input: CpuV3MemoryArbiterInput,
    output: CpuV3MemoryArbiterOutput,
    port: Port,
    backing: Vec<u16>,
    sampled: MemoryInput,
    write_logic_edges: usize,
    first_write_accept: bool,
}
impl Memory {
    pub(crate) fn new() -> Self {
        let (circuit, (input, output)) = build_circuit(|| {
            let i = CpuV3MemoryArbiterInput::allocate();
            let o = CpuV3MemoryArbiter::emu(&i);
            (i, o)
        });
        let mut backing = vec![0u16; 65536];
        for (n, q) in backing.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let value = pattern((n * 4) as u32, 0);
            for (k, w) in q.iter_mut().enumerate() {
                *w = (value >> (k * 16)) as u16;
            }
        }
        Self {
            circuit,
            input,
            output,
            port: Port::default(),
            backing,
            sampled: MemoryInput::default(),
            write_logic_edges: 0,
            first_write_accept: false,
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
    pub(crate) fn feedback(
        &mut self,
        gpu: gpu_v2::system::vliw_cycle::router::Output,
        reset: bool,
    ) -> [Physical; 3] {
        let mut previous = None;
        for _ in 0..16 {
            let mo = self.port.output(&self.sampled);
            // Undriven circuit input bits start at zero. Explicitly keep every
            // non-GPU requester idle; no CPU/Display contention is claimed.
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
            // Same accepted-client classes as self_test.v. This fixture binds
            // directly at arbiter pins; board request-slot stages are not modeled.
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
        panic!("GPU/native/arbiter combinational convergence exceeded16");
    }
    pub(crate) fn clock(&mut self) {
        let before = self.backing[0x4000..0x4100].to_vec();
        let first = !self.first_write_accept
            && self.sampled.request_valid
            && self.sampled.writing
            && self.port.output(&self.sampled).cpu_request_ready;
        self.circuit.clock_tick();
        self.port.clock(&self.sampled, &mut self.backing).unwrap();
        if first {
            assert_eq!(
                before,
                self.backing[0x4000..0x4100],
                "request accept must not instantaneously copy a sector"
            );
            self.first_write_accept = true;
        }
        if before != self.backing[0x4000..0x4100] {
            self.write_logic_edges += 1;
        }
    }
    /// Explicit fixture preload; does not claim loader-final equivalence.
    #[allow(dead_code)] // Used by the separate command-control integration target.
    pub(crate) fn preload(&mut self, address_halfword: usize, words: &[u16]) {
        self.backing[address_halfword..address_halfword + words.len()].copy_from_slice(words);
    }
    pub(crate) fn native_cycles(&self) -> u64 {
        self.port.native_cycle()
    }
}

pub(super) fn run() -> Run {
    let mut memory = Memory::new();
    let mut transport = Transport::default();
    let mut actor = Actor::default();
    let mut run = Run {
        trace: Vec::new(),
        stats: Stats::default(),
        reset_while_active: false,
        native_cycles: 0,
        backing_write_logic_edges: 0,
    };
    for wall in 0..200000 {
        let mut i = actor.input(wall);
        i.reset = wall < 2;
        let pins = transport.outputs(i).router;
        i.physical = memory.feedback(pins, i.reset);
        let o = transport.outputs(i);
        run.trace.push(Edge {
            input: i,
            output: o,
        });
        let occ = transport.dma.occupancy();
        run.stats.max_dma_returns = run.stats.max_dma_returns.max(occ.1);
        run.stats.max_ro_owners = run.stats.max_ro_owners.max(transport.router.occupancy().0);
        transport
            .clock(i)
            .unwrap_or_else(|e| panic!("native wall{wall}: {e}"));
        if !i.reset {
            actor.observe(i, o, &mut run.stats);
        }
        memory.clock();
        if actor.done(&run.stats) && o.dma.idle && !o.router.busy {
            for beat in 0..64 {
                let at = 0x4000 + beat * 4;
                let value = (0..4).fold(0u64, |v, k| {
                    v | (u64::from(memory.backing[at + k]) << (k * 16))
                });
                assert_eq!(value, written(beat), "physical raw framebuffer beat{beat}");
            }
            actor.check_ram();
            run.native_cycles = memory.native_cycles();
            run.backing_write_logic_edges = memory.write_logic_edges;
            return run;
        }
    }
    panic!("native transport exceeded200000 logic edges");
}
