use digital_design_ip_common::{
    DeviceAllocation, MemoryRegion, MemoryRegionKind, PhysicalWordAddress, SystemDeviceLayout,
    SystemMemoryLayout,
};

pub const FRAMEBUFFER_WIDTH: u32 = 400;
pub const FRAMEBUFFER_HEIGHT: u32 = 240;
/// Framebuffer V1 is tile-linear: 16x16 RGB565 tiles packed tile-row-major.
pub const FRAMEBUFFER_TILE: u32 = 16;
/// One 16x16 tile is 256 16-bit words.
pub const FRAMEBUFFER_TILE_WORDS: u32 = FRAMEBUFFER_TILE * FRAMEBUFFER_TILE;
pub const FRAMEBUFFER_TILE_COLUMNS: u32 = FRAMEBUFFER_WIDTH / FRAMEBUFFER_TILE;
pub const FRAMEBUFFER_TILE_ROWS: u32 = FRAMEBUFFER_HEIGHT / FRAMEBUFFER_TILE;
/// Words between the starts of two adjacent tile rows (25 tiles x 256 words).
pub const FRAMEBUFFER_TILE_ROW_STRIDE_WORDS: u32 =
    FRAMEBUFFER_TILE_COLUMNS * FRAMEBUFFER_TILE_WORDS;
/// Live pixel payload of one framebuffer, in 16-bit words.
pub const FRAMEBUFFER_WORDS: u32 = FRAMEBUFFER_WIDTH * FRAMEBUFFER_HEIGHT;
/// Per-framebuffer slot, including the padding that 16x16 tiles leave after
/// the last tile row.
pub const FRAMEBUFFER_SLOT_WORDS: u32 = 0x0001_8000;
pub const FRAMEBUFFER_A_BASE_WORD: u32 = 0x0020_0000;
pub const FRAMEBUFFER_B_BASE_WORD: u32 = FRAMEBUFFER_A_BASE_WORD + FRAMEBUFFER_SLOT_WORDS;
pub const FRAMEBUFFER_BASE_WORD: u32 = FRAMEBUFFER_A_BASE_WORD;
pub const FRAMEBUFFER_END_WORD: u32 = FRAMEBUFFER_B_BASE_WORD + FRAMEBUFFER_SLOT_WORDS;

/// Physical word address of pixel `(x, y)` relative to an explicit framebuffer base.
pub const fn framebuffer_word_at(base: u32, x: u32, y: u32) -> u32 {
    let tile_x = x / FRAMEBUFFER_TILE;
    let tile_y = y / FRAMEBUFFER_TILE;
    let local_x = x % FRAMEBUFFER_TILE;
    let local_y = y % FRAMEBUFFER_TILE;
    let tile = tile_y * FRAMEBUFFER_TILE_COLUMNS + tile_x;
    base + tile * FRAMEBUFFER_TILE_WORDS + local_y * FRAMEBUFFER_TILE + local_x
}

/// Physical word address of the 16-pixel segment `tile_x` of source row `y`,
/// i.e. the first pixel of the 32-byte line the display fetches for that
/// position. The display walks `tile_x` 0..24 with a +256 word step, then the
/// next source row with +16 words, and the next tile row with +6400 words.
pub const fn framebuffer_segment_word(base: u32, y: u32, tile_x: u32) -> u32 {
    let tile_y = y / FRAMEBUFFER_TILE;
    let local_y = y % FRAMEBUFFER_TILE;
    base + tile_y * FRAMEBUFFER_TILE_ROW_STRIDE_WORDS
        + tile_x * FRAMEBUFFER_TILE_WORDS
        + local_y * FRAMEBUFFER_TILE
}

