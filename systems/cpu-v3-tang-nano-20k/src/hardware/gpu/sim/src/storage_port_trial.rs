//! Clocked, value-carrying storage ports for geometry scheduling trials.
//!
//! A RAM16 read is physically asynchronous, but this trial captures it at an
//! edge before a DSP may consume it. This is an explicit frequency-oriented
//! register boundary, not an intrinsic RAM16 latency. BSRAM reads are
//! synchronous. Both kinds therefore deliver a tagged value on the next
//! `tick`. There is no array inspection or unmetered operand access here.

use std::collections::HashSet;

use crate::fixed::{Q14, Q16};

pub const TRANSFORMED_V6_ROWS: usize = 3;
pub const TRANSFORMED_V6_ROW_BITS: u8 = 72;
pub const TRANSFORMED_V6_BITS: usize = 216;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransformedLayout {
    pub word_bits: u8,
    pub banks: usize,
}

impl TransformedLayout {
    pub fn rows_per_vertex(self) -> usize {
        assert!([36, 72, 108].contains(&self.word_bits));
        TRANSFORMED_V6_BITS / usize::from(self.word_bits)
    }

    pub fn vertices_per_bank(self) -> usize {
        512 / self.rows_per_vertex()
    }

    pub fn shape(self) -> StorageShape {
        assert!(self.banks > 0);
        StorageShape {
            kind: StorageKind::Bsram,
            banks: self.banks,
            rows_per_bank: 512,
            width_bits: self.word_bits,
            read_views: 1,
        }
    }
}

/// Trial output ABI: 4xQ16 clip, 3xQ2.14 normal, 2xUNORM12 UV, RGB565.
/// The 216 logical bits exactly fill three 72-bit BSRAM rows. Unlike the
/// earlier RGBA8888 prototype this record preserves the mesh UV channels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransformedV6 {
    pub clip: [Q16; 4],
    pub normal: [Q14; 3],
    pub uv12: [u16; 2],
    pub color565: u16,
}

impl TransformedV6 {
    pub fn rows(self) -> [u128; TRANSFORMED_V6_ROWS] {
        assert!(self.uv12.iter().all(|&uv| uv <= 0xfff));
        let mut bytes = [0_u8; 27];
        for (axis, value) in self.clip.into_iter().enumerate() {
            bytes[axis * 4..axis * 4 + 4].copy_from_slice(&(value.raw() as i32).to_le_bytes());
        }
        for (axis, value) in self.normal.into_iter().enumerate() {
            bytes[16 + axis * 2..18 + axis * 2]
                .copy_from_slice(&(value.raw() as i16).to_le_bytes());
        }
        let uv = u32::from(self.uv12[0]) | (u32::from(self.uv12[1]) << 12);
        bytes[22..25].copy_from_slice(&uv.to_le_bytes()[..3]);
        bytes[25..27].copy_from_slice(&self.color565.to_le_bytes());
        std::array::from_fn(|row| {
            bytes[row * 9..row * 9 + 9]
                .iter()
                .enumerate()
                .fold(0_u128, |word, (byte, &value)| {
                    word | (u128::from(value) << (byte * 8))
                })
        })
    }

    pub fn from_rows(rows: [u128; TRANSFORMED_V6_ROWS]) -> Self {
        let mut bytes = [0_u8; 27];
        for row in 0..3 {
            assert!(rows[row] < (1_u128 << 72));
            for byte in 0..9 {
                bytes[row * 9 + byte] = (rows[row] >> (byte * 8)) as u8;
            }
        }
        let uv = u32::from_le_bytes([bytes[22], bytes[23], bytes[24], 0]);
        Self {
            clip: std::array::from_fn(|axis| {
                Q16::from_raw(i128::from(i32::from_le_bytes(
                    bytes[axis * 4..axis * 4 + 4].try_into().unwrap(),
                )))
                .unwrap()
            }),
            normal: std::array::from_fn(|axis| {
                Q14::from_raw(i128::from(i16::from_le_bytes(
                    bytes[16 + axis * 2..18 + axis * 2].try_into().unwrap(),
                )))
                .unwrap()
            }),
            uv12: [(uv & 0xfff) as u16, ((uv >> 12) & 0xfff) as u16],
            color565: u16::from_le_bytes(bytes[25..27].try_into().unwrap()),
        }
    }
}

