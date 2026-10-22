//! Single-CB Device4 ABI and a control-only host adapter.
//!
//! Rendering is tested by the production RTL harness. This host adapter never
//! pretends to execute GPU work: an attached executor must take the descriptor
//! and report actual completion; otherwise BUSY remains set.
use crate::{Device, Word};
pub use gpu_v2::system::vliw_command::DeviceSubmission;

pub const GPU_DEVICE: u8 = 4;
pub const GPU_COMMAND_COUNT: u8 = 0;
pub const GPU_COMMAND_START: u8 = 1;
pub const GPU_STATUS: u8 = 2;
pub const GPU_STATUS_BUSY: u16 = 1;
pub const GPU_STATUS_TRANSPORT_ERROR: u16 = 2;

#[derive(Default)]
pub struct GpuDevice {
    count_minus_one: u16,
    staged: bool,
    busy: bool,
    transport_error: bool,
    pending: Option<DeviceSubmission>,
}
impl GpuDevice {
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
        if channel == GPU_STATUS {
            u16::from(self.busy) | (u16::from(self.transport_error) << 1)
        } else {
            0
        }
    }
    fn write(&mut self, _memory: &mut [Word], channel: u8, value: Word) {
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
                self.busy = true;
                self.staged = false;
            }
            _ => {}
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
