//! Placement shared by immutable assets and zero-initialized working storage.
use std::ops::Range;

/// An available, non-overlapping byte range approved by the system/packer.
#[derive(Clone, Debug)]
pub struct StorageBank {
    pub bytes: Range<usize>,
}

pub(super) const SECTOR_BYTES: usize = 128;
const ADDRESS_BYTES: usize = 65536 * SECTOR_BYTES;

pub(super) struct Allocator {
    banks: Vec<Range<usize>>,
    occupied: Vec<Range<usize>>,
}
impl Allocator {
    pub(super) fn new(banks: &[StorageBank]) -> Result<Self, &'static str> {
        for (index, bank) in banks.iter().enumerate() {
            if bank.bytes.start > bank.bytes.end || bank.bytes.end > ADDRESS_BYTES {
                return Err("storage bank is outside the sector-address space");
            }
            if banks[..index].iter().any(|other| {
                bank.bytes.start < other.bytes.end && other.bytes.start < bank.bytes.end
            }) {
                return Err("approved storage banks overlap");
            }
        }
        Ok(Self {
            banks: banks.iter().map(|b| b.bytes.clone()).collect(),
            occupied: vec![],
        })
    }
    pub(super) fn reserve(&mut self, range: Range<usize>) {
        self.occupied.push(range);
        self.occupied.sort_by_key(|r| r.start);
    }
    pub(super) fn allocate(
        &mut self,
        bytes: usize,
        alignment: usize,
    ) -> Result<Range<usize>, &'static str> {
        if bytes == 0 {
            return Err("storage must not be empty");
        }
        if alignment < SECTOR_BYTES || !alignment.is_power_of_two() {
            return Err("storage alignment must be a power of two and at least 128 bytes");
        }
        let length = bytes.checked_add(127).ok_or("storage size overflow")? & !127;
        if length / SECTOR_BYTES > u16::MAX as usize {
            return Err("storage sector count overflow");
        }
        let align = |value: usize| {
            value
                .checked_add(alignment - 1)
                .map(|v| v & !(alignment - 1))
        };
        for bank in &self.banks {
            let Some(mut next) = align(bank.start) else {
                continue;
            };
            for used in &self.occupied {
                if used.end <= next || used.start >= bank.end {
                    continue;
                }
                if next
                    .checked_add(length)
                    .is_some_and(|end| end <= used.start)
                {
                    break;
                }
                next = align(used.end).ok_or("storage address overflow")?;
            }
            if next.checked_add(length).is_some_and(|end| end <= bank.end) {
                let range = next..next + length;
                self.reserve(range.clone());
                return Ok(range);
            }
        }
        Err("no approved storage bank can satisfy size and alignment")
    }
}
