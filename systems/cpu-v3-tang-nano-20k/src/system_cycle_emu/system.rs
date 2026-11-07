//! Fixed production composition. A scheduler instant is sampled before any owner commits.
use super::{
    clock::{Clock, Instant},
    input::InputPeer,
    memory::{CacheRequest, Clients, Sample, SharedMemory},
    request_slot::Slot,
    startup::{self, ColdGate, ImageReport, Release},
};
use crate::{
    cpu_subsystem::{self as wiring, CpuSide},
    hardware::display_cycle::{ControlInput, ControlOutput, DisplayControl, Edges},
};
use gpu_v2::system::vliw_cycle::{
    command::DeviceBus,
    draw::{Gpu, Input as GpuInput, Output as GpuOutput},
    postquad::Profile,
};
#[derive(Debug)]
pub enum Error {
    Boot(crate::boot::SingleBootError),
    Cpu(crate::cpu_subsystem::Error),
    Gpu(gpu_v2::system::vliw_cycle::draw::Error),
    Memory(digital_design_hardware_gowin::sdram_memory_controller::cycle::memory::Error),
    UnsupportedDevice { device: u8, channel: u8 },
    CombinationalLoop,
    DisplayStartup,
}
#[derive(Clone, Debug, Default)]
pub struct Statistics {
    pub cpu_retired_words: u64,
    pub gpu_submits: u64,
    pub gpu_completed: u64,
    pub present_requests: u64,
    pub presents_applied: u64,
    pub frame_events: u64,
    pub accepts: [u64; 7],
    pub wait_cycles: [u64; 7],
    pub response_beats: [u64; 7],
    pub terminal_responses: [u64; 7],
    pub write_beats: [u64; 7],
    pub three_way_pending: u64,
}
/// Passive old-state event surface consumed by the separate profiling recorder.
/// It contains no decoded CB replay or functional GPU/Display results.
#[derive(Clone, Copy)]
pub struct Observation<'a> {
    pub instant: Instant,
    /// Old-state scalar execute address; distinct from the fetch/output PC.
    pub executing_pc: Option<(u16, u16)>,
    /// Already-selected CPU read mux. Reading this value has no side effects.
    pub device_read_data: u16,
    /// Passive architectural GPR snapshot before this scheduler edge.
    /// Register meanings and calling conventions belong to the application.
    pub cpu_registers: [u16; 16],
    pub cpu: Option<&'a wiring::Output>,
    pub gpu: Option<&'a GpuOutput>,
    pub memory: Option<&'a Sample>,
    pub bus: Option<DeviceBus>,
    pub display: ControlOutput,
    pub startup_hold: bool,
    pub gpu_phase: u8,
}
pub struct Step {
    pub instant: Instant,
    pub cpu: Option<cpu_v3::CpuV3CoreOutputValue>,
    pub display: ControlOutput,
    pub gpu_commands: Option<gpu_v2::system::vliw_cycle::command::CommandEdge>,
    pub gpu_trace: Option<gpu_v2::system::vliw_cycle::trace::Trace>,
    pub native:
        Option<digital_design_hardware_gowin::sdram_memory_controller::cycle::memory::Activity>,
}
struct LogicSample {
    ci: wiring::Input,
    cpu: wiring::Output,
    gi: GpuInput,
    gpu: GpuOutput,
    di: ControlInput,
    display: ControlOutput,
    memory: Sample,
    bus: DeviceBus,
}
pub struct System {
    cpu: CpuSide,
    gpu: Gpu,
    display: DisplayControl,
    clock: Clock,
    memory: SharedMemory,
    slots: [Slot; 3],
    cold: ColdGate,
    inputs: InputPeer,
    pub image: ImageReport,
    pub release: Option<Release>,
    pub statistics: Statistics,
    observations: [Option<u16>; 16],
    first_fetch: Option<u32>,
    previous_gpu_busy: bool,
}
impl System {
    pub fn from_preloaded_cold(package: &[u8], seed: u16) -> Result<Self, Error> {
        let (backing, image) = startup::image(package, seed).map_err(Error::Boot)?;
        Ok(Self {
            cpu: CpuSide::new(7),
            gpu: Gpu::with_all_commands(Profile::Native),
            display: Default::default(),
            clock: Default::default(),
            memory: SharedMemory::new(backing),
            slots: Default::default(),
            cold: Default::default(),
            inputs: Default::default(),
            image,
            release: None,
            statistics: Default::default(),
            observations: [None; 16],
            first_fetch: None,
            previous_gpu_busy: false,
        })
    }
    pub fn backing(&self) -> &[u16] {
        self.memory.backing()
    }
    /// Host inspection of CPU-visible RAM, including pending store/cache data.
    /// No clock, cache replacement, MMIO read or memory transaction is issued.
    pub fn inspect_cpu_word(&self, address: u32) -> Option<u16> {
        if address >= self.backing().len() as u32 {
            return None;
        }
        let physical = cpu_v3::PhysicalWordAddress::new(address);
        self.cpu
            .inspected_word(physical)
            .or_else(|| self.backing().get(address as usize).copied())
    }
    pub fn clock(&self) -> &Clock {
        &self.clock
    }
    pub fn publish_input(&mut self, input: Option<crate::input_device::ControllerSnapshot>) {
        self.inputs.publish(input);
    }
    pub fn set_buttons(&mut self, buttons: u8) {
        self.inputs.buttons = buttons & 3;
    }
    pub fn native_cycles(&self) -> u64 {
        self.memory.native_cycles()
    }
    /// Declared preloaded boundary after the real cold prefix, not an imported
    /// SPI-loader checkpoint. The first unheld CPU edge has not yet committed.
    pub fn application_ready(&self) -> bool {
        self.cold.boot_done
    }
    pub fn first_fetch(&self) -> Option<u32> {
        self.first_fetch
    }
    pub fn observation(&self, channel: usize) -> Option<u16> {
        self.observations[channel]
    }
    pub fn core(&self) -> std::cell::Ref<'_, cpu_v3::CpuV3CoreState> {
        self.cpu.core()
    }
    fn display_input(&self) -> ControlInput {
        ControlInput {
            video_locked: self.cold.video_locked(),
            boot_enable: self.cold.boot_enable(),
            ..Default::default()
        }
    }
    pub fn display_register(&self, channel: u8) -> u16 {
        self.display
            .outputs(&ControlInput {
                device_index: 3,
                device_channel: channel,
                device_read_enable: true,
                ..self.display_input()
            })
            .logic
            .device_read_data
    }
    fn query(cpu: &wiring::Output) -> DeviceBus {
        let watch = cpu.system.watch_read_enable;
        DeviceBus {
            index: if watch {
                cpu.system.watch_device_index
            } else {
                cpu.core.device_index
            } as u8,
            channel: if watch {
                cpu.system.watch_device_channel
            } else {
                cpu.core.device_channel
            } as u8,
            read: cpu.core.device_read_enable || watch,
            write: cpu.core.device_write_enable && !watch,
            data: cpu.core.device_write_data as u16,
        }
    }
    fn external_read(
        &self,
        bus: DeviceBus,
        gpu: &GpuOutput,
        display: &ControlOutput,
    ) -> Result<u16, Error> {
        if !bus.read {
            return Ok(0);
        }
        match bus.index {
            3 => Ok(display.logic.device_read_data),
            4 => Ok(gpu.control.device_read_data),
            0 | 5 | 7 => Ok(self.inputs.read(bus.index, bus.channel).unwrap()),
            _ => Err(Error::UnsupportedDevice {
                device: bus.index,
                channel: bus.channel,
            }),
        }
    }
    fn sample_logic(&mut self) -> Result<LogicSample, Error> {
        let mut ci = wiring::Input {
            startup_hold: !self.cold.boot_done,
            memory_initialized: self.memory.initialized(),
            ..Default::default()
        };
        let mut gi = GpuInput {
            reset: !self.cold.cold_ready,
            ce: true,
            ..Default::default()
        };
        let mut di = self.display_input();
        let mut previous = None;
        for _ in 0..16 {
            let cpu = self.cpu.settle(&ci).map_err(Error::Cpu)?;
            let bus = Self::query(&cpu);
            if bus.write && !matches!(bus.index, 0 | 3 | 4 | 5 | 7) {
                return Err(Error::UnsupportedDevice {
                    device: bus.index,
                    channel: bus.channel,
                });
            }
            gi.bus = bus;
            di.device_index = bus.index;
            di.device_channel = bus.channel;
            di.device_read_enable = bus.read;
            di.device_write_enable = bus.write;
            di.device_write_data = bus.data;
            let gpu = self.gpu.outputs(gi);
            let display = self.display.outputs(&di);
            let clients = Clients {
                instruction: CacheRequest {
                    valid: cpu.icache.memory_request_valid,
                    address: cpu.icache.memory_address as u32,
                    response_ready: cpu.icache.memory_response_ready,
                    ..Default::default()
                },
                data: CacheRequest {
                    valid: cpu.dcache.memory_request_valid,
                    writing: cpu.dcache.memory_write,
                    line: cpu.dcache.memory_line,
                    address: cpu.dcache.memory_address as u32,
                    write_data: cpu.dcache.memory_write_data,
                    response_ready: cpu.dcache.memory_response_ready,
                },
                gpu: self.slots,
                gpu_write_data: gpu.write_data,
                display_valid: display.logic.memory_request_valid,
                display_address: display.logic.memory_address,
            };
            let memory = self.memory.sample(clients, false);
            let f = memory.feedback;
            ci.instruction = wiring::InstructionResponse {
                request_ready: f.instruction.request_ready,
                response_valid: f.instruction.response_valid,
                read_data: f.instruction.read_data,
                error: f.instruction.error,
            };
            ci.data = wiring::DataResponse {
                request_ready: f.data.request_ready,
                write_data_ready: f.data.write_data_ready,
                response_valid: f.data.response_valid,
                read_data: f.data.read_data,
                error: f.data.error,
            };
            let external = self.external_read(bus, &gpu, &display)?;
            ci.external_device_read_data = external;
            ci.watch_read_data = if cpu.system.watch_read_enable {
                external
                    | if bus.index == 0 {
                        self.cpu.read_system_channel(bus.channel)
                    } else {
                        0
                    }
            } else {
                0
            };
            gi.physical = f.gpu;
            for (n, p) in gi.physical.iter_mut().enumerate() {
                p.request_ready =
                    self.slots[n].ready(!self.cold.cold_ready || !self.cold.boot_done);
            }
            di.memory_request_ready = f.display.request_ready;
            di.memory_data_valid = f.display.response.valid;
            di.memory_read_data = f.display.response.data;
            di.memory_last = f.display.response.last;
            di.memory_error = f.display.response.error;
            let key = (ci, gi.physical, f.display);
            if previous == Some(key) {
                return Ok(LogicSample {
                    ci,
                    cpu,
                    gi,
                    gpu,
                    di,
                    display,
                    memory,
                    bus,
                });
            }
            previous = Some(key);
        }
        Err(Error::CombinationalLoop)
    }
    fn statistics(&mut self, s: &LogicSample) {
        let c = &s.cpu;
        let a = &s.memory.arbiter;
        let f = s.memory.feedback;
        let valid = [
            s.display.logic.memory_request_valid,
            c.icache.memory_request_valid,
            c.dcache.memory_request_valid,
            false,
            self.slots[0].request.valid,
            self.slots[1].request.valid,
            self.slots[2].request.valid,
        ];
        let ready = [
            a.display_request_ready,
            a.instruction_request_ready,
            a.data_request_ready,
            a.dma_request_ready,
            a.gpu_ro_request_ready,
            a.gpu_fb_r_request_ready,
            a.gpu_fb_w_request_ready,
        ];
        let responses = [
            a.display_response_valid,
            a.instruction_response_valid && c.icache.memory_response_ready,
            a.data_response_valid && c.dcache.memory_response_ready,
            false,
            a.gpu_ro_response_valid,
            a.gpu_fb_r_response_valid,
            a.gpu_fb_w_response_valid,
        ];
        let terminal = s.memory.endpoint.cpu_response_valid
            && s.memory.endpoint.cpu_response_last
            && a.memory_response_ready;
        debug_assert!(!terminal || responses.iter().filter(|&&v| v).count() == 1);
        for n in 0..7 {
            self.statistics.accepts[n] += u64::from(valid[n] && ready[n]);
            self.statistics.wait_cycles[n] += u64::from(valid[n] && !ready[n]);
            self.statistics.response_beats[n] += u64::from(responses[n]);
            self.statistics.terminal_responses[n] += u64::from(terminal && responses[n]);
        }
        self.statistics.write_beats[2] += u64::from(a.data_write_data_ready);
        self.statistics.write_beats[6] += u64::from(a.gpu_fb_w_write_data_ready);
        self.statistics.three_way_pending +=
            u64::from((valid[1] || valid[2]) && (valid[4] || valid[5] || valid[6]) && valid[0]);
        self.statistics.cpu_retired_words = c.core.retired_words;
        self.statistics.gpu_submits +=
            u64::from(s.bus.write && s.bus.index == 4 && s.bus.channel == 1);
        self.statistics.gpu_completed +=
            u64::from(self.previous_gpu_busy && !s.gpu.control.processor.busy);
        self.previous_gpu_busy = s.gpu.control.processor.busy;
        self.statistics.present_requests +=
            u64::from(s.bus.write && s.bus.index == 3 && s.bus.channel == 3 && s.bus.data == 1);
        self.statistics.frame_events += u64::from(s.display.logic.frame_event);
        if self.first_fetch.is_none()
            && c.icache.memory_request_valid
            && f.instruction.request_ready
        {
            self.first_fetch = Some(c.icache.memory_address as u32);
        }
    }
    pub fn step(&mut self) -> Result<Step, Error> {
        self.step_inner(false, |_| {})
    }
    pub fn step_observing(&mut self, observe: impl FnMut(Observation<'_>)) -> Result<Step, Error> {
        self.step_inner(true, observe)
    }
    fn step_inner(
        &mut self,
        observing: bool,
        mut observe: impl FnMut(Observation<'_>),
    ) -> Result<Step, Error> {
        let e = self.clock.peek();
        let logic = if e.logic_rise {
            Some(self.sample_logic()?)
        } else {
            None
        };
        let di = logic
            .as_ref()
            .map_or_else(|| self.display_input(), |s| s.di);
        let display = logic
            .as_ref()
            .map_or_else(|| self.display.outputs(&di), |s| s.display);
        observe(Observation {
            instant: e,
            executing_pc: self.cpu.core().executing_instruction_address(),
            device_read_data: logic.as_ref().map_or(0, |s| {
                (s.cpu.system.device_read_data as u16) | s.ci.external_device_read_data
            }),
            cpu_registers: self.cpu.core().architectural_gprs(),
            cpu: logic.as_ref().map(|s| &s.cpu),
            gpu: logic.as_ref().map(|s| &s.gpu),
            memory: logic.as_ref().map(|s| &s.memory),
            bus: logic.as_ref().map(|s| s.bus),
            display,
            startup_hold: logic.as_ref().is_some_and(|s| s.ci.startup_hold),
            gpu_phase: self.gpu.control.processor.phase() as u8,
        });
        let mut cpu_out = None;
        let mut gpu_commands = None;
        let mut gpu_trace = None;
        let old_pending = self.display_register(3) & 1 != 0;
        let old_ready = self.cold.cold_ready;
        let old_done = self.cold.boot_done;
        let memory_initialized = self.memory.initialized();
        let cache_ready = logic.as_ref().is_none_or(|s| !s.cpu.dcache.valid_sweep);
        if let Some(s) = logic {
            if self.release.is_none() && !s.ci.startup_hold {
                self.release = Some(Release {
                    time_ticks: e.ticks,
                    logic_edges: self.clock.logic_edges,
                    native_cycles: self.memory.native_cycles(),
                    pixel_edges: self.clock.pixel_edges,
                    system_milliseconds: self.cpu.read_system_channel(10),
                    system_microseconds: self.cpu.read_system_channel(11),
                });
            }
            self.statistics(&s);
            let actual = self.cpu.clock(&s.ci).map_err(Error::Cpu)?;
            debug_assert_eq!(actual, s.cpu);
            cpu_out = Some(s.cpu.core.clone());
            if observing {
                let gpu_edge = self.gpu.clock_observing(s.gi).map_err(Error::Gpu)?;
                gpu_commands = Some(gpu_edge.command);
                gpu_trace = Some(gpu_edge.trace);
            } else {
                self.gpu.clock(s.gi).map_err(Error::Gpu)?;
            }
            let ready = [
                s.memory.arbiter.gpu_ro_request_ready,
                s.memory.arbiter.gpu_fb_r_request_ready,
                s.memory.arbiter.gpu_fb_w_request_ready,
            ];
            for (n, slot) in self.slots.iter_mut().enumerate() {
                slot.clock(!old_ready || !old_done, s.gpu.physical[n], n == 2, ready[n]);
            }
            if s.bus.write && s.bus.index == 7 {
                self.observations[usize::from(s.bus.channel & 15)] = Some(s.bus.data);
            }
            self.inputs.clock(s.bus.index, s.bus.channel, s.bus.write);
            self.memory.commit_logic(s.memory);
        }
        self.display.clock(
            Edges {
                logic_rise: e.logic_rise,
                pixel_rise: e.pixel_rise,
            },
            &di,
        );
        self.cold.clock(
            e.logic_rise,
            e.pixel_rise,
            memory_initialized,
            display.logic.started_ack,
            cache_ready,
        );
        if e.logic_rise && old_pending && self.display_register(3) & 1 == 0 {
            self.statistics.presents_applied += 1;
        }
        let native = if e.native_half {
            Some(
                self.memory
                    .clock_native_half_observing()
                    .map_err(Error::Memory)?,
            )
        } else {
            None
        };
        if display.logic.startup_error {
            return Err(Error::DisplayStartup);
        }
        assert_eq!(self.clock.advance(), e);
        Ok(Step {
            gpu_commands,
            gpu_trace,
            native,
            instant: e,
            cpu: cpu_out,
            display,
        })
    }
}
