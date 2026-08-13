//! Pure address/lane mapping between G16 words and Tang Nano 20K SDRAM beats.

use super::{Word, CACHE_LINE_WORDS};

pub const SDRAM_DATA_BITS: usize = 32;
pub const SDRAM_BURST_BEATS: usize = 8;
pub const SDRAM_BANK_BITS: usize = 2;
pub const SDRAM_ROW_BITS: usize = 11;
pub const SDRAM_COLUMN_BITS: usize = 8;
/// 600 cycles at 54 MHz is about 11.1 us, leaving margin below the SDRAM's
/// distributed refresh deadline used by the characterized board test.
pub const REFRESH_INTERVAL_CYCLES: u16 = 600;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerAddress {
    pub bank: u8,
    pub row: u16,
    pub column: u8,
}

impl ControllerAddress {
    pub const fn packed(self) -> u32 {
        ((self.bank as u32) << (SDRAM_ROW_BITS + SDRAM_COLUMN_BITS))
            | ((self.row as u32) << SDRAM_COLUMN_BITS)
            | self.column as u32
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaskedWriteBeat {
    pub address: ControllerAddress,
    pub data: u32,
    /// Controller HS/Gowin byte mask: one disables the corresponding byte.
    pub mask: u8,
}

/// Maps the initial 128-KiB CPU window to the bottom of the fitted SDRAM.
pub const fn word_address(address: Word) -> ControllerAddress {
    let beat = (address as u32) >> 1;
    ControllerAddress {
        bank: ((beat >> (SDRAM_ROW_BITS + SDRAM_COLUMN_BITS)) & 0x3) as u8,
        row: ((beat >> SDRAM_COLUMN_BITS) & 0x7ff) as u16,
        column: (beat & 0xff) as u8,
    }
}

pub const fn line_address(address: Word) -> ControllerAddress {
    word_address(address & !((CACHE_LINE_WORDS as Word) - 1))
}

pub const fn masked_word_write(address: Word, value: Word) -> MaskedWriteBeat {
    if address & 1 == 0 {
        MaskedWriteBeat {
            address: word_address(address),
            data: value as u32,
            mask: 0b1100,
        }
    } else {
        MaskedWriteBeat {
            address: word_address(address),
            data: (value as u32) << 16,
            mask: 0b0011,
        }
    }
}

pub fn unpack_line(beats: [u32; SDRAM_BURST_BEATS]) -> [Word; CACHE_LINE_WORDS] {
    let mut words = [0; CACHE_LINE_WORDS];
    for (index, beat) in beats.into_iter().enumerate() {
        words[index * 2] = beat as Word;
        words[index * 2 + 1] = (beat >> 16) as Word;
    }
    words
}

pub fn pack_line(words: [Word; CACHE_LINE_WORDS]) -> [u32; SDRAM_BURST_BEATS] {
    std::array::from_fn(|index| {
        u32::from(words[index * 2]) | (u32::from(words[index * 2 + 1]) << 16)
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryClient {
    Instruction,
    Data,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScheduledOperation {
    Refresh,
    Client {
        client: MemoryClient,
        request: super::MainMemoryRequest,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchedulerError {
    Busy,
    DuplicatePendingClient(MemoryClient),
    CompleteWhileIdle,
}

/// Single-outstanding transaction arbiter in the 54-MHz project domain.
///
/// Refresh wins whenever due. Data wins ties with instruction traffic, but an
/// instruction request already accepted by `start_next` cannot be preempted.
/// Neither client may queue more than one request: the caches already enforce
/// one outstanding miss/write each.
#[derive(Default)]
pub struct MemoryScheduler {
    cycles_since_refresh: u16,
    instruction: Option<super::MainMemoryRequest>,
    data: Option<super::MainMemoryRequest>,
    active: Option<ScheduledOperation>,
}

impl MemoryScheduler {
    pub fn tick(&mut self) {
        self.cycles_since_refresh = self.cycles_since_refresh.saturating_add(1);
    }

    pub fn enqueue(
        &mut self,
        client: MemoryClient,
        request: super::MainMemoryRequest,
    ) -> Result<(), SchedulerError> {
        let slot = match client {
            MemoryClient::Instruction => &mut self.instruction,
            MemoryClient::Data => &mut self.data,
        };
        if slot.is_some() {
            return Err(SchedulerError::DuplicatePendingClient(client));
        }
        *slot = Some(request);
        Ok(())
    }

    pub fn start_next(&mut self) -> Result<Option<ScheduledOperation>, SchedulerError> {
        if self.active.is_some() {
            return Err(SchedulerError::Busy);
        }
        let operation = if self.cycles_since_refresh >= REFRESH_INTERVAL_CYCLES {
            Some(ScheduledOperation::Refresh)
        } else if let Some(request) = self.data.take() {
            Some(ScheduledOperation::Client {
                client: MemoryClient::Data,
                request,
            })
        } else {
            self.instruction
                .take()
                .map(|request| ScheduledOperation::Client {
                    client: MemoryClient::Instruction,
                    request,
                })
        };
        self.active = operation;
        Ok(operation)
    }

    pub fn complete(&mut self) -> Result<ScheduledOperation, SchedulerError> {
        let operation = self
            .active
            .take()
            .ok_or(SchedulerError::CompleteWhileIdle)?;
        if operation == ScheduledOperation::Refresh {
            self.cycles_since_refresh = 0;
        }
        Ok(operation)
    }

    pub fn refresh_due(&self) -> bool {
        self.cycles_since_refresh >= REFRESH_INTERVAL_CYCLES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_line_is_exactly_one_controller_burst() {
        assert_eq!(CACHE_LINE_WORDS, SDRAM_BURST_BEATS * 2);
        assert_eq!(line_address(0x123f).column, 0x18);
        assert_eq!(line_address(0x123f), line_address(0x1230));
    }

    #[test]
    fn halfword_stores_select_the_little_endian_byte_lanes() {
        assert_eq!(
            masked_word_write(0x20, 0xabcd),
            MaskedWriteBeat {
                address: word_address(0x20),
                data: 0x0000_abcd,
                mask: 0b1100,
            }
        );
        assert_eq!(
            masked_word_write(0x21, 0x1234),
            MaskedWriteBeat {
                address: word_address(0x20),
                data: 0x1234_0000,
                mask: 0b0011,
            }
        );
    }

    #[test]
    fn line_pack_round_trips_all_word_lanes() {
        let words = std::array::from_fn(|index| 0x1000 + index as u16);
        assert_eq!(unpack_line(pack_line(words)), words);
    }

    #[test]
    fn refresh_preempts_queued_clients_without_discarding_them() {
        let mut scheduler = MemoryScheduler::default();
        scheduler
            .enqueue(
                MemoryClient::Instruction,
                super::super::MainMemoryRequest::ReadLine { line_address: 0 },
            )
            .unwrap();
        for _ in 0..REFRESH_INTERVAL_CYCLES {
            scheduler.tick();
        }
        assert_eq!(
            scheduler.start_next(),
            Ok(Some(ScheduledOperation::Refresh))
        );
        assert_eq!(scheduler.complete(), Ok(ScheduledOperation::Refresh));
        assert!(matches!(
            scheduler.start_next(),
            Ok(Some(ScheduledOperation::Client {
                client: MemoryClient::Instruction,
                ..
            }))
        ));
    }

    #[test]
    fn data_wins_an_idle_tie_and_accepted_work_is_not_preempted() {
        let mut scheduler = MemoryScheduler::default();
        let read = super::super::MainMemoryRequest::ReadLine { line_address: 0 };
        scheduler.enqueue(MemoryClient::Instruction, read).unwrap();
        scheduler.enqueue(MemoryClient::Data, read).unwrap();
        assert!(matches!(
            scheduler.start_next(),
            Ok(Some(ScheduledOperation::Client {
                client: MemoryClient::Data,
                ..
            }))
        ));
        for _ in 0..REFRESH_INTERVAL_CYCLES {
            scheduler.tick();
        }
        assert_eq!(scheduler.start_next(), Err(SchedulerError::Busy));
        assert!(matches!(
            scheduler.complete(),
            Ok(ScheduledOperation::Client {
                client: MemoryClient::Data,
                ..
            })
        ));
        assert_eq!(
            scheduler.start_next(),
            Ok(Some(ScheduledOperation::Refresh))
        );
    }
}
