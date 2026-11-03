//! Single-CB Device4 ABI with optional synchronous functional execution.
//!
//! The default control-only adapter leaves BUSY set until an external executor
//! takes and completes its submission. The functional adapter finishes actual
//! shared-memory work inside START, then retires through that same owner boundary.
use crate::{Device, Word};
use gpu_v2::system::functional::{Effects, ExecutionError, Executor};
pub use gpu_v2::system::vliw_command::DeviceSubmission;

pub const GPU_DEVICE: u8 = 4;
pub const GPU_COMMAND_COUNT: u8 = 0;
pub const GPU_COMMAND_START: u8 = 1;
pub const GPU_STATUS: u8 = 2;
pub const GPU_CAPABILITIES: u8 = 3;
pub const GPU_CAP_DEPTH_SHIFT: u16 = 1;
pub const GPU_STATUS_BUSY: u16 = 1;
pub const GPU_STATUS_TRANSPORT_ERROR: u16 = 2;

#[derive(Default)]
pub struct GpuDevice {
    count_minus_one: u16,
    staged: bool,
    busy: bool,
    transport_error: bool,
    pending: Option<DeviceSubmission>,
    executor: Option<Executor>,
    last_error: Option<ExecutionError>,
    last_effects: Effects,
}
impl GpuDevice {
    pub fn functional() -> Self {
        Self {
            executor: Some(Executor::default()),
            ..Self::default()
        }
    }
    /// Metadata only; framebuffer contents remain in shared physical memory.
    pub fn last_effects(&self) -> Effects {
        self.last_effects
    }

