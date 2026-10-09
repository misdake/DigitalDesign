//! One outstanding naturally aligned 128-byte transaction per port instance.
//! This interface describes transport, not a latency or arbitration model.

pub const BURST_BYTES: usize = 128;
pub const BURST_BEATS: usize = BURST_BYTES / 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub address_bytes: u64,
    pub write: bool,
}
impl Request {
    /// Physical address bounds are checked by the board adapter separately.
    pub fn validate(self) -> Result<(), String> {
        if !self.address_bytes.is_multiple_of(BURST_BYTES as u64)
            || self.address_bytes.checked_add(BURST_BYTES as u64).is_none()
        {
            return Err("GPU burst alignment/address overflow".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// Request valid/ready handshake on this edge. Not physical completion.
    pub accepted: bool,
    /// Current write beat consumed on this edge; advances the source once.
    pub write_accepted: bool,
    /// Ordered index 0..15. No response backpressure after request acceptance.
    pub read: Option<(u8, u64)>,
    /// Exactly one terminal response: true = success, false = memory error.
    /// A read may terminate on its last data edge; a write must wait for ACK.
    /// An error may terminate before all beats; partial writes are not rolled back.
    pub complete: Option<bool>,
}

pub trait MemoryPort {
    /// Advance one wall-clock edge, including when the compute CE is zero.
    ///
    /// A blocked request and blocked write beat must remain stable. The caller
    /// reserves all 16 read destinations before acceptance. For a write it
    /// prefetches the first beat before presenting the request and reserves
    /// continuous delivery of the remaining beats. `None` is allowed before
    /// request admission, not whenever an accepted physical write needs data:
    /// the current Gowin MC has no arbitrary mid-burst source backpressure.
    ///
    /// Drop request valid after `accepted`, advance write only on
    /// `write_accepted`, and retain ownership until `complete`. A fault stops
    /// new requests but must keep clocking accepted work to its terminal event.
    /// Errors returned here mean caller/adapter protocol failure, not a memory
    /// error response. No reset or implicit cancellation is part of this port.
    fn cycle(&mut self, request: Option<Request>, write: Option<u64>) -> Result<Response, String>;
}
