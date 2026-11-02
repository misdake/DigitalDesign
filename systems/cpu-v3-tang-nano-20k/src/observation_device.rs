//! Host-only Device7 observation sink; CPU reads retain the unattached value0.
use crate::{Device, Word};
/// Writes are diagnostics, not memory traffic or a source of CPU delay.
#[derive(Default)]
pub struct ObservationDevice {
    last: [u16; 16],
    writes: [u64; 16],
}
impl ObservationDevice {
    pub fn last(&self, channel: u8) -> Option<u16> {
        let i = usize::from(channel);
        (self.writes[i] != 0).then_some(self.last[i])
    }
    pub fn writes(&self, channel: u8) -> u64 {
        self.writes[usize::from(channel)]
    }
}
impl Device for ObservationDevice {
    fn read(&mut self, _: &mut [Word], _: u8) -> Word {
        0
    }
    fn write(&mut self, _: &mut [Word], channel: u8, value: Word) {
        let i = usize::from(channel);
        self.last[i] = value;
        self.writes[i] += 1;
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
