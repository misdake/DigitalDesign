//! Demand-only texture-cache leaf: Rust register model.
//!
//! This is a deliberately conservative baseline, not a replacement for the
//! existing timed controller. It executes exactly one demand head, one miss
//! descriptor, no prefetch and no hint queue:
//!
//! - Input is one canonical 72-bit `Group4` packet. The retained output is that
//!   verbatim packet plus the four RAW565 texels read for its 2x2 local taps.
//! - One head slot and one captured result slot. A free input is admitted only
//!   from the pre-edge head state; an output transfers only from the pre-edge
//!   result slot. No same-edge transfer fall-through.
//! - 64 tag lines (16 sets x 4 ways, `TileKey::set`), tree-PLRU with three bits
//!   per set. Allocation prefers an Invalid way, then the PLRU Ready way.
//! - Four explicit 1024x16 synchronous banks. A tap group (2x2 local, wrapped
//!   7->0) is read by one address/config issue that touches all four banks; the
//!   following enabled edge performs the synchronous RAM read into the held
//!   result slot. The address stage is one FF stage, the RAM read is the next
//!   enabled edge; a paused address waits and a captured output holds.
//! - The single demand miss walks one 128-byte, sixteen-beat refill through the
//!   shared GPU-owned [`MemoryPort`] request/response ABI. That port is stepped
//!   exactly once per wall edge, including under `ce=0`; accepted maintenance
//!   drains to its terminal event and a fault never cancels a burst.
//!
//! ## Old-state contract
//!
//! Every compute decision (head hit/miss, read issue, capture, output transfer,
//! descriptor allocation, admission) is evaluated against the register state
//! that existed *before* the current edge, exactly as a Verilog nonblocking
//! always block does. The refill response updates banks, tags and the descriptor
//! in the same edge, but those updates are only visible to compute on the next
//! edge. In particular:
//!
//! - a successful terminal acknowledgement cannot be spent by the head's read
//!   address issue on the same edge (the head still sees `Filling`);
//! - a read captured on this edge cannot fund a new read address on the same
//!   edge (the issue requires an empty pre-edge read stage);
//! - an output transfer *may* fund a new read address on the same edge when the
//!   pre-edge read stage is empty and the pre-edge result is present with
//!   `out_ready`, because the synchronous RAM read into the result happens on a
//!   later enabled edge.
//!
//! The accepted refill, however, is never blocked by `ce`, backpressure or a
//! fault: a presented request is held until the memory accepts it, and an
//! accepted burst drains to its terminal event. [`CacheEmu::abort`] forces that
//! state explicitly; [`CacheEmu::drain_tick`] performs the drain.
//!
//! No oracle or counted helper is called on `tick`, and no precomputed result is
//! stored in the machine. The handler is the GPU-owned memory ABI; the test
//! adapter owns all latency and load behavior.

use crate::memory::ports::{MemoryPort, Request, Response};
use crate::texture::ports::{Group4, Slot, TileKey};

/// Tag sets.
pub const SETS: usize = 16;
/// Ways per set.
pub const WAYS: usize = 4;
/// Total tag lines.
pub const LINES: usize = SETS * WAYS;
/// Words per data bank.
pub const BANK_DEPTH: usize = 1024;
/// Number of data banks.
pub const BANKS: usize = 4;
/// Words per 128-byte tile (`TILE_BYTES / 2`).
pub const TILE_WORDS: usize = 64;
/// 64-bit beats per 128-byte refill.
pub const REFILL_BEATS: usize = 16;

/// Tag state. `Ready` is published only after every numbered beat and a
/// successful terminal acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineState {
    Invalid,
    Filling,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub state: LineState,
    pub key: Option<TileKey>,
}

/// Retained demand output: the verbatim packet plus the actual tap texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    pub payload: i128,
    pub texels: [u16; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct CacheTick {
    pub ce: bool,
    pub input: Option<i128>,
    pub output_ready: bool,
}
impl Default for CacheTick {
    fn default() -> Self {
        Self {
            ce: true,
            input: None,
            output_ready: true,
        }
    }
}

/// Diagnostic classification of the head touch this edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Hit {
        key: TileKey,
        line: usize,
    },
    Filling {
        key: TileKey,
        line: usize,
    },
    Miss {
        key: TileKey,
    },
    Allocate {
        key: TileKey,
        line: usize,
        address: u64,
    },
}

