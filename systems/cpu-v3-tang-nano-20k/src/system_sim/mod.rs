//! Functional Tang Nano 20K system simulator.
//!
//! This composes the architecture-only [`CpuV3Sim`] with board-level device
//! models. It deliberately does not model RTL timing, caches, or arbitration.

use crate::boot::{SystemControlDevice, SYSTEM_CONTROL_DEVICE};
use crate::display::render_framebuffer_at;
use crate::{
    CpuV3Sim, DisplayDevice, Fault, GpuDevice, RunOutcome, StepOutcome, DISPLAY_DEVICE,
    FRAMEBUFFER_HEIGHT, FRAMEBUFFER_WIDTH, GPU_DEVICE,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VBlankMode {
    /// Frame-index reads never pause the CPU; the host advances vblank explicitly.
    #[default]
    Manual,
    /// A repeated frame-index read marks a wait that a host event must release.
    PauseOnFrameIndexWait,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayState {
    pub active_base: u32,
    pub frame_index: u16,
    pub swap_pending: bool,
    /// The CPU observed the same frame index again and is waiting for a host vblank.
    pub waiting_for_vblank: bool,
}

pub struct CpuV3SystemSim {
    cpu: CpuV3Sim,
    vblank_mode: VBlankMode,
}

impl Default for CpuV3SystemSim {
    fn default() -> Self {
        Self::new(VBlankMode::Manual)
    }
}

impl CpuV3SystemSim {
    pub fn new(vblank_mode: VBlankMode) -> Self {
        let mut cpu = CpuV3Sim::default();
        cpu.attach_device(SYSTEM_CONTROL_DEVICE, Box::<SystemControlDevice>::default());
        cpu.attach_device(
            DISPLAY_DEVICE,
            Box::new(DisplayDevice::with_pause_on_frame_index_wait(
                vblank_mode == VBlankMode::PauseOnFrameIndexWait,
            )),
        );
        cpu.attach_device(GPU_DEVICE, Box::<GpuDevice>::default());
        Self { cpu, vblank_mode }
    }

    pub fn cpu(&self) -> &CpuV3Sim {
        &self.cpu
    }

    pub fn cpu_mut(&mut self) -> &mut CpuV3Sim {
        &mut self.cpu
    }

    pub fn vblank_mode(&self) -> VBlankMode {
        self.vblank_mode
    }

    pub fn step(&mut self) -> Result<StepOutcome, Fault> {
        self.cpu.step()
    }

    pub fn run(&mut self, maximum_steps: usize) -> Result<RunOutcome, Fault> {
        for steps in 0..maximum_steps {
            if self.waiting_for_vblank() {
                return Ok(RunOutcome::StepLimit { steps });
            }
            if let StepOutcome::Halted { signal } = self.step()? {
                return Ok(RunOutcome::Halted {
                    steps: steps + 1,
                    signal,
                });
            }
        }
        Ok(RunOutcome::StepLimit {
            steps: maximum_steps,
        })
    }

    /// Advances one display vblank and reports whether it published a swap.
    pub fn advance_vblank(&mut self) -> bool {
        self.display().advance_frame()
    }

    pub fn display_state(&self) -> DisplayState {
        let display = self.display();
        DisplayState {
            active_base: display.active_base(),
            frame_index: display.frame_index(),
            swap_pending: display.swap_pending(),
            waiting_for_vblank: display.waiting_for_vblank(),
        }
    }

    pub fn waiting_for_vblank(&self) -> bool {
        self.display().waiting_for_vblank()
            || (self.vblank_mode == VBlankMode::PauseOnFrameIndexWait
                && self
                    .cpu
                    .device::<SystemControlDevice>(SYSTEM_CONTROL_DEVICE)
                    .and_then(SystemControlDevice::active_watch)
                    .is_some_and(|(device, channel, expected)| {
                        device == DISPLAY_DEVICE
                            && channel == crate::DISPLAY_FRAME_INDEX
                            && self.display().frame_index() == expected
                    }))
    }

    pub fn render_active_framebuffer(&self) -> Vec<u32> {
        render_framebuffer_at(&self.cpu, self.display().active_base())
    }

    pub const fn framebuffer_dimensions() -> (usize, usize) {
        (FRAMEBUFFER_WIDTH as usize, FRAMEBUFFER_HEIGHT as usize)
    }

    fn display(&self) -> &DisplayDevice {
        self.cpu
            .device::<DisplayDevice>(DISPLAY_DEVICE)
            .expect("CpuV3SystemSim display device must remain attached")
    }

    /// Read-only access to the GPU device model, e.g. for test assertions.
    pub fn gpu(&self) -> &GpuDevice {
        self.cpu
            .device::<GpuDevice>(GPU_DEVICE)
            .expect("CpuV3SystemSim GPU device must remain attached")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::{CACHE_MAINTENANCE_STATUS, CACHE_MAINTENANCE_STATUS_SUCCESS};
    use crate::{
        device_receive, device_send, halt, PhysicalWordAddress, DISPLAY_CONTROL,
        DISPLAY_FRAMEBUFFER_HIGH, DISPLAY_FRAMEBUFFER_LOW, DISPLAY_FRAME_INDEX, DISPLAY_NEXT_SWAP,
        FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD,
    };

    #[test]
    fn system_devices_are_attached() {
        let sim = CpuV3SystemSim::default();
        assert!(sim
            .cpu()
            .device::<SystemControlDevice>(SYSTEM_CONTROL_DEVICE)
            .is_some());
        assert!(sim.cpu().device::<DisplayDevice>(DISPLAY_DEVICE).is_some());
        assert!(sim.cpu().device::<GpuDevice>(GPU_DEVICE).is_some());
    }

    #[test]
    fn cpu_stages_and_submits_a_gpu_command_buffer() {
        use crate::{
            GPU_CMD_BASE_HIGH, GPU_CMD_BASE_LOW, GPU_CMD_WORDS_HIGH, GPU_CMD_WORDS_LOW,
            GPU_EXECUTED_COUNT, GPU_OPCODE_END, GPU_OPCODE_FAKE_DRAW, GPU_OPCODE_SET_TARGET,
            GPU_SUBMIT, GPU_TILE_TOTAL,
        };
        let mut sim = CpuV3SystemSim::default();
        // Build the temporary command shell in physical memory. The FAKE_DRAW
        // is the three-qword framebuffer-cache form with a tile list that
        // covers the whole 375-tile framebuffer as a solid clear.
        let base = 0x100u32;
        let list_base = 0x400u32;
        let tile_count = GPU_TILE_TOTAL as u16;
        let list: Vec<u16> = (0..tile_count).collect();
        let fake_arg0 = u64::from(tile_count) | (1u64 << 16); // CLEAR
        let qwords: [u64; 5] = [
            GPU_OPCODE_SET_TARGET as u64 | (1u64 << 8) | (u64::from(FRAMEBUFFER_A_BASE_WORD) << 32),
            GPU_OPCODE_FAKE_DRAW as u64 | (3u64 << 8) | (fake_arg0 << 32),
            u64::from(list_base),
            0xffffu64 << 32, // clear 0, draw 0, every tile row selected
            GPU_OPCODE_END as u64 | (1u64 << 8),
        ];
        {
            let memory = sim.cpu_mut().physical_memory_mut();
            let mut cursor = base as usize;
            for qword in qwords {
                memory[cursor] = qword as u16;
                memory[cursor + 1] = (qword >> 16) as u16;
                memory[cursor + 2] = (qword >> 32) as u16;
                memory[cursor + 3] = (qword >> 48) as u16;
                cursor += 4;
            }
            for (offset, index) in list.iter().enumerate() {
                memory[list_base as usize + offset] = *index;
            }
        }
        let mut words = Vec::new();
        words.extend(crate::load_immediate16(1, base as u16));
        words.extend(crate::load_immediate16(2, (base >> 16) as u16));
        words.extend(crate::load_immediate16(3, (qwords.len() * 4) as u16));
        words.extend(crate::load_immediate16(4, 0));
        words.extend(crate::load_immediate16(5, 0));
        words.push(device_send(1, GPU_DEVICE, GPU_CMD_BASE_LOW));
        words.push(device_send(2, GPU_DEVICE, GPU_CMD_BASE_HIGH));
        words.push(device_send(3, GPU_DEVICE, GPU_CMD_WORDS_LOW));
        words.push(device_send(4, GPU_DEVICE, GPU_CMD_WORDS_HIGH));
        words.push(device_send(5, GPU_DEVICE, GPU_SUBMIT));
        // Arm a generic watch on GPU executed_count == 0. While CPU
        // retirement is held, the system model keeps probing the GPU, which
        // advances the autonomous renderer until the value changes.
        words.extend(crate::load_immediate16(
            7,
            crate::boot::sysctl_watch_target(GPU_DEVICE, GPU_EXECUTED_COUNT),
        ));
        words.extend(crate::load_immediate16(8, 0));
        words.push(device_send(
            7,
            SYSTEM_CONTROL_DEVICE,
            crate::boot::SYSCTL_WATCH_TARGET,
        ));
        words.push(device_send(
            8,
            SYSTEM_CONTROL_DEVICE,
            crate::boot::SYSCTL_WATCH_EXPECTED,
        ));
        words.push(device_receive(6, GPU_DEVICE, GPU_EXECUTED_COUNT));
        words.push(halt());
        sim.cpu_mut().load_program(0, &words).unwrap();
        assert!(
            matches!(sim.run(2_000_000), Ok(RunOutcome::Halted { .. })),
            "GPU device watch did not finish"
        );
        assert_eq!(sim.cpu().register(6), Some(1), "executed_count");
        assert_eq!(sim.gpu().executed_count(), 1);
        assert_eq!(sim.gpu().received_count(), 1);
        assert!(!sim.gpu().command_error());
        // The solid tile at (0, 0) is black for phase 0, biases 0.
        let slot = FRAMEBUFFER_A_BASE_WORD as usize;
        assert_eq!(sim.cpu_mut().physical_memory_mut()[slot], 0);
        assert_eq!(sim.cpu_mut().physical_memory_mut()[slot + 16], 0);
    }

    #[test]
    fn manual_vblank_applies_a_pending_swap() {
        let mut sim = CpuV3SystemSim::default();
        let program = [
            device_send(1, DISPLAY_DEVICE, DISPLAY_FRAMEBUFFER_LOW),
            device_send(2, DISPLAY_DEVICE, DISPLAY_FRAMEBUFFER_HIGH),
            device_send(3, DISPLAY_DEVICE, DISPLAY_CONTROL),
            halt(),
        ];
        let mut words = Vec::new();
        words.extend(crate::load_immediate16(1, FRAMEBUFFER_B_BASE_WORD as u16));
        words.extend(crate::load_immediate16(
            2,
            (FRAMEBUFFER_B_BASE_WORD >> 16) as u16,
        ));
        words.extend(crate::load_immediate16(3, DISPLAY_NEXT_SWAP));
        words.extend(program);
        sim.cpu_mut().load_program(0, &words).unwrap();
        assert!(matches!(sim.run(16), Ok(RunOutcome::Halted { .. })));
        assert_eq!(
            sim.display_state(),
            DisplayState {
                active_base: FRAMEBUFFER_A_BASE_WORD,
                frame_index: 0,
                swap_pending: true,
                waiting_for_vblank: false,
            }
        );
        assert!(sim.advance_vblank());
        assert_eq!(sim.display_state().active_base, FRAMEBUFFER_B_BASE_WORD);
        assert_eq!(sim.display_state().frame_index, 1);
        assert!(!sim.display_state().swap_pending);
    }

    #[test]
    fn pause_mode_stops_at_a_frame_index_wait() {
        let mut sim = CpuV3SystemSim::new(VBlankMode::PauseOnFrameIndexWait);
        let mut program = Vec::new();
        program.extend(crate::load_immediate16(
            1,
            crate::boot::sysctl_watch_target(DISPLAY_DEVICE, DISPLAY_FRAME_INDEX),
        ));
        program.extend([
            device_send(1, SYSTEM_CONTROL_DEVICE, crate::boot::SYSCTL_WATCH_TARGET),
            device_send(2, SYSTEM_CONTROL_DEVICE, crate::boot::SYSCTL_WATCH_EXPECTED),
            halt(),
        ]);
        sim.cpu_mut().load_program(0, &program).unwrap();
        assert_eq!(sim.run(16), Ok(RunOutcome::StepLimit { steps: 4 }));
        assert_eq!(sim.cpu().register(1), Some(3));
        assert_eq!(sim.cpu().register(2), Some(0));
        assert!(sim.waiting_for_vblank());
        assert!(!sim.advance_vblank());
        assert!(matches!(sim.run(16), Ok(RunOutcome::Halted { .. })));
        assert_eq!(sim.display_state().frame_index, 1);
    }

    #[test]
    fn system_control_and_framebuffer_render_are_functional() {
        let mut sim = CpuV3SystemSim::default();
        sim.cpu_mut()
            .load_program(
                0,
                &[
                    device_receive(1, SYSTEM_CONTROL_DEVICE, CACHE_MAINTENANCE_STATUS),
                    halt(),
                ],
            )
            .unwrap();
        assert!(matches!(
            sim.run(2),
            Ok(RunOutcome::Halted { steps: 2, .. })
        ));
        assert_eq!(
            sim.cpu().register(1),
            Some(CACHE_MAINTENANCE_STATUS_SUCCESS)
        );

        let base = PhysicalWordAddress::new(FRAMEBUFFER_A_BASE_WORD);
        sim.cpu_mut().physical_memory_mut()[base.get() as usize] = 0xf800;
        sim.cpu_mut().physical_memory_mut()[base.get() as usize + 1] = 0x07e0;
        let pixels = sim.render_active_framebuffer();
        assert_eq!(
            pixels.len(),
            crate::FRAMEBUFFER_WIDTH as usize * crate::FRAMEBUFFER_HEIGHT as usize
        );
        assert_eq!(pixels[0], 0xff0000);
        assert_eq!(pixels[1], 0x00ff00);
    }
}
