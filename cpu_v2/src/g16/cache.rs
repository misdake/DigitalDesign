//! Transaction model for the first G16 BSRAM data cache.
//!
//! The FPGA implementation uses the same geometry and state transitions. This
//! model deliberately stops at 32-byte line transactions so Gowin Controller
//! HS command timing remains in the target-specific SDRAM adapter.

use super::Word;

pub const CACHE_LINE_WORDS: usize = 16;
pub const CACHE_LINE_BYTES: usize = CACHE_LINE_WORDS * size_of::<Word>();
pub const CACHE_SETS: usize = 64;
pub const CACHE_CAPACITY_BYTES: usize = CACHE_SETS * CACHE_LINE_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuMemoryRequest {
    Read { address: Word },
    Write { address: Word, value: Word },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuMemoryResponse {
    Read { value: Word },
    WriteComplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MainMemoryRequest {
    ReadLine {
        line_address: Word,
    },
    /// A write-through half-word. The SDRAM adapter performs a one-beat
    /// 32-bit write with the appropriate two byte-mask bits disabled.
    WriteWord {
        address: Word,
        value: Word,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MainMemoryResponse {
    ReadLine { words: [Word; CACHE_LINE_WORDS] },
    WriteComplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheError {
    Busy,
    UnexpectedMemoryResponse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending {
    ReadMiss { address: Word },
    WriteThrough { address: Word, hit: bool },
}

/// A 2-KiB direct-mapped, write-through, no-write-allocate cache.
///
/// Direct mapping is intentional for the first RTL: all 1,024 cached words fit
/// one measured 1024x16 BSRAM leaf, and one synchronous lookup does not require
/// duplicate data banks. Associativity remains a replaceable policy above the
/// CPU transaction interface if measurements later justify more BSRAM blocks.
pub struct DataCache {
    words: Box<[[Word; CACHE_LINE_WORDS]; CACHE_SETS]>,
    tags: [u16; CACHE_SETS],
    valid: [bool; CACHE_SETS],
    pending: Option<Pending>,
}

/// The instruction side uses the same measured one-BSRAM geometry. The first
/// processor has split instruction and data instances, with an arbiter sharing
/// one SDRAM line-transaction port.
pub type InstructionCache = DataCache;

impl Default for DataCache {
    fn default() -> Self {
        Self {
            words: Box::new([[0; CACHE_LINE_WORDS]; CACHE_SETS]),
            tags: [0; CACHE_SETS],
            valid: [false; CACHE_SETS],
            pending: None,
        }
    }
}

impl DataCache {
    pub fn request(&mut self, request: CpuMemoryRequest) -> Result<CacheAction, CacheError> {
        if self.pending.is_some() {
            return Err(CacheError::Busy);
        }
        let address = match request {
            CpuMemoryRequest::Read { address } | CpuMemoryRequest::Write { address, .. } => address,
        };
        let decoded = decode(address);
        let hit = self.valid[decoded.set] && self.tags[decoded.set] == decoded.tag;
        match request {
            CpuMemoryRequest::Read { .. } if hit => {
                Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                    value: self.words[decoded.set][decoded.word],
                }))
            }
            CpuMemoryRequest::Read { address } => {
                self.pending = Some(Pending::ReadMiss { address });
                Ok(CacheAction::MainMemoryRequest(
                    MainMemoryRequest::ReadLine {
                        line_address: address & !((CACHE_LINE_WORDS as Word) - 1),
                    },
                ))
            }
            CpuMemoryRequest::Write { address, value } => {
                if hit {
                    self.words[decoded.set][decoded.word] = value;
                }
                self.pending = Some(Pending::WriteThrough { address, hit });
                Ok(CacheAction::MainMemoryRequest(
                    MainMemoryRequest::WriteWord { address, value },
                ))
            }
        }
    }

    pub fn complete(
        &mut self,
        response: MainMemoryResponse,
    ) -> Result<CpuMemoryResponse, CacheError> {
        let pending = self.pending.ok_or(CacheError::UnexpectedMemoryResponse)?;
        match (pending, response) {
            (Pending::ReadMiss { address }, MainMemoryResponse::ReadLine { words }) => {
                self.pending = None;
                let decoded = decode(address);
                self.words[decoded.set] = words;
                self.tags[decoded.set] = decoded.tag;
                self.valid[decoded.set] = true;
                Ok(CpuMemoryResponse::Read {
                    value: words[decoded.word],
                })
            }
            (Pending::WriteThrough { address, hit }, MainMemoryResponse::WriteComplete) => {
                self.pending = None;
                debug_assert_eq!(
                    hit,
                    self.valid[decode(address).set]
                        && self.tags[decode(address).set] == decode(address).tag
                );
                Ok(CpuMemoryResponse::WriteComplete)
            }
            _ => Err(CacheError::UnexpectedMemoryResponse),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheAction {
    CpuResponse(CpuMemoryResponse),
    MainMemoryRequest(MainMemoryRequest),
}

#[derive(Clone, Copy)]
struct DecodedAddress {
    set: usize,
    word: usize,
    tag: u16,
}

fn decode(address: Word) -> DecodedAddress {
    let word = usize::from(address) & (CACHE_LINE_WORDS - 1);
    let line = usize::from(address) / CACHE_LINE_WORDS;
    DecodedAddress {
        set: line & (CACHE_SETS - 1),
        word,
        tag: (line / CACHE_SETS) as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(base: u16) -> [Word; CACHE_LINE_WORDS] {
        std::array::from_fn(|index| base.wrapping_add(index as u16))
    }

    #[test]
    fn geometry_fits_one_1024x16_bsram_data_leaf() {
        assert_eq!(CACHE_CAPACITY_BYTES, 2_048);
        assert_eq!(CACHE_SETS * CACHE_LINE_WORDS, 1_024);
    }

    #[test]
    fn miss_refill_and_subsequent_line_hits_are_explicit() {
        let mut cache = DataCache::default();
        assert_eq!(
            cache.request(CpuMemoryRequest::Read { address: 0x1237 }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: 0x1230
                }
            ))
        );
        assert_eq!(
            cache.request(CpuMemoryRequest::Read { address: 0 }),
            Err(CacheError::Busy)
        );
        assert_eq!(
            cache.complete(MainMemoryResponse::ReadLine { words: line(100) }),
            Ok(CpuMemoryResponse::Read { value: 107 })
        );
        assert_eq!(
            cache.request(CpuMemoryRequest::Read { address: 0x123f }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                value: 115
            }))
        );
    }

    #[test]
    fn conflicting_line_replaces_the_direct_mapped_set() {
        let mut cache = DataCache::default();
        cache
            .request(CpuMemoryRequest::Read { address: 0 })
            .unwrap();
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(10) })
            .unwrap();
        let conflict = (CACHE_SETS * CACHE_LINE_WORDS) as Word;
        cache
            .request(CpuMemoryRequest::Read { address: conflict })
            .unwrap();
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(20) })
            .unwrap();
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read { address: 0 }),
            Ok(CacheAction::MainMemoryRequest(_))
        ));
    }

    #[test]
    fn stores_are_write_through_and_misses_do_not_allocate() {
        let mut cache = DataCache::default();
        assert_eq!(
            cache.request(CpuMemoryRequest::Write {
                address: 9,
                value: 0xabcd
            }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::WriteWord {
                    address: 9,
                    value: 0xabcd
                }
            ))
        );
        assert_eq!(
            cache.complete(MainMemoryResponse::WriteComplete),
            Ok(CpuMemoryResponse::WriteComplete)
        );
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read { address: 9 }),
            Ok(CacheAction::MainMemoryRequest(_))
        ));
    }

    #[test]
    fn wrong_completion_does_not_discard_the_outstanding_request() {
        let mut cache = DataCache::default();
        cache
            .request(CpuMemoryRequest::Read { address: 0 })
            .unwrap();
        assert_eq!(
            cache.complete(MainMemoryResponse::WriteComplete),
            Err(CacheError::UnexpectedMemoryResponse)
        );
        assert_eq!(
            cache.complete(MainMemoryResponse::ReadLine { words: line(1) }),
            Ok(CpuMemoryResponse::Read { value: 1 })
        );
    }
}