/// Physical word address of pixel `(x, y)` in the default framebuffer A.
pub const fn framebuffer_word(x: u32, y: u32) -> u32 {
    framebuffer_word_at(FRAMEBUFFER_A_BASE_WORD, x, y)
}

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
            name: "main-before-framebuffer",
            base: PhysicalWordAddress::new(0x100),
            words: FRAMEBUFFER_BASE_WORD - 0x100,
            kind: MemoryRegionKind::Main,
        },
        MemoryRegion {
            name: "framebuffer",
            base: PhysicalWordAddress::new(FRAMEBUFFER_BASE_WORD),
            words: FRAMEBUFFER_SLOT_WORDS * 2,
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
    const DEVICE_ADDRESS_BITS: u8 = 3;
    const CHANNEL_ADDRESS_BITS: u8 = 4;
    const ALLOCATIONS: &'static [DeviceAllocation] = &[
        DeviceAllocation {
            name: "system-control",
            device: crate::boot::SYSTEM_CONTROL_DEVICE,
            channels: &[
                crate::boot::ICACHE_INVALIDATE_ALL_DELAYED,
                crate::boot::D_INVALIDATE_ALL,
                crate::boot::SYSCTL_LED,
                crate::boot::SYSCTL_UART,
                crate::boot::D_CLEAN_ALL,
                crate::boot::CACHE_MAINTENANCE_STATUS,
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
        DeviceAllocation {
            name: "display",
            device: crate::DISPLAY_DEVICE,
            channels: &[
                crate::DISPLAY_FRAME_INDEX,
                crate::DISPLAY_FRAMEBUFFER_LOW,
                crate::DISPLAY_FRAMEBUFFER_HIGH,
                crate::DISPLAY_CONTROL,
            ],
        },
        DeviceAllocation {
            // Read and write channels share one 4-bit channel space, so each
            // GPU channel number appears once even though channels 0..3 are
            // read/write pairs.
            name: "gpu",
            device: crate::GPU_DEVICE,
            channels: &[
                crate::GPU_CMD_BASE_LOW,
                crate::GPU_CMD_BASE_HIGH,
                crate::GPU_CMD_WORDS_LOW,
                crate::GPU_CMD_WORDS_HIGH,
                crate::GPU_SUBMIT,
                crate::GPU_CONTROL,
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
    pub fn start_accelerator(
        &mut self,
        cpu_writes_visible_in_dram: bool,
    ) -> Result<(), &'static str> {
        if *self != Self::Cpu {
            return Err("buffer is not owned by the CPU");
        }
        if !cpu_writes_visible_in_dram {
            return Err("CPU writes must be visible in DRAM before accelerator handoff");
        }
        *self = Self::AcceleratorRunning;
        Ok(())
    }

    pub fn accelerator_complete(
        &mut self,
        accelerator_writes_visible_in_dram: bool,
    ) -> Result<(), &'static str> {
        if *self != Self::AcceleratorRunning {
            return Err("accelerator is not running");
        }
        if !accelerator_writes_visible_in_dram {
            return Err("accelerator completion must make every write visible in DRAM");
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
    fn framebuffer_is_one_contiguous_physical_region() {
        let framebuffer = TangNano20kMemoryLayout::REGIONS
            .iter()
            .find(|region| region.name == "framebuffer")
            .unwrap();
        assert_eq!(framebuffer.base.get(), FRAMEBUFFER_BASE_WORD);
        assert_eq!(framebuffer.words, FRAMEBUFFER_SLOT_WORDS * 2);
        assert_eq!(framebuffer.kind, MemoryRegionKind::Shared);
        assert_eq!(framebuffer_word(0, 203), 0x0021_2cb0);
        assert_eq!(framebuffer_word(399, 203), 0x0021_44bf);
        assert_eq!(framebuffer_word(0, 204), 0x0021_2cc0);
        assert_eq!(framebuffer_word(399, 239), 0x0021_76ff);
        assert_eq!(FRAMEBUFFER_B_BASE_WORD, 0x0021_8000);
        assert_eq!(FRAMEBUFFER_END_WORD, 0x0023_0000);
        assert_eq!(
            framebuffer_word_at(FRAMEBUFFER_B_BASE_WORD, 399, 239),
            FRAMEBUFFER_B_BASE_WORD + 0x0001_76ff
        );
    }

    #[test]
    fn tile_linear_segments_cover_the_payload_exactly_once() {
        let mut seen = vec![false; FRAMEBUFFER_WORDS as usize];
        for y in 0..FRAMEBUFFER_HEIGHT {
            for tile_x in 0..FRAMEBUFFER_TILE_COLUMNS {
                let segment = framebuffer_segment_word(0, y, tile_x);
                for offset in 0..16 {
                    let address = segment + offset;
                    assert!(address < FRAMEBUFFER_WORDS, "segment left the payload");
                    assert!(!seen[address as usize], "word {address:#x} covered twice");
                    seen[address as usize] = true;
                }
            }
        }
        assert!(seen.into_iter().all(|covered| covered));
        // A single segment spans 16 consecutive pixels of one source row.
        for x in 0..16 {
            assert_eq!(
                framebuffer_word_at(0, 16 + x, 7),
                framebuffer_segment_word(0, 7, 1) + x
            );
        }
    }

    #[test]
    fn accelerator_handoff_requires_dram_visibility_and_cpu_cache_invalidation() {
        let mut owner = SharedBufferOwner::Cpu;
        assert!(owner.start_accelerator(false).is_err());
        owner.start_accelerator(true).unwrap();
        assert!(owner.accelerator_complete(false).is_err());
        owner.accelerator_complete(true).unwrap();
        owner.cpu_invalidate_complete().unwrap();
        assert_eq!(owner, SharedBufferOwner::Cpu);
    }
}