fn remap_words(input: &[u128], input_bits: usize, output_bits: usize) -> Vec<u128> {
    assert!(input_bits <= 128 && output_bits <= 128);
    assert_eq!(input.len() * input_bits, TRANSFORMED_V6_BITS);
    assert_eq!(TRANSFORMED_V6_BITS % output_bits, 0);
    let mut output = vec![0_u128; TRANSFORMED_V6_BITS / output_bits];
    for bit in 0..TRANSFORMED_V6_BITS {
        let value = (input[bit / input_bits] >> (bit % input_bits)) & 1;
        output[bit / output_bits] |= value << (bit % output_bits);
    }
    output
}

pub fn write_transformed_vertex(
    store: &mut PortedStorage,
    layout: TransformedLayout,
    bank: usize,
    id: usize,
    vertex: TransformedV6,
) -> Result<u64, StorageFault> {
    if store.shape() != layout.shape() || id >= layout.vertices_per_bank() {
        return Err(StorageFault::Address);
    }
    let start = store.edges();
    let packed = remap_words(&vertex.rows(), 72, usize::from(layout.word_bits));
    for (part, value) in packed.into_iter().enumerate() {
        store.tick(
            &[],
            &[WritePort {
                bank,
                row: id * layout.rows_per_vertex() + part,
                value,
            }],
        )?;
    }
    Ok(store.edges() - start)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageKind {
    Ssram,
    Bsram,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageShape {
    pub kind: StorageKind,
    pub banks: usize,
    pub rows_per_bank: usize,
    pub width_bits: u8,
    /// Identical physical copies with independent read addresses and one
    /// mirrored write. One view is the normal one-read/one-write layout.
    pub read_views: usize,
}

impl StorageShape {
    pub fn validate(self) {
        assert!(self.banks > 0);
        assert!(self.rows_per_bank > 0);
        assert!((1..=128).contains(&self.width_bits));
        assert!(self.read_views > 0);
    }

    /// Counts physical primitives, not LUT address selection or routing.
    /// RAM16SDP4 is 16x4. For BSRAM select the smaller of 512x36 and
    /// 1024x18 packing; the caller must still account for page multiplexers.
    pub fn primitive_count(self) -> usize {
        self.validate();
        let width = usize::from(self.width_bits);
        let per_view = match self.kind {
            StorageKind::Ssram => width.div_ceil(4) * self.rows_per_bank.div_ceil(16),
            StorageKind::Bsram => (width.div_ceil(36) * self.rows_per_bank.div_ceil(512))
                .min(width.div_ceil(18) * self.rows_per_bank.div_ceil(1024)),
        };
        self.banks * self.read_views * per_view
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadPort {
    pub bank: usize,
    pub view: usize,
    pub row: usize,
    pub tag: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WritePort {
    pub bank: usize,
    pub row: usize,
    pub value: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadValue {
    pub bank: usize,
    pub view: usize,
    pub row: usize,
    pub tag: u32,
    pub value: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageFault {
    Address,
    Width,
    ReadPortBusy,
    WritePortBusy,
    ReadWriteCollision,
    Uninitialized,
    UnexpectedResponse,
}

#[derive(Clone, Debug)]
pub struct PortedStorage {
    shape: StorageShape,
    /// `views[bank][view][row]`; writes are sent to every view.
    views: Vec<Vec<Vec<Option<u128>>>>,
    pending: Vec<ReadValue>,
    edges: u64,
}

impl PortedStorage {
    pub fn new(shape: StorageShape) -> Self {
        shape.validate();
        Self {
            shape,
            views: vec![vec![vec![None; shape.rows_per_bank]; shape.read_views]; shape.banks],
            pending: Vec::new(),
            edges: 0,
        }
    }

    pub fn shape(&self) -> StorageShape {
        self.shape
    }

    pub fn edges(&self) -> u64 {
        self.edges
    }

    /// One read per `(bank,view)` and one write per bank on an edge. An
    /// accepted read returns on the following call. Failed requests are
    /// atomic: they consume neither the edge nor the previous response.
    pub fn tick(
        &mut self,
        reads: &[ReadPort],
        writes: &[WritePort],
    ) -> Result<Vec<ReadValue>, StorageFault> {
        let mut read_owners = HashSet::new();
        let mut write_owners = HashSet::new();
        let mask = if self.shape.width_bits == 128 {
            u128::MAX
        } else {
            (1_u128 << self.shape.width_bits) - 1
        };
        for request in reads {
            if request.bank >= self.shape.banks
                || request.view >= self.shape.read_views
                || request.row >= self.shape.rows_per_bank
            {
                return Err(StorageFault::Address);
            }
            if !read_owners.insert((request.bank, request.view)) {
                return Err(StorageFault::ReadPortBusy);
            }
        }
        for request in writes {
            if request.bank >= self.shape.banks || request.row >= self.shape.rows_per_bank {
                return Err(StorageFault::Address);
            }
            if request.value & !mask != 0 {
                return Err(StorageFault::Width);
            }
            if !write_owners.insert(request.bank) {
                return Err(StorageFault::WritePortBusy);
            }
            if reads
                .iter()
                .any(|read| read.bank == request.bank && read.row == request.row)
            {
                return Err(StorageFault::ReadWriteCollision);
            }
        }
        let mut next = Vec::with_capacity(reads.len());
        for request in reads {
            let value = self.views[request.bank][request.view][request.row]
                .ok_or(StorageFault::Uninitialized)?;
            next.push(ReadValue {
                bank: request.bank,
                view: request.view,
                row: request.row,
                tag: request.tag,
                value,
            });
        }
        for request in writes {
            for view in &mut self.views[request.bank] {
                view[request.row] = Some(request.value);
            }
        }
        self.edges += 1;
        Ok(std::mem::replace(&mut self.pending, next))
    }
}

/// A deliberately serial baseline. Source rows only enter the returned FF
/// latches through the 72-bit BSRAM read port, with the final response drained
/// on a tenth edge. Two-job scheduling may overlap these reads with arithmetic
/// but cannot read a second address from this single view in the same edge.
pub fn read_transformed_triangle(
    store: &mut PortedStorage,
    vertex_ids: [usize; 3],
) -> Result<([TransformedV6; 3], u64), StorageFault> {
    read_transformed_triangle_layout(
        store,
        TransformedLayout {
            word_bits: 72,
            banks: 1,
        },
        0,
        vertex_ids,
    )
}

pub fn read_transformed_triangle_layout(
    store: &mut PortedStorage,
    layout: TransformedLayout,
    bank: usize,
    vertex_ids: [usize; 3],
) -> Result<([TransformedV6; 3], u64), StorageFault> {
    if store.shape() != layout.shape()
        || bank >= layout.banks
        || vertex_ids
            .iter()
            .any(|&id| id >= layout.vertices_per_bank())
    {
        return Err(StorageFault::Address);
    }
    let start = store.edges();
    let rows_per_vertex = layout.rows_per_vertex();
    let total = 3 * rows_per_vertex;
    let mut words = vec![vec![0_u128; rows_per_vertex]; 3];
    for step in 0..=total {
        let request = (step < total).then(|| ReadPort {
            bank,
            view: 0,
            row: vertex_ids[step / rows_per_vertex] * rows_per_vertex + step % rows_per_vertex,
            tag: step as u32,
        });
        let ready = store.tick(request.as_slice(), &[])?;
        if step == 0 {
            if !ready.is_empty() {
                return Err(StorageFault::UnexpectedResponse);
            }
        } else {
            let [value] = ready.as_slice() else {
                return Err(StorageFault::UnexpectedResponse);
            };
            let index = step - 1;
            if value.tag != index as u32
                || value.bank != bank
                || value.row
                    != vertex_ids[index / rows_per_vertex] * rows_per_vertex
                        + index % rows_per_vertex
            {
                return Err(StorageFault::UnexpectedResponse);
            }
            words[index / rows_per_vertex][index % rows_per_vertex] = value.value;
        }
    }
    let mut vertices = Vec::with_capacity(3);
    for vertex in words {
        let rows = remap_words(&vertex, usize::from(layout.word_bits), 72);
        vertices.push(TransformedV6::from_rows(rows.try_into().unwrap()));
    }
    Ok((vertices.try_into().unwrap(), store.edges() - start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_banks_and_mirrored_reads_are_clocked() {
        let shape = StorageShape {
            kind: StorageKind::Ssram,
            banks: 2,
            rows_per_bank: 16,
            width_bits: 36,
            read_views: 2,
        };
        assert_eq!(shape.primitive_count(), 36);
        let mut ram = PortedStorage::new(shape);
        assert!(ram
            .tick(
                &[],
                &[
                    WritePort {
                        bank: 0,
                        row: 3,
                        value: 0x12345,
                    },
                    WritePort {
                        bank: 1,
                        row: 3,
                        value: 0x98765,
                    },
                ]
            )
            .unwrap()
            .is_empty());
        assert!(ram
            .tick(
                &[
                    ReadPort {
                        bank: 0,
                        view: 0,
                        row: 3,
                        tag: 11,
                    },
                    ReadPort {
                        bank: 0,
                        view: 1,
                        row: 3,
                        tag: 12,
                    },
                    ReadPort {
                        bank: 1,
                        view: 0,
                        row: 3,
                        tag: 13,
                    },
                ],
                &[]
            )
            .unwrap()
            .is_empty());
        let ready = ram.tick(&[], &[]).unwrap();
        assert_eq!(
            ready.iter().map(|read| read.value).collect::<Vec<_>>(),
            [0x12345, 0x12345, 0x98765]
        );
        assert_eq!(
            ready.iter().map(|read| read.tag).collect::<Vec<_>>(),
            [11, 12, 13]
        );
    }

    #[test]
    fn illegal_access_does_not_consume_a_pending_read() {
        let mut ram = PortedStorage::new(StorageShape {
            kind: StorageKind::Bsram,
            banks: 1,
            rows_per_bank: 512,
            width_bits: 72,
            read_views: 1,
        });
        assert_eq!(ram.shape().primitive_count(), 2);
        ram.tick(
            &[],
            &[WritePort {
                bank: 0,
                row: 7,
                value: 42,
            }],
        )
        .unwrap();
        let read = ReadPort {
            bank: 0,
            view: 0,
            row: 7,
            tag: 4,
        };
        ram.tick(&[read], &[]).unwrap();
        let edge = ram.edges();
        assert_eq!(
            ram.tick(&[read, read], &[]),
            Err(StorageFault::ReadPortBusy)
        );
        assert_eq!(
            ram.tick(
                &[read],
                &[WritePort {
                    bank: 0,
                    row: 7,
                    value: 43
                }]
            ),
            Err(StorageFault::ReadWriteCollision)
        );
        assert_eq!(ram.edges(), edge);
        assert_eq!(ram.tick(&[], &[]).unwrap()[0].value, 42);
    }

    #[test]
    fn uninitialized_read_and_width_check_fail_explicitly() {
        let mut ram = PortedStorage::new(StorageShape {
            kind: StorageKind::Ssram,
            banks: 1,
            rows_per_bank: 16,
            width_bits: 18,
            read_views: 1,
        });
        assert_eq!(
            ram.tick(
                &[ReadPort {
                    bank: 0,
                    view: 0,
                    row: 0,
                    tag: 0
                }],
                &[]
            ),
            Err(StorageFault::Uninitialized)
        );
        assert_eq!(
            ram.tick(
                &[],
                &[WritePort {
                    bank: 0,
                    row: 0,
                    value: 1 << 18
                }]
            ),
            Err(StorageFault::Width)
        );
        assert_eq!(ram.edges(), 0);
    }

    #[test]
    fn transformed_v6_triangle_enters_only_through_bsram_port() {
        let sample = |id: i128| TransformedV6 {
            clip: [
                Q16::from_raw(-id).unwrap(),
                Q16::from_raw(id * 17).unwrap(),
                Q16::from_raw(0x1234_5678 + id).unwrap(),
                Q16::from_raw(65_536).unwrap(),
            ],
            normal: [
                Q14::from_raw(-8_000 + id).unwrap(),
                Q14::from_raw(99 + id).unwrap(),
                Q14::from_raw(16_384 - id).unwrap(),
            ],
            uv12: [0xabc, 0x123 + id as u16],
            color565: 0xf800 + id as u16,
        };
        let expected = [sample(1), sample(2), sample(3)];
        let mut store = PortedStorage::new(StorageShape {
            kind: StorageKind::Bsram,
            banks: 1,
            rows_per_bank: 512,
            width_bits: 72,
            read_views: 1,
        });
        for (id, vertex) in expected.iter().enumerate() {
            for (row, value) in vertex.rows().into_iter().enumerate() {
                store
                    .tick(
                        &[],
                        &[WritePort {
                            bank: 0,
                            row: id * 3 + row,
                            value,
                        }],
                    )
                    .unwrap();
            }
        }
        let (captured, edges) = read_transformed_triangle(&mut store, [2, 0, 1]).unwrap();
        assert_eq!(captured, [expected[2], expected[0], expected[1]]);
        assert_eq!(edges, 10);
        assert_eq!(store.shape().primitive_count(), 2);
    }

    #[test]
    fn transformed_widths_trade_reads_for_blocks_without_losing_bits() {
        let expected = [1_i128, 2, 3].map(|id| TransformedV6 {
            clip: [
                Q16::from_raw(id).unwrap(),
                Q16::from_raw(-id * 20_000).unwrap(),
                Q16::from_raw(id * 30_000).unwrap(),
                Q16::from_raw(id * 65_536).unwrap(),
            ],
            normal: [
                Q14::from_raw(-123 * id).unwrap(),
                Q14::from_raw(2_000 * id).unwrap(),
                Q14::from_raw(8_000).unwrap(),
            ],
            uv12: [0x123 * id as u16, 0xfff - id as u16],
            color565: 0xa55a ^ id as u16,
        });
        for (width, rows, blocks, capacity) in [(36, 6, 1, 85), (72, 3, 2, 170), (108, 2, 3, 256)] {
            let layout = TransformedLayout {
                word_bits: width,
                banks: 1,
            };
            assert_eq!(
                (
                    layout.rows_per_vertex(),
                    layout.shape().primitive_count(),
                    layout.vertices_per_bank()
                ),
                (rows, blocks, capacity)
            );
            let mut store = PortedStorage::new(layout.shape());
            for (id, vertex) in expected.into_iter().enumerate() {
                assert_eq!(
                    write_transformed_vertex(&mut store, layout, 0, id, vertex).unwrap(),
                    rows as u64
                );
            }
            let (actual, edges) =
                read_transformed_triangle_layout(&mut store, layout, 0, [2, 1, 0]).unwrap();
            assert_eq!(actual, [expected[2], expected[1], expected[0]]);
            assert_eq!(edges, (rows * 3 + 1) as u64);
        }
        let pingpong = TransformedLayout {
            word_bits: 36,
            banks: 2,
        };
        assert_eq!(pingpong.shape().primitive_count(), 2);
        assert!(pingpong.vertices_per_bank() >= 64);
    }
}
