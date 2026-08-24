//! Versioned G16 boot container shared by the host packer and both loaders.

mod devices;
mod loader;
mod manifest;
mod mmio;

pub use devices::*;
pub use loader::*;
pub use manifest::*;
pub use mmio::*;

use std::collections::HashSet;
use std::fmt;

use super::{PhysicalWordAddress, Word, MMIO_BASE, TANG_NANO_20K_SDRAM_WORDS};

pub const BOOT_FORMAT_VERSION: u16 = 3;
pub const BOOT_DESCRIPTOR_SIZE: usize = 64;
pub const BOOT_MANIFEST_HEADER_SIZE: usize = 48;
pub const BOOT_SECTION_RECORD_SIZE: usize = 32;
pub const BOOT_DATA_ALIGNMENT: u32 = 256;
pub const TANG_NANO_20K_CONFIGURATION_RESERVE_BYTES: u32 = 1 << 20;
pub const STAGE1_HANDOFF_OFFSET: Word = 0x0100;
pub const STAGE1_HANDOFF_SIZE_BYTES: u32 = BOOT_DESCRIPTOR_SIZE as u32;

const BOOT_MAGIC: &[u8; 8] = b"G16BOOT\0";
const MANIFEST_MAGIC: &[u8; 8] = b"G16SECT\0";

pub const SECTION_READ: u16 = 1 << 0;
pub const SECTION_WRITE: u16 = 1 << 1;
pub const SECTION_EXECUTE: u16 = 1 << 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum BootTarget {
    TangNano20K = 0x544e_3230,
}

impl BootTarget {
    pub const fn physical_memory_words(self) -> u32 {
        match self {
            Self::TangNano20K => TANG_NANO_20K_SDRAM_WORDS,
        }
    }

    pub const fn flash_bytes(self) -> u32 {
        match self {
            Self::TangNano20K => 8 * 1024 * 1024,
        }
    }

    pub const fn payload_flash_offset(self) -> u32 {
        match self {
            Self::TangNano20K => TANG_NANO_20K_CONFIGURATION_RESERVE_BYTES,
        }
    }

    pub const fn payload_capacity_bytes(self) -> u32 {
        self.flash_bytes() - self.payload_flash_offset()
    }