    pub fn last_error(&self) -> Option<&ExecutionError> {
        self.last_error.as_ref()
    }
    pub fn busy(&self) -> bool {
        self.busy
    }
    /// Transfer the accepted descriptor to a host GPU executor, without retiring it.
    pub fn take_submission(&mut self) -> Option<DeviceSubmission> {
        self.pending.take()
    }
    /// Called only after the executor and its memory writes have really drained.
    pub fn complete(&mut self, transport_error: bool) {
        assert!(
            self.busy && self.pending.is_none(),
            "GPU executor completion ownership"
        );
        self.transport_error |= transport_error;
        self.busy = false;
    }
}
impl Device for GpuDevice {
    fn read(&mut self, _memory: &mut [Word], channel: u8) -> Word {
        match channel {
            GPU_STATUS => u16::from(self.busy) | (u16::from(self.transport_error) << 1),
            GPU_CAPABILITIES => GPU_CAP_DEPTH_SHIFT,
            _ => 0,
        }
    }
    fn write(&mut self, memory: &mut [Word], channel: u8, value: Word) {
        match channel {
            GPU_COMMAND_COUNT => {
                assert!(!self.busy && value < 16384, "GPU count SEND contract");
                self.count_minus_one = value;
                self.staged = true;
            }
            GPU_COMMAND_START => {
                assert!(!self.busy && self.staged, "GPU start SEND contract");
                let descriptor = DeviceSubmission::new(
                    u32::from(value) * 128,
                    (u32::from(self.count_minus_one) + 1) * 4,
                )
                .expect("GPU submission range");
                self.pending = Some(descriptor);
                self.last_effects = Effects::default();
                self.busy = true;
                self.staged = false;
            }
            _ => {}
        }
        if self.executor.is_some() && self.pending.is_some() {
            let submission = self.take_submission().expect("accepted GPU submission");
            let result = self.executor.as_mut().unwrap().execute(memory, submission);
            self.last_effects = match &result {
                Ok(report) => report.effects,
                Err(error) => error.effects,
            };
            let failed = result.is_err();
            if let Err(error) = result {
                self.last_error = Some(error);
            }
            self.complete(failed);
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_later_submission_preserves_sticky_error_diagnostic() {
        let mut memory = vec![0u16; 4096];
        memory[1] = 0x3000; // DRAW without a loaded page.
        let mut device = GpuDevice::functional();
        device.write(&mut memory, GPU_COMMAND_COUNT, 0);
        device.write(&mut memory, GPU_COMMAND_START, 0);
        let diagnostic = device.last_error().cloned().unwrap();
        memory[..6].copy_from_slice(&[3, 0x4000, 0, 0x7000, 0, 0xf000]);
        device.write(&mut memory, GPU_COMMAND_COUNT, 2);
        device.write(&mut memory, GPU_COMMAND_START, 0);
        assert!(!device.busy());
        assert_eq!(
            device.read(&mut memory, GPU_STATUS),
            GPU_STATUS_TRANSPORT_ERROR
        );
        assert_eq!(device.last_error(), Some(&diagnostic));
        assert_eq!(device.last_effects(), Effects::default());
    }

    #[test]
    fn partial_failure_notifies_written_memory_and_target_before_error_completion() {
        use gpu_v2::{framebuffer::ports::MaterializedSurface, system::vliw_command as abi};
        let mut memory = vec![0xa55a; 0x400000];
        let target = abi::target_params(MaterializedSurface {
            color_base_bytes: 0x400000,
            depth_base_bytes: 0x480000,
            width: 400,
            height: 240,
        })
        .unwrap();
        for (out, bytes) in memory[0x48000 / 2..]
            .iter_mut()
            .zip(target.as_chunks::<2>().0)
        {
            *out = u16::from_le_bytes(*bytes);
        }
        let commands = [
            abi::Command::wait(false, true),
            abi::Command::FLUSH,
            abi::Command::dma_local(0x48000, 264, 32).unwrap(),
            abi::Command::wait(true, false),
            abi::Command::set_target(264).unwrap(),
            abi::Command::clear(false, 0x1357),
            abi::Command(0xd0000000),
            abi::Command::END,
        ];
        for (out, word) in memory[0x40000 / 2..]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(commands)
        {
            out[0] = word.0 as u16;
            out[1] = (word.0 >> 16) as u16;
        }
        let mut device = GpuDevice::functional();
        device.write(&mut memory, GPU_COMMAND_COUNT, (commands.len() - 1) as u16);
        device.write(&mut memory, GPU_COMMAND_START, (0x40000u32 / 128) as u16);
        assert!(!device.busy());
        assert_eq!(
            device.read(&mut memory, GPU_STATUS),
            GPU_STATUS_TRANSPORT_ERROR
        );
        assert_eq!(device.last_effects().target, Some([0x400000, 0x480000]));
        assert_eq!(
            device.last_effects().written_bytes,
            Some((0x400000, 0x432000))
        );
        assert_eq!(device.last_error().unwrap().effects, device.last_effects());
        assert_eq!(device.last_error().unwrap().byte_pc, 24);
        assert!(memory[0x400000 / 2..0x432000 / 2]
            .iter()
            .all(|&value| value == 0x1357));
        assert_eq!(memory[0x480000 / 2], 0xa55a);
    }
    #[test]
    fn functional_failure_retires_with_sticky_error_and_exact_diagnostic() {
        let mut device = GpuDevice::functional();
        let mut memory = vec![0; 64];
        memory[1] = 0x3000; // Unbound native DRAW must fail, never become a no-op.
        device.write(&mut memory, GPU_COMMAND_COUNT, 0);
        device.write(&mut memory, GPU_COMMAND_START, 0);
        assert!(!device.busy());
        assert!(device.take_submission().is_none());
        let error = device.last_error().unwrap();
        assert_eq!(error.byte_pc, 0);
        assert_eq!(error.word, 0x30000000);
        for _ in 0..4 {
            assert_eq!(
                device.read(&mut memory, GPU_STATUS),
                GPU_STATUS_TRANSPORT_ERROR
            );
        }
    }
    #[test]
    fn single_cb_lifetime_and_read_without_ack() {
        let mut device = GpuDevice::default();
        let mut memory = [];
        for error in [false, true] {
            device.write(&mut memory, GPU_COMMAND_COUNT, 92);
            assert_eq!(device.read(&mut memory, GPU_STATUS), 0);
            device.write(&mut memory, GPU_COMMAND_START, 0x800);
            for _ in 0..4 {
                assert_eq!(device.read(&mut memory, GPU_STATUS), 1);
            }
            let submission = device.take_submission().unwrap();
            assert_eq!(submission, DeviceSubmission::new(0x40000, 372).unwrap());
            assert!(device.busy());
            device.complete(error);
            for _ in 0..4 {
                assert_eq!(
                    device.read(&mut memory, GPU_STATUS),
                    if error { 2 } else { 0 }
                );
            }
        }
        assert_eq!(device.read(&mut memory, 0), 0);
    }
}
