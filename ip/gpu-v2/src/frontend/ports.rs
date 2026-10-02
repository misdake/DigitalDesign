use crate::{command_processor::ports::Command, vertex::ports::Transformed};
#[derive(Clone, Debug)]
pub struct Input {
    pub commands: Vec<Command>,
    pub memory_base: u64,
    pub memory: Vec<u8>,
}
impl Input {
    pub fn read_beat(&self, address: u64) -> Result<u64, String> {
        let offset = usize::try_from(
            address
                .checked_sub(self.memory_base)
                .ok_or("DMA source below supplied memory")?,
        )
        .map_err(|_| "DMA source offset")?;
        let bytes = self
            .memory
            .get(offset..offset.checked_add(8).ok_or("DMA source end")?)
            .ok_or("DMA source response fault")?;
        Ok(u64::from_le_bytes(bytes.try_into().expect("eight bytes")))
    }
}
/// GPU-owned functional DMA source. A test adapter may use a vendor Service;
/// production GPU code has no dependency on that implementation or its timing.
pub trait MemoryPort {
    /// Return exactly bytes/8 little-endian beats, in address order, or an error.
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String>;
}
impl MemoryPort for &Input {
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String> {
        if bytes == 0
            || bytes & 7 != 0
            || address & 7 != 0
            || bytes > crate::scratchpad::ports::REGION_BYTES
            || address.checked_add(bytes as u64).is_none()
        {
            return Err("DMA source request shape".into());
        }
        (0..bytes / 8)
            .map(|beat| self.read_beat(address + beat as u64 * 8))
            .collect()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawOutput {
    pub slot: usize,
    pub epoch: u32,
    pub vertices: Vec<Transformed>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub epoch: u32,
    pub held: bool,
    pub producing: bool,
    pub rows: Vec<u64>,
    pub ready: Vec<bool>,
}
impl Default for Slot {
    fn default() -> Self {
        Self {
            epoch: 0,
            held: false,
            producing: false,
            rows: vec![0; 512],
            ready: vec![false; 64],
        }
    }
}
impl Slot {
    pub fn allocate(&mut self) -> Result<u32, String> {
        if self.held {
            return Err("transformed slot still held by consumer".into());
        }
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or("transformed slot epoch overflow")?;
        self.held = true;
        self.producing = true;
        self.ready.fill(false);
        Ok(self.epoch)
    }
    pub fn release(&mut self, epoch: u32) -> Result<(), String> {
        if !self.held || self.producing || self.epoch != epoch {
            return Err("stale transformed release".into());
        }
        self.held = false;
        Ok(())
    }
    pub fn finish_production(&mut self, epoch: u32) -> Result<(), String> {
        if !self.held || !self.producing || self.epoch != epoch || !self.ready.iter().any(|r| *r) {
            return Err("transformed producer completion state".into());
        }
        self.producing = false;
        Ok(())
    }
}
