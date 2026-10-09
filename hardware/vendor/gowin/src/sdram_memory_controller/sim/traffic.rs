//! Deterministic estimated traffic, independent of a CPU or display implementation.
use super::super::ports::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainPolicy {
    ExistingUnchained,
    ChainedCandidate,
}
#[derive(Clone, Debug)]
pub struct Stream {
    pub client: Client,
    pub period: u64,
    pub phase: u64,
    pub bytes: usize,
    /// Every Nth transaction writes; zero means read only.
    pub write_every: u64,
    pub base: u64,
    pub span: u64,
    pub stride: u64,
    /// A batch arrives together, preserving the configured average bandwidth.
    pub batch: usize,
}
impl Stream {
    pub fn validate(&self) -> Result<(), String> {
        Burst {
            address: self.base,
            bytes: self.bytes,
            access: Access::Read,
        }
        .class()?;
        if self.period == 0
            || self.period > 100_000_000
            || self.batch == 0
            || self.batch > 256
            || self.span < self.bytes as u64
            || !self.span.is_multiple_of(self.bytes as u64)
            || !self.stride.is_multiple_of(self.bytes as u64)
            || self
                .base
                .checked_add(self.span)
                .is_none_or(|e| e > ADDRESS_BYTES)
            || self.phase > 100_000_000
        {
            return Err("SDRAM traffic configuration".into());
        }
        Ok(())
    }
    pub fn burst(&self, index: u64) -> Burst {
        Burst {
            address: self.base + index.wrapping_mul(self.stride) % self.span,
            bytes: self.bytes,
            access: if self.write_every != 0 && (index + 1).is_multiple_of(self.write_every) {
                Access::Write
            } else {
                Access::Read
            },
        }
    }
    pub fn arrival(&self, index: u64) -> Option<u64> {
        (index / self.batch as u64)
            .checked_mul(self.period)?
            .checked_add(self.phase)
    }
}
#[derive(Clone, Debug, Default)]
pub struct Load {
    pub streams: Vec<Stream>,
}
impl Load {
    pub fn solo() -> Self {
        Self::default()
    }
    /// 11.52 MB/s: 400*240*60*2 bytes, at 54 MHz. An estimate, not a board trace.
    pub fn display(batch: usize) -> Self {
        Self {
            streams: vec![Stream {
                client: Client::Display,
                period: 150 * batch as u64,
                phase: 0,
                bytes: 32,
                write_every: 0,
                base: 4 * 1024 * 1024,
                span: 192_000,
                stride: 32,
                batch,
            }],
        }
    }
    /// Estimated I-cache 1 MB/s and D-cache 6 MB/s (2 reads : 1 write).
    pub fn cpu() -> Self {
        Self {
            streams: vec![
                Stream {
                    client: Client::Instruction,
                    period: 1728,
                    phase: 17,
                    bytes: 32,
                    write_every: 0,
                    base: 5 * 1024 * 1024,
                    span: 65536,
                    stride: 32,
                    batch: 1,
                },
                Stream {
                    client: Client::Data,
                    period: 288,
                    phase: 43,
                    bytes: 32,
                    write_every: 3,
                    base: 6 * 1024 * 1024,
                    span: 65536,
                    stride: 4096 + 32,
                    batch: 1,
                },
            ],
        }
    }
    pub fn display_and_cpu(batch: usize) -> Self {
        let mut load = Self::display(batch);
        load.streams.extend(Self::cpu().streams);
        load
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.streams.len() > 16 {
            return Err("SDRAM stream count".into());
        }
        for stream in &self.streams {
            stream.validate()?;
        }
        Ok(())
    }
}
