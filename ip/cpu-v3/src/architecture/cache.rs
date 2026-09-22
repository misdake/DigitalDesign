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
#[derive(Clone)]
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
#[derive(Clone, Default)]
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
    /// A dirty victim is written back before the copy source refills.
    EvictThenCopy {
        source: PhysicalWordAddress,
        destination_page: u8,
        way: usize,
    },
    /// The copy source is being refilled into the selected way.
    CopyRefill {
        source: PhysicalWordAddress,
        destination_page: u8,
        way: usize,
    },
    /// The resident source line is being written to the redirected segment.
    CopyWriteback,
    /// One dirty resident line is being written back for a clean-line hint.
    CleanLine { way: usize, set: usize },
}

#[derive(Clone, Copy)]
struct MaintenanceState {
    command: MaintenanceCommand,
    /// The (way, set) whose write-back is in flight; its dirty bit clears on
    /// completion.
    writing: Option<(usize, usize)>,
}

/// A 4-KiB two-way write-back data cache with write-allocate and dirty eviction.
#[derive(Clone)]
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
    /// Cleans one resident line. A miss or an already-clean hit completes
    /// without memory traffic and does not allocate a line.
    pub fn clean_line(&mut self, address: PhysicalWordAddress) -> Result<CacheAction, CacheError> {
        if self.pending.is_some() || self.maintenance.is_some() {
            return Err(CacheError::Busy);
        }
        let decoded = decode(address);
        let Some(way) = self.store.hit_way(address) else {
            return Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete));
        };
        if !self.dirty[way][decoded.set] {
            return Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete));
        }
        self.pending = Some(Pending::CleanLine {
            way,
            set: decoded.set,
        });
        Ok(CacheAction::MainMemoryRequest(
            MainMemoryRequest::WriteLine {
                line_address: self.line_address(way, decoded.set),
                words: self.store.line_words(way, decoded.set),
            },
        ))
    }

    /// Copies one aligned resident-or-refilled line to the same offset in a
    /// different physical segment. A resident destination alias is discarded
    /// first because the complete destination line is overwritten. The
    /// destination is not installed in the cache.
    pub fn copy_line(
        &mut self,
        source: PhysicalWordAddress,
        destination_page: u8,
    ) -> Result<CacheAction, CacheError> {
        if self.pending.is_some() || self.maintenance.is_some() {
            return Err(CacheError::Busy);
        }
        let decoded = decode(source);
        let way = self.store.victim_way(decoded.set);
        let destination = copy_destination(source, destination_page);
        if destination != source {
            if let Some(destination_way) = self.store.hit_way(destination) {
                self.store.valid[destination_way][decoded.set] = false;
                self.dirty[destination_way][decoded.set] = false;
            }
        }
        if let Some(way) = self.store.hit_way(source) {
            self.pending = Some(Pending::CopyWriteback);
            return Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::WriteLine {
                    line_address: destination,
                    words: self.store.line_words(way, decoded.set),
                },
            ));
        }
        if self.dirty[way][decoded.set] {
            self.pending = Some(Pending::EvictThenCopy {
                source,
                destination_page,
                way,
            });
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::WriteLine {
                    line_address: self.line_address(way, decoded.set),
                    words: self.store.line_words(way, decoded.set),
                },
            ))
        } else {
            self.pending = Some(Pending::CopyRefill {
                source,
                destination_page,
                way,
            });
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: source.line_base(CACHE_LINE_WORDS as u32),
                },
            ))
        }
    }

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
            (
                Pending::EvictThenCopy {
                    source,
                    destination_page,
                    way,
                },
                MainMemoryResponse::WriteComplete,
            ) => {
                let decoded = decode(source);
                self.dirty[way][decoded.set] = false;
                self.pending = Some(Pending::CopyRefill {
                    source,
                    destination_page,
                    way,
                });
                Ok(CacheAction::MainMemoryRequest(
                    MainMemoryRequest::ReadLine {
                        line_address: source.line_base(CACHE_LINE_WORDS as u32),
                    },
                ))
            }
            (
                Pending::CopyRefill {
                    source,
                    destination_page,
                    way,
                },
                MainMemoryResponse::ReadLine { words },
            ) => {
                let decoded = decode(source);
                self.store.install(way, source, words);
                self.dirty[way][decoded.set] = false;
                self.pending = Some(Pending::CopyWriteback);
                Ok(CacheAction::MainMemoryRequest(
                    MainMemoryRequest::WriteLine {
                        line_address: copy_destination(source, destination_page),
                        words,
                    },
                ))
            }
            (Pending::CopyWriteback, MainMemoryResponse::WriteComplete) => {
                Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
            }
            (Pending::CleanLine { way, set }, MainMemoryResponse::WriteComplete) => {
                self.dirty[way][set] = false;
                Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
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

    /// Picks the next dirty line in the same order as the hardware
    /// maintenance scan: entry `way * CACHE_SETS + set` ascending, i.e. way
    /// major. The overlapped window scan and the Rust wrapper therefore
    /// select identical lines without carrying a separate bitmap.
    fn next_maintenance_write(&mut self) -> Result<Option<MainMemoryRequest>, CacheError> {
        let state = self.maintenance.as_mut().expect("maintenance is active");
        for way in 0..CACHE_WAYS {
            for set in 0..CACHE_SETS {
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

    /// Dirty bitmap with entry `way * CACHE_SETS + set` at the bit position of
    /// that index. The hardware wrapper's overlapped maintenance scan mirrors
    /// the RTL window scan against this bitmap.
    pub fn dirty_bits(&self) -> u128 {
        let mut bits = 0u128;
        for way in 0..CACHE_WAYS {
            for set in 0..CACHE_SETS {
                if self.dirty[way][set] {
                    bits |= 1u128 << (way * CACHE_SETS + set);
                }
            }
        }
        bits
    }
}

fn copy_destination(source: PhysicalWordAddress, destination_page: u8) -> PhysicalWordAddress {
    PhysicalWordAddress::new(
        (u32::from(destination_page) << 14)
            | (source.get() & 0x0000_3fff & !((CACHE_LINE_WORDS as u32) - 1)),
    )
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
    fn clean_line_writes_one_dirty_hit_and_keeps_it_resident() {
        let mut cache = DataCache::default();
        let address = PhysicalWordAddress::new(0x1234);
        assert!(matches!(
            cache.request(CpuMemoryRequest::Read { address }).unwrap(),
            CacheAction::MainMemoryRequest(MainMemoryRequest::ReadLine { .. })
        ));
        cache
            .complete(MainMemoryResponse::ReadLine { words: line(100) })
            .unwrap();
        cache
            .request(CpuMemoryRequest::Write {
                address,
                value: 0xabcd,
            })
            .unwrap();
        assert!(matches!(
            cache.clean_line(address).unwrap(),
            CacheAction::MainMemoryRequest(MainMemoryRequest::WriteLine {
                line_address,
                words
            }) if line_address == PhysicalWordAddress::new(0x1230) && words[4] == 0xabcd
        ));
        assert_eq!(
            cache.complete(MainMemoryResponse::WriteComplete),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
        );
        assert_eq!(cache.dirty_bits(), 0);
        assert_eq!(
            cache.request(CpuMemoryRequest::Read { address }),
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
    fn line_copy_refills_source_redirects_writeback_and_preserves_source_dirty() {
        let mut cache = DataCache::default();
        let source = PhysicalWordAddress::new(0x1230);
        assert_eq!(
            cache.copy_line(source, 3),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::ReadLine {
                    line_address: source
                }
            ))
        );
        let source_words = line(0x4000);
        assert_eq!(
            cache.complete(MainMemoryResponse::ReadLine {
                words: source_words
            }),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::WriteLine {
                    line_address: PhysicalWordAddress::new(0x0000_d230),
                    words: source_words
                }
            ))
        );
        assert_eq!(
            cache.complete(MainMemoryResponse::WriteComplete),
            Ok(CacheAction::CpuResponse(CpuMemoryResponse::WriteComplete))
        );

        cache
            .request(CpuMemoryRequest::Write {
                address: PhysicalWordAddress::new(0x1232),
                value: 0x5a5a,
            })
            .unwrap();
        let mut dirty_words = source_words;
        dirty_words[2] = 0x5a5a;
        assert_eq!(
            cache.copy_line(source, 4),
            Ok(CacheAction::MainMemoryRequest(
                MainMemoryRequest::WriteLine {
                    line_address: PhysicalWordAddress::new(0x0001_1230),
                    words: dirty_words
                }
            ))
        );
        cache.complete(MainMemoryResponse::WriteComplete).unwrap();
        assert_eq!(
            cache.begin_maintenance(MaintenanceCommand::Clean),
            Ok(Some(MainMemoryRequest::WriteLine {
                line_address: source,
                words: dirty_words
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
