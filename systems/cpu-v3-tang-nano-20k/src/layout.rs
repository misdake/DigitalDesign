use digital_design_ip_common::{
    DeviceChannel, MemoryRegion, MemoryRegionKind, PhysicalWordAddress, SystemDeviceLayout,
    SystemMemoryLayout,
};

pub struct TangNano20kMemoryLayout;

impl SystemMemoryLayout for TangNano20kMemoryLayout {
    const PHYSICAL_ADDRESS_BITS: u8 = 22;
    const REGIONS: &'static [MemoryRegion] = &[
        MemoryRegion {
            name: "boot",
            base: PhysicalWordAddress::new(0),
            words: 0x100,
            kind: MemoryRegionKind::Boot,
        },
        MemoryRegion {
            name: "main-low",
            base: PhysicalWordAddress::new(0x100),
            words: 0xfe00,
            kind: MemoryRegionKind::Main,
        },
        MemoryRegion {
            name: "mmio",
            base: PhysicalWordAddress::new(0xff00),
            words: 0x100,
            kind: MemoryRegionKind::Reserved,
        },
        MemoryRegion {
            name: "main-high",
            base: PhysicalWordAddress::new(0x1_0000),
            words: (1 << 22) - 0x1_0000,
            kind: MemoryRegionKind::Main,
        },
    ];
}

pub struct TangNano20kDeviceLayout;

impl SystemDeviceLayout for TangNano20kDeviceLayout {
    const CHANNELS: &'static [(&'static str, DeviceChannel)] = &[
        (
            "system-control",
            DeviceChannel {
                device: 0,
                channel: 0,
            },
        ),
        (
            "boot-dma",
            DeviceChannel {
                device: 2,
                channel: 0,
            },
        ),
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SdramWordAddress(u32);

impl TryFrom<PhysicalWordAddress> for SdramWordAddress {
    type Error = PhysicalWordAddress;

    fn try_from(address: PhysicalWordAddress) -> Result<Self, Self::Error> {
        (address.get() < crate::TANG_NANO_20K_SDRAM_WORDS)
            .then_some(Self(address.get()))
            .ok_or(address)
    }
}

impl SdramWordAddress {
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Software-managed ownership state for shared CPU/GPU buffers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferOwner {
    Cpu,
    GpuRunning,
    GpuCompleteNeedsInvalidate,
}

impl BufferOwner {
    pub fn start_gpu(&mut self, cpu_stores_complete: bool) -> Result<(), &'static str> {
        if *self != Self::Cpu {
            return Err("buffer is not owned by the CPU");
        }
        if !cpu_stores_complete {
            return Err("CPU stores must complete before GPU handoff");
        }
        *self = Self::GpuRunning;
        Ok(())
    }

    pub fn gpu_complete(&mut self, gpu_writes_complete: bool) -> Result<(), &'static str> {
        if *self != Self::GpuRunning {
            return Err("GPU is not running");
        }
        if !gpu_writes_complete {
            return Err("GPU completion must drain memory writes");
        }
        *self = Self::GpuCompleteNeedsInvalidate;
        Ok(())
    }

    pub fn cpu_invalidate_complete(&mut self) -> Result<(), &'static str> {
        if *self != Self::GpuCompleteNeedsInvalidate {
            return Err("GPU completion is not pending");
        }
        *self = Self::Cpu;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_ip_common::validate_memory_layout;

    #[test]
    fn fitted_layout_and_target_adapter_reject_out_of_range_addresses() {
        validate_memory_layout::<TangNano20kMemoryLayout>().unwrap();
        assert!(SdramWordAddress::try_from(PhysicalWordAddress::new((1 << 22) - 1)).is_ok());
        assert!(SdramWordAddress::try_from(PhysicalWordAddress::new(1 << 22)).is_err());
    }

    #[test]
    fn gpu_handoff_requires_completions_and_cpu_cache_invalidation() {
        let mut owner = BufferOwner::Cpu;
        assert!(owner.start_gpu(false).is_err());
        owner.start_gpu(true).unwrap();
        assert!(owner.gpu_complete(false).is_err());
        owner.gpu_complete(true).unwrap();
        owner.cpu_invalidate_complete().unwrap();
        assert_eq!(owner, BufferOwner::Cpu);
    }
}
