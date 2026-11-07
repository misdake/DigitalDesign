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
    UnalignedLine,
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
        destination: PhysicalWordAddress,
        way: usize,
    },
    /// The copy source is being refilled into the selected way.
    CopyRefill {
        source: PhysicalWordAddress,
        destination: PhysicalWordAddress,
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
    /// Read the resident CPU view without replacement, allocation or dirty changes.
    pub(crate) fn cached_word(&self, address: PhysicalWordAddress) -> Option<Word> {
        let d = decode(address);
        self.store
            .hit_way(address)
            .map(|way| self.store.words[way][d.set][d.word])
    }
    /// Addressed metadata view for a cycle model's synchronous maintenance
    /// scan. Invalid records never produce a write-back candidate.
    pub(crate) fn dirty_line_address(&self, way: usize, set: usize) -> Option<PhysicalWordAddress> {
        assert!(way < CACHE_WAYS && set < CACHE_SETS);
        (self.store.valid[way][set] && self.dirty[way][set]).then(|| self.line_address(way, set))
    }

    /// Independent data view for the cycle wrapper's two ordered maintenance
    /// contexts. Inspection does not clear dirty state or claim a terminal.
    pub(crate) fn maintenance_line(&self, index: u8) -> MainMemoryRequest {
        let way = usize::from(index / 64);
        let set = usize::from(index % 64);
        let line_address = self
            .dirty_line_address(way, set)
            .expect("dirty maintenance candidate");
        MainMemoryRequest::WriteLine {
            line_address,
            words: self.store.line_words(way, set),
        }
    }

    pub(crate) fn retire_maintenance_line(&mut self, index: u8) {
        let way = usize::from(index / 64);
        let set = usize::from(index % 64);
        assert!(
            self.dirty_line_address(way, set).is_some(),
            "ordered dirty retirement"
        );
        self.dirty[way][set] = false;
    }

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

    /// Copies the newest aligned source line to an aligned physical destination. A resident destination alias is discarded
    /// first because the complete destination line is overwritten. The
    /// destination is not installed in the cache.
    pub fn copy_line(
        &mut self,
        source: PhysicalWordAddress,
        destination: PhysicalWordAddress,
    ) -> Result<CacheAction, CacheError> {
        if self.pending.is_some() || self.maintenance.is_some() {
            return Err(CacheError::Busy);
        }
        let decoded = decode(source);
        if (source.get() | destination.get()) & 15 != 0 {
            return Err(CacheError::UnalignedLine);
        }
        if destination != source {
            if let Some(destination_way) = self.store.hit_way(destination) {
                self.store.valid[destination_way][decode(destination).set] = false;
                self.dirty[destination_way][decode(destination).set] = false;
            }
        }
        // The discarded destination is now a free way for a cold source.
        let way = self.store.victim_way(decoded.set);
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
                destination,
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
                destination,
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
                    destination,
                    way,
                },
                MainMemoryResponse::WriteComplete,
            ) => {
                let decoded = decode(source);
                self.dirty[way][decoded.set] = false;
                self.pending = Some(Pending::CopyRefill {
                    source,
                    destination,
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
                    destination,
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
                        line_address: destination,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheAction {
    CpuResponse(CpuMemoryResponse),
    MainMemoryRequest(MainMemoryRequest),
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
