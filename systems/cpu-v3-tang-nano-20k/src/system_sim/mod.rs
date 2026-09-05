//! Functional Tang Nano 20K system simulator.
//!
//! This composes the architecture-only [`CpuV3Sim`] with board-level device
//! models. It deliberately does not model RTL timing, caches, or arbitration.

use crate::boot::{SystemControlDevice, SYSTEM_CONTROL_DEVICE};
use crate::display::render_framebuffer_at;
use crate::{
    CpuV3Sim, DisplayDevice, Fault, RunOutcome, StepOutcome, DISPLAY_DEVICE, FRAMEBUFFER_HEIGHT,
    FRAMEBUFFER_WIDTH,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum VBlankMode {
    #[default]
    Manual,
    AutoOnFrameIndexRead,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayState {
    pub active_base: u32,
    pub frame_index: u16,
    pub swap_pending: bool,
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
            Box::new(DisplayDevice::with_auto_vblank_on_frame_index_read(
                vblank_mode == VBlankMode::AutoOnFrameIndexRead,
            )),
        );
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
        self.cpu.run(maximum_steps)
    }

    pub fn advance_vblank(&mut self) {
        self.display().advance_frame();
    }

    pub fn display_state(&self) -> DisplayState {
        let display = self.display();
        DisplayState {
            active_base: display.active_base(),
            frame_index: display.frame_index(),
            swap_pending: display.swap_pending(),
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::{CACHE_MAINTENANCE_STATUS, CACHE_MAINTENANCE_STATUS_SUCCESS};
    use crate::{
        branch, compare_unsigned, device_receive, device_send, halt, PhysicalWordAddress,
        TestCondition, DISPLAY_CONTROL, DISPLAY_FRAMEBUFFER_HIGH, DISPLAY_FRAMEBUFFER_LOW,
        DISPLAY_FRAME_INDEX, DISPLAY_NEXT_SWAP, FRAMEBUFFER_A_BASE_WORD, FRAMEBUFFER_B_BASE_WORD,
    };

    #[test]
    fn system_devices_are_attached() {
        let sim = CpuV3SystemSim::default();
        assert!(sim
            .cpu()
            .device::<SystemControlDevice>(SYSTEM_CONTROL_DEVICE)
            .is_some());
        assert!(sim.cpu().device::<DisplayDevice>(DISPLAY_DEVICE).is_some());
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
            }
        );
        sim.advance_vblank();
        assert_eq!(sim.display_state().active_base, FRAMEBUFFER_B_BASE_WORD);
        assert_eq!(sim.display_state().frame_index, 1);
        assert!(!sim.display_state().swap_pending);
    }

    #[test]
    fn automatic_mode_advances_after_frame_index_reads() {
        let mut sim = CpuV3SystemSim::new(VBlankMode::AutoOnFrameIndexRead);
        sim.cpu_mut()
            .load_program(
                0,
                &[
                    device_receive(1, DISPLAY_DEVICE, DISPLAY_FRAME_INDEX),
                    device_receive(2, DISPLAY_DEVICE, DISPLAY_FRAME_INDEX),
                    compare_unsigned(2, 1),
                    branch(TestCondition::Equal, -3),
                    halt(),
                ],
            )
            .unwrap();
        assert!(matches!(
            sim.run(5),
            Ok(RunOutcome::Halted { steps: 5, .. })
        ));
        assert_eq!(sim.cpu().register(1), Some(0));
        assert_eq!(sim.cpu().register(2), Some(1));
        assert_eq!(sim.display_state().frame_index, 2);
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
        assert_eq!(pixels.len(), 320 * 240);
        assert_eq!(pixels[0], 0xff0000);
        assert_eq!(pixels[1], 0x00ff00);
    }
}
