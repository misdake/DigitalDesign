//! Selected fixed-stage execution. No calendar addresses or retained whole input.
//! The generic packed-bank emulator remains an independent reference.
use super::{
    pack_metadata, unpack_metadata, validate, Event, Fault, MetadataAccess, Output, ReadyAccess,
    Site, Snapshot, Step, Tick, CAPTURE_AGE, VALID_MASK,
};
pub use super::{
    COHORT_CAPACITY, METADATA_ROWS, READY_CAPACITY, READY_RAM16_BITS, READY_SDP4_CELLS,
};

pub const NUMERIC_BITS: usize = 50 + 17 + 18 + 36;
pub const ALLOCATION: super::Allocation = super::Allocation {
    numeric_ff_bits: NUMERIC_BITS,
    phase_valid_bits: 1 + 5,
    soft_bits: NUMERIC_BITS + 6 + 14 + 1 + READY_CAPACITY * 72,
    ..super::ALLOCATION
};

/// Read-only view of the actual named registers; host widths are not FF charges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub input_parents: [u16; 2],
    pub input_fractions: [[u8; 2]; 2],
    pub horizontal_parent: u16,
    pub horizontal_u: u8,
    /// Top, bottom; one edge's fine/coarse row totals share this pair.
    pub row_totals: [u16; 2],
    pub fine_corners: [u16; 4],
}

