//! Transaction model for the first CpuV3 BSRAM data cache.
//!
//! The FPGA implementation uses the same geometry and state transitions. This
//! model deliberately stops at 32-byte line transactions so Gowin Controller
//! HS command timing remains in the target-specific SDRAM adapter.

use super::{PhysicalWordAddress, Word};

pub const CACHE_LINE_WORDS: usize = 16;
pub const CACHE_LINE_BYTES: usize = CACHE_LINE_WORDS * size_of::<Word>();
pub const CACHE_SETS: usize = 64;
pub const CACHE_WAYS: usize = 2;
pub const CACHE_CAPACITY_BYTES: usize = CACHE_WAYS * CACHE_SETS * CACHE_LINE_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuMemoryRequest {
    Read {
        address: PhysicalWordAddress,
    },
    Write {
        address: PhysicalWordAddress,
        value: Word,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuMemoryResponse {
    Read { value: Word },
    WriteComplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MainMemoryRequest {
    ReadLine {
        line_address: PhysicalWordAddress,
    },
    /// A write-through half-word. The SDRAM adapter performs a one-beat
    /// 32-bit write with the appropriate two byte-mask bits disabled.
    WriteWord {
        address: PhysicalWordAddress,
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
    ReadMiss {
        address: PhysicalWordAddress,
        way: usize,
    },
    WriteThrough {
        address: PhysicalWordAddress,
        hit: bool,
    },
}

/// A 4-KiB two-way, write-through, no-write-allocate cache.
///
/// Each way has 64 sets of sixteen words. Even and odd words are held in two
/// 1024x16 BSRAM leaves so both halves of a 32-bit refill beat install together.
pub struct DataCache {
    words: Box<[[[Word; CACHE_LINE_WORDS]; CACHE_SETS]; CACHE_WAYS]>,
    tags: [[u32; CACHE_SETS]; CACHE_WAYS],
    valid: [[bool; CACHE_SETS]; CACHE_WAYS],
    victim: [usize; CACHE_SETS],
    pending: Option<Pending>,
}

/// The instruction side uses the same two-way, parity-split BSRAM geometry. The first
/// processor has split instruction and data instances, with an arbiter sharing
/// one SDRAM line-transaction port.
pub type InstructionCache = DataCache;

impl Default for DataCache {
    fn default() -> Self {
        Self {
            words: Box::new([[[0; CACHE_LINE_WORDS]; CACHE_SETS]; CACHE_WAYS]),
            tags: [[0; CACHE_SETS]; CACHE_WAYS],
            valid: [[false; CACHE_SETS]; CACHE_WAYS],
            victim: [0; CACHE_SETS],
            pending: None,
        }
    }
}

impl DataCache {
    pub fn invalidate_all(&mut self) -> Result<(), CacheError> {
        if self.pending.is_some() {
            return Err(CacheError::Busy);
        }
        self.valid.fill([false; CACHE_SETS]);
        Ok(())
    }

    pub fn request(&mut self, request: CpuMemoryRequest) -> Result<CacheAction, CacheError> {
        if self.pending.is_some() {
            return Err(CacheError::Busy);
        }
        let address = match request {
            CpuMemoryRequest::Read { address } | CpuMemoryRequest::Write { address, .. } => address,
        };
        let decoded = decode(address);
        let hit_way = (0..CACHE_WAYS).find(|way| {
            self.valid[*way][decoded.set] && self.tags[*way][decoded.set] == decoded.tag
        });
        match request {
            CpuMemoryRequest::Read { .. } if hit_way.is_some() => {
                Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                    value: self.words[hit_way.unwrap()][decoded.set][decoded.word],
                }))
            }
            CpuMemoryRequest::Read { address } => {
                let way = (0..CACHE_WAYS)
                    .find(|way| !self.valid[*way][decoded.set])
                    .unwrap_or(self.victim[decoded.set]);
                self.pending = Some(Pending::ReadMiss { address, way });
                Ok(CacheAction::MainMemoryRequest(
                    MainMemoryRequest::ReadLine {
                        line_address: address.line_base(CACHE_LINE_WORDS as u32),
                    },
                ))
            }
            CpuMemoryRequest::Write { address, value } => {
                if let Some(way) = hit_way {
                    self.words[way][decoded.set][decoded.word] = value;
                }
                self.pending = Some(Pending::WriteThrough {
                    address,
                    hit: hit_way.is_some(),
                });
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
            (Pending::ReadMiss { address, way }, MainMemoryResponse::ReadLine { words }) => {
                self.pending = None;
                let decoded = decode(address);
                self.words[way][decoded.set] = words;
                self.tags[way][decoded.set] = decoded.tag;
                self.valid[way][decoded.set] = true;
                self.victim[decoded.set] = 1 - way;
                Ok(CpuMemoryResponse::Read {
                    value: words[decoded.word],
                })
            }
            (Pending::WriteThrough { address, hit }, MainMemoryResponse::WriteComplete) => {
                self.pending = None;
                debug_assert_eq!(
                    hit,
                    (0..CACHE_WAYS).any(|way| {
                        self.valid[way][decode(address).set]
                            && self.tags[way][decode(address).set] == decode(address).tag
                    })
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
    tag: u32,
}

fn decode(address: PhysicalWordAddress) -> DecodedAddress {
    let word_address = address.get() as usize;
    let word = word_address & (CACHE_LINE_WORDS - 1);
    let line = word_address / CACHE_LINE_WORDS;
    DecodedAddress {
        set: line & (CACHE_SETS - 1),
        word,
        tag: (line / CACHE_SETS) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(base: u16) -> [Word; CACHE_LINE_WORDS] {
        std::array::from_fn(|index| base.wrapping_add(index as u16))
    }

    #[test]
    fn geometry_fits_two_fully_used_1024x16_bsram_data_leaves() {
        assert_eq!(CACHE_CAPACITY_BYTES, 4_096);
        assert_eq!(CACHE_WAYS * CACHE_SETS * CACHE_LINE_WORDS, 2_048);
        assert_eq!(CACHE_WAYS * CACHE_SETS * (CACHE_LINE_WORDS / 2), 1_024);
    }

    #[test]
    fn miss_refill_and_subsequent_line_hits_are_explicit() {
        let mut cache = DataCache::default();
        assert_eq!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(0x1237)
            }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: PhysicalWordAddress::new(0x1230)
                }
            ))
        );
        assert_eq!(
            cache.request(CpuMemoryRequest::Read { address: 0.into() }),
            Err(CacheError::Busy)
        );
        assert_eq!(
            cache.complete(MainMemoryResponse::ReadLine { words: line(100) }),
            Ok(CpuMemoryResponse::Read { value: 107 })
        );
        assert_eq!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(0x123f)
            }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                value: 115
            }))
        );
    }

    #[test]
    fn same_set_uses_invalid_ways_then_replaces_deterministically() {
        let mut cache = DataCache::default();
        let stride = (CACHE_SETS * CACHE_LINE_WORDS) as u32;
        let addresses = [
            0.into(),
            PhysicalWordAddress::new(stride),
            PhysicalWordAddress::new(2 * stride),
        ];
        for (address, base) in addresses[..2].iter().copied().zip([10, 20]) {
            cache.request(CpuMemoryRequest::Read { address }).unwrap();
            cache
                .complete(MainMemoryResponse::ReadLine { words: line(base) })
                .unwrap();
        }
        for (address, value) in addresses[..2].iter().copied().zip([10, 20]) {
            assert_eq!(
                cache.request(CpuMemoryRequest::Read { address }),
                Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read { value }))
            );
        }
        cache
            .request(CpuMemoryRequest::Read {
                address: addresses[2],
            })
            .unwrap();
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(30) })
            .unwrap();
        assert_eq!(
            cache.request(CpuMemoryRequest::Read {
                address: addresses[1]
            }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                value: 20
            }))
        );
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read {
                address: addresses[0]
            }),
            Ok(CacheAction::MainMemoryRequest(_))
        ));
    }

    #[test]
    fn stores_are_write_through_and_misses_do_not_allocate() {
        let mut cache = DataCache::default();
        assert_eq!(
            cache.request(CpuMemoryRequest::Write {
                address: 9.into(),
                value: 0xabcd
            }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::WriteWord {
                    address: 9.into(),
                    value: 0xabcd
                }
            ))
        );
        assert_eq!(
            cache.complete(MainMemoryResponse::WriteComplete),
            Ok(CpuMemoryResponse::WriteComplete)
        );
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read { address: 9.into() }),
            Ok(CacheAction::MainMemoryRequest(_))
        ));
    }

    #[test]
    fn wrong_completion_does_not_discard_the_outstanding_request() {
        let mut cache = DataCache::default();
        cache
            .request(CpuMemoryRequest::Read { address: 0.into() })
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

    #[test]
    fn equal_offsets_in_different_segments_have_distinct_tags() {
        let mut cache = DataCache::default();
        let first = PhysicalWordAddress::from_segment_offset(1, 0x1234);
        let second = PhysicalWordAddress::from_segment_offset(2, 0x1234);

        cache
            .request(CpuMemoryRequest::Read { address: first })
            .unwrap();
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(10) })
            .unwrap();

        assert_eq!(
            cache.request(CpuMemoryRequest::Read { address: second }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: second.line_base(CACHE_LINE_WORDS as u32),
                }
            ))
        );
    }

    #[test]
    fn full_invalidate_discards_every_resident_line() {
        let mut cache = DataCache::default();
        let address = PhysicalWordAddress::from_segment_offset(3, 0x2012);
        cache.request(CpuMemoryRequest::Read { address }).unwrap();
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(30) })
            .unwrap();
        assert_eq!(
            cache.request(CpuMemoryRequest::Read { address }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                value: 32
            }))
        );

        cache.invalidate_all().unwrap();
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read { address }),
            Ok(CacheAction::MainMemoryRequest(_))
        ));
    }
}