/// Bounded diagnostic state. Not a fitted storage invoice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheState {
    pub wall: u64,
    pub enabled: u64,
    pub head: bool,
    pub result: bool,
    pub read_pending: bool,
    pub pin: Option<usize>,
    pub descriptor: bool,
    pub presented: bool,
    pub started: bool,
    pub next_beat: usize,
    pub ready_lines: usize,
    pub filling_lines: usize,
}

/// Pre-edge observations plus the wall-edge outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheStep {
    pub input_ready: bool,
    pub accepted: bool,
    pub output: Option<Output>,
    pub transferred: bool,
    pub request: Option<Request>,
    pub response: Response,
    pub access: Option<Access>,
    pub state: CacheState,
}

/// Outcome of one explicit drain edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheDrain {
    pub request: Option<Request>,
    pub response: Response,
    pub drained: bool,
}

/// Logical declaration inventory for this leaf. `bank_bits` are the explicit
/// four synchronous 1024x16 banks (BSRAM-class in a fitted design); the two
/// captured slots, tag, PLRU, read pipe, descriptor and control are registers.
/// The slot parameter ROM and the mip-prefix ROM are declared separately. These
/// are exact declared capacities, not a fitted Gowin cell result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub tag_bits: usize,
    pub plru_bits: usize,
    pub bank_bits: usize,
    pub head_bits: usize,
    pub read_bits: usize,
    pub result_bits: usize,
    pub descriptor_bits: usize,
    pub control_bits: usize,
    pub slot_rom_bits: usize,
    pub layer_rom_bits: usize,
}
impl Default for Allocation {
    fn default() -> Self {
        Self {
            // 64 lines x (2 state + 22 key).
            tag_bits: LINES * (2 + 22),
            // 16 sets x 3 tree bits.
            plru_bits: SETS * 3,
            // Four explicit banks.
            bank_bits: BANKS * BANK_DEPTH * 16,
            // Captured head slot: the verbatim 72-bit packet plus valid.
            head_bits: 72 + 1,
            // 72 payload + 6 line + 4x4 local + 4x2 tap routing + valid. The key
            // is not retained separately: it is the low 22 bits of the payload.
            read_bits: 72 + 6 + 4 * 4 + 4 * 2 + 1,
            // 72 payload + 4x16 texels + valid.
            result_bits: 72 + 4 * 16 + 1,
            // 22 key + 6 line + 32 address + started + presented + 5 next + valid.
            descriptor_bits: 22 + 6 + 32 + 1 + 1 + 5 + 1,
            // pin valid + 6-bit pin line + fault.
            control_bits: 1 + 6 + 1,
            // 16 slots x (32-bit base + full flag + 4-bit max level + valid).
            slot_rom_bits: 16 * (32 + 1 + 4 + 1),
            // 11 mip prefixes x 32 bits, literal ROM.
            layer_rom_bits: 11 * 32,
        }
    }
}
impl Allocation {
    pub fn total_state_bits(self) -> usize {
        self.tag_bits
            + self.plru_bits
            + self.bank_bits
            + self.head_bits
            + self.read_bits
            + self.result_bits
            + self.descriptor_bits
            + self.control_bits
            + self.slot_rom_bits
            + self.layer_rom_bits
    }
}

/// The 2x2 tap local coordinates (wrapped 7->0) mapped to bank and bank-local
/// word index using the documented rotated layout: `bank = {y0^x1, x0}` and
/// `local = {y[2:0], x[2]}`.
pub fn tap_map(top_left_local: [u8; 2]) -> ([u8; WAYS], [usize; WAYS]) {
    let mut bank = [0_u8; WAYS];
    let mut local = [0_usize; WAYS];
    for (t, (b, l)) in bank.iter_mut().zip(local.iter_mut()).enumerate() {
        let x = (usize::from(top_left_local[0]) + (t & 1)) & 7;
        let y = (usize::from(top_left_local[1]) + (t >> 1)) & 7;
        *b = ((((y & 1) ^ ((x >> 1) & 1)) << 1) | (x & 1)) as u8;
        *l = (y & 7) * 2 + ((x >> 2) & 1);
    }
    (bank, local)
}

