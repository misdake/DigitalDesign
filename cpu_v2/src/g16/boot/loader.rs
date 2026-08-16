//! Reference boot loaders and transaction-level Flash-to-SDRAM DMA model.

use std::fmt;

use super::{
    BootDescriptor, BootEntry, BootImageError, BootManifest, BootTarget, SectionKind,
    SectionRecord, BOOT_MANIFEST_HEADER_SIZE, BOOT_SECTION_RECORD_SIZE, SECTION_EXECUTE,
};
use crate::g16::{
    jump_segment, load_immediate16, write_data_segment, Machine, PhysicalWordAddress,
    ProgramLoadError, Word, MMIO_BASE,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DmaCommand {
    pub flash_offset: u32,
    pub destination: PhysicalWordAddress,
    pub file_size_bytes: u32,
    pub memory_size_bytes: u32,
    pub expected_crc32: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaError {
    Busy,
    FileLargerThanMemory {
        file_bytes: u32,
        memory_bytes: u32,
    },
    FlashRangeExceeded {
        offset: u32,
        bytes: u32,
        available: usize,
    },
    PhysicalMemoryExceeded {
        destination: PhysicalWordAddress,
        memory_bytes: u32,
        available_words: usize,
    },
    CrcMismatch {
        expected: u32,
        actual: u32,
    },
}

impl fmt::Display for DmaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => write!(f, "Flash-to-SDRAM DMA is already busy"),
            Self::FileLargerThanMemory {
                file_bytes,
                memory_bytes,
            } => write!(
                f,
                "DMA file size {file_bytes:#x} exceeds memory size {memory_bytes:#x}"
            ),
            Self::FlashRangeExceeded {
                offset,
                bytes,
                available,
            } => write!(
                f,
                "DMA Flash range {offset:#010x}+{bytes:#x} exceeds {available:#x} available bytes"
            ),
            Self::PhysicalMemoryExceeded {
                destination,
                memory_bytes,
                available_words,
            } => write!(
                f,
                "DMA SDRAM range at word {:#010x} for {memory_bytes:#x} bytes exceeds {available_words:#x} physical words",
                destination.get()
            ),
            Self::CrcMismatch { expected, actual } => write!(
                f,
                "DMA CRC mismatch: expected {expected:08x}, calculated {actual:08x}"
            ),
        }
    }
}

impl std::error::Error for DmaError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DmaStatus {
    Idle,
    Busy,
    Done,
    Error(DmaError),
}

#[derive(Clone, Copy, Debug)]
struct ActiveDma {
    command: DmaCommand,
    next_word: u32,
    crc: u32,
}

/// Transaction-level model of the boot copy engine.
///
/// One accepted tick writes one 16-bit SDRAM word. A false `dram_ready` holds
/// all state, matching the backpressure contract without simulating SPI edges.
#[derive(Default)]
pub struct FlashToDramDma {
    active: Option<ActiveDma>,
    status: Option<DmaStatus>,
}

impl FlashToDramDma {
    pub fn status(&self) -> DmaStatus {
        self.active.map_or_else(
            || self.status.unwrap_or(DmaStatus::Idle),
            |_| DmaStatus::Busy,
        )
    }

    pub fn start(
        &mut self,
        command: DmaCommand,
        flash_bytes: usize,
        physical_memory_words: usize,
    ) -> Result<(), DmaError> {
        if self.active.is_some() {
            return Err(DmaError::Busy);
        }
        validate_dma(command, flash_bytes, physical_memory_words)?;
        self.status = None;
        self.active = Some(ActiveDma {
            command,
            next_word: 0,
            crc: !0,
        });
        if command.memory_size_bytes == 0 {
            self.finish();
        }
        Ok(())
    }

