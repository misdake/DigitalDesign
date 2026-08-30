//! Transaction model for the CpuV3 BSRAM caches.
//!
//! The instruction cache is read-only. The data cache is write-back with
//! write-allocate: stores update the resident line and set its dirty bit, a
//! miss refills the line first, and a dirty victim is written back before the
//! incoming line is installed. Global clean and invalidate both walk the dirty
//! lines and issue one eight-beat write-back burst per dirty line.

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
    /// An eight-beat write-back burst carrying a complete 16-word line.
    WriteLine {
        line_address: PhysicalWordAddress,
        words: [Word; CACHE_LINE_WORDS],
    },
    /// A masked half-word write, retained for device/uncached and DMA traffic.
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
pub enum MaintenanceCommand {
    /// Write every dirty line back, then leave every line valid and clean.
    Clean,
    /// Write every dirty line back, then invalidate every way.
    Invalidate,
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

/// Two-way set-associative line store shared by both cache kinds.
struct LineStore {
    words: Box<[[[Word; CACHE_LINE_WORDS]; CACHE_SETS]; CACHE_WAYS]>,
    tags: [[u32; CACHE_SETS]; CACHE_WAYS],
    valid: [[bool; CACHE_SETS]; CACHE_WAYS],
    victim: [usize; CACHE_SETS],
}

impl Default for LineStore {
    fn default() -> Self {
        Self {
            words: Box::new([[[0; CACHE_LINE_WORDS]; CACHE_SETS]; CACHE_WAYS]),
            tags: [[0; CACHE_SETS]; CACHE_WAYS],
            valid: [[false; CACHE_SETS]; CACHE_WAYS],
            victim: [0; CACHE_SETS],
        }
    }
}

impl LineStore {
    fn hit_way(&self, address: PhysicalWordAddress) -> Option<usize> {
        let decoded = decode(address);
        (0..CACHE_WAYS).find(|way| {
            self.valid[*way][decoded.set] && self.tags[*way][decoded.set] == decoded.tag
        })
    }

    fn victim_way(&self, set: usize) -> usize {
        (0..CACHE_WAYS)
            .find(|way| !self.valid[*way][set])
            .unwrap_or(self.victim[set])
    }

    fn line_words(&self, way: usize, set: usize) -> [Word; CACHE_LINE_WORDS] {
        self.words[way][set]
    }

    fn install(
        &mut self,
        way: usize,
        address: PhysicalWordAddress,
        words: [Word; CACHE_LINE_WORDS],
    ) {
        let decoded = decode(address);
        self.words[way][decoded.set] = words;
        self.tags[way][decoded.set] = decoded.tag;
        self.valid[way][decoded.set] = true;
        self.victim[decoded.set] = 1 - way;
    }

    fn invalidate_all(&mut self) {
        self.valid.fill([false; CACHE_SETS]);
    }
}

#[derive(Clone, Copy)]
struct PendingMiss {
    address: PhysicalWordAddress,
    way: usize,
}

/// Read-only instruction cache: read hits and refills only.
#[derive(Default)]
pub struct InstructionCache {
    store: LineStore,
    pending: Option<PendingMiss>,
}

impl InstructionCache {
    pub fn invalidate_all(&mut self) -> Result<(), CacheError> {
        if self.pending.is_some() {
            return Err(CacheError::Busy);
        }
        self.store.invalidate_all();
        Ok(())
    }

