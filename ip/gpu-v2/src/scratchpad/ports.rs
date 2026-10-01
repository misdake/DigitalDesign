pub const BYTES: usize = 8192;
pub const REGION_BYTES: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Free,
    Filling,
    Ready,
    InUse,
    Faulted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lease {
    pub region: usize,
    pub epoch: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaDescriptor {
    pub physical_addr: u64,
    pub scratchpad_addr: usize,
    pub byte_count: usize,
    pub completion_token: u8,
}
impl DmaDescriptor {
    pub fn validate(self) -> Result<usize, String> {
        let end = self
            .scratchpad_addr
            .checked_add(self.byte_count)
            .ok_or("DMA destination overflow")?;
        self.physical_addr
            .checked_add(self.byte_count as u64)
            .ok_or("DMA source overflow")?;
        if self.byte_count == 0
            || self.byte_count > REGION_BYTES
            || self.byte_count & 7 != 0
            || self.scratchpad_addr & 7 != 0
            || self.physical_addr & 7 != 0
            || self.completion_token > 3
            || end > BYTES
            || self.scratchpad_addr / REGION_BYTES != (end - 1) / REGION_BYTES
        {
            return Err("DMA alignment, region, size or token".into());
        }
        Ok(self.scratchpad_addr / REGION_BYTES)
    }
}