    fn from_raw(value: u32) -> Result<Self, BootImageError> {
        match value {
            value if value == Self::TangNano20K as u32 => Ok(Self::TangNano20K),
            _ => Err(BootImageError::UnsupportedTarget(value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootEntry {
    pub code_segment: Word,
    pub offset: Word,
    pub data_segment: Word,
    pub stack_offset: Word,
}

impl BootEntry {
    pub const fn physical_entry(self) -> PhysicalWordAddress {
        PhysicalWordAddress::from_segment_offset(self.code_segment, self.offset)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum SectionKind {
    Load = 1,
    Zero = 2,
}

impl SectionKind {
    fn from_raw(value: u16) -> Result<Self, BootImageError> {
        match value {
            1 => Ok(Self::Load),
            2 => Ok(Self::Zero),
            _ => Err(BootImageError::InvalidSectionKind(value)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSection {
    pub name: String,
    pub kind: SectionKind,
    pub flags: u16,
    pub destination: PhysicalWordAddress,
    pub data: Vec<u8>,
    pub memory_size_bytes: u32,
    pub alignment_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootImageSpec {
    pub target: BootTarget,
    pub stage1_section: String,
    pub stage1_entry: BootEntry,
    pub application_entry: BootEntry,
    pub sections: Vec<InputSection>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BootDescriptor {
    pub target: BootTarget,
    pub package_size_bytes: u32,
    pub stage1_flash_offset: u32,
    pub stage1_file_size_bytes: u32,
    pub stage1_memory_size_bytes: u32,
    pub stage1_destination: PhysicalWordAddress,
    pub stage1_entry: BootEntry,
    pub manifest_flash_offset: u32,
    pub manifest_size_bytes: u32,
    /// Physical SDRAM address where Stage0 mirrors this complete descriptor.
    pub stage1_handoff_destination: PhysicalWordAddress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionRecord {
    pub kind: SectionKind,
    pub flags: u16,
    pub flash_offset: u32,
    pub destination: PhysicalWordAddress,
    pub file_size_bytes: u32,
    pub memory_size_bytes: u32,
    pub alignment_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootManifest {
    pub target_package_size_bytes: u32,
    pub application_entry: BootEntry,
    pub sections: Vec<SectionRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedSection {
    pub name: String,
    pub record: SectionRecord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootImage {
    pub bytes: Vec<u8>,
    pub descriptor: BootDescriptor,
    pub manifest: BootManifest,
    pub sections: Vec<PackedSection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetFlashImage {
    pub bytes: Vec<u8>,
    pub payload_offset: u32,
}

impl BootImage {
    pub fn map(&self) -> String {
        let mut text = format!(
            "G16 boot image v{BOOT_FORMAT_VERSION}\n\
             target: {:?}\n\
             package: {:#010x} bytes\n\
             target Flash placement: {:#010x}..{:#010x}\n\
             stage1: flash+{:#010x} -> word {:#010x}, {:#x}/{:#x} bytes, entry {:04x}:{:04x}\n\
             stage1 handoff descriptor: word {:#010x}, {:#x} bytes\n\
             application entry: {:04x}:{:04x}, dseg={:04x}, sp={:04x}\n\
             sections:\n",
            self.descriptor.target,
            self.descriptor.package_size_bytes,
            self.descriptor.target.payload_flash_offset(),
            self.descriptor.target.payload_flash_offset() + self.descriptor.package_size_bytes,
            self.descriptor.stage1_flash_offset,
            self.descriptor.stage1_destination.get(),
            self.descriptor.stage1_file_size_bytes,
            self.descriptor.stage1_memory_size_bytes,
            self.descriptor.stage1_entry.code_segment,
            self.descriptor.stage1_entry.offset,
            self.descriptor.stage1_handoff_destination.get(),
            STAGE1_HANDOFF_SIZE_BYTES,
            self.manifest.application_entry.code_segment,
            self.manifest.application_entry.offset,
            self.manifest.application_entry.data_segment,
            self.manifest.application_entry.stack_offset,
        );
        for section in &self.sections {
            let record = section.record;
            text.push_str(&format!(
                "  {:<20} {:?} flags={:#05x} flash+{:#010x} -> word {:#010x} file={:#x} memory={:#x} align={}\n",
                section.name,
                record.kind,
                record.flags,
                record.flash_offset,
                record.destination.get(),
                record.file_size_bytes,
                record.memory_size_bytes,
                record.alignment_bytes,
            ));
        }
        text
    }

    pub fn place_after_configuration(
        &self,
        configuration: &[u8],
    ) -> Result<TargetFlashImage, BootImageError> {
        let reserve = self.descriptor.target.payload_flash_offset();
        if configuration.len() > reserve as usize {
            return Err(BootImageError::ConfigurationTooLarge {
                bytes: configuration.len(),
                reserved: reserve,
            });
        }
        let total = reserve as usize + self.bytes.len();
        if total > self.descriptor.target.flash_bytes() as usize {
            return Err(BootImageError::PackageTooLarge {
                bytes: self.bytes.len() as u64,
                capacity: self.descriptor.target.payload_capacity_bytes(),
            });
        }
        let mut bytes = vec![0xff; total];
        bytes[..configuration.len()].copy_from_slice(configuration);
        bytes[reserve as usize..].copy_from_slice(&self.bytes);
        Ok(TargetFlashImage {
            bytes,
            payload_offset: reserve,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BootImageError {
    DuplicateSection(String),
    MissingStage1(String),
    EmptySectionName,
    InvalidAlignment {
        section: String,
        alignment: u32,
    },
    MisalignedDestination {
        section: String,
        byte_address: u64,
        alignment: u32,
    },
    MemorySmallerThanFile {
        section: String,
        file_bytes: usize,
        memory_bytes: u32,
    },
    ZeroSectionContainsData(String),
    EmptyLoadSection(String),
    EmptyMemorySection(String),
    PhysicalRangeOverflow(String),
    PhysicalMemoryExceeded {
        section: String,
        end_byte: u64,
        capacity_bytes: u64,
    },
    OverlappingSections {
        first: String,
        second: String,
    },
    EntryOutsideExecutableSection {
        stage: &'static str,
        address: u32,
    },
    StackInMmio {
        stage: &'static str,
        offset: Word,
    },
    StackOutsidePhysicalMemory {
        stage: &'static str,
        address: PhysicalWordAddress,
    },
    TooManySections(usize),
    PackageTooLarge {
        bytes: u64,
        capacity: u32,
    },
    ConfigurationTooLarge {
        bytes: usize,
        reserved: u32,
    },
    IntegerOverflow(&'static str),
    Truncated(&'static str),
    InvalidMagic(&'static str),
    UnsupportedVersion {
        found: u16,
        expected: u16,
    },
    UnsupportedTarget(u32),
    InvalidSectionKind(u16),
    InvalidFormat(&'static str),
}

impl fmt::Display for BootImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSection(name) => write!(f, "duplicate boot section name `{name}`"),
            Self::MissingStage1(name) => write!(f, "stage1 section `{name}` does not exist"),
            Self::EmptySectionName => write!(f, "boot section names may not be empty"),
            Self::InvalidAlignment { section, alignment } => write!(
                f,
                "section `{section}` alignment {alignment} is not a nonzero power of two"
            ),
            Self::MisalignedDestination {
                section,
                byte_address,
                alignment,
            } => write!(
                f,
                "section `{section}` destination byte address {byte_address:#x} is not aligned to {alignment} bytes"
            ),
            Self::MemorySmallerThanFile {
                section,
                file_bytes,
                memory_bytes,
            } => write!(
                f,
                "section `{section}` has {file_bytes} file bytes but only {memory_bytes} memory bytes"
            ),
            Self::ZeroSectionContainsData(name) => {
                write!(f, "zero section `{name}` unexpectedly contains file data")
            }
            Self::EmptyLoadSection(name) => write!(f, "load section `{name}` is empty"),
            Self::EmptyMemorySection(name) => {
                write!(f, "section `{name}` has an empty memory extent")
            }
            Self::PhysicalRangeOverflow(name) => {
                write!(f, "section `{name}` physical byte range overflows")
            }
            Self::PhysicalMemoryExceeded {
                section,
                end_byte,
                capacity_bytes,
            } => write!(
                f,
                "section `{section}` ends at physical byte {end_byte:#x}, beyond target capacity {capacity_bytes:#x}"
            ),
            Self::OverlappingSections { first, second } => {
                write!(f, "physical sections `{first}` and `{second}` overlap")
            }
            Self::EntryOutsideExecutableSection { stage, address } => write!(
                f,
                "{stage} physical entry word {address:#010x} is not inside an executable load section"
            ),
            Self::StackInMmio { stage, offset } => write!(
                f,
                "{stage} stack offset {offset:#06x} is zero or enters the fixed MMIO page"
            ),
            Self::StackOutsidePhysicalMemory { stage, address } => write!(
                f,
                "{stage} initial stack word {:#010x} is outside fitted physical memory",
                address.get()
            ),
            Self::TooManySections(count) => {
                write!(f, "boot image has {count} sections; the format permits 65535")
            }
            Self::PackageTooLarge { bytes, capacity } => write!(
                f,
                "boot package uses {bytes:#x} bytes, beyond target Flash capacity {capacity:#x}"
            ),
            Self::ConfigurationTooLarge { bytes, reserved } => write!(
                f,
                "FPGA configuration uses {bytes:#x} bytes, beyond the reserved {reserved:#x}-byte Flash region"
            ),
            Self::IntegerOverflow(field) => write!(f, "boot image field `{field}` exceeds u32"),
            Self::Truncated(part) => write!(f, "truncated boot {part}"),
            Self::InvalidMagic(part) => write!(f, "invalid boot {part} magic"),
            Self::UnsupportedVersion { found, expected } => write!(
                f,
                "unsupported boot format version {found}; expected {expected}"
            ),
            Self::UnsupportedTarget(target) => {
                write!(f, "unsupported boot target identifier {target:#010x}")
            }
            Self::InvalidSectionKind(kind) => write!(f, "invalid boot section kind {kind}"),
            Self::InvalidFormat(message) => write!(f, "invalid boot image: {message}"),
        }
    }
}

impl std::error::Error for BootImageError {}

pub fn build_boot_image(spec: BootImageSpec) -> Result<BootImage, BootImageError> {
    validate_entries(&spec)?;
    validate_sections(&spec)?;
    let stage1_index = spec
        .sections
        .iter()
        .position(|section| section.name == spec.stage1_section)
        .ok_or_else(|| BootImageError::MissingStage1(spec.stage1_section.clone()))?;

    let section_count = u16::try_from(spec.sections.len())
        .map_err(|_| BootImageError::TooManySections(spec.sections.len()))?;
    let records_bytes = usize::from(section_count)
        .checked_mul(BOOT_SECTION_RECORD_SIZE)
        .ok_or(BootImageError::IntegerOverflow("section table size"))?;
    let manifest_size = BOOT_MANIFEST_HEADER_SIZE
        .checked_add(records_bytes)
        .ok_or(BootImageError::IntegerOverflow("manifest size"))?;
    let data_start = align_up(
        u32::try_from(BOOT_DESCRIPTOR_SIZE + manifest_size)
            .map_err(|_| BootImageError::IntegerOverflow("data offset"))?,
        BOOT_DATA_ALIGNMENT,
    )?;

    let mut order = Vec::with_capacity(spec.sections.len());
    order.push(stage1_index);
    order.extend((0..spec.sections.len()).filter(|index| *index != stage1_index));
    let mut flash_offsets = vec![0; spec.sections.len()];
    let mut cursor = data_start;
    for index in order {
        let section = &spec.sections[index];
        if section.kind == SectionKind::Load {
            cursor = align_up(cursor, BOOT_DATA_ALIGNMENT)?;
            flash_offsets[index] = cursor;
            cursor = cursor
                .checked_add(u32_len(&section.data, "section file size")?)
                .ok_or(BootImageError::IntegerOverflow("package size"))?;
        }
    }
    let package_size = cursor;
    if u64::from(package_size) > u64::from(spec.target.payload_capacity_bytes()) {
        return Err(BootImageError::PackageTooLarge {
            bytes: u64::from(package_size),
            capacity: spec.target.payload_capacity_bytes(),
        });
    }

    let mut packed_sections = Vec::with_capacity(spec.sections.len());
    for (index, section) in spec.sections.iter().enumerate() {
        packed_sections.push(PackedSection {
            name: section.name.clone(),
            record: SectionRecord {
                kind: section.kind,
                flags: section.flags,
                flash_offset: flash_offsets[index],
                destination: section.destination,
                file_size_bytes: u32_len(&section.data, "section file size")?,
                memory_size_bytes: section.memory_size_bytes,
                alignment_bytes: section.alignment_bytes,
            },
        });
    }

    let manifest = BootManifest {
        target_package_size_bytes: package_size,
        application_entry: spec.application_entry,
        sections: packed_sections
            .iter()
            .map(|section| section.record)
            .collect(),
    };
    let manifest_bytes = manifest.encode()?;
    let stage1 = &packed_sections[stage1_index].record;
    let descriptor = BootDescriptor {
        target: spec.target,
        package_size_bytes: package_size,
        stage1_flash_offset: stage1.flash_offset,
        stage1_file_size_bytes: stage1.file_size_bytes,
        stage1_memory_size_bytes: stage1.memory_size_bytes,
        stage1_destination: stage1.destination,
        stage1_entry: spec.stage1_entry,
        manifest_flash_offset: BOOT_DESCRIPTOR_SIZE as u32,
        manifest_size_bytes: u32_len(&manifest_bytes, "manifest size")?,
        stage1_handoff_destination: PhysicalWordAddress::from_segment_offset(
            spec.stage1_entry.data_segment,
            STAGE1_HANDOFF_OFFSET,
        ),
    };

    let mut bytes = vec![0xff; package_size as usize];
    bytes[..BOOT_DESCRIPTOR_SIZE].copy_from_slice(&descriptor.encode());
    bytes[BOOT_DESCRIPTOR_SIZE..BOOT_DESCRIPTOR_SIZE + manifest_bytes.len()]
        .copy_from_slice(&manifest_bytes);
    for (section, packed) in spec.sections.iter().zip(&packed_sections) {
        if section.kind == SectionKind::Load {
            let start = packed.record.flash_offset as usize;
            let end = start + section.data.len();
            bytes[start..end].copy_from_slice(&section.data);
        }
    }

    Ok(BootImage {
        bytes,
        descriptor,
        manifest,
        sections: packed_sections,
    })
}

impl BootDescriptor {
    pub fn encode(self) -> [u8; BOOT_DESCRIPTOR_SIZE] {
        let mut bytes = [0; BOOT_DESCRIPTOR_SIZE];
        bytes[0..8].copy_from_slice(BOOT_MAGIC);
        put_u16(&mut bytes, 8, BOOT_FORMAT_VERSION);
        put_u16(&mut bytes, 10, BOOT_DESCRIPTOR_SIZE as u16);
        put_u32(&mut bytes, 12, self.target as u32);
        put_u32(&mut bytes, 16, self.package_size_bytes);
        put_u32(&mut bytes, 20, self.stage1_flash_offset);
        put_u32(&mut bytes, 24, self.stage1_file_size_bytes);
        put_u32(&mut bytes, 28, self.stage1_memory_size_bytes);
        put_u32(&mut bytes, 32, self.stage1_destination.get());
        put_entry(&mut bytes, 36, self.stage1_entry);
        put_u32(&mut bytes, 44, self.manifest_flash_offset);
        put_u32(&mut bytes, 48, self.manifest_size_bytes);
        // Offsets 52 and 56 held CRC32 fields before format version 3 and are
        // now reserved zero.
        put_u32(&mut bytes, 52, 0);
        put_u32(&mut bytes, 56, 0);
        put_u32(&mut bytes, 60, self.stage1_handoff_destination.get());
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BootImageError> {
        let bytes = bytes
            .get(..BOOT_DESCRIPTOR_SIZE)
            .ok_or(BootImageError::Truncated("descriptor"))?;
        if &bytes[0..8] != BOOT_MAGIC {
            return Err(BootImageError::InvalidMagic("descriptor"));
        }
        validate_version(bytes)?;
        if get_u16(bytes, 10) != BOOT_DESCRIPTOR_SIZE as u16 {
            return Err(BootImageError::InvalidFormat("descriptor size mismatch"));
        }
        // Offsets 52 and 56 are reserved zero since format version 3 and are
        // not interpreted.
        Ok(Self {
            target: BootTarget::from_raw(get_u32(bytes, 12))?,
            package_size_bytes: get_u32(bytes, 16),
            stage1_flash_offset: get_u32(bytes, 20),
            stage1_file_size_bytes: get_u32(bytes, 24),
            stage1_memory_size_bytes: get_u32(bytes, 28),
            stage1_destination: PhysicalWordAddress::new(get_u32(bytes, 32)),
            stage1_entry: get_entry(bytes, 36),
            manifest_flash_offset: get_u32(bytes, 44),
            manifest_size_bytes: get_u32(bytes, 48),
            stage1_handoff_destination: PhysicalWordAddress::new(get_u32(bytes, 60)),
        })
    }
}

impl BootManifest {
    pub fn encode(&self) -> Result<Vec<u8>, BootImageError> {
        let count = u16::try_from(self.sections.len())
            .map_err(|_| BootImageError::TooManySections(self.sections.len()))?;
        let records_size = self
            .sections
            .len()
            .checked_mul(BOOT_SECTION_RECORD_SIZE)
            .ok_or(BootImageError::IntegerOverflow("section table size"))?;
        let mut bytes = vec![0; BOOT_MANIFEST_HEADER_SIZE + records_size];
        bytes[0..8].copy_from_slice(MANIFEST_MAGIC);
        put_u16(&mut bytes, 8, BOOT_FORMAT_VERSION);
        put_u16(&mut bytes, 10, BOOT_MANIFEST_HEADER_SIZE as u16);
        put_u16(&mut bytes, 12, BOOT_SECTION_RECORD_SIZE as u16);
        put_u16(&mut bytes, 14, count);
        put_u32(&mut bytes, 16, self.target_package_size_bytes);
        put_entry(&mut bytes, 20, self.application_entry);
        put_u32(&mut bytes, 28, BOOT_MANIFEST_HEADER_SIZE as u32);
        put_u32(&mut bytes, 32, records_size as u32);
        // Offsets 36 and 40 held CRC32 fields before format version 3 and are
        // now reserved zero.
        put_u32(&mut bytes, 36, 0);
        put_u32(&mut bytes, 40, 0);

        for (index, section) in self.sections.iter().copied().enumerate() {
            let start = BOOT_MANIFEST_HEADER_SIZE + index * BOOT_SECTION_RECORD_SIZE;
            put_u16(&mut bytes, start, section.kind as u16);
            put_u16(&mut bytes, start + 2, section.flags);
            put_u32(&mut bytes, start + 4, section.flash_offset);
            put_u32(&mut bytes, start + 8, section.destination.get());
            put_u32(&mut bytes, start + 12, section.file_size_bytes);
            put_u32(&mut bytes, start + 16, section.memory_size_bytes);
            put_u32(&mut bytes, start + 20, section.alignment_bytes);
            // Record offset 24 held the file CRC32 before format version 3
            // and is now reserved zero.
            put_u32(&mut bytes, start + 24, 0);
        }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, BootImageError> {
        let header = bytes
            .get(..BOOT_MANIFEST_HEADER_SIZE)
            .ok_or(BootImageError::Truncated("manifest header"))?;
        if &header[0..8] != MANIFEST_MAGIC {
            return Err(BootImageError::InvalidMagic("manifest"));
        }
        validate_version(header)?;
        if get_u16(header, 10) != BOOT_MANIFEST_HEADER_SIZE as u16
            || get_u16(header, 12) != BOOT_SECTION_RECORD_SIZE as u16
        {
            return Err(BootImageError::InvalidFormat(
                "manifest or section record size mismatch",
            ));
        }
        let count = usize::from(get_u16(header, 14));
        let records_offset = get_u32(header, 28) as usize;
        let records_size = get_u32(header, 32) as usize;
        let expected_size = count
            .checked_mul(BOOT_SECTION_RECORD_SIZE)
            .ok_or(BootImageError::InvalidFormat("section table overflow"))?;
        if records_offset != BOOT_MANIFEST_HEADER_SIZE || records_size != expected_size {
            return Err(BootImageError::InvalidFormat(
                "section table extent mismatch",
            ));
        }
        let complete = bytes
            .get(..records_offset + records_size)
            .ok_or(BootImageError::Truncated("section table"))?;
        // Header offsets 36 and 40 and record offset 24 are reserved zero
        // since format version 3 and are not interpreted.

        let mut sections = Vec::with_capacity(count);
        for index in 0..count {
            let start = records_offset + index * BOOT_SECTION_RECORD_SIZE;
            sections.push(SectionRecord {
                kind: SectionKind::from_raw(get_u16(complete, start))?,
                flags: get_u16(complete, start + 2),
                flash_offset: get_u32(complete, start + 4),
                destination: PhysicalWordAddress::new(get_u32(complete, start + 8)),
                file_size_bytes: get_u32(complete, start + 12),
                memory_size_bytes: get_u32(complete, start + 16),
                alignment_bytes: get_u32(complete, start + 20),
            });
        }
        Ok(Self {
            target_package_size_bytes: get_u32(header, 16),
            application_entry: get_entry(header, 20),
            sections,
        })
    }
}

fn validate_entries(spec: &BootImageSpec) -> Result<(), BootImageError> {
    for (stage, entry) in [
        ("stage1", spec.stage1_entry),
        ("application", spec.application_entry),
    ] {
        if entry.stack_offset == 0 || entry.stack_offset > MMIO_BASE {
            return Err(BootImageError::StackInMmio {
                stage,
                offset: entry.stack_offset,
            });
        }
        let first_stack_word = PhysicalWordAddress::from_segment_offset(
            entry.data_segment,
            entry.stack_offset.wrapping_sub(1),
        );
        if first_stack_word.get() >= spec.target.physical_memory_words() {
            return Err(BootImageError::StackOutsidePhysicalMemory {
                stage,
                address: first_stack_word,
            });
        }
    }
    Ok(())
}

fn validate_sections(spec: &BootImageSpec) -> Result<(), BootImageError> {
    let mut names = HashSet::new();
    let capacity_bytes = u64::from(spec.target.physical_memory_words()) * 2;
    let mut ranges = Vec::with_capacity(spec.sections.len() + 1);
    let handoff = PhysicalWordAddress::from_segment_offset(
        spec.stage1_entry.data_segment,
        STAGE1_HANDOFF_OFFSET,
    );
    let handoff_start = handoff.byte_address();
    let handoff_end = handoff_start + u64::from(STAGE1_HANDOFF_SIZE_BYTES);
    if handoff_end > capacity_bytes {
        return Err(BootImageError::PhysicalMemoryExceeded {
            section: "<stage1-handoff>".to_string(),
            end_byte: handoff_end,
            capacity_bytes,
        });
    }
    ranges.push((handoff_start, handoff_end, "<stage1-handoff>".to_string()));
    let scratch_start = u64::from(STAGE0_DESCRIPTOR_SCRATCH_WORD) * 2;
    let scratch_end = scratch_start + BOOT_DESCRIPTOR_SIZE as u64;
    if scratch_end > capacity_bytes {
        return Err(BootImageError::PhysicalMemoryExceeded {
            section: "<stage0-descriptor-scratch>".to_string(),
            end_byte: scratch_end,
            capacity_bytes,
        });
    }
    ranges.push((
        scratch_start,
        scratch_end,
        "<stage0-descriptor-scratch>".to_string(),
    ));
    for section in &spec.sections {
        if section.name.is_empty() {
            return Err(BootImageError::EmptySectionName);
        }
        if !names.insert(section.name.clone()) {
            return Err(BootImageError::DuplicateSection(section.name.clone()));
        }
        if section.alignment_bytes == 0 || !section.alignment_bytes.is_power_of_two() {
            return Err(BootImageError::InvalidAlignment {
                section: section.name.clone(),
                alignment: section.alignment_bytes,
            });
        }
        let start = section.destination.byte_address();
        if start % u64::from(section.alignment_bytes) != 0 {
            return Err(BootImageError::MisalignedDestination {
                section: section.name.clone(),
                byte_address: start,
                alignment: section.alignment_bytes,
            });
        }
        if section.data.len() > section.memory_size_bytes as usize {
            return Err(BootImageError::MemorySmallerThanFile {
                section: section.name.clone(),
                file_bytes: section.data.len(),
                memory_bytes: section.memory_size_bytes,
            });
        }
        if section.memory_size_bytes == 0 {
            return Err(BootImageError::EmptyMemorySection(section.name.clone()));
        }
        match section.kind {
            SectionKind::Load if section.data.is_empty() => {
                return Err(BootImageError::EmptyLoadSection(section.name.clone()));
            }
            SectionKind::Zero if !section.data.is_empty() => {
                return Err(BootImageError::ZeroSectionContainsData(
                    section.name.clone(),
                ));
            }
            _ => {}
        }
        let end = start
            .checked_add(u64::from(section.memory_size_bytes))
            .ok_or_else(|| BootImageError::PhysicalRangeOverflow(section.name.clone()))?;
        if end > capacity_bytes {
            return Err(BootImageError::PhysicalMemoryExceeded {
                section: section.name.clone(),
                end_byte: end,
                capacity_bytes,
            });
        }
        ranges.push((start, end, section.name.clone()));
    }

    ranges.sort_by_key(|range| range.0);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(BootImageError::OverlappingSections {
                first: pair[0].2.clone(),
                second: pair[1].2.clone(),
            });
        }
    }

    validate_entry_in_section("stage1", spec.stage1_entry, &spec.sections, |section| {
        section.name == spec.stage1_section
    })?;
    validate_entry_in_section(
        "application",
        spec.application_entry,
        &spec.sections,
        |section| section.name != spec.stage1_section,
    )
}

fn validate_entry_in_section(
    stage: &'static str,
    entry: BootEntry,
    sections: &[InputSection],
    select: impl Fn(&InputSection) -> bool,
) -> Result<(), BootImageError> {
    let address = entry.physical_entry().get();
    let byte = u64::from(address) * 2;
    let contained = sections.iter().any(|section| {
        let start = section.destination.byte_address();
        let end = start + section.data.len() as u64;
        select(section)
            && section.kind == SectionKind::Load
            && section.flags & SECTION_EXECUTE != 0
            && byte >= start
            && byte < end
    });
    if contained {
        Ok(())
    } else {
        Err(BootImageError::EntryOutsideExecutableSection { stage, address })
    }
}

fn align_up(value: u32, alignment: u32) -> Result<u32, BootImageError> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or(BootImageError::IntegerOverflow("alignment"))
}

fn u32_len(bytes: &[u8], field: &'static str) -> Result<u32, BootImageError> {
    u32::try_from(bytes.len()).map_err(|_| BootImageError::IntegerOverflow(field))
}

fn validate_version(bytes: &[u8]) -> Result<(), BootImageError> {
    let version = get_u16(bytes, 8);
    if version == BOOT_FORMAT_VERSION {
        Ok(())
    } else {
        Err(BootImageError::UnsupportedVersion {
            found: version,
            expected: BOOT_FORMAT_VERSION,
        })
    }
}

fn put_entry(bytes: &mut [u8], offset: usize, entry: BootEntry) {
    put_u16(bytes, offset, entry.code_segment);
    put_u16(bytes, offset + 2, entry.offset);
    put_u16(bytes, offset + 4, entry.data_segment);
    put_u16(bytes, offset + 6, entry.stack_offset);
}

fn get_entry(bytes: &[u8], offset: usize) -> BootEntry {
    BootEntry {
        code_segment: get_u16(bytes, offset),
        offset: get_u16(bytes, offset + 2),
        data_segment: get_u16(bytes, offset + 4),
        stack_offset: get_u16(bytes, offset + 6),
    }
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn get_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn get_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(segment: u16, offset: u16, data_segment: u16) -> BootEntry {
        BootEntry {
            code_segment: segment,
            offset,
            data_segment,
            stack_offset: 0xf000,
        }
    }

    fn section(
        name: &str,
        destination: u32,
        data: &[u8],
        memory_size_bytes: u32,
        flags: u16,
    ) -> InputSection {
        InputSection {
            name: name.into(),
            kind: SectionKind::Load,
            flags,
            destination: PhysicalWordAddress::new(destination),
            data: data.into(),
            memory_size_bytes,
            alignment_bytes: 32,
        }
    }

    fn example_spec() -> BootImageSpec {
        BootImageSpec {
            target: BootTarget::TangNano20K,
            stage1_section: "stage1".into(),
            stage1_entry: entry(1, 0x0100, 2),
            application_entry: entry(3, 0x0200, 4),
            sections: vec![
                section(
                    "application",
                    0x0003_0200,
                    &[0x34, 0x12, 0x00, 0xe8],
                    4,
                    SECTION_READ | SECTION_EXECUTE,
                ),
                section(
                    "stage1",
                    0x0001_0100,
                    &[0xaa; 48],
                    64,
                    SECTION_READ | SECTION_EXECUTE,
                ),
                InputSection {
                    name: "bss".into(),
                    kind: SectionKind::Zero,
                    flags: SECTION_READ | SECTION_WRITE,
                    destination: PhysicalWordAddress::new(0x0004_4000),
                    data: vec![],
                    memory_size_bytes: 128,
                    alignment_bytes: 32,
                },
            ],
        }
    }

    #[test]
    fn package_round_trips_descriptor_manifest_and_section_data() {
        let image = build_boot_image(example_spec()).unwrap();
        assert_eq!(
            image.bytes.len(),
            image.descriptor.package_size_bytes as usize
        );
        assert_eq!(
            BootDescriptor::decode(&image.bytes).unwrap(),
            image.descriptor
        );

        let manifest_start = image.descriptor.manifest_flash_offset as usize;
        let manifest_end = manifest_start + image.descriptor.manifest_size_bytes as usize;
        assert_eq!(
            BootManifest::decode(&image.bytes[manifest_start..manifest_end]).unwrap(),
            image.manifest
        );
        assert_eq!(image.sections[0].name, "application");
        assert_eq!(image.sections[1].name, "stage1");
        let stage1 = image.sections[1].record;
        assert_eq!(stage1.flash_offset, BOOT_DATA_ALIGNMENT);
        assert_eq!(
            &image.bytes[stage1.flash_offset as usize
                ..(stage1.flash_offset + stage1.file_size_bytes) as usize],
            &[0xaa; 48]
        );
        assert!(image.map().contains("application entry: 0003:0200"));
    }

    #[test]
    fn reserved_zero_fields_stay_zero_and_are_ignored_on_decode() {
        let mut image = build_boot_image(example_spec()).unwrap();
        assert_eq!(&image.bytes[52..60], &[0; 8]);
        assert_eq!(&image.bytes[64 + 36..64 + 44], &[0; 8]);
        let record_crc_offset = 64 + BOOT_MANIFEST_HEADER_SIZE + 24;
        assert_eq!(&image.bytes[record_crc_offset..record_crc_offset + 4], &[0; 4]);

        // Decoding tolerates garbage in the reserved fields.
        image.bytes[52] = 0xff;
        image.bytes[56] = 0xff;
        image.bytes[64 + 36] = 0xff;
        image.bytes[64 + 40] = 0xff;
        image.bytes[record_crc_offset] = 0xff;
        assert_eq!(
            BootDescriptor::decode(&image.bytes).unwrap(),
            image.descriptor
        );
        let manifest_start = image.descriptor.manifest_flash_offset as usize;
        let manifest_end = manifest_start + image.descriptor.manifest_size_bytes as usize;
        assert_eq!(
            BootManifest::decode(&image.bytes[manifest_start..manifest_end]).unwrap(),
            image.manifest
        );
    }

    #[test]
    fn descriptor_magic_is_checked_before_offsets_are_trusted() {
        let mut image = build_boot_image(example_spec()).unwrap().bytes;
        image[0] ^= 1;
        assert_eq!(
            BootDescriptor::decode(&image),
            Err(BootImageError::InvalidMagic("descriptor"))
        );
    }

    #[test]
    fn descriptor_scratch_range_is_reserved_against_all_sections() {
        let mut spec = example_spec();
        spec.sections[2].destination =
            PhysicalWordAddress::new(STAGE0_DESCRIPTOR_SCRATCH_WORD - 16);
        assert!(matches!(
            build_boot_image(spec),
            Err(BootImageError::OverlappingSections { first, second })
                if first == "bss" && second == "<stage0-descriptor-scratch>"
        ));
    }

    #[test]
    fn invalid_physical_layouts_fail_with_section_names() {
        let mut spec = example_spec();
        spec.sections[0].destination = spec.sections[1].destination;
        assert_eq!(
            build_boot_image(spec),
            Err(BootImageError::OverlappingSections {
                first: "application".into(),
                second: "stage1".into(),
            })
        );

        let mut spec = example_spec();
        spec.sections[2].destination = PhysicalWordAddress::new(TANG_NANO_20K_SDRAM_WORDS - 16);
        assert!(matches!(
            build_boot_image(spec),
            Err(BootImageError::PhysicalMemoryExceeded { section, .. }) if section == "bss"
        ));
    }

    #[test]
    fn entry_must_land_in_its_executable_section() {
        let mut spec = example_spec();
        spec.application_entry.offset = 0x0300;
        assert_eq!(
            build_boot_image(spec),
            Err(BootImageError::EntryOutsideExecutableSection {
                stage: "application",
                address: 0x0003_0300,
            })
        );
    }

    #[test]
    fn complete_flash_image_preserves_the_reserved_configuration_region() {
        let image = build_boot_image(example_spec()).unwrap();
        let configuration = vec![0x5a; 577_178];
        let placed = image.place_after_configuration(&configuration).unwrap();
        assert_eq!(placed.payload_offset, 0x0010_0000);
        assert_eq!(&placed.bytes[..configuration.len()], &configuration);
        assert!(placed.bytes[configuration.len()..0x0010_0000]
            .iter()
            .all(|byte| *byte == 0xff));
        assert_eq!(&placed.bytes[0x0010_0000..], &image.bytes);
    }
}
