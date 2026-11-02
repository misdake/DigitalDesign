//! Persistent CPU/cache logic emulators with fitted native SDRAM edges.
//! Device0 clocks its hardware model; external functional devices share physical RAM.
use crate::input_device::{ControllerSnapshot, InputSnapshotDevice, INPUT_DEVICE};
use crate::system_sim::{DisplayState, VBlankMode};
use crate::{
    CpuV3MemoryArbiter, CpuV3MemoryArbiterInput, CpuV3MemoryArbiterOutput, SystemControlDevice,
    SystemControlDeviceInput, SystemControlDeviceOutput, SystemControlDeviceState,
};
use crate::{DisplayDevice, GpuDevice, DISPLAY_DEVICE, GPU_DEVICE};
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

pub use cpu_v3::DeviceWrite;

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
    external_read_data: Wires<16>,
    watch_read_data: Wires<16>,
}
/// One machine with persistent CPU/cache/native state and one physical backing.
pub struct CpuV3SystemEmu {
    circuit: Circuit,
    io: Io,
    backing: Rc<RefCell<Vec<u16>>>,
    port: Rc<RefCell<Port>>,
    error: Rc<RefCell<Option<memory::Error>>>,
    cycle: u64,
    core_state: Rc<RefCell<CpuV3CoreState>>,
    system_state: Rc<RefCell<SystemControlDeviceState>>,
    devices: [Option<Box<dyn Device>>; 8],
    vblank_mode: VBlankMode,
}
impl CpuV3SystemEmu {
    /// Preloaded physical image starting at reset PC/CSEG0; not Flash boot.
    pub fn new(backing: Vec<u16>) -> Self {
        Self::with_initial_code_segment(backing, 0)
    }
    fn with_initial_code_segment(backing: Vec<u16>, segment: u16) -> Self {
        assert_eq!(backing.len(), 1 << 22, "fitted SDRAM has 4M 16-bit words");
        let backing = Rc::new(RefCell::new(backing));
        let port = Rc::new(RefCell::new(Port::default()));
        let error = Rc::new(RefCell::new(None));
        let core_state = Rc::new(RefCell::new(CpuV3CoreState::with_initial_code_segment(
            segment,
        )));
        let system_state = Rc::new(RefCell::new(SystemControlDeviceState::default()));
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
            let external_read_data = digital_design_circuit::input_w::<16>();
            let watch_read_data = system_input.watch_read_data;
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
            core_input.device_read_data = system_output.device_read_data | external_read_data;
            system_input.dcache_maintenance_ready = dcache_output.maintenance_ready;
            system_input.dcache_maintenance_done = dcache_output.maintenance_done;
            system_input.dcache_maintenance_error = dcache_output.maintenance_error;
            dcache_input.invalidate_all = system_output.dcache_invalidate;
            dcache_input.clean_all = system_output.dcache_clean;
            icache_input.invalidate_all = system_output.icache_invalidate;
            fetch_input.flush = frontend_flush(&system_output, &core_output);
            external(SharedModule::<CpuV3Core> {
                state: core_state.clone(),
                input: core_input.clone(),
                output: core_output.clone(),
            });
            CpuV3InstructionFetchQueue::emu_connect(&fetch_input, &fetch_output);
            CpuV3InstructionCache::emu_connect(&icache_input, &icache_output);
            CpuV3DataCache::emu_connect(&dcache_input, &dcache_output);
            CpuV3MemoryArbiter::emu_connect(&arbiter_input, &arbiter_output);
            external(SharedModule::<SystemControlDevice<469>> {
                state: system_state.clone(),
                input: system_input,
                output: system_output.clone(),
            });
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
                external_read_data,
                watch_read_data,
            }
        });
        Self {
            circuit,
            io,
            backing,
            port,
            error,
            cycle: 0,
            core_state,
            system_state,
            devices: std::array::from_fn(|_| None),
            vblank_mode: VBlankMode::Manual,
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
    /// Install the same CB06 COPY/ZERO package used by instruction simulation.
    /// Loading is functional; runtime starts at fitted CSEG7 with cold caches/MC.
    pub fn from_single_flash(
        package: &[u8],
        memory_seed: u16,
        vblank_mode: VBlankMode,
    ) -> Result<Self, crate::boot::SingleBootError> {
        let mut backing = vec![memory_seed; 1 << 22];
        crate::boot::install_single_application(package, &mut backing)?;
        let mut machine = Self::with_initial_code_segment(backing, 7);
        machine.vblank_mode = vblank_mode;
        machine.attach_device(
            DISPLAY_DEVICE,
            Box::new(DisplayDevice::with_pause_on_frame_index_wait(
                vblank_mode == VBlankMode::PauseOnFrameIndexWait,
            )),
        );
        machine.attach_device(GPU_DEVICE, Box::<GpuDevice>::default());
        machine.attach_device(INPUT_DEVICE, Box::<InputSnapshotDevice>::default());
        machine.attach_device(7, Box::<crate::ObservationDevice>::default());
        Ok(machine)
    }
    pub fn attach_device(&mut self, index: u8, device: Box<dyn Device>) {
        assert!(
            index > 0 && index < 8,
            "Device0 remains the clocked hardware owner"
        );
        self.devices[usize::from(index)] = Some(device);
    }
    pub fn device<T: 'static>(&self, index: u8) -> Option<&T> {
        self.devices
            .get(usize::from(index))?
            .as_deref()?
            .as_any()
            .downcast_ref()
    }
    pub fn with_functional_gpu(mut self) -> Self {
        assert!(!self.gpu().busy(), "cannot replace active GPU executor");
        self.attach_device(GPU_DEVICE, Box::new(GpuDevice::functional()));
        self
    }
    pub fn core(&self) -> Ref<'_, CpuV3CoreState> {
        self.core_state.borrow()
    }
    pub fn gpu(&self) -> &GpuDevice {
        self.device(GPU_DEVICE)
            .expect("GPU device must remain attached")
    }
    fn display(&self) -> &DisplayDevice {
        self.device(DISPLAY_DEVICE)
            .expect("Display device must remain attached")
    }
    pub fn publish_input(&mut self, input: Option<ControllerSnapshot>) {
        self.device::<InputSnapshotDevice>(INPUT_DEVICE)
            .expect("input device must remain attached")
            .publish(input);
    }
    pub fn advance_vblank(&mut self) -> bool {
        self.display().advance_frame()
    }
    pub fn waiting_for_vblank(&self) -> bool {
        self.display().waiting_for_vblank()
            || (self.vblank_mode == VBlankMode::PauseOnFrameIndexWait
                && (self.display().swap_pending()
                    || self
                        .system_state
                        .borrow()
                        .active_watch()
                        .is_some_and(|(d, c, e)| {
                            d == DISPLAY_DEVICE
                                && c == crate::DISPLAY_FRAME_INDEX
                                && self.display().frame_index() == e
                        })))
    }
    pub fn display_state(&self) -> DisplayState {
        let d = self.display();
        DisplayState {
            active_base: d.active_base(),
            frame_index: d.frame_index(),
            swap_pending: d.swap_pending(),
            waiting_for_vblank: self.waiting_for_vblank(),
        }
    }
    pub fn render_active_framebuffer(&self) -> Vec<u32> {
        let d = self.display();
        if d.front_valid() {
            crate::display::render_framebuffer_words_at(
                &self.physical_memory(),
                d.active_base(),
                d.linear_rgb565(),
            )
        } else {
            vec![0; crate::FRAMEBUFFER_WIDTH as usize * crate::FRAMEBUFFER_HEIGHT as usize]
        }
    }
    pub fn render_frame(&self) -> Vec<u32> {
        let d = self.display();
        let config = &crate::display::ACTIVE_DISPLAY_CONFIG;
        if d.front_valid() {
            crate::display::render_frame_words_at_config(
                config,
                &self.physical_memory(),
                d.active_base(),
                d.linear_rgb565(),
            )
        } else {
            vec![0; config.hdmi_width * config.hdmi_height]
        }
    }
    fn read_device(&mut self, index: u8, channel: u8) -> Result<u16, Error> {
        if index == 0 {
            return Ok(self.system_state.borrow().read_channel(channel));
        }
        let d = self
            .devices
            .get_mut(usize::from(index))
            .and_then(Option::as_mut)
            .ok_or(Error::UnsupportedDevice {
                device: index,
                channel,
                writing: false,
            })?;
        Ok(d.read(&mut self.backing.borrow_mut(), channel))
    }
    fn write_device(&mut self, index: u8, channel: u8, value: u16) -> Result<(), Error> {
        let d = self
            .devices
            .get_mut(usize::from(index))
            .and_then(Option::as_mut)
            .ok_or(Error::UnsupportedDevice {
                device: index,
                channel,
                writing: true,
            })?;
        d.write(&mut self.backing.borrow_mut(), channel, value);
        Ok(())
    }
    fn settle(&mut self) -> Result<(), Error> {
        let mut previous = None;
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
                return Ok(());
            }
            previous = Some(current);
        }
        Err(Error::CombinationalLoop)
    }
    /// Device side effects occur once after pure convergence, before this edge commits.
    pub fn step(&mut self) -> Result<CpuV3CoreOutputValue, Error> {
        Ok(self
            .step_observing(|_, _| true)?
            .expect("unconditional step advances"))
    }
    /// Return None to pause before an external write, without advancing any clock.
    /// The callback borrows physical DRAM only; dirty CPU data stays in its cache.
    pub fn step_observing(
        &mut self,
        mut before_write: impl FnMut(DeviceWrite, &[u16]) -> bool,
    ) -> Result<Option<CpuV3CoreOutputValue>, Error> {
        self.circuit
            .set_wire(self.io.reset, (self.cycle < 2).into());
        bits(&mut self.circuit, self.io.external_read_data, 0);
        bits(&mut self.circuit, self.io.watch_read_data, 0);
        self.settle()?;
        let watch = self.io.system_output.sample(&self.circuit);
        if watch.watch_read_enable {
            let value = self.read_device(
                watch.watch_device_index as u8,
                watch.watch_device_channel as u8,
            )?;
            bits(&mut self.circuit, self.io.watch_read_data, u64::from(value));
        }
        let o = self.io.core_output.sample(&self.circuit);
        if o.device_index != 0 && o.device_read_enable {
            let value = self.read_device(o.device_index as u8, o.device_channel as u8)?;
            bits(
                &mut self.circuit,
                self.io.external_read_data,
                u64::from(value),
            );
            self.settle()?;
        }
        let o = self.io.core_output.sample(&self.circuit);
        if o.device_index != 0 && o.device_write_enable {
            let write = DeviceWrite {
                device: o.device_index as u8,
                channel: o.device_channel as u8,
                value: o.device_write_data as u16,
            };
            if self.devices[usize::from(write.device)].is_none() {
                return Err(Error::UnsupportedDevice {
                    device: write.device,
                    channel: write.channel,
                    writing: true,
                });
            }
            let advance = {
                let memory = self.backing.borrow();
                before_write(write, &memory)
            };
            if !advance {
                return Ok(None);
            }
            self.write_device(
                o.device_index as u8,
                o.device_channel as u8,
                o.device_write_data as u16,
            )?;
        }
        self.circuit.clock_tick();
        self.cycle += 1;
        if let Some(e) = self.error.borrow_mut().take() {
            return Err(Error::NativeMemory(e));
        }
        Ok(Some(o))
    }
}

#[cfg(test)]
mod flush_tests;