fn key_from_payload(payload: i128) -> TileKey {
    let w = payload as u128;
    TileKey {
        slot: (w & 15) as u8,
        n: ((w >> 4) & 15) as u8,
        x: ((w >> 8) & 127) as u8,
        y: ((w >> 15) & 127) as u8,
    }
}

fn lookup_in(lines: &[Line; LINES], key: TileKey) -> Option<(usize, LineState)> {
    let base = key.set() * WAYS;
    (base..base + WAYS)
        .find(|&i| lines[i].state != LineState::Invalid && lines[i].key == Some(key))
        .map(|i| (i, lines[i].state))
}

fn touch_value(bits: u8, line: usize) -> u8 {
    let way = line % WAYS;
    if way < 2 {
        (bits & !3) | 1 | (((way ^ 1) as u8) << 1)
    } else {
        (bits & !5) | ((((way ^ 1) as u8) & 1) << 2)
    }
}

fn victim_in(
    lines: &[Line; LINES],
    plru: &[u8; SETS],
    pin: Option<usize>,
    key: TileKey,
) -> Option<usize> {
    let base = key.set() * WAYS;
    let usable = |w: usize| {
        let i = base + w;
        pin != Some(i) && lines[i].state != LineState::Filling
    };
    for w in 0..WAYS {
        if lines[base + w].state == LineState::Invalid && usable(w) {
            return Some(base + w);
        }
    }
    let b = plru[key.set()];
    let root = usize::from(b & 1);
    let left = usize::from(b >> 1 & 1);
    let right = usize::from(b >> 2 & 1);
    let halves = [[left, left ^ 1], [2 + right, 2 + (right ^ 1)]];
    [
        halves[root][0],
        halves[root][1],
        halves[root ^ 1][0],
        halves[root ^ 1][1],
    ]
    .into_iter()
    .find(|&w| lines[base + w].state == LineState::Ready && usable(w))
    .map(|w| base + w)
}

#[derive(Clone, Copy)]
struct Descriptor {
    key: TileKey,
    line: usize,
    address: u64,
    started: bool,
    presented: bool,
    next: usize,
}

/// One address/config FF stage. The key is the low 22 bits of `payload`, so no
/// duplicate key field is retained.
#[derive(Clone, Copy)]
struct ReadIssue {
    payload: i128,
    line: usize,
    /// Bank-local word index for each bank.
    local_of_bank: [usize; BANKS],
    /// Tap index the word captured from each bank belongs to.
    tap_of_bank: [u8; BANKS],
}

/// Snapshot of the pre-edge register state used by the old-state compute.
#[derive(Clone)]
struct OldState {
    head: Option<Group4>,
    read: Option<ReadIssue>,
    result: Option<Output>,
    descriptor: Option<Descriptor>,
    lines: [Line; LINES],
    plru: [u8; SETS],
    pin: Option<usize>,
}

pub struct CacheEmu<M: MemoryPort> {
    slots: Vec<Slot>,
    memory: M,
    lines: [Line; LINES],
    banks: Box<[[u16; BANK_DEPTH]; BANKS]>,
    plru: [u8; SETS],
    head: Option<Group4>,
    read: Option<ReadIssue>,
    result: Option<Output>,
    pin: Option<usize>,
    descriptor: Option<Descriptor>,
    faulted: bool,
    wall: u64,
    enabled: u64,
    max_wall: u64,
}

impl<M: MemoryPort> CacheEmu<M> {
    pub fn new(slots: Vec<Slot>, memory: M, max_wall: u64) -> Result<Self, String> {
        if slots.is_empty() || slots.len() > 16 {
            return Err("texture cache slot count".into());
        }
        if max_wall == 0 || max_wall > 2_000_000 {
            return Err("texture cache watchdog bound".into());
        }
        for slot in &slots {
            slot.validate()?;
        }
        Ok(Self {
            slots,
            memory,
            lines: [Line {
                state: LineState::Invalid,
                key: None,
            }; LINES],
            banks: Box::new([[0; BANK_DEPTH]; BANKS]),
            plru: [0; SETS],
            head: None,
            read: None,
            result: None,
            pin: None,
            descriptor: None,
            faulted: false,
            wall: 0,
            enabled: 0,
            max_wall,
        })
    }