    pub fn request(&mut self, request: CpuMemoryRequest) -> Result<CacheAction, CacheError> {
        if self.pending.is_some() {
            return Err(CacheError::Busy);
        }
        let CpuMemoryRequest::Read { address } = request else {
            panic!("instruction cache accepts only reads");
        };
        let decoded = decode(address);
        if let Some(way) = self.store.hit_way(address) {
            return Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                value: self.store.words[way][decoded.set][decoded.word],
            }));
        }
        let way = self.store.victim_way(decoded.set);
        self.pending = Some(PendingMiss { address, way });
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::ReadLine {
                line_address: address.line_base(CACHE_LINE_WORDS as u32),
            },
        ))
    }

    pub fn complete(&mut self, response: MainMemoryResponse) -> Result<CacheAction, CacheError> {
        let pending = self
            .pending
            .take()
            .ok_or(CacheError::UnexpectedMemoryResponse)?;
        match response {
            MainMemoryResponse::ReadLine { words } => {
                let decoded = decode(pending.address);
                self.store.install(pending.way, pending.address, words);
                Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                    value: words[decoded.word],
                }))
            }
            MainMemoryResponse::WriteComplete => Err(CacheError::UnexpectedMemoryResponse),
        }
    }
}

#[derive(Clone, Copy)]
enum Pending {
    /// A read miss: one ReadLine is in flight.
    ReadMiss {
        address: PhysicalWordAddress,
        way: usize,
    },
    /// A dirty victim is being written back, then the read refills.
    EvictThenRead {
        address: PhysicalWordAddress,
        way: usize,
    },
    /// A write-allocate: one ReadLine is in flight, then the word is stored.
    WriteAllocate {
        address: PhysicalWordAddress,
        value: Word,
        way: usize,
    },
    /// A dirty victim is written back, then the line is refilled and stored.
    EvictThenWrite {
        address: PhysicalWordAddress,
        value: Word,
        way: usize,
    },
}

#[derive(Clone, Copy)]
struct MaintenanceState {
    command: MaintenanceCommand,
    /// The (way, set) whose write-back is in flight; its dirty bit clears on
    /// completion.
    writing: Option<(usize, usize)>,
}

/// A 4-KiB two-way write-back data cache with write-allocate and dirty eviction.
pub struct DataCache {
    store: LineStore,
    dirty: [[bool; CACHE_SETS]; CACHE_WAYS],
    pending: Option<Pending>,
    maintenance: Option<MaintenanceState>,
}

impl Default for DataCache {
    fn default() -> Self {
        Self {
            store: LineStore::default(),
            dirty: [[false; CACHE_SETS]; CACHE_WAYS],
            pending: None,
            maintenance: None,
        }
    }
}

