use digital_design_ip_common::{
    DeviceAllocation, MemoryRegion, MemoryRegionKind, PhysicalWordAddress, SystemDeviceLayout,
    SystemMemoryLayout,
};

pub const FRAMEBUFFER_WIDTH: u32 = 320;
pub const FRAMEBUFFER_HEIGHT: u32 = 240;
pub const FRAMEBUFFER_STRIDE_WORDS: u32 = FRAMEBUFFER_WIDTH;
pub const FRAMEBUFFER_WORDS: u32 = FRAMEBUFFER_STRIDE_WORDS * FRAMEBUFFER_HEIGHT;
pub const FRAMEBUFFER_BASE_WORD: u32 = 0x0020_0100;
pub const FRAMEBUFFER_END_WORD: u32 = FRAMEBUFFER_BASE_WORD + FRAMEBUFFER_WORDS;
pub const FRAMEBUFFER_FIRST_SEGMENT: u16 = 0x0020;
pub const FRAMEBUFFER_FIRST_OFFSET: u16 = 0x0100;
pub const FRAMEBUFFER_SECOND_SEGMENT: u16 = 0x0021;
pub const FRAMEBUFFER_ROWS_IN_FIRST_SEGMENT: u16 = 204;

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
            name: "main-high-before-framebuffer",
            base: PhysicalWordAddress::new(0x1_0000),
            words: FRAMEBUFFER_BASE_WORD - 0x1_0000,
            kind: MemoryRegionKind::Main,
        },
        MemoryRegion {
            name: "framebuffer",
            base: PhysicalWordAddress::new(FRAMEBUFFER_BASE_WORD),
            words: FRAMEBUFFER_WORDS,
            kind: MemoryRegionKind::Shared,
        },
        MemoryRegion {
            name: "main-high-after-framebuffer",
            base: PhysicalWordAddress::new(FRAMEBUFFER_END_WORD),
            words: (1 << 22) - FRAMEBUFFER_END_WORD,
            kind: MemoryRegionKind::Main,
        },
    ];
}

pub struct TangNano20kDeviceLayout;

impl SystemDeviceLayout for TangNano20kDeviceLayout {
    const DEVICE_ADDRESS_BITS: u8 = 4;
    const CHANNEL_ADDRESS_BITS: u8 = 4;
    const ALLOCATIONS: &'static [DeviceAllocation] = &[
        DeviceAllocation {
            name: "system-control",
            device: crate::boot::SYSTEM_CONTROL_DEVICE,
            channels: &[
                crate::boot::SYSCTL_INVALIDATE_ICACHE,
                crate::boot::SYSCTL_INVALIDATE_DCACHE,
                crate::boot::SYSCTL_LED,
                crate::boot::SYSCTL_UART,
            ],
        },
        DeviceAllocation {
            name: "boot-select",
            device: crate::boot::BOOT_SELECT_DEVICE,
            channels: &[crate::boot::BOOT_SELECT_VALUE],
        },
        DeviceAllocation {
            name: "boot-dma",
            device: crate::boot::BOOT_DMA_DEVICE,
            channels: &[
                crate::boot::DMA_COMMAND,
                crate::boot::DMA_STATUS,
                crate::boot::DMA_FLASH_OFFSET_LOW,
                crate::boot::DMA_FLASH_OFFSET_HIGH,
                crate::boot::DMA_DESTINATION_LOW,
                crate::boot::DMA_DESTINATION_HIGH,
                crate::boot::DMA_FILE_SIZE_LOW,
                crate::boot::DMA_FILE_SIZE_HIGH,
                crate::boot::DMA_MEMORY_SIZE_LOW,
                crate::boot::DMA_MEMORY_SIZE_HIGH,
                crate::boot::DMA_ERROR,
                crate::boot::DMA_COMPLETED_WORDS_LOW,
            ],
        },
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

/// Software-managed ownership state for a buffer shared with an accelerator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedBufferOwner {
    Cpu,
    AcceleratorRunning,
    AcceleratorCompleteNeedsInvalidate,
}

impl SharedBufferOwner {
    pub fn start_accelerator(&mut self, cpu_stores_complete: bool) -> Result<(), &'static str> {
        if *self != Self::Cpu {
            return Err("buffer is not owned by the CPU");
        }
        if !cpu_stores_complete {
            return Err("CPU stores must complete before accelerator handoff");
        }
        *self = Self::AcceleratorRunning;
        Ok(())
    }

    pub fn accelerator_complete(
        &mut self,
        accelerator_writes_complete: bool,
    ) -> Result<(), &'static str> {
        if *self != Self::AcceleratorRunning {
            return Err("accelerator is not running");
        }
        if !accelerator_writes_complete {
            return Err("accelerator completion must drain memory writes");
        }
        *self = Self::AcceleratorCompleteNeedsInvalidate;
        Ok(())
    }

    pub fn cpu_invalidate_complete(&mut self) -> Result<(), &'static str> {
        if *self != Self::AcceleratorCompleteNeedsInvalidate {
            return Err("accelerator completion is not pending");
        }
        *self = Self::Cpu;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use digital_design_ip_common::{validate_device_layout, validate_memory_layout};

    #[test]
    fn fitted_layout_and_target_adapter_reject_out_of_range_addresses() {
        validate_memory_layout::<TangNano20kMemoryLayout>().unwrap();
        validate_device_layout::<TangNano20kDeviceLayout>().unwrap();
        assert!(SdramWordAddress::try_from(PhysicalWordAddress::new((1 << 22) - 1)).is_ok());
        assert!(SdramWordAddress::try_from(PhysicalWordAddress::new(1 << 22)).is_err());
    }

    #[test]
    fn framebuffer_is_reserved_and_crosses_segments_on_a_row_boundary() {
        let framebuffer = TangNano20kMemoryLayout::REGIONS
            .iter()
            .find(|region| region.name == "framebuffer")
            .unwrap();
        assert_eq!(framebuffer.base.get(), FRAMEBUFFER_BASE_WORD);
        assert_eq!(framebuffer.words, 320 * 240);
        assert_eq!(framebuffer.kind, MemoryRegionKind::Shared);
        assert_eq!(FRAMEBUFFER_END_WORD - 1, 0x0021_2cff);
        assert_eq!(
            u32::from(FRAMEBUFFER_FIRST_OFFSET)
                + u32::from(FRAMEBUFFER_ROWS_IN_FIRST_SEGMENT) * FRAMEBUFFER_STRIDE_WORDS,
            0x1_0000
        );
    }

    #[test]
    fn accelerator_handoff_requires_completions_and_cpu_cache_invalidation() {
        let mut owner = SharedBufferOwner::Cpu;
        assert!(owner.start_accelerator(false).is_err());
        owner.start_accelerator(true).unwrap();
        assert!(owner.accelerator_complete(false).is_err());
        owner.accelerator_complete(true).unwrap();
        owner.cpu_invalidate_complete().unwrap();
        assert_eq!(owner, SharedBufferOwner::Cpu);
    }
}