    pub fn tick(&mut self, flash: &[u8], memory: &mut Machine, dram_ready: bool) -> DmaStatus {
        let Some(mut active) = self.active else {
            return self.status();
        };
        if !dram_ready {
            return DmaStatus::Busy;
        }

        let byte_index = active.next_word * 2;
        let mut bytes = [0; 2];
        for lane in 0..2u32 {
            let index = byte_index + lane;
            if index < active.command.file_size_bytes {
                let byte = flash[(active.command.flash_offset + index) as usize];
                bytes[lane as usize] = byte;
                active.crc = crc32_update(active.crc, byte);
            }
        }
        let address = PhysicalWordAddress::new(active.command.destination.get() + active.next_word);
        let value = u16::from_le_bytes(bytes);
        memory
            .load_physical(address, &[value])
            .expect("DMA range was validated before the transfer started");
        active.next_word += 1;

        let memory_words = active.command.memory_size_bytes.div_ceil(2);
        if active.next_word == memory_words {
            let actual = !active.crc;
            self.active = None;
            if actual == active.command.expected_crc32 {
                self.status = Some(DmaStatus::Done);
            } else {
                self.status = Some(DmaStatus::Error(DmaError::CrcMismatch {
                    expected: active.command.expected_crc32,
                    actual,
                }));
            }
        } else {
            self.active = Some(active);
        }
        self.status()
    }

    fn finish(&mut self) {
        let active = self.active.take().unwrap();
        let actual = !active.crc;
        self.status = Some(if actual == active.command.expected_crc32 {
            DmaStatus::Done
        } else {
            DmaStatus::Error(DmaError::CrcMismatch {
                expected: active.command.expected_crc32,
                actual,
            })
        });
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoaderError {
    Format(BootImageError),
    Dma(DmaError),
    TargetMismatch {
        expected: BootTarget,
        found: BootTarget,
    },
    ExtentOutsidePackage(&'static str),
    PackageLargerThanFlash {
        package_bytes: u32,
        flash_bytes: usize,
    },
    ManifestPackageSizeMismatch {
        descriptor: u32,
        manifest: u32,
    },
    Stage1RecordMismatch,
    InvalidSection {
        index: usize,
        reason: &'static str,
    },
    OverlappingSections {
        first: usize,
        second: usize,
    },
    EntryOutsideExecutableSection {
        address: PhysicalWordAddress,
    },
    InvalidInitialStack {
        stage: &'static str,
        data_segment: Word,
        stack_offset: Word,
    },
    ProgramLoad(ProgramLoadError),
}

impl fmt::Display for LoaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(error) => error.fmt(f),
            Self::Dma(error) => error.fmt(f),
            Self::TargetMismatch { expected, found } => {
                write!(f, "boot target mismatch: expected {expected:?}, found {found:?}")
            }
            Self::ExtentOutsidePackage(name) => {
                write!(f, "boot {name} extent is outside the declared package")
            }
            Self::PackageLargerThanFlash {
                package_bytes,
                flash_bytes,
            } => write!(
                f,
                "declared package size {package_bytes:#x} exceeds {flash_bytes:#x} available Flash bytes"
            ),
            Self::ManifestPackageSizeMismatch {
                descriptor,
                manifest,
            } => write!(
                f,
                "manifest package size {manifest:#x} differs from descriptor size {descriptor:#x}"
            ),
            Self::Stage1RecordMismatch => {
                write!(f, "manifest does not contain exactly one matching Stage1 section")
            }
            Self::InvalidSection { index, reason } => {
                write!(f, "section {index} is invalid: {reason}")
            }
            Self::OverlappingSections { first, second } => {
                write!(f, "physical sections {first} and {second} overlap")
            }
            Self::EntryOutsideExecutableSection { address } => write!(
                f,
                "application entry word {:#010x} is outside executable file data",
                address.get()
            ),
            Self::InvalidInitialStack {
                stage,
                data_segment,
                stack_offset,
            } => write!(
                f,
                "{stage} initial stack {data_segment:04x}:{stack_offset:04x} is outside usable physical data memory"
            ),
            Self::ProgramLoad(error) => write!(
                f,
                "cannot write physical words at {:#010x}: {} words exceed memory",
                error.base.get(), error.words
            ),
        }
    }
}

impl std::error::Error for LoaderError {}

impl From<BootImageError> for LoaderError {
    fn from(value: BootImageError) -> Self {
        Self::Format(value)
    }
}

impl From<DmaError> for LoaderError {
    fn from(value: DmaError) -> Self {
        Self::Dma(value)
    }
}