    pub fn lookup(&self, key: TileKey) -> Option<(usize, LineState)> {
        lookup_in(&self.lines, key)
    }

    pub fn line(&self, index: usize) -> Line {
        self.lines[index]
    }

    pub fn head(&self) -> Option<Group4> {
        self.head.clone()
    }

    /// Pre-edge retained output, without advancing the machine. Continuous
    /// fanout only: no state or port is added.
    pub fn output(&self) -> Option<Output> {
        self.result
    }

    /// Pre-edge admission predicate, without advancing the machine. Continuous
    /// fanout only: no state or port is added.
    pub fn input_ready(&self, ce: bool) -> bool {
        ce && self.head.is_none() && !self.faulted
    }

    pub fn bank(&self, bank: usize, index: usize) -> u16 {
        self.banks[bank][index]
    }

    pub fn memory(&self) -> &M {
        &self.memory
    }
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    pub fn memory_mut(&mut self) -> &mut M {
        &mut self.memory
    }

    pub fn faulted(&self) -> bool {
        self.faulted
    }

    /// True when no accepted or presented refill remains to drain.
    pub fn drained(&self) -> bool {
        self.descriptor.is_none()
    }

    pub fn idle(&self) -> bool {
        self.head.is_none()
            && self.read.is_none()
            && self.result.is_none()
            && self.pin.is_none()
            && self.descriptor.is_none()
    }

    /// Explicit abort. Suppresses compute and clears the demand head, read
    /// stage, result and pin; invalidates any `Filling` publication. A request
    /// that was already presented to the memory is held until acceptance and
    /// drains to its terminal event; a never-presented descriptor is released.
    pub fn abort(&mut self) {
        self.faulted = true;
        self.head = None;
        self.read = None;
        self.result = None;
        self.pin = None;
        if let Some(descriptor) = self.descriptor {
            self.lines[descriptor.line] = Line {
                state: LineState::Invalid,
                key: None,
            };
            if !descriptor.presented {
                self.descriptor = None;
            }
        }
    }

    /// Discard a READY line before a new refill. FILLING lines are protected:
    /// an in-flight descriptor and any pinned demand are never modified here.
    pub fn invalidate(&mut self, key: TileKey) -> bool {
        let base = key.set() * WAYS;
        let mut hit = false;
        for i in base..base + WAYS {
            if self.lines[i].state == LineState::Ready && self.lines[i].key == Some(key) {
                self.lines[i] = Line {
                    state: LineState::Invalid,
                    key: None,
                };
                hit = true;
            }
        }
        hit
    }

    /// Rebind the immutable slot table. Requires a healthy, drained sampler so
    /// no accepted request is silently cancelled.
    pub fn rebind(&mut self, slots: Vec<Slot>) -> Result<(), String> {
        if self.faulted || !self.idle() {
            return Err("texture cache rebind requires a healthy drained sampler".into());
        }
        for slot in &slots {
            slot.validate()?;
        }
        self.slots = slots;
        self.lines = [Line {
            state: LineState::Invalid,
            key: None,
        }; LINES];
        self.plru = [0; SETS];
        Ok(())
    }

    fn touch(&mut self, line: usize) {
        let set = line / WAYS;
        self.plru[set] = touch_value(self.plru[set], line);
    }

    pub fn state(&self) -> CacheState {
        CacheState {
            wall: self.wall,
            enabled: self.enabled,
            head: self.head.is_some(),
            result: self.result.is_some(),
            read_pending: self.read.is_some(),
            pin: self.pin,
            descriptor: self.descriptor.is_some(),
            presented: self.descriptor.is_some_and(|d| d.presented),
            started: self.descriptor.is_some_and(|d| d.started),
            next_beat: self.descriptor.map_or(0, |d| d.next),
            ready_lines: self
                .lines
                .iter()
                .filter(|l| l.state == LineState::Ready)
                .count(),
            filling_lines: self
                .lines
                .iter()
                .filter(|l| l.state == LineState::Filling)
                .count(),
        }
    }

