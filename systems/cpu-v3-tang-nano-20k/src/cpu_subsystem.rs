//! Request-side CPU composition shared by legacy native and full-system roots.
//! This owner has no arbiter, memory controller, physical backing or device callbacks.
use crate::{
    SystemControlDevice, SystemControlDeviceInput, SystemControlDeviceOutput,
    SystemControlDeviceOutputValue, SystemControlDeviceState,
};
use cpu_v3::*;
use digital_design_circuit::{
    build_circuit, external, input, input_w, Circuit, CircuitWires, External, Wire, Wires,
};
use digital_design_hardware::{Module, ModuleIo};
use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

#[derive(Debug)]
pub enum Error {
    CombinationalLoop,
}

// Match the fitted self_test.v frontend boundary, including terminal states.
fn frontend_flush(system: &SystemControlDeviceOutput, core: &CpuV3CoreOutput) -> Wire {
    system.icache_invalidate | core.halted | core.fault
}

struct SharedModule<M: Module> {
    state: Rc<RefCell<M::EmuState>>,
    input: M::Input,
    output: M::Output,
}
impl<M: Module> External for SharedModule<M> {
    fn execute(&mut self, c: &mut CircuitWires) {
        M::execute_emu(&mut self.state.borrow_mut(), c, &self.input, &self.output);
    }
    fn clock(&mut self, c: &mut CircuitWires) {
        M::clock_emu(&mut self.state.borrow_mut(), c, &self.input, &self.output);
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub(crate) fn bits<const N: usize>(c: &mut CircuitWires, w: Wires<N>, v: u64) {
    for (k, b) in w.wires.iter().enumerate() {
        c.set_wire(*b, ((v >> k) & 1) as u8);
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InstructionResponse {
    pub request_ready: bool,
    pub response_valid: bool,
    pub read_data: u64,
    pub error: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DataResponse {
    pub request_ready: bool,
    pub write_data_ready: bool,
    pub response_valid: bool,
    pub read_data: u64,
    pub error: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub reset: bool,
    pub startup_hold: bool,
    pub memory_initialized: bool,
    pub instruction: InstructionResponse,
    pub data: DataResponse,
    /// Already selected external mux result, ORed with Device0's hardware read.
    pub external_device_read_data: u16,
    pub watch_read_data: u16,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub core: CpuV3CoreOutputValue,
    pub icache: CpuV3InstructionCacheOutputValue,
    pub dcache: CpuV3DataCacheOutputValue,
    pub system: SystemControlDeviceOutputValue,
}

pub(crate) struct States {
    pub core: Rc<RefCell<CpuV3CoreState>>,
    pub dcache: Rc<RefCell<CpuV3DataCacheState>>,
    pub system: Rc<RefCell<SystemControlDeviceState>>,
}
impl States {
    pub fn new(segment: u16) -> Self {
        Self {
            core: Rc::new(RefCell::new(CpuV3CoreState::with_initial_code_segment(
                segment,
            ))),
            dcache: Rc::new(RefCell::new(CpuV3DataCacheState::default())),
            system: Rc::new(RefCell::new(SystemControlDeviceState::default())),
        }
    }
}
// Boundary wires are supplied before registering any external module. Legacy
// roots can therefore retain their original single-Circuit memory feedback.
pub(crate) struct Boundary {
    pub reset: Wire,
    pub startup_hold: Wire,
    pub memory_initialized: Wire,
    pub instruction_request_ready: Wire,
    pub instruction_response_valid: Wire,
    pub instruction_read_data: Wires<64>,
    pub instruction_error: Wire,
    pub data_request_ready: Wire,
    pub data_write_data_ready: Wire,
    pub data_response_valid: Wire,
    pub data_read_data: Wires<64>,
    pub data_error: Wire,
    pub external_device_read_data: Wires<16>,
    pub watch_read_data: Wires<16>,
}
impl Boundary {
    fn allocate() -> Self {
        Self {
            reset: input(),
            startup_hold: input(),
            memory_initialized: input(),
            instruction_request_ready: input(),
            instruction_response_valid: input(),
            instruction_read_data: input_w(),
            instruction_error: input(),
            data_request_ready: input(),
            data_write_data_ready: input(),
            data_response_valid: input(),
            data_read_data: input_w(),
            data_error: input(),
            external_device_read_data: input_w(),
            watch_read_data: input_w(),
        }
    }
    fn drive(&self, c: &mut CircuitWires, i: &Input) {
        for (wire, value) in [
            (self.reset, i.reset),
            (self.startup_hold, i.startup_hold),
            (self.memory_initialized, i.memory_initialized),
            (self.instruction_request_ready, i.instruction.request_ready),
            (
                self.instruction_response_valid,
                i.instruction.response_valid,
            ),
            (self.instruction_error, i.instruction.error),
            (self.data_request_ready, i.data.request_ready),
            (self.data_write_data_ready, i.data.write_data_ready),
            (self.data_response_valid, i.data.response_valid),
            (self.data_error, i.data.error),
        ] {
            c.set_wire(wire, u8::from(value));
        }
        bits(c, self.instruction_read_data, i.instruction.read_data);
        bits(c, self.data_read_data, i.data.read_data);
        bits(
            c,
            self.external_device_read_data,
            u64::from(i.external_device_read_data),
        );
        bits(c, self.watch_read_data, u64::from(i.watch_read_data));
    }
}
pub(crate) struct Connected {
    pub core_input: CpuV3CoreInput,
    pub core_output: CpuV3CoreOutput,
    pub fetch_output: CpuV3InstructionFetchQueueOutput,
    pub icache_output: CpuV3InstructionCacheOutput,
    pub dcache_output: CpuV3DataCacheOutput,
    pub system_output: SystemControlDeviceOutput,
}
pub(crate) fn connect(boundary: &Boundary, states: &States) -> Connected {
    let mut core_input = CpuV3CoreInput::allocate();
    let core_output = CpuV3CoreOutput::allocate();
    let mut fetch_input = CpuV3InstructionFetchQueueInput::allocate();
    let fetch_output = CpuV3InstructionFetchQueueOutput::allocate();
    let mut icache_input = CpuV3InstructionCacheInput::allocate();
    let icache_output = CpuV3InstructionCacheOutput::allocate();
    let mut dcache_input = CpuV3DataCacheInput::allocate();
    let dcache_output = CpuV3DataCacheOutput::allocate();
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
    core_input.hold = dcache_output.valid_sweep
        | system_output.cpu_hold
        | !boundary.memory_initialized
        | boundary.startup_hold;

    icache_input.memory_request_ready = boundary.instruction_request_ready;
    icache_input.memory_response_valid = boundary.instruction_response_valid;
    icache_input.memory_read_data = boundary.instruction_read_data;
    icache_input.memory_error = boundary.instruction_error;
    dcache_input.memory_request_ready = boundary.data_request_ready;
    dcache_input.memory_write_data_ready = boundary.data_write_data_ready;
    dcache_input.memory_response_valid = boundary.data_response_valid;
    dcache_input.memory_read_data = boundary.data_read_data;
    dcache_input.memory_error = boundary.data_error;
    core_input.reset = boundary.reset;
    fetch_input.reset = boundary.reset;
    icache_input.reset = boundary.reset;
    dcache_input.reset = boundary.reset;
    system_input.reset = boundary.reset;
    system_input.device_index = core_output.device_index;
    system_input.device_channel = core_output.device_channel;
    system_input.device_read_enable = core_output.device_read_enable;
    system_input.device_write_enable = core_output.device_write_enable;
    system_input.device_write_data = core_output.device_write_data;
    core_input.device_read_data =
        system_output.device_read_data | boundary.external_device_read_data;
    system_input.dcache_maintenance_ready = dcache_output.maintenance_ready;
    system_input.dcache_maintenance_done = dcache_output.maintenance_done;
    system_input.dcache_maintenance_error = dcache_output.maintenance_error;
    dcache_input.invalidate_all = system_output.dcache_invalidate;
    dcache_input.clean_all = system_output.dcache_clean;
    icache_input.invalidate_all = system_output.icache_invalidate;
    fetch_input.flush = frontend_flush(&system_output, &core_output);
    system_input.watch_read_data = boundary.watch_read_data;
    external(SharedModule::<CpuV3Core> {
        state: states.core.clone(),
        input: core_input.clone(),
        output: core_output.clone(),
    });
    CpuV3InstructionFetchQueue::emu_connect(&fetch_input, &fetch_output);
    CpuV3InstructionCache::emu_connect(&icache_input, &icache_output);
    external(SharedModule::<CpuV3DataCache> {
        state: states.dcache.clone(),
        input: dcache_input.clone(),
        output: dcache_output.clone(),
    });
    external(SharedModule::<SystemControlDevice<469>> {
        state: states.system.clone(),
        input: system_input,
        output: system_output.clone(),
    });
    Connected {
        core_input,
        core_output,
        fetch_output,
        icache_output,
        dcache_output,
        system_output,
    }
}
impl Connected {
    fn sample(&self, c: &CircuitWires) -> Output {
        Output {
            core: self.core_output.sample(c),
            icache: self.icache_output.sample(c),
            dcache: self.dcache_output.sample(c),
            system: self.system_output.sample(c),
        }
    }
}
/// Persistent CPU/FPU/fetch/cache/SystemControl state, driven only by its root.
pub struct CpuSide {
    circuit: Circuit,
    boundary: Boundary,
    io: Connected,
    states: States,
}
impl CpuSide {
    pub fn new(initial_code_segment: u16) -> Self {
        let states = States::new(initial_code_segment);
        let (circuit, (boundary, io)) = build_circuit(|| {
            let boundary = Boundary::allocate();
            let io = connect(&boundary, &states);
            (boundary, io)
        });
        Self {
            circuit,
            boundary,
            io,
            states,
        }
    }
    /// Pure bounded combinational convergence. No state edge, MMIO or memory access.
    pub fn settle(&mut self, input: &Input) -> Result<Output, Error> {
        self.boundary.drive(&mut self.circuit, input);
        let mut previous = None;
        for _ in 0..8 {
            self.circuit.execute_gates();
            let output = self.io.sample(&self.circuit);
            let current = (
                self.io.core_input.sample(&self.circuit),
                self.io.fetch_output.sample(&self.circuit),
                output,
            );
            if previous.as_ref() == Some(&current) {
                return Ok(current.2);
            }
            previous = Some(current);
        }
        Err(Error::CombinationalLoop)
    }
    /// Submit exactly one logic edge using frozen inputs; return its pre-edge outputs.
    pub fn clock(&mut self, input: &Input) -> Result<Output, Error> {
        let output = self.settle(input)?;
        self.circuit.clock_tick();
        Ok(output)
    }
    pub fn core(&self) -> Ref<'_, CpuV3CoreState> {
        self.states.core.borrow()
    }
    /// Passive CPU-visible data inspection; never issues a modeled request.
    pub fn inspected_word(&self, address: PhysicalWordAddress) -> Option<u16> {
        self.states
            .core
            .borrow()
            .pending_scalar_store_word(address)
            .or_else(|| self.states.dcache.borrow().cached_word(address))
    }
    pub fn read_system_channel(&self, channel: u8) -> u16 {
        self.states.system.borrow().read_channel(channel)
    }
}

#[cfg(test)]
#[path = "cpu_subsystem_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cpu_frontend_tests.rs"]
mod frontend_tests;