pub struct FixedCoefficientEmu {
    registers: Registers,
    products: [u32; 3],
    odd: bool,
    valid: u16,
    metadata: [u128; METADATA_ROWS],
    metadata_return: u128,
    metadata_read: u8,
    cohort_write: u8,
    cohorts: u8,
    ready_ram: [[u64; 3]; READY_CAPACITY],
    ready_read: u8,
    ready_write: u8,
    queued: u8,
    faulted: bool,
    // Diagnostics/watchdog only: never used for arithmetic or stage selection.
    wall: u64,
    enabled: u64,
    max_wall: u64,
}
impl FixedCoefficientEmu {
    pub fn new(max_wall: u64) -> Result<Self, Fault> {
        if max_wall == 0 || max_wall > 2_000_000 {
            return Err(Fault::WatchdogBound);
        }
        Ok(Self {
            registers: Registers::default(),
            products: [0; 3],
            odd: false,
            valid: 0,
            metadata: [0; METADATA_ROWS],
            metadata_return: 0,
            metadata_read: 0,
            cohort_write: 0,
            cohorts: 0,
            ready_ram: std::array::from_fn(|i| [0xdead_beef ^ i as u64, 0x1234_abcd, 0x13579]),
            ready_read: 0,
            ready_write: 0,
            queued: 0,
            faulted: false,
            wall: 0,
            enabled: 0,
            max_wall,
        })
    }
    pub fn registers(&self) -> Registers {
        self.registers
    }
    pub fn snapshot(&self) -> Snapshot {
        // A checker packing of named fields, not the generic placement bank.
        let r = self.registers;
        let fields = r
            .input_parents
            .into_iter()
            .map(|v| (u64::from(v), 9))
            .chain(
                r.input_fractions
                    .into_iter()
                    .flatten()
                    .map(|v| (u64::from(v), 8)),
            )
            .chain([
                (u64::from(r.horizontal_parent), 9),
                (u64::from(r.horizontal_u), 8),
                (u64::from(r.row_totals[1]), 9),
                (u64::from(r.row_totals[0]), 9),
            ])
            .chain(r.fine_corners.into_iter().map(|v| (u64::from(v), 9)));
        let mut numeric_words = [0; 2];
        let mut at = 0;
        for (value, width) in fields {
            for bit in 0..width {
                numeric_words[at / 64] |= ((value >> bit) & 1) << (at % 64);
                at += 1;
            }
        }
        Snapshot {
            wall: self.wall,
            enabled: self.enabled,
            phase: 1 << u8::from(self.odd),
            valid: self.valid,
            cohorts: self.cohorts,
            queued: self.queued,
            pending_capture: self.valid & (1 << (CAPTURE_AGE - 1)) != 0,
            cohort_pointers: [(self.metadata_read + self.queued) & 15, self.cohort_write],
            ready_pointers: [self.ready_read, self.ready_write],
            numeric_words,
            product_registers: self.products.map(|v| [v]),
        }
    }
    pub fn idle(&self) -> bool {
        self.valid == 0 && self.queued == 0
    }
    pub fn faulted(&self) -> bool {
        self.faulted
    }
    fn ready_row(&self, address: u8, metadata: u128) -> Output {
        let raw = self.ready_ram[usize::from(address)];
        let weights = u128::from(raw[0]) | (u128::from(raw[1] & 255) << 64);
        Output {
            weights: std::array::from_fn(|p| {
                std::array::from_fn(|t| ((weights >> (9 * (p * 4 + t))) & 511) as u16)
            }),
            metadata: unpack_metadata(metadata),
        }
    }
    pub fn output(&self) -> Option<Output> {
        (!self.faulted && self.queued != 0)
            .then(|| self.ready_row(self.ready_read, self.metadata_return))
    }
    // Diagnostic owner lookup only. Arithmetic never reads metadata or a job.
    fn owner(&self, age: u8) -> Option<u128> {
        (self.valid & (1 << (age - 1)) != 0).then(|| {
            let older = (self.valid >> age).count_ones() as usize;
            self.metadata[(usize::from(self.metadata_read + self.queued) + older) % METADATA_ROWS]
        })
    }
    pub fn tick(&mut self, tick: Tick) -> Result<Step, Fault> {
        if self.faulted {
            return Err(Fault::Terminal);
        }
        if self.wall == self.max_wall {
            self.faulted = true;
            return Err(Fault::Watchdog);
        }
        self.wall += 1;
        let result = self.advance(tick);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }
    fn advance(&mut self, tick: Tick) -> Result<Step, Fault> {
        let output = self.output();
        let advance = tick.ce && self.queued < 2;
        let cost = tick.input.map_or(0, |v| {
            v.parents.into_iter().filter(|&p| p != 0).count() as u8
        });
        let input_ready = advance
            && usize::from(self.cohorts) < COHORT_CAPACITY
            && !self.odd
            && tick.work_available >= cost;
        let incoming = tick.input.filter(|_| input_ready);
        if let Some(input) = incoming {
            validate(input)?;
            if tick.work_available > 16 {
                return Err(Fault::WorkCredit);
            }
        }
        let accepted = incoming.is_some();
        let consumed = tick.ce && tick.output_ready && output.is_some();
        let publication = advance && self.valid & (1 << 4) != 0;
        let owners = std::array::from_fn::<_, 5, _>(|i| self.owner(i as u8 + 1));
        let metadata_access = MetadataAccess {
            write: incoming.map(|v| (self.cohort_write, pack_metadata(v.metadata))),
            read_address: (publication || consumed)
                .then_some((self.metadata_read + u8::from(consumed)) & 15),
        };
        let returned = metadata_access
            .read_address
            .map(|a| self.metadata[usize::from(a)]);
        let mut ready_access = ReadyAccess {
            read_address: self.ready_read,
            ..Default::default()
        };
        let mut events = Vec::new();
        if consumed {
            self.metadata_read = (self.metadata_read + 1) & 15;
            self.ready_read ^= 1;
            self.queued -= 1;
            events.push(Event::Consumed(output.unwrap()));
        }
        if advance {
            let old = self.registers;
            let [bottom, br, tr] = self.products.map(|v| (v >> 8) as u16);
            let parent = if self.odd {
                old.horizontal_parent
            } else {
                old.input_parents[0]
            };
            let top = parent.wrapping_sub(bottom) & 511;
            let fu = if self.odd {
                old.horizontal_u
            } else {
                old.input_fractions[0][0]
            };
            let vertical = if self.odd {
                [old.input_parents[0], u16::from(old.input_fractions[0][1])]
            } else {
                [old.input_parents[1], u16::from(old.input_fractions[1][1])]
            };
            let operands = [vertical, [bottom, u16::from(fu)], [top, u16::from(fu)]];
            let corners = [
                old.row_totals[0].wrapping_sub(tr) & 511,
                tr,
                old.row_totals[1].wrapping_sub(br) & 511,
                br,
            ];
            for (age, site) in [(1, 0), (2, 0), (2, 1), (2, 2), (3, 1), (3, 2)] {
                if let Some(metadata) = owners[age - 1] {
                    events.push(Event::Issued {
                        site: Site::Multiply(site as u8),
                        key: ((metadata >> 92) & 63) as u8,
                        age: age as u8,
                        operands: operands[site],
                    });
                }
            }
            for (age, site) in [(2, 0), (3, 0), (3, 1), (3, 2), (4, 1), (4, 2)] {
                if let Some(metadata) = owners[age - 1] {
                    events.push(Event::Returned {
                        site: Site::Multiply(site as u8),
                        key: ((metadata >> 92) & 63) as u8,
                        age: age as u8,
                        value: self.products[site],
                    });
                }
            }
            if let Some(metadata) = owners[3] {
                let mut weights = 0u128;
                for (i, v) in old.fine_corners.into_iter().chain(corners).enumerate() {
                    weights |= u128::from(v) << (9 * i);
                }
                let raw = [weights as u64, (weights >> 64) as u64, 0];
                self.ready_ram[usize::from(self.ready_write)] = raw;
                ready_access.write = Some((self.ready_write, raw));
                events.push(Event::OutputRegister {
                    key: ((metadata >> 92) & 63) as u8,
                });
            }
            // Every read above uses old registers/products, including shared rows.
            if self.valid & 2 != 0 {
                self.registers.horizontal_parent = old.input_parents[1];
                self.registers.horizontal_u = old.input_fractions[1][0];
            }
            if self.valid & 6 != 0 {
                self.registers.row_totals = [top, bottom];
            }
            if self.valid & 4 != 0 {
                self.registers.fine_corners = corners;
            }
            if let Some(input) = incoming {
                self.registers.input_parents = input.parents;
                self.registers.input_fractions = if input.nearest {
                    [[0; 2]; 2]
                } else {
                    input.fractions
                };
            }
            // Like the DSPs, all three product registers update on local CE;
            // invalid-age products are unobserved and never drive a valid row.
            self.products = operands.map(|v| u32::from(v[0]) * u32::from(v[1]));
            if publication {
                let value = self.ready_row(self.ready_write, owners[4].unwrap());
                ready_access.publish = true;
                events.push(Event::Captured(value));
                self.ready_write ^= 1;
                self.queued += 1;
                self.cohorts -= 1;
            }
            if let Some(input) = incoming {
                self.metadata[usize::from(self.cohort_write)] = pack_metadata(input.metadata);
                self.cohort_write = (self.cohort_write + 1) & 15;
                self.cohorts += 1;
                events.push(Event::Accepted {
                    key: input.metadata.key,
                    work_reserved: cost,
                });
            }
            self.valid = ((self.valid << 1) | u16::from(accepted)) & VALID_MASK;
            self.odd = !self.odd;
            self.enabled += 1;
        }
        if let Some(data) = returned {
            self.metadata_return = data;
        }
        if self.cohorts != self.valid.count_ones() as u8
            || usize::from(self.cohorts) > COHORT_CAPACITY
            || self.queued + u8::from(self.valid & (1 << 4) != 0) > 2
            || self.products.iter().any(|v| v >> 17 != 0)
        {
            return Err(Fault::Storage);
        }
        Ok(Step {
            metadata_access,
            ready_access,
            input_ready,
            accepted,
            output,
            consumed,
            work_reserved: if accepted { cost } else { 0 },
            events,
            snapshot: self.snapshot(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(parent: u16, fractions: [[u8; 2]; 2]) -> super::super::Input {
        super::super::Input {
            parents: [parent, 511 - parent],
            fractions,
            nearest: false,
            metadata: super::super::Metadata {
                coordinates: [[0, 1, 0, 1]; 2],
                levels: [5, 4],
                slot: 2,
                key: 17,
                last_fine: parent == 511,
            },
        }
    }
    fn drain(e: &mut FixedCoefficientEmu) -> Output {
        for _ in 0..100 {
            let s = e.tick(Tick::default()).unwrap();
            if s.consumed {
                return s.output.unwrap();
            }
        }
        panic!("bounded selected coefficient drain");
    }
    #[test]
    fn retained_fraction_changes_actual_result() {
        let mut e = FixedCoefficientEmu::new(100).unwrap();
        e.tick(Tick {
            input: Some(sample(511, [[128; 2]; 2])),
            ..Tick::default()
        })
        .unwrap();
        e.registers.input_fractions[0][0] ^= 128;
        assert_eq!(drain(&mut e).weights, [[256, 0, 255, 0], [0; 4]]);
    }
    #[test]
    fn returned_product_is_consumed_without_recomputation() {
        let mut e = FixedCoefficientEmu::new(100).unwrap();
        e.tick(Tick {
            input: Some(sample(511, [[128; 2]; 2])),
            ..Tick::default()
        })
        .unwrap();
        e.tick(Tick::default()).unwrap();
        assert_eq!(e.products[0], 65408);
        e.products[0] = 0;
        assert_eq!(drain(&mut e).weights, [[256, 255, 0, 0], [0; 4]]);
    }
    #[test]
    fn shared_row_corruption_reaches_only_its_old_plane_before_overwrite() {
        for fine in [true, false] {
            let mut e = FixedCoefficientEmu::new(100).unwrap();
            e.tick(Tick {
                input: Some(sample(200, [[128, 192], [64, 128]])),
                ..Tick::default()
            })
            .unwrap();
            for _ in 0..if fine { 2 } else { 3 } {
                e.tick(Tick::default()).unwrap();
            }
            if fine {
                e.registers.row_totals[0] += 1;
            } else {
                e.registers.row_totals[1] += 1;
            }
            let out = drain(&mut e);
            assert_eq!(
                out.weights,
                if fine {
                    [[26, 25, 75, 75], [117, 39, 117, 38]]
                } else {
                    [[25, 25, 75, 75], [117, 39, 118, 38]]
                }
            );
        }
    }
}