    /// One compute wall edge. Refuses to run after a fault or abort; use
    /// [`CacheEmu::drain_tick`] to finish an accepted refill.
    pub fn tick(&mut self, tick: CacheTick) -> Result<CacheStep, String> {
        if self.faulted {
            return Err("texture cache terminal fault; recreate before reuse".into());
        }
        if self.wall >= self.max_wall {
            self.abort();
            return Err("texture cache wall watchdog".into());
        }
        let step = self.advance(tick);
        if step.is_err() {
            self.abort();
        }
        step
    }

    /// One drain wall edge. Steps the shared memory exactly once, discards beat
    /// numbers, consumes the terminal event and releases the descriptor. The
    /// caller owns the bound; this method has no implicit loop or watchdog.
    pub fn drain_tick(&mut self, _ce: bool) -> Result<CacheDrain, String> {
        self.wall += 1;
        let request = self
            .descriptor
            .filter(|d| !d.started && d.presented)
            .map(|d| Request {
                address_bytes: d.address,
                write: false,
            });
        if request.is_some() {
            if let Some(d) = self.descriptor.as_mut() {
                d.presented = true;
            }
        }
        let response = match self.memory.cycle(request, None) {
            Ok(response) => response,
            Err(error) => {
                if let Some(descriptor) = self.descriptor.take() {
                    self.lines[descriptor.line] = Line {
                        state: LineState::Invalid,
                        key: None,
                    };
                }
                return Err(error);
            }
        };
        let mut protocol_error = None;
        if response.accepted {
            if let Some(d) = self.descriptor.as_mut() {
                if d.started {
                    protocol_error = Some("texture cache duplicate refill acceptance");
                }
                d.started = true;
            }
        }
        if response.complete.is_some() {
            if let Some(descriptor) = self.descriptor.take() {
                self.lines[descriptor.line] = Line {
                    state: LineState::Invalid,
                    key: None,
                };
            }
        }
        if let Some(error) = protocol_error {
            return Err(error.into());
        }
        Ok(CacheDrain {
            request,
            response,
            drained: self.descriptor.is_none(),
        })
    }

    fn refill_response(&mut self, response: Response) -> Result<(), String> {
        let mut protocol_error = None;
        if response.accepted {
            match self.descriptor.as_mut() {
                Some(d) if !d.started => d.started = true,
                Some(_) => protocol_error = Some("texture cache duplicate refill acceptance"),
                None => protocol_error = Some("texture cache unsolicited refill acceptance"),
            }
        }
        if let Some((index, data)) = response.read {
            if let Some(descriptor) = self.descriptor.as_mut() {
                if !descriptor.started
                    || usize::from(index) != descriptor.next
                    || usize::from(index) >= REFILL_BEATS
                {
                    protocol_error = Some("texture cache refill beat identity/order");
                } else {
                    let line = descriptor.line;
                    for j in 0..4 {
                        let linear = usize::from(index) * 4 + j;
                        let x = linear % 8;
                        let y = linear / 8;
                        let bank = (((y & 1) ^ ((x >> 1) & 1)) << 1) | (x & 1);
                        let local = (y & 7) * 2 + ((x >> 2) & 1);
                        self.banks[bank][line * 16 + local] = (data >> (j * 16)) as u16;
                    }
                    descriptor.next += 1;
                }
            } else {
                protocol_error = Some("texture cache beat without descriptor");
            }
        }
        if let Some(ok) = response.complete {
            // A terminal always ends the transport: release the descriptor and
            // never publish a partial or failed line.
            let descriptor = self
                .descriptor
                .take()
                .ok_or("texture cache completion without descriptor")?;
            self.lines[descriptor.line] = Line {
                state: LineState::Invalid,
                key: None,
            };
            if ok {
                if !descriptor.started || descriptor.next != REFILL_BEATS {
                    protocol_error.get_or_insert("texture cache early refill completion");
                } else if protocol_error.is_none() && !self.faulted {
                    self.lines[descriptor.line] = Line {
                        state: LineState::Ready,
                        key: Some(descriptor.key),
                    };
                    self.touch(descriptor.line);
                }
            } else {
                protocol_error = Some("texture cache memory error");
            }
        }
        protocol_error.map_or(Ok(()), |e| Err(e.into()))
    }

