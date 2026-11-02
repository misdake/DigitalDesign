//! Persistent CPU/cache logic emulators with fitted native SDRAM edges.
//! Device0 uses its hardware model; other devices are not yet connected.
use crate::{
    CpuV3MemoryArbiter, CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput, SystemControlDevice,
    SystemControlDeviceInput, SystemControlDeviceOutput,
};
use cpu_v3::*;
use digital_design_circuit::{
    build_circuit, external, input, Circuit, CircuitWires, External, Wire, Wires,
};
use digital_design_hardware::{Module, ModuleIo};
use digital_design_hardware_gowin::sdram_memory_controller::cycle::{
    memory,
    port::{Input as MemoryInput, Port},
};
use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};
#[derive(Debug)]
pub enum Error {
    NativeMemory(memory::Error),
    CombinationalLoop,
    UnsupportedDevice {
        device: u8,
        channel: u8,
        writing: bool,
    },
}
// Match the fitted self_test.v frontend boundary, including terminal states.
fn frontend_flush(system: &SystemControlDeviceOutput, core: &CpuV3CoreOutput) -> Wire {
    system.icache_invalidate | core.halted | core.fault
}

struct MemoryExternal {
    port: Rc<RefCell<Port>>,
    backing: Rc<RefCell<Vec<u16>>>,
    input: CpuV3MemoryArbiterInput,
    output: CpuV3MemoryArbiterOutput,
    initialized: Wire,
    error: Rc<RefCell<Option<memory::Error>>>,
}
impl MemoryExternal {
    fn input(&self, c: &CircuitWires) -> MemoryInput {
        let a = self.input.sample(c);
        let o = self.output.sample(c);
        MemoryInput {
            reset: a.reset,
            request_valid: o.memory_request_valid,
            writing: o.memory_write,
            line: o.memory_line,
            address: o.memory_address as u32,
            line_count_minus_1: o.memory_line_count_minus_1 as u8,
            chain_class: if a.data_request_valid
                && o.data_request_ready
                && a.data_write
                && a.data_line
            {
                3
            } else {
                0
            },
            write_data: o.memory_write_data,
            response_ready: o.memory_response_ready,
        }
    }
}
fn bits<const N: usize>(c: &mut CircuitWires, w: Wires<N>, v: u64) {
    for (k, b) in w.wires.iter().enumerate() {
        c.set_wire(*b, ((v >> k) & 1) as u8);
    }
}
impl External for MemoryExternal {
    fn execute(&mut self, c: &mut CircuitWires) {
        let o = self.port.borrow().output(&self.input(c));
        c.set_wire(self.input.lookahead_enable, o.cpu_lookahead_window.into());
        c.set_wire(self.input.memory_request_ready, o.cpu_request_ready.into());
        c.set_wire(
            self.input.memory_write_data_ready,
            o.cpu_write_data_ready.into(),
        );
        c.set_wire(
            self.input.memory_response_valid,
            o.cpu_response_valid.into(),
        );
        c.set_wire(self.input.memory_response_last, o.cpu_response_last.into());
        c.set_wire(self.input.memory_error, o.cpu_error.into());
        bits(c, self.input.memory_read_data, o.cpu_read_data);
        c.set_wire(self.initialized, self.port.borrow().initialized().into());
    }
    fn clock(&mut self, c: &mut CircuitWires) {
        let i = self.input(c);
        if let Err(e) = self
            .port
            .borrow_mut()
            .clock(&i, &mut self.backing.borrow_mut())
        {
            *self.error.borrow_mut() = Some(e);
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
struct Io {
    reset: Wire,
    core_input: CpuV3CoreInput,
    core_output: CpuV3CoreOutput,
    fetch_output: CpuV3InstructionFetchQueueOutput,
    icache_output: CpuV3InstructionCacheOutput,
    dcache_output: CpuV3DataCacheOutput,
    arbiter_output: CpuV3MemoryArbiterOutput,
    system_output: SystemControlDeviceOutput,
}
/// One machine with persistent CPU/cache/native state and one physical backing.
pub struct CpuV3SystemEmu {
    circuit: Circuit,
    io: Io,
    backing: Rc<RefCell<Vec<u16>>>,
    port: Rc<RefCell<Port>>,
    error: Rc<RefCell<Option<memory::Error>>>,
    cycle: u64,
}
impl CpuV3SystemEmu {
    /// Preloaded physical image starting at reset PC/CSEG0; not Flash boot.
    pub fn new(backing: Vec<u16>) -> Self {
        assert_eq!(backing.len(), 1 << 22, "fitted SDRAM has 4M 16-bit words");
        let backing = Rc::new(RefCell::new(backing));
        let port = Rc::new(RefCell::new(Port::default()));
        let error = Rc::new(RefCell::new(None));
        let (circuit, io) = build_circuit(|| {
            let reset = input();
            let initialized = input();
            let mut core_input = CpuV3CoreInput::allocate();
            let core_output = CpuV3CoreOutput::allocate();
            let mut fetch_input = CpuV3InstructionFetchQueueInput::allocate();
            let fetch_output = CpuV3InstructionFetchQueueOutput::allocate();
            let mut icache_input = CpuV3InstructionCacheInput::allocate();
            let icache_output = CpuV3InstructionCacheOutput::allocate();
            let mut dcache_input = CpuV3DataCacheInput::allocate();
            let dcache_output = CpuV3DataCacheOutput::allocate();
            let mut arbiter_input = CpuV3MemoryArbiterInput::allocate();
            let arbiter_output = CpuV3MemoryArbiterOutput::allocate();
            let mut system_input = SystemControlDeviceInput::allocate();
            let system_output = SystemControlDeviceOutput::allocate();
            // Core and fetch share the production request/response handshake.
            fetch_input.core_request_valid = core_output.instruction_request_valid;
            fetch_input.core_address = core_output.instruction_address;
            fetch_input.core_response_ready = core_output.instruction_response_ready;
            core_input.instruction_request_ready = fetch_output.core_request_ready;
            core_input.instruction_response_valid = fetch_output.core_response_valid;
            core_input.instruction_data = fetch_output.core_read_data;
            core_input.instruction_error = fetch_output.core_error;
            // fetch queue -> I-cache
            icache_input.cpu_request_valid = fetch_output.memory_request_valid;
            icache_input.cpu_address = fetch_output.memory_address;
            icache_input.cpu_response_ready = fetch_output.memory_response_ready;
            fetch_input.memory_request_ready = icache_output.cpu_request_ready;
            fetch_input.memory_response_valid = icache_output.cpu_response_valid;
            fetch_input.memory_read_data = icache_output.cpu_read_data;
            fetch_input.memory_error = icache_output.cpu_error;

            // core <-> D-cache
            dcache_input.cpu_request_valid = core_output.data_request_valid;
            dcache_input.cpu_write = core_output.data_write;
            dcache_input.cpu_address = core_output.data_address;
            dcache_input.cpu_write_data = core_output.data_write_data;
            dcache_input.cpu_response_ready = core_output.data_response_ready;
            core_input.data_request_ready = dcache_output.cpu_request_ready;
            core_input.data_response_valid = dcache_output.cpu_response_valid;
            core_input.data_read_data = dcache_output.cpu_read_data;
            core_input.data_error = dcache_output.cpu_error;
            dcache_input.line_copy_start = core_output.data_line_copy_valid;
            dcache_input.line_copy_source = core_output.data_line_copy_source;
            dcache_input.line_clean_start = core_output.data_line_clean_valid;
            dcache_input.line_clean_address = core_output.data_line_clean_address;
            core_input.data_line_copy_ready = dcache_output.line_copy_ready;
            core_input.data_cache_command_error = dcache_output.maintenance_error;
            // Hold the core while the D-cache RAM16 valid arrays sweep-clear,
            // mirroring the system template's `sysctl_cpu_hold || valid_sweep`.
            core_input.hold = dcache_output.valid_sweep | system_output.cpu_hold | !initialized;

            // I-cache <-> arbiter
            arbiter_input.instruction_request_valid = icache_output.memory_request_valid;
            arbiter_input.instruction_address = icache_output.memory_address;
            arbiter_input.instruction_response_ready = icache_output.memory_response_ready;
            icache_input.memory_request_ready = arbiter_output.instruction_request_ready;
            icache_input.memory_response_valid = arbiter_output.instruction_response_valid;
            icache_input.memory_read_data = arbiter_output.instruction_read_data;
            icache_input.memory_error = arbiter_output.instruction_error;

            // D-cache <-> arbiter
            arbiter_input.data_request_valid = dcache_output.memory_request_valid;
            arbiter_input.data_write = dcache_output.memory_write;
            arbiter_input.data_line = dcache_output.memory_line;
            arbiter_input.data_address = dcache_output.memory_address;
            arbiter_input.data_write_data = dcache_output.memory_write_data;
            arbiter_input.data_response_ready = dcache_output.memory_response_ready;
            dcache_input.memory_request_ready = arbiter_output.data_request_ready;
            dcache_input.memory_write_data_ready = arbiter_output.data_write_data_ready;
            dcache_input.memory_response_valid = arbiter_output.data_response_valid;
            dcache_input.memory_read_data = arbiter_output.data_read_data;
            dcache_input.memory_error = arbiter_output.data_error;

            core_input.reset = reset;
            fetch_input.reset = reset;
            icache_input.reset = reset;
            dcache_input.reset = reset;
            arbiter_input.reset = reset;
            system_input.reset = reset;
            system_input.device_index = core_output.device_index;
            system_input.device_channel = core_output.device_channel;
            system_input.device_read_enable = core_output.device_read_enable;
            system_input.device_write_enable = core_output.device_write_enable;
            system_input.device_write_data = core_output.device_write_data;
            core_input.device_read_data = system_output.device_read_data;
            system_input.dcache_maintenance_ready = dcache_output.maintenance_ready;
            system_input.dcache_maintenance_done = dcache_output.maintenance_done;
            system_input.dcache_maintenance_error = dcache_output.maintenance_error;
            dcache_input.invalidate_all = system_output.dcache_invalidate;
            dcache_input.clean_all = system_output.dcache_clean;
            icache_input.invalidate_all = system_output.icache_invalidate;
            fetch_input.flush = frontend_flush(&system_output, &core_output);
            CpuV3Core::emu_connect(&core_input, &core_output);
            CpuV3InstructionFetchQueue::emu_connect(&fetch_input, &fetch_output);
            CpuV3InstructionCache::emu_connect(&icache_input, &icache_output);
            CpuV3DataCache::emu_connect(&dcache_input, &dcache_output);
            CpuV3MemoryArbiter::emu_connect(&arbiter_input, &arbiter_output);
            SystemControlDevice::<469>::emu_connect(&system_input, &system_output);
            external(MemoryExternal {
                port: port.clone(),
                backing: backing.clone(),
                input: arbiter_input,
                output: arbiter_output.clone(),
                initialized,
                error: error.clone(),
            });
            Io {
                reset,
                core_input,
                core_output,
                fetch_output,
                icache_output,
                dcache_output,
                arbiter_output,
                system_output,
            }
        });
        Self {
            circuit,
            io,
            backing,
            port,
            error,
            cycle: 0,
        }
    }
    pub fn physical_memory(&self) -> Ref<'_, [u16]> {
        Ref::map(self.backing.borrow(), |m| &m[..])
    }
    pub fn cycles(&self) -> u64 {
        self.cycle
    }
    pub fn native_cycles(&self) -> u64 {
        self.port.borrow().native_cycle()
    }
    /// Advance one logic edge and two native cycles. Errors stop this run.
    pub fn step(&mut self) -> Result<CpuV3CoreOutputValue, Error> {
        self.circuit
            .set_wire(self.io.reset, (self.cycle < 2).into());
        let mut previous = None;
        let mut settled = false;
        for _ in 0..8 {
            self.circuit.execute_gates();
            let current = (
                self.io.core_input.sample(&self.circuit),
                self.io.core_output.sample(&self.circuit),
                self.io.fetch_output.sample(&self.circuit),
                self.io.icache_output.sample(&self.circuit),
                self.io.dcache_output.sample(&self.circuit),
                self.io.arbiter_output.sample(&self.circuit),
                self.io.system_output.sample(&self.circuit),
            );
            if previous.as_ref() == Some(&current) {
                settled = true;
                break;
            }
            previous = Some(current);
        }
        if !settled {
            return Err(Error::CombinationalLoop);
        }
        let o = self.io.core_output.sample(&self.circuit);
        if o.device_index != 0 && (o.device_read_enable || o.device_write_enable) {
            return Err(Error::UnsupportedDevice {
                device: o.device_index as u8,
                channel: o.device_channel as u8,
                writing: o.device_write_enable,
            });
        }
        let watch = self.io.system_output.sample(&self.circuit);
        if watch.watch_read_enable {
            return Err(Error::UnsupportedDevice {
                device: watch.watch_device_index as u8,
                channel: watch.watch_device_channel as u8,
                writing: false,
            });
        }
        self.circuit.clock_tick();
        self.cycle += 1;
        if let Some(e) = self.error.borrow_mut().take() {
            return Err(Error::NativeMemory(e));
        }
        Ok(o)
    }
}

#[cfg(test)]
mod flush_tests;