impl From<ProgramLoadError> for LoaderError {
    fn from(value: ProgramLoadError) -> Self {
        Self::ProgramLoad(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stage0Handoff {
    pub descriptor: BootDescriptor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplicationHandoff {
    pub entry: BootEntry,
}

impl ApplicationHandoff {
    /// Canonical final sequence used by Stage1 after its last memory access.
    pub fn instructions(self) -> Vec<Word> {
        let mut words = vec![];
        words.extend(load_immediate16(1, self.entry.data_segment));
        words.extend(load_immediate16(13, self.entry.stack_offset));
        words.extend(load_immediate16(2, self.entry.code_segment));
        words.extend(load_immediate16(3, self.entry.offset));
        words.extend([write_data_segment(1), jump_segment(2, 3)]);
        words
    }
}

pub fn run_stage0(
    flash: &[u8],
    memory: &mut Machine,
    expected_target: BootTarget,
) -> Result<Stage0Handoff, LoaderError> {
    let descriptor = BootDescriptor::decode(flash)?;
    validate_stage0_descriptor(&descriptor, flash, memory, expected_target)?;
    run_dma(
        flash,
        memory,
        DmaCommand {
            flash_offset: descriptor.stage1_flash_offset,
            destination: descriptor.stage1_destination,
            file_size_bytes: descriptor.stage1_file_size_bytes,
            memory_size_bytes: descriptor.stage1_memory_size_bytes,
            expected_crc32: descriptor.stage1_crc32,
        },
    )?;
    Ok(Stage0Handoff { descriptor })
}

pub fn run_stage1(
    flash: &[u8],
    memory: &mut Machine,
    stage0: Stage0Handoff,
) -> Result<ApplicationHandoff, LoaderError> {
    let descriptor = stage0.descriptor;
    let manifest_end = extent_end(
        descriptor.manifest_flash_offset,
        descriptor.manifest_size_bytes,
        descriptor.package_size_bytes,
        "manifest",
    )?;
    let manifest = BootManifest::decode(
        &flash[descriptor.manifest_flash_offset as usize..manifest_end as usize],
    )?;
    validate_manifest(&descriptor, &manifest, memory.physical_memory_words())?;

    for section in &manifest.sections {
        if matches_stage1(section, &descriptor) {
            continue;
        }
        run_dma(flash, memory, command_for_section(*section))?;
    }
    Ok(ApplicationHandoff {
        entry: manifest.application_entry,
    })
}

fn run_dma(flash: &[u8], memory: &mut Machine, command: DmaCommand) -> Result<(), LoaderError> {
    let mut dma = FlashToDramDma::default();
    dma.start(command, flash.len(), memory.physical_memory_words())?;
    loop {
        match dma.tick(flash, memory, true) {
            DmaStatus::Busy => {}
            DmaStatus::Done => return Ok(()),
            DmaStatus::Error(error) => return Err(error.into()),
            DmaStatus::Idle => unreachable!("started DMA returned to idle"),
        }
    }
}

fn command_for_section(section: SectionRecord) -> DmaCommand {
    DmaCommand {
        flash_offset: section.flash_offset,
        destination: section.destination,
        file_size_bytes: section.file_size_bytes,
        memory_size_bytes: section.memory_size_bytes,
        expected_crc32: section.crc32,
    }
}

fn validate_stage0_descriptor(
    descriptor: &BootDescriptor,
    flash: &[u8],
    memory: &Machine,
    expected_target: BootTarget,
) -> Result<(), LoaderError> {
    if descriptor.target != expected_target {
        return Err(LoaderError::TargetMismatch {
            expected: expected_target,
            found: descriptor.target,
        });
    }
    if descriptor.package_size_bytes as usize > flash.len() {
        return Err(LoaderError::PackageLargerThanFlash {
            package_bytes: descriptor.package_size_bytes,
            flash_bytes: flash.len(),
        });
    }
    extent_end(
        descriptor.stage1_flash_offset,
        descriptor.stage1_file_size_bytes,
        descriptor.package_size_bytes,
        "Stage1 Flash",
    )?;
    extent_end(
        descriptor.manifest_flash_offset,
        descriptor.manifest_size_bytes,
        descriptor.package_size_bytes,
        "manifest",
    )?;
    validate_dma(
        DmaCommand {
            flash_offset: descriptor.stage1_flash_offset,
            destination: descriptor.stage1_destination,
            file_size_bytes: descriptor.stage1_file_size_bytes,
            memory_size_bytes: descriptor.stage1_memory_size_bytes,
            expected_crc32: descriptor.stage1_crc32,
        },
        descriptor.package_size_bytes as usize,
        memory.physical_memory_words(),
    )?;
    let entry_byte = descriptor.stage1_entry.physical_entry().byte_address();
    let start = descriptor.stage1_destination.byte_address();
    let end = start + u64::from(descriptor.stage1_file_size_bytes);
    if entry_byte < start || entry_byte >= end {
        return Err(LoaderError::EntryOutsideExecutableSection {
            address: descriptor.stage1_entry.physical_entry(),
        });
    }
    validate_initial_stack(
        "stage1",
        descriptor.stage1_entry,
        memory.physical_memory_words(),
    )?;
    Ok(())
}

fn validate_manifest(
    descriptor: &BootDescriptor,
    manifest: &BootManifest,
    physical_memory_words: usize,
) -> Result<(), LoaderError> {
    if manifest.target_package_size_bytes != descriptor.package_size_bytes {
        return Err(LoaderError::ManifestPackageSizeMismatch {
            descriptor: descriptor.package_size_bytes,
            manifest: manifest.target_package_size_bytes,
        });
    }
    let encoded_manifest_size = BOOT_MANIFEST_HEADER_SIZE
        .checked_add(manifest.sections.len() * BOOT_SECTION_RECORD_SIZE)
        .expect("manifest section count is encoded as u16");
    if descriptor.manifest_size_bytes as usize != encoded_manifest_size {
        return Err(LoaderError::ExtentOutsidePackage("manifest size"));
    }
    let mut ranges = vec![];
    let mut stage1_matches = 0;
    for (index, section) in manifest.sections.iter().copied().enumerate() {
        if section.alignment_bytes == 0 || !section.alignment_bytes.is_power_of_two() {
            return Err(LoaderError::InvalidSection {
                index,
                reason: "alignment is not a nonzero power of two",
            });
        }
        if section.memory_size_bytes == 0 {
            return Err(LoaderError::InvalidSection {
                index,
                reason: "memory extent is empty",
            });
        }
        if section.destination.byte_address() % u64::from(section.alignment_bytes) != 0 {
            return Err(LoaderError::InvalidSection {
                index,
                reason: "physical destination is misaligned",
            });
        }
        match section.kind {
            SectionKind::Load if section.file_size_bytes == 0 => {
                return Err(LoaderError::InvalidSection {
                    index,
                    reason: "load section has no file data",
                })
            }
            SectionKind::Zero
                if section.file_size_bytes != 0
                    || section.flash_offset != 0
                    || section.crc32 != 0 =>
            {
                return Err(LoaderError::InvalidSection {
                    index,
                    reason: "zero section contains file metadata",
                })
            }
            _ => {}
        }
        validate_dma(
            command_for_section(section),
            descriptor.package_size_bytes as usize,
            physical_memory_words,
        )
        .map_err(LoaderError::Dma)?;
        let start = section.destination.byte_address();
        let end = start + u64::from(section.memory_size_bytes);
        ranges.push((start, end, index));
        if matches_stage1(&section, descriptor) {
            stage1_matches += 1;
        }
    }
    ranges.sort_by_key(|range| range.0);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(LoaderError::OverlappingSections {
                first: pair[0].2,
                second: pair[1].2,
            });
        }
    }
    if stage1_matches != 1 {
        return Err(LoaderError::Stage1RecordMismatch);
    }
    validate_initial_stack(
        "application",
        manifest.application_entry,
        physical_memory_words,
    )?;
    let entry = manifest.application_entry.physical_entry();
    let entry_byte = entry.byte_address();
    let executable = manifest.sections.iter().any(|section| {
        section.kind == SectionKind::Load
            && section.flags & SECTION_EXECUTE != 0
            && !matches_stage1(section, descriptor)
            && entry_byte >= section.destination.byte_address()
            && entry_byte < section.destination.byte_address() + u64::from(section.file_size_bytes)
    });
    if !executable {
        return Err(LoaderError::EntryOutsideExecutableSection { address: entry });
    }
    Ok(())
}

fn matches_stage1(section: &SectionRecord, descriptor: &BootDescriptor) -> bool {
    section.kind == SectionKind::Load
        && section.flags & SECTION_EXECUTE != 0
        && section.flash_offset == descriptor.stage1_flash_offset
        && section.destination == descriptor.stage1_destination
        && section.file_size_bytes == descriptor.stage1_file_size_bytes
        && section.memory_size_bytes == descriptor.stage1_memory_size_bytes
        && section.crc32 == descriptor.stage1_crc32
}

fn validate_initial_stack(
    stage: &'static str,
    entry: BootEntry,
    physical_memory_words: usize,
) -> Result<(), LoaderError> {
    if entry.stack_offset == 0 || entry.stack_offset > MMIO_BASE {
        return Err(LoaderError::InvalidInitialStack {
            stage,
            data_segment: entry.data_segment,
            stack_offset: entry.stack_offset,
        });
    }
    let first_word =
        PhysicalWordAddress::from_segment_offset(entry.data_segment, entry.stack_offset - 1);
    if first_word.get() as usize >= physical_memory_words {
        return Err(LoaderError::InvalidInitialStack {
            stage,
            data_segment: entry.data_segment,
            stack_offset: entry.stack_offset,
        });
    }
    Ok(())
}

fn validate_dma(
    command: DmaCommand,
    flash_bytes: usize,
    physical_memory_words: usize,
) -> Result<(), DmaError> {
    if command.file_size_bytes > command.memory_size_bytes {
        return Err(DmaError::FileLargerThanMemory {
            file_bytes: command.file_size_bytes,
            memory_bytes: command.memory_size_bytes,
        });
    }
    let flash_end = command
        .flash_offset
        .checked_add(command.file_size_bytes)
        .map(usize::try_from)
        .and_then(Result::ok)
        .filter(|end| *end <= flash_bytes);
    if flash_end.is_none() {
        return Err(DmaError::FlashRangeExceeded {
            offset: command.flash_offset,
            bytes: command.file_size_bytes,
            available: flash_bytes,
        });
    }
    let memory_words = command.memory_size_bytes.div_ceil(2);
    let end = command
        .destination
        .get()
        .checked_add(memory_words)
        .map(u64::from)
        .filter(|end| *end <= physical_memory_words as u64);
    if end.is_none() {
        return Err(DmaError::PhysicalMemoryExceeded {
            destination: command.destination,
            memory_bytes: command.memory_size_bytes,
            available_words: physical_memory_words,
        });
    }
    Ok(())
}

fn extent_end(
    offset: u32,
    size: u32,
    package_size: u32,
    name: &'static str,
) -> Result<u32, LoaderError> {
    offset
        .checked_add(size)
        .filter(|end| *end <= package_size)
        .ok_or(LoaderError::ExtentOutsidePackage(name))
}

fn crc32_update(mut crc: u32, byte: u8) -> u32 {
    crc ^= u32::from(byte);
    for _ in 0..8 {
        crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g16::boot::{
        build_boot_image, crc32, BootImageSpec, InputSection, SECTION_READ, SECTION_WRITE,
    };
    use crate::g16::{halt, load};

    fn section(
        name: &str,
        destination: u32,
        data: Vec<u8>,
        memory_size_bytes: u32,
        flags: u16,
    ) -> InputSection {
        InputSection {
            name: name.into(),
            kind: SectionKind::Load,
            flags,
            destination: PhysicalWordAddress::new(destination),
            data,
            memory_size_bytes,
            alignment_bytes: 32,
        }
    }

    fn words_bytes(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn image() -> Vec<u8> {
        let application = words_bytes(&[load(0, 0, 0), halt()]);
        build_boot_image(BootImageSpec {
            target: BootTarget::TangNano20K,
            stage1_section: "stage1".into(),
            stage1_entry: BootEntry {
                code_segment: 1,
                offset: 0x0100,
                data_segment: 2,
                stack_offset: 0xf000,
            },
            application_entry: BootEntry {
                code_segment: 3,
                offset: 0x0200,
                data_segment: 4,
                stack_offset: 0xe000,
            },
            sections: vec![
                section(
                    "stage1",
                    0x0001_0100,
                    vec![0xaa; 64],
                    96,
                    SECTION_READ | SECTION_EXECUTE,
                ),
                section(
                    "application",
                    0x0003_0200,
                    application,
                    4,
                    SECTION_READ | SECTION_EXECUTE,
                ),
                section(
                    "data",
                    0x0004_0000,
                    vec![0xef, 0xbe, 0x55],
                    8,
                    SECTION_READ | SECTION_WRITE,
                ),
                InputSection {
                    name: "bss".into(),
                    kind: SectionKind::Zero,
                    flags: SECTION_READ | SECTION_WRITE,
                    destination: PhysicalWordAddress::new(0x0004_0100),
                    data: vec![],
                    memory_size_bytes: 64,
                    alignment_bytes: 32,
                },
            ],
        })
        .unwrap()
        .bytes
    }

    #[test]
    fn dma_holds_state_during_backpressure_and_zero_fills_the_tail() {
        let flash = [0x11, 0x22, 0x33];
        let mut memory = Machine::with_physical_memory_words(64);
        let mut dma = FlashToDramDma::default();
        dma.start(
            DmaCommand {
                flash_offset: 0,
                destination: PhysicalWordAddress::new(5),
                file_size_bytes: 3,
                memory_size_bytes: 6,
                expected_crc32: crc32(&flash),
            },
            flash.len(),
            memory.physical_memory_words(),
        )
        .unwrap();
        assert_eq!(dma.tick(&flash, &mut memory, false), DmaStatus::Busy);
        assert_eq!(memory.physical_memory(5.into()), 0);
        assert_eq!(dma.tick(&flash, &mut memory, true), DmaStatus::Busy);
        assert_eq!(memory.physical_memory(5.into()), 0x2211);
        assert_eq!(dma.tick(&flash, &mut memory, true), DmaStatus::Busy);
        assert_eq!(memory.physical_memory(6.into()), 0x0033);
        assert_eq!(dma.tick(&flash, &mut memory, true), DmaStatus::Done);
        assert_eq!(memory.physical_memory(7.into()), 0);
    }

    #[test]
    fn stage0_and_stage1_load_then_enter_the_segmented_application() {
        let flash = image();
        let mut memory = Machine::default();
        let stage0 = run_stage0(&flash, &mut memory, BootTarget::TangNano20K).unwrap();
        assert_eq!(
            memory.physical_memory(PhysicalWordAddress::new(0x0001_0100)),
            0xaaaa
        );
        assert_eq!(
            memory.physical_memory(PhysicalWordAddress::new(0x0001_012f)),
            0
        );

        memory
            .load_physical(PhysicalWordAddress::new(0x0004_0100), &[0xffff; 32])
            .unwrap();
        let application = run_stage1(&flash, &mut memory, stage0).unwrap();
        assert_eq!(
            memory.physical_memory(PhysicalWordAddress::new(0x0004_0000)),
            0xbeef
        );
        assert_eq!(
            memory.physical_memory(PhysicalWordAddress::new(0x0004_0001)),
            0x0055
        );
        assert_eq!(
            memory.physical_memory(PhysicalWordAddress::new(0x0004_0100)),
            0
        );
        memory.load_program(0, &application.instructions()).unwrap();
        assert_eq!(
            memory.load_physical(PhysicalWordAddress::new(0x0004_0000), &[0x5a5a]),
            Ok(())
        );
        assert!(matches!(
            memory.run(32),
            Ok(crate::g16::RunOutcome::Halted { signal: 0x5a5a, .. })
        ));
        assert_eq!(memory.code_segment(), 3);
        assert_eq!(memory.data_segment(), 4);
        assert_eq!(memory.register(13), Some(0xe000));
    }

    #[test]
    fn corrupted_stage1_is_rejected_after_copy() {
        let mut flash = image();
        let descriptor = BootDescriptor::decode(&flash).unwrap();
        flash[descriptor.stage1_flash_offset as usize] ^= 1;
        let error =
            run_stage0(&flash, &mut Machine::default(), BootTarget::TangNano20K).unwrap_err();
        assert!(matches!(
            error,
            LoaderError::Dma(DmaError::CrcMismatch { .. })
        ));
    }
}