    fn advance(&mut self, tick: CacheTick) -> Result<CacheStep, String> {
        self.wall += 1;
        let output = self.result;
        let input_ready = self.input_ready(tick.ce);
        let accepted = input_ready && tick.input.is_some();
        // Validate only an accepted request; a held offer is ignored.
        let incoming = (|| -> Result<Option<Group4>, String> {
            if accepted {
                let payload = tick.input.expect("accepted offer");
                let group = Group4::unpack72(payload)?;
                group
                    .key
                    .address(&self.slots)
                    .map_err(|e| format!("texture cache address: {e}"))?;
                Ok(Some(group))
            } else {
                Ok(None)
            }
        })();

        let old = OldState {
            head: self.head.clone(),
            read: self.read,
            result: self.result,
            descriptor: self.descriptor,
            lines: self.lines,
            plru: self.plru,
            pin: self.pin,
        };

        // The demand miss descriptor presents its immutable request every wall
        // edge until the GPU-owned memory accepts it. Exactly one call per edge.
        let request = old
            .descriptor
            .filter(|d| !d.started && (d.presented || !self.faulted))
            .map(|d| Request {
                address_bytes: d.address,
                write: false,
            });
        if request.is_some() {
            if let Some(d) = self.descriptor.as_mut() {
                d.presented = true;
            }
        }
        let response = match self.memory.cycle(request, None) {
            Ok(response) => response,
            Err(error) => {
                // Fatal adapter error: external ownership is unknown, so the
                // ambiguous request is abandoned rather than retried.
                if let Some(descriptor) = self.descriptor.take() {
                    self.lines[descriptor.line] = Line {
                        state: LineState::Invalid,
                        key: None,
                    };
                }
                return Err(error);
            }
        };

        // Accepted maintenance is independent of the compute CE.
        if incoming.is_err() {
            // Invalid ingress must not erase an unrelated same-edge terminal.
            // Suppress publication while still consuming that response.
            self.abort();
        }
        self.refill_response(response)?;
        let incoming = incoming?;

        let mut access = None;
        let mut transferred = false;
        if tick.ce {
            self.enabled += 1;
            transferred = self.apply_enabled(&old, tick.output_ready, &incoming, &mut access)?;
        }
        Ok(CacheStep {
            input_ready,
            accepted,
            output,
            transferred,
            request,
            response,
            access,
            state: self.state(),
        })
    }

