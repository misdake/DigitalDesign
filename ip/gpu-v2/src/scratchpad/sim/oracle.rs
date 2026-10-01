use super::super::ports::*;
#[derive(Clone, Debug)]
pub struct Region {
    pub owner: Owner,
    pub epoch: u32,
    pub start: usize,
    pub bytes: usize,
    pub received: usize,
}
impl Default for Region {
    fn default() -> Self {
        Self {
            owner: Owner::Free,
            epoch: 0,
            start: 0,
            bytes: 0,
            received: 0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Scratchpad {
    pub banks: Vec<Vec<u16>>,
    pub regions: [Region; 2],
}
impl Default for Scratchpad {
    fn default() -> Self {
        Self {
            banks: vec![vec![0; 1024]; 4],
            regions: std::array::from_fn(|_| Region::default()),
        }
    }
}
impl Scratchpad {
    pub fn reserve(&mut self, descriptor: DmaDescriptor) -> Result<Lease, String> {
        let region = descriptor.validate()?;
        let prior = &self.regions[region];
        if prior.owner != Owner::Free {
            return Err("DMA region is owned".into());
        }
        let epoch = prior
            .epoch
            .checked_add(1)
            .ok_or("scratchpad epoch overflow")?;
        self.regions[region] = Region {
            owner: Owner::Filling,
            epoch,
            start: descriptor.scratchpad_addr,
            bytes: descriptor.byte_count,
            received: 0,
        };
        Ok(Lease { region, epoch })
    }
    fn check(&self, lease: Lease) -> Result<&Region, String> {
        let r = self.regions.get(lease.region).ok_or("scratchpad region")?;
        if r.epoch != lease.epoch {
            return Err("stale scratchpad lease".into());
        }
        Ok(r)
    }
    pub fn dma_beat(&mut self, lease: Lease, data: u64) -> Result<usize, String> {
        let r = self.check(lease)?;
        if r.owner != Owner::Filling || r.received >= r.bytes {
            return Err("unexpected DMA beat".into());
        }
        let address = r.start + r.received;
        let row = address / 8;
        for bank in 0..4 {
            self.banks[bank][row] = (data >> (16 * bank)) as u16;
        }
        self.regions[lease.region].received += 8;
        Ok(address)
    }
    pub fn complete(&mut self, lease: Lease) -> Result<(), String> {
        let r = self.check(lease)?;
        if r.owner != Owner::Filling || r.received != r.bytes {
            return Err("DMA completion before last committed beat".into());
        }
        self.regions[lease.region].owner = Owner::Ready;
        Ok(())
    }
    pub fn fault(&mut self, lease: Lease) -> Result<(), String> {
        if self.check(lease)?.owner != Owner::Filling {
            return Err("DMA fault outside FILLING region".into());
        }
        self.regions[lease.region].owner = Owner::Faulted;
        Ok(())
    }
    pub fn acquire(&mut self, lease: Lease) -> Result<(), String> {
        if self.check(lease)?.owner != Owner::Ready {
            return Err("core acquire before DMA READY".into());
        }
        self.regions[lease.region].owner = Owner::InUse;
        Ok(())
    }
    pub fn read64(&self, lease: Lease, address: usize) -> Result<u64, String> {
        let r = self.check(lease)?;
        if r.owner != Owner::InUse
            || address & 7 != 0
            || address < r.start
            || address
                .checked_add(8)
                .is_none_or(|end| end > r.start + r.bytes)
        {
            return Err("core read outside completed owned range".into());
        }
        let mut result = 0;
        for bank in 0..4 {
            result |= u64::from(self.banks[bank][address / 8]) << (bank * 16);
        }
        Ok(result)
    }
    pub fn write64(&mut self, lease: Lease, address: usize, data: u64) -> Result<(), String> {
        self.read64(lease, address)?;
        for bank in 0..4 {
            self.banks[bank][address / 8] = (data >> (bank * 16)) as u16;
        }
        Ok(())
    }
    pub fn release(&mut self, lease: Lease) -> Result<(), String> {
        if self.check(lease)?.owner != Owner::InUse {
            return Err("scratchpad release without core ownership".into());
        }
        self.regions[lease.region].owner = Owner::Free;
        Ok(())
    }
}
