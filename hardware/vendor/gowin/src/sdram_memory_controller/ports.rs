//! Host-facing service contract. IDs are simulation metadata, not hardware tags.
pub const ADDRESS_BYTES: u64 = 8 * 1024 * 1024;
pub const BURST_BYTES: [usize; 4] = [32, 64, 128, 512];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client {
    Display,
    Instruction,
    Data,
    Dma,
    GpuReadOnly,
    FramebufferRead,
    FramebufferWrite,
}
impl Client {
    pub const ALL: [Self; 7] = [
        Self::Display,
        Self::Instruction,
        Self::Data,
        Self::Dma,
        Self::GpuReadOnly,
        Self::FramebufferRead,
        Self::FramebufferWrite,
    ];
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&v| v == self).unwrap()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Constant,
    Host(&'static str),
    Memory,
}
/// An explicit oracle injection boundary, distinct from an audited arithmetic value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OracleWord {
    bits: u64,
    origin: Origin,
}
impl OracleWord {
    pub const fn constant<const VALUE: u64>() -> Self {
        Self {
            bits: VALUE,
            origin: Origin::Constant,
        }
    }
    /// Explicitly inject a host variable. This function performs no unsafe memory access.
    ///
    /// # Safety
    /// The caller must use this only for external stimulus, never to re-import a
    /// host-computed intermediate into a closed audited computation. `source`
    /// identifies that stimulus boundary; it is not a proof of numerical provenance.
    pub unsafe fn from_host(bits: u64, source: &'static str) -> Result<Self, String> {
        if source.is_empty() {
            return Err("oracle host source must be named".into());
        }
        Ok(Self {
            bits,
            origin: Origin::Host(source),
        })
    }
    pub(crate) fn from_memory(bits: u64) -> Self {
        Self {
            bits,
            origin: Origin::Memory,
        }
    }
    pub fn bits(self) -> u64 {
        self.bits
    }
    pub fn origin(self) -> Origin {
        self.origin
    }
    pub fn to_le_bytes(self) -> [u8; 8] {
        self.bits.to_le_bytes()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Burst {
    pub address: u64,
    pub bytes: usize,
    pub access: Access,
}
impl Burst {
    pub fn class(self) -> Result<usize, String> {
        let size = BURST_BYTES
            .iter()
            .position(|&n| n == self.bytes)
            .ok_or("SDRAM burst must be 32/64/128 B, or a 512 B sector group")?;
        if !self.address.is_multiple_of(self.bytes as u64)
            || self
                .address
                .checked_add(self.bytes as u64)
                .is_none_or(|end| end > ADDRESS_BYTES)
        {
            return Err("SDRAM alignment/address range".into());
        }
        Ok(size + if self.access == Access::Write { 4 } else { 0 })
    }
    pub fn sectors(self) -> Result<Vec<Self>, String> {
        self.class()?;
        Ok(if self.bytes == 512 {
            (0..4)
                .map(|i| Self {
                    address: self.address + i * 128,
                    bytes: 128,
                    ..self
                })
                .collect()
        } else {
            vec![self]
        })
    }
}
#[derive(Clone, Debug)]
pub enum Request {
    Read {
        address: u64,
        bytes: usize,
    },
    /// Complete little-endian source payload. One enable bit per byte.
    Write {
        address: u64,
        data: Vec<OracleWord>,
        enables: Vec<u8>,
    },
}
impl Request {
    pub fn burst(&self) -> Result<Burst, String> {
        let burst = match self {
            Self::Read { address, bytes } => Burst {
                address: *address,
                bytes: *bytes,
                access: Access::Read,
            },
            Self::Write {
                address,
                data,
                enables,
            } => {
                if data.len() != enables.len() || data.len() > 64 {
                    return Err("SDRAM write payload/enables".into());
                }
                Burst {
                    address: *address,
                    bytes: data.len() * 8,
                    access: Access::Write,
                }
            }
        };
        burst.class()?;
        Ok(burst)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Started {
        id: u64,
        cycle: u64,
        queued_cycles: u64,
    },
    ReadBeat {
        id: u64,
        cycle: u64,
        index: usize,
        data: OracleWord,
        last: bool,
    },
    Complete {
        id: u64,
        cycle: u64,
    },
}
/// High-level facade. Low-level emu/RTL still implements real ready/valid signals.
/// Read responses cannot be stalled here: reserve sink capacity before submit.
/// A successful submit accepts into the bounded host queue, not onto physical DQ.
pub trait Service {
    fn submit(&mut self, client: Client, request: Request) -> Result<u64, String>;
    fn step(&mut self) -> Result<Vec<Event>, String>;
    fn cycle(&self) -> u64;
    fn idle(&self) -> bool;
}

/// Initial memory image. Runtime initialization uses the explicit host boundary.
#[derive(Clone, Debug)]
pub struct OracleImage {
    base: u64,
    bytes: Vec<u8>,
    source: &'static str,
}
impl OracleImage {
    pub fn filled<const VALUE: u8>(base: u64, bytes: usize) -> Result<Self, String> {
        if bytes == 0
            || base
                .checked_add(bytes as u64)
                .is_none_or(|end| end > ADDRESS_BYTES)
        {
            return Err("SDRAM image range".into());
        }
        Self::checked(base, vec![VALUE; bytes], "constant image")
    }
    /// # Safety
    /// `bytes` must be external stimulus, not a host-computed intermediate being
    /// injected back into an audited frame. The caller names that input boundary.
    pub unsafe fn from_host(
        base: u64,
        bytes: Vec<u8>,
        source: &'static str,
    ) -> Result<Self, String> {
        Self::checked(base, bytes, source)
    }
    fn checked(base: u64, bytes: Vec<u8>, source: &'static str) -> Result<Self, String> {
        if bytes.is_empty()
            || source.is_empty()
            || base
                .checked_add(bytes.len() as u64)
                .is_none_or(|e| e > ADDRESS_BYTES)
        {
            return Err("SDRAM image range/source".into());
        }
        Ok(Self {
            base,
            bytes,
            source,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn base(&self) -> u64 {
        self.base
    }
    pub fn source(&self) -> &'static str {
        self.source
    }
    pub(crate) fn offset(&self, burst: Burst) -> Result<usize, String> {
        burst.class()?;
        let offset = burst
            .address
            .checked_sub(self.base)
            .ok_or("SDRAM below supplied image")?;
        if offset + burst.bytes as u64 > self.bytes.len() as u64 {
            return Err("SDRAM beyond supplied image".into());
        }
        Ok(offset as usize)
    }
    pub(crate) fn read(&self, offset: usize) -> OracleWord {
        OracleWord::from_memory(u64::from_le_bytes(
            self.bytes[offset..offset + 8].try_into().unwrap(),
        ))
    }
    pub(crate) fn write(&mut self, offset: usize, data: &[OracleWord], enables: &[u8]) {
        for (beat, (&word, &enable)) in data.iter().zip(enables).enumerate() {
            for (byte, value) in word.to_le_bytes().into_iter().enumerate() {
                if enable & (1 << byte) != 0 {
                    self.bytes[offset + beat * 8 + byte] = value;
                }
            }
        }
    }
}