    fn apply_enabled(
        &mut self,
        old: &OldState,
        output_ready: bool,
        incoming: &Option<Group4>,
        access: &mut Option<Access>,
    ) -> Result<bool, String> {
        // 1. Output transfer uses only the pre-edge result slot. A transfer on
        //    this edge cannot fund a same-edge capture.
        let transferred = output_ready && old.result.is_some();
        if transferred {
            self.result = None;
        }

        // 2. Capture the pending synchronous RAM read if the result slot was
        //    free before this edge. The address was registered on an earlier
        //    edge; this enabled edge performs the RAM read into the held result.
        if let Some(issue) = old.read {
            if old.result.is_none() {
                let mut texels = [0_u16; BANKS];
                for (b, texel) in texels.iter_mut().enumerate() {
                    *texel = self.banks[b][issue.line * 16 + issue.local_of_bank[b]];
                }
                let mut ordered = [0_u16; 4];
                for b in 0..BANKS {
                    ordered[usize::from(issue.tap_of_bank[b])] = texels[b];
                }
                self.result = Some(Output {
                    payload: issue.payload,
                    texels: ordered,
                });
                self.read = None;
                self.pin = None;
                *access = Some(Access::Hit {
                    key: key_from_payload(issue.payload),
                    line: issue.line,
                });
            }
        }

        // 3. Issue one address/config stage for a READY-hit head. The head sees
        //    only the pre-edge tag state, so a same-edge terminal acknowledgement
        //    cannot be spent here; a captured read cannot fund a new address.
        let result_free = old.result.is_none() || (old.result.is_some() && output_ready);
        if old.read.is_none() && result_free {
            if let Some(head) = old.head.clone() {
                match lookup_in(&old.lines, head.key) {
                    Some((line, LineState::Ready)) => {
                        let (bank_of_tap, local_of_tap) = tap_map(head.top_left_local);
                        let mut local_of_bank = [0_usize; BANKS];
                        let mut tap_of_bank = [0_u8; BANKS];
                        for t in 0..WAYS {
                            let b = usize::from(bank_of_tap[t]);
                            local_of_bank[b] = local_of_tap[t];
                            tap_of_bank[b] = t as u8;
                        }
                        let payload = head
                            .pack72()
                            .map_err(|e| format!("texture cache head packet: {e}"))?
                            as i128;
                        self.read = Some(ReadIssue {
                            payload,
                            line,
                            local_of_bank,
                            tap_of_bank,
                        });
                        self.pin = Some(line);
                        let set = line / WAYS;
                        self.plru[set] = touch_value(old.plru[set], line);
                        self.head = None;
                        *access = Some(Access::Hit {
                            key: head.key,
                            line,
                        });
                    }
                    Some((line, LineState::Filling)) => {
                        *access = Some(Access::Filling {
                            key: head.key,
                            line,
                        });
                    }
                    Some((_, LineState::Invalid)) | None => {
                        *access = Some(Access::Miss { key: head.key });
                    }
                }
            }
        }

        // 4. Allocate the single demand descriptor on a genuine miss.
        if old.descriptor.is_none() {
            if let Some(head) = old.head.clone() {
                if lookup_in(&old.lines, head.key).is_none() {
                    if let Some(line) = victim_in(&old.lines, &old.plru, old.pin, head.key) {
                        let address = head
                            .key
                            .address(&self.slots)
                            .map_err(|e| format!("texture cache address: {e}"))?;
                        self.lines[line] = Line {
                            state: LineState::Filling,
                            key: Some(head.key),
                        };
                        self.descriptor = Some(Descriptor {
                            key: head.key,
                            line,
                            address,
                            started: false,
                            presented: false,
                            next: 0,
                        });
                        *access = Some(Access::Allocate {
                            key: head.key,
                            line,
                            address,
                        });
                    }
                }
            }
        }

        // 5. Admit a new input only when the head was free before this edge.
        if incoming.is_some() {
            self.head = incoming.clone();
        }
        Ok(transferred)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal identity memory: one 16-beat terminal per accepted request.
    struct Identity {
        ready: bool,
        busy: bool,
        count: usize,
    }
    impl MemoryPort for Identity {
        fn cycle(
            &mut self,
            request: Option<Request>,
            _write: Option<u64>,
        ) -> Result<Response, String> {
            let mut response = Response::default();
            if self.busy {
                response.read = Some((self.count as u8, 0));
                if self.count + 1 == REFILL_BEATS {
                    response.complete = Some(true);
                    self.busy = false;
                } else {
                    self.count += 1;
                }
            } else if let Some(request) = request {
                if self.ready && !request.write {
                    response.accepted = true;
                    self.busy = true;
                    self.count = 0;
                }
            }
            Ok(response)
        }
    }

    #[test]
    fn tap_map_covers_all_four_banks_for_every_local_pair() {
        for x in 0..8_u8 {
            for y in 0..8_u8 {
                let (bank, local) = tap_map([x, y]);
                let mut seen = [false; BANKS];
                for b in bank {
                    seen[usize::from(b)] = true;
                }
                assert!(seen.iter().all(|s| *s), "banks at ({x},{y})");
                assert!(local.iter().all(|l| *l < 16));
            }
        }
    }

    #[test]
    fn allocation_is_bounded_and_exact() {
        let allocation = Allocation::default();
        assert_eq!(allocation.tag_bits, 64 * 24);
        assert_eq!(allocation.plru_bits, 48);
        assert_eq!(allocation.bank_bits, 4 * 1024 * 16);
        assert_eq!(allocation.read_bits, 103);
        assert_eq!(allocation.descriptor_bits, 68);
        assert_eq!(allocation.slot_rom_bits, 16 * 38);
        assert_eq!(allocation.layer_rom_bits, 352);
        assert_eq!(
            allocation.total_state_bits(),
            1536 + 48 + 65536 + 73 + 103 + 137 + 68 + 8 + 608 + 352
        );
    }

    #[test]
    fn invalid_slot_is_rejected() {
        let slots = vec![Slot {
            base_address: 0x1001,
            has_full_mip: false,
            max_size_log2: 10,
            valid: true,
        }];
        assert!(CacheEmu::new(
            slots,
            Identity {
                ready: true,
                busy: false,
                count: 0
            },
            64
        )
        .is_err());
    }
}
