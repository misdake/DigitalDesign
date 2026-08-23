//! Protocols and address types shared by independently reusable IP blocks.

use core::fmt;

pub type Word = u16;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PhysicalWordAddress(u32);

impl PhysicalWordAddress {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }
    pub const fn from_segment_offset(segment: Word, offset: Word) -> Self {
        Self(((segment as u32) << 16) | offset as u32)
    }
    pub const fn get(self) -> u32 {
        self.0
    }
    pub const fn byte_address(self) -> u64 {
        (self.0 as u64) << 1
    }
    pub const fn line_base(self, line_words: u32) -> Self {
        assert!(line_words.is_power_of_two());
        Self(self.0 & !(line_words - 1))
    }
}

impl From<Word> for PhysicalWordAddress {
    fn from(value: Word) -> Self {
        Self(u32::from(value))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryRegionKind {
    Boot,
    Main,
    Shared,
    Reserved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRegion {
    pub name: &'static str,
    pub base: PhysicalWordAddress,
    pub words: u32,
    pub kind: MemoryRegionKind,
}

impl MemoryRegion {
    pub const fn end(self) -> Option<u32> {
        self.base.get().checked_add(self.words)
    }
    pub const fn contains(self, address: PhysicalWordAddress) -> bool {
        address.get() >= self.base.get()
            && match self.end() {
                Some(end) => address.get() < end,
                None => false,
            }
    }
}

pub trait SystemMemoryLayout {
    const PHYSICAL_ADDRESS_BITS: u8;
    const REGIONS: &'static [MemoryRegion];
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    InvalidAddressBits(u8),
    EmptyRegion(&'static str),
    RegionOverflow(&'static str),
    RegionOutsideCapacity {
        region: &'static str,
        end: u64,
        capacity: u64,
    },
    Overlap {
        first: &'static str,
        second: &'static str,
    },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAddressBits(bits) => {
                write!(f, "physical address width {bits} is not in 1..=32")
            }
            Self::EmptyRegion(name) => write!(f, "memory region `{name}` is empty"),
            Self::RegionOverflow(name) => write!(
                f,
                "memory region `{name}` overflows the physical word address"
            ),
            Self::RegionOutsideCapacity {
                region,
                end,
                capacity,
            } => write!(
                f,
                "memory region `{region}` ends at word {end:#x}, beyond capacity {capacity:#x}"
            ),
            Self::Overlap { first, second } => {
                write!(f, "memory regions `{first}` and `{second}` overlap")
            }
        }
    }
}

impl std::error::Error for LayoutError {}

pub fn validate_memory_layout<L: SystemMemoryLayout>() -> Result<(), LayoutError> {
    if !(1..=32).contains(&L::PHYSICAL_ADDRESS_BITS) {
        return Err(LayoutError::InvalidAddressBits(L::PHYSICAL_ADDRESS_BITS));
    }
    let capacity = 1u64 << L::PHYSICAL_ADDRESS_BITS;
    let mut regions = L::REGIONS.to_vec();
    regions.sort_by_key(|region| region.base);
    for region in &regions {
        if region.words == 0 {
            return Err(LayoutError::EmptyRegion(region.name));
        }
        let end = region
            .end()
            .ok_or(LayoutError::RegionOverflow(region.name))?;
        if u64::from(end) > capacity {
            return Err(LayoutError::RegionOutsideCapacity {
                region: region.name,
                end: u64::from(end),
                capacity,
            });
        }
    }
    for pair in regions.windows(2) {
        if pair[0].end().expect("validated") > pair[1].base.get() {
            return Err(LayoutError::Overlap {
                first: pair[0].name,
                second: pair[1].name,
            });
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteMask(pub u8);

impl WriteMask {
    pub const NONE: Self = Self(0);
    pub const LOW_BYTE: Self = Self(1);
    pub const HIGH_BYTE: Self = Self(2);
    pub const BOTH: Self = Self(3);
    pub const fn is_valid(self) -> bool {
        self.0 <= 3
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryOperation {
    Read,
    Write { data: Word, mask: WriteMask },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryRequest {
    pub address: PhysicalWordAddress,
    pub operation: MemoryOperation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryCompletion {
    Read { data: Word },
    Write,
    Error,
}

/// Contract for a ready/valid, single-outstanding memory master.
pub trait MemoryMasterPort {
    fn request(&self) -> Option<MemoryRequest>;
    fn request_accepted(&mut self);
    fn complete(&mut self, completion: MemoryCompletion);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceChannel {
    pub device: u8,
    pub channel: u8,
}

pub trait SystemDeviceLayout {
    const CHANNELS: &'static [(&'static str, DeviceChannel)];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_is_concatenated_and_regions_are_validated() {
        assert_eq!(
            PhysicalWordAddress::from_segment_offset(0x003f, 0xffff).get(),
            0x003f_ffff
        );
        struct Layout;
        impl SystemMemoryLayout for Layout {
            const PHYSICAL_ADDRESS_BITS: u8 = 22;
            const REGIONS: &'static [MemoryRegion] = &[
                MemoryRegion {
                    name: "boot",
                    base: PhysicalWordAddress::new(0),
                    words: 256,
                    kind: MemoryRegionKind::Boot,
                },
                MemoryRegion {
                    name: "main",
                    base: PhysicalWordAddress::new(256),
                    words: (1 << 22) - 256,
                    kind: MemoryRegionKind::Main,
                },
            ];
        }
        assert_eq!(validate_memory_layout::<Layout>(), Ok(()));
    }
}