impl DataCache {
    pub fn request(&mut self, request: CpuMemoryRequest) -> Result<CacheAction, CacheError> {
        if self.pending.is_some() || self.maintenance.is_some() {
            return Err(CacheError::Busy);
        }
        let address = match request {
            CpuMemoryRequest::Read { address } | CpuMemoryRequest::Write { address, .. } => address,
        };
        let decoded = decode(address);
        match request {
            CpuMemoryRequest::Read { address } => {
                if let Some(way) = self.store.hit_way(address) {
                    return Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                        value: self.store.words[way][decoded.set][decoded.word],
                    }));
                }
                let way = self.store.victim_way(decoded.set);
                if self.dirty[way][decoded.set] {
                    self.pending = Some(Pending::EvictThenRead { address, way });
                    Ok(CacheAction::MainMemoryRequest(
                        MainMemoryRequest::WriteLine {
                            line_address: self.line_address(way, decoded.set),
                            words: self.store.line_words(way, decoded.set),
                        },
                    ))
                } else {
                    self.pending = Some(Pending::ReadMiss { address, way });
                    Ok(CacheAction::MainMemoryRequest(
                        MainMemoryRequest::ReadLine {
                            line_address: address.line_base(CACHE_LINE_WORDS as u32),
                        },
                    ))
                }
            }
            CpuMemoryRequest::Write { address, value } => {
                if let Some(way) = self.store.hit_way(address) {
                    self.store.words[way][decoded.set][decoded.word] = value;
                    self.dirty[way][decoded.set] = true;
                    return Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete));
                }
                let way = self.store.victim_way(decoded.set);
                if self.dirty[way][decoded.set] {
                    self.pending = Some(Pending::EvictThenWrite {
                        address,
                        value,
                        way,
                    });
                    Ok(CacheAction::MainMemoryRequest(
                        MainMemoryRequest::WriteLine {
                            line_address: self.line_address(way, decoded.set),
                            words: self.store.line_words(way, decoded.set),
                        },
                    ))
                } else {
                    self.pending = Some(Pending::WriteAllocate {
                        address,
                        value,
                        way,
                    });
                    Ok(CacheAction::MainMemoryRequest(
                        MainMemoryRequest::ReadLine {
                            line_address: address.line_base(CACHE_LINE_WORDS as u32),
                        },
                    ))
                }
            }
        }
    }

    pub fn complete(&mut self, response: MainMemoryResponse) -> Result<CacheAction, CacheError> {
        let pending = self
            .pending
            .take()
            .ok_or(CacheError::UnexpectedMemoryResponse)?;
        match (pending, response) {
            (Pending::ReadMiss { address, way }, MainMemoryResponse::ReadLine { words }) => {
                let decoded = decode(address);
                self.store.install(way, address, words);
                Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                    value: words[decoded.word],
                }))
            }
            (
                Pending::WriteAllocate {
                    address,
                    value,
                    way,
                },
                MainMemoryResponse::ReadLine { words },
            ) => {
                let decoded = decode(address);
                self.store.install(way, address, words);
                self.store.words[way][decoded.set][decoded.word] = value;
                self.dirty[way][decoded.set] = true;
                Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
            }
            (Pending::EvictThenRead { address, way }, MainMemoryResponse::WriteComplete) => {
                let decoded = decode(address);
                self.dirty[way][decoded.set] = false;
                self.pending = Some(Pending::ReadMiss { address, way });
                Ok(CacheAction::MainMemoryRequest(
                    MainMemoryRequest::ReadLine {
                        line_address: address.line_base(CACHE_LINE_WORDS as u32),
                    },
                ))
            }
            (
                Pending::EvictThenWrite {
                    address,
                    value,
                    way,
                },
                MainMemoryResponse::WriteComplete,
            ) => {
                let decoded = decode(address);
                self.dirty[way][decoded.set] = false;
                self.pending = Some(Pending::WriteAllocate {
                    address,
                    value,
                    way,
                });
                Ok(CacheAction::MainMemoryRequest(
                    MainMemoryRequest::ReadLine {
                        line_address: address.line_base(CACHE_LINE_WORDS as u32),
                    },
                ))
            }
            _ => Err(CacheError::UnexpectedMemoryResponse),
        }
    }

    /// Begins a global clean or clean-plus-invalidate and returns the first
    /// dirty line to write back, or `None` when the cache already holds no
    /// dirty line (the command is then already fully applied).
    pub fn begin_maintenance(
        &mut self,
        command: MaintenanceCommand,
    ) -> Result<Option<MainMemoryRequest>, CacheError> {
        if self.pending.is_some() || self.maintenance.is_some() {
            return Err(CacheError::Busy);
        }
        self.maintenance = Some(MaintenanceState {
            command,
            writing: None,
        });
        self.next_maintenance_write()
    }

    /// Completes the outstanding write-back and returns the next dirty line to
    /// write back, or `None` once maintenance is complete.
    pub fn continue_maintenance(
        &mut self,
        response: MainMemoryResponse,
    ) -> Result<Option<MainMemoryRequest>, CacheError> {
        if response != MainMemoryResponse::WriteComplete {
            return Err(CacheError::UnexpectedMemoryResponse);
        }
        let state = self
            .maintenance
            .ok_or(CacheError::UnexpectedMemoryResponse)?;
        if let Some((way, set)) = state.writing {
            self.dirty[way][set] = false;
        }
        self.next_maintenance_write()
    }

    fn next_maintenance_write(&mut self) -> Result<Option<MainMemoryRequest>, CacheError> {
        let state = self.maintenance.as_mut().expect("maintenance is active");
        for set in 0..CACHE_SETS {
            for way in 0..CACHE_WAYS {
                if self.dirty[way][set] {
                    state.writing = Some((way, set));
                    return Ok(Some(MainMemoryRequest::WriteLine {
                        line_address: self.line_address(way, set),
                        words: self.store.line_words(way, set),
                    }));
                }
            }
        }
        // No dirty line remains: finish and apply the command.
        let state = self.maintenance.take().expect("maintenance is active");
        if state.command == MaintenanceCommand::Invalidate {
            self.store.invalidate_all();
        }
        self.dirty.fill([false; CACHE_SETS]);
        Ok(None)
    }

    fn line_address(&self, way: usize, set: usize) -> PhysicalWordAddress {
        let line = self.store.tags[way][set] * CACHE_SETS as u32 + set as u32;
        PhysicalWordAddress::new(line * CACHE_LINE_WORDS as u32)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheAction {
    CpuResponse(CpuMemoryResponse),
    MainMemoryRequest(MainMemoryRequest),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(base: u16) -> [Word; CACHE_LINE_WORDS] {
        std::array::from_fn(|index| base.wrapping_add(index as u16))
    }

    fn read(cache: &mut InstructionCache, address: u32) -> Word {
        match cache
            .request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(address),
            })
            .unwrap()
        {
            CacheAction::CpuResponse(CpuMemoryResponse::Read { value }) => value,
            CacheAction::MainMemoryRequest(MainMemoryRequest::ReadLine { line_address }) => {
                let words = line(line_address.get() as u16);
                match cache
                    .complete(MainMemoryResponse::ReadLine { words })
                    .unwrap()
                {
                    CacheAction::CpuResponse(CpuMemoryResponse::Read { value }) => value,
                    _ => panic!("expected a read response"),
                }
            }
            _ => panic!("instruction cache produced an unexpected action"),
        }
    }

    #[test]
    fn geometry_is_two_ways_of_sixty_four_sixteen_word_lines() {
        assert_eq!(CACHE_CAPACITY_BYTES, 4_096);
        assert_eq!(CACHE_WAYS * CACHE_SETS * CACHE_LINE_WORDS, 2_048);
        assert_eq!(CACHE_WAYS * CACHE_SETS * (CACHE_LINE_WORDS / 2), 1_024);
    }

    #[test]
    fn instruction_cache_refills_then_hits_within_the_line() {
        let mut cache = InstructionCache::default();
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
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                value: 107
            }))
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
    fn instruction_cache_invalidates_every_resident_line() {
        let mut cache = InstructionCache::default();
        assert_eq!(read(&mut cache, 0x2012), 0x2012);
        cache.invalidate_all().unwrap();
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(0x2012)
            }),
            Ok(CacheAction::MainMemoryRequest(_))
        ));
    }

    #[test]
    fn data_cache_write_hit_stays_in_the_cache_without_memory_traffic() {
        let mut cache = DataCache::default();
        assert_eq!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(0x1230)
            }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: PhysicalWordAddress::new(0x1230)
                }
            ))
        );
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(100) })
            .unwrap();
        // A write hit updates the resident word, marks it dirty, and issues no
        // memory request.
        assert_eq!(
            cache.request(CpuMemoryRequest::Write {
                address: PhysicalWordAddress::new(0x1234),
                value: 0xabcd
            }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
        );
        assert_eq!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(0x1234)
            }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                value: 0xabcd
            }))
        );
    }

    #[test]
    fn data_cache_write_miss_read_allocates_the_line() {
        let mut cache = DataCache::default();
        assert_eq!(
            cache.request(CpuMemoryRequest::Write {
                address: PhysicalWordAddress::new(0x1234),
                value: 0xabcd
            }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: PhysicalWordAddress::new(0x1230)
                }
            ))
        );
        assert_eq!(
            cache.complete(MainMemoryResponse::ReadLine { words: line(100) }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
        );
        assert_eq!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(0x1234)
            }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                value: 0xabcd
            }))
        );
    }

    #[test]
    fn dirty_victim_is_written_back_before_the_incoming_line_installs() {
        let stride = (CACHE_SETS * CACHE_LINE_WORDS) as u32;
        let mut cache = DataCache::default();

        // Line 0 in way 0, made dirty by a store.
        assert_eq!(
            cache.request(CpuMemoryRequest::Read { address: 0.into() }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: PhysicalWordAddress::new(0)
                }
            ))
        );
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(10) })
            .unwrap();
        cache
            .request(CpuMemoryRequest::Write {
                address: 0.into(),
                value: 0xbeef,
            })
            .unwrap();

        // Line 64 in way 1 (same set, distinct tag).
        assert_eq!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(stride)
            }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: PhysicalWordAddress::new(stride)
                }
            ))
        );
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(20) })
            .unwrap();

        // Line 128 (same set) evicts the dirty way 0: it writes line 0 back
        // first (with the stored word), then requests line 128.
        let mut victim = line(10);
        victim[0] = 0xbeef;
        assert_eq!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(2 * stride)
            }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::WriteLine {
                    line_address: PhysicalWordAddress::new(0),
                    words: victim,
                }
            ))
        );
        assert_eq!(
            cache.complete(MainMemoryResponse::WriteComplete),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: PhysicalWordAddress::new(2 * stride)
                }
            ))
        );
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(30) })
            .unwrap();

        // Line 64 is intact, and the evicted line 0 now misses again.
        assert_eq!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(stride)
            }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read {
                value: 20
            }))
        );
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read { address: 0.into() }),
            Ok(CacheAction::MainMemoryRequest(_))
        ));
    }

    fn dirty_line(cache: &mut DataCache, address: u32) {
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read {
                address: PhysicalWordAddress::new(address)
            }),
            Ok(CacheAction::MainMemoryRequest(_))
        ));
        cache
            .complete(MainMemoryResponse::ReadLine {
                words: line((address & 0xff) as u16),
            })
            .unwrap();
        cache
            .request(CpuMemoryRequest::Write {
                address: PhysicalWordAddress::new(address),
                value: 0xdead,
            })
            .unwrap();
    }

    #[test]
    fn clean_writes_back_each_dirty_line_exactly_once_and_keeps_lines_valid() {
        let stride = (CACHE_SETS * CACHE_LINE_WORDS) as u32;
        let mut cache = DataCache::default();
        dirty_line(&mut cache, 0);
        dirty_line(&mut cache, stride);

        let mut written = 0;
        let mut request = cache.begin_maintenance(MaintenanceCommand::Clean).unwrap();
        while let Some(req) = request {
            match req {
                MainMemoryRequest::WriteLine { .. } => written += 1,
                _ => panic!("clean must only issue write-backs"),
            }
            request = cache
                .continue_maintenance(MainMemoryResponse::WriteComplete)
                .unwrap();
        }
        assert_eq!(written, 2);

        // Both lines remain valid (clean), so a second clean writes nothing.
        assert_eq!(
            cache.begin_maintenance(MaintenanceCommand::Clean).unwrap(),
            None
        );
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read { address: 0.into() }),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::Read { .. }))
        ));
    }

    #[test]
    fn invalidate_writes_back_dirty_lines_then_clears_every_way() {
        let mut cache = DataCache::default();
        dirty_line(&mut cache, 0);

        assert!(matches!(
            cache
                .begin_maintenance(MaintenanceCommand::Invalidate)
                .unwrap(),
            Some(MainMemoryRequest::WriteLine { .. })
        ));
        assert_eq!(
            cache
                .continue_maintenance(MainMemoryResponse::WriteComplete)
                .unwrap(),
            None
        );

        // Every line is now invalid, so the read misses again.
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read { address: 0.into() }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine { .. }
            ))
        ));
    }

    #[test]
    fn equal_offsets_in_different_segments_have_distinct_tags() {
        let mut cache = InstructionCache::default();
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
}
