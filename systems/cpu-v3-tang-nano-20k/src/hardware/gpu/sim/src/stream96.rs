//! Provisional 96-bit compact vertex stream experiment.
//!
//! GV2M v6 vertices are 32-bit-cell aligned over the existing 64-bit synchronous
//! scratchpad port. It deliberately does not replace the current v4 meshlet
//! format or freeze a driver ABI. Vertex IDs are implicit in stream order.

use crate::fixed::{round_shift_ties_even, NumericFault, Q14};
use crate::timing::{RamRead, SyncRam64};

const VERTEX_TAG: u32 = 0;
const TRIANGLE_TAG: u32 = 1;
const END_TAG: u32 = 2;
const POSITION_MASK: u32 = 0x3ff;
const NORMAL_MASK: u32 = 0xff;
const UV_MASK: u32 = 0xfff;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Record {
    Vertex {
        xyz10: [u16; 3],
        normal: [Q14; 3],
        uv12: [u16; 2],
        color565: u16,
    },
    Triangle([u8; 3]),
    End,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamFault {
    Numeric(NumericFault),
    BadTag,
    ReservedBits,
    ForwardReference,
    Truncated,
    TrailingData,
}

impl From<NumericFault> for StreamFault {
    fn from(value: NumericFault) -> Self {
        Self::Numeric(value)
    }
}

fn normal8(normal: Q14) -> Result<u32, StreamFault> {
    if !(-16384..=16384).contains(&normal.raw()) {
        return Err(StreamFault::Numeric(NumericFault::OutOfRange));
    }
    let raw = round_shift_ties_even(i128::from(normal.raw()), 7)?.clamp(-128, 127);
    Ok((raw as i32 as u32) & NORMAL_MASK)
}

fn decode_normal(bits: u32) -> Q14 {
    let signed = ((bits as i32) << 24) >> 24;
    Q14::from_raw(i128::from(signed) << 7).expect("8-bit normal fits Q2.14")
}

/// 2-bit tag, XYZ10, signed normal3x8, UV12x2, RGB565; exactly 96 bits.
pub fn encode_record(record: &Record) -> Result<Vec<u32>, StreamFault> {
    match record {
        Record::Vertex {
            xyz10,
            normal,
            uv12,
            color565,
        } => {
            if xyz10.iter().any(|x| u32::from(*x) > POSITION_MASK)
                || uv12.iter().any(|x| u32::from(*x) > UV_MASK)
            {
                return Err(StreamFault::Numeric(NumericFault::OutOfRange));
            }
            let n = [
                normal8(normal[0])?,
                normal8(normal[1])?,
                normal8(normal[2])?,
            ];
            Ok(vec![
                VERTEX_TAG
                    | (u32::from(xyz10[0]) << 2)
                    | (u32::from(xyz10[1]) << 12)
                    | (u32::from(xyz10[2]) << 22),
                n[0] | (n[1] << 8) | (n[2] << 16) | ((u32::from(uv12[0]) & 0xff) << 24),
                (u32::from(uv12[0]) >> 8)
                    | (u32::from(uv12[1]) << 4)
                    | (u32::from(*color565) << 16),
            ])
        }
        Record::Triangle(refs) => {
            if refs.iter().any(|x| *x >= 64) {
                return Err(StreamFault::Numeric(NumericFault::OutOfRange));
            }
            Ok(vec![
                TRIANGLE_TAG
                    | (u32::from(refs[0]) << 2)
                    | (u32::from(refs[1]) << 8)
                    | (u32::from(refs[2]) << 14),
            ])
        }
        Record::End => Ok(vec![END_TAG]),
    }
}

/// The packed stream is padded to a full 64-bit beat only after END.
pub fn encode_stream(records: &[Record]) -> Result<Vec<u64>, StreamFault> {
    if !matches!(records.last(), Some(Record::End))
        || records[..records.len() - 1]
            .iter()
            .any(|record| matches!(record, Record::End))
    {
        return Err(StreamFault::TrailingData);
    }
    let mut cells = Vec::new();
    for record in records {
        cells.extend(encode_record(record)?);
    }
    if cells.len() % 2 != 0 {
        cells.push(0);
    }
    Ok(cells
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u64::from(pair[0]) | (u64::from(pair[1]) << 32))
        .collect())
}

/// One cell retires per edge. Every second edge reads a new BSRAM word; the
/// intervening edge consumes its buffered upper half without a port request.
pub struct CellReader {
    ram: SyncRam64,
    word_count: usize,
    next_word: usize,
    buffered_high: Option<u32>,
    edges: usize,
    reads: usize,
}

impl CellReader {
    pub fn new(words: Vec<u64>) -> Self {
        Self {
            word_count: words.len(),
            ram: SyncRam64::new(words),
            next_word: 0,
            buffered_high: None,
            edges: 0,
            reads: 0,
        }
    }

    pub fn tick(&mut self) -> Result<u32, StreamFault> {
        self.edges += 1;
        if let Some(high) = self.buffered_high.take() {
            self.ram.tick(None, None);
            return Ok(high);
        }
        if self.next_word >= self.word_count {
            return Err(StreamFault::Truncated);
        }
        let data = self.ram.tick(Some(self.next_word), None);
        self.next_word += 1;
        self.reads += 1;
        match data {
            RamRead::Data(word) => {
                self.buffered_high = Some((word >> 32) as u32);
                Ok(word as u32)
            }
            RamRead::Collision => unreachable!("read-only prototype"),
        }
    }

    pub fn edges(&self) -> usize {
        self.edges
    }

    pub fn reads(&self) -> usize {
        self.reads
    }
}

/// Parse at most `maximum_cells` clocked cells, checking triangle publication
/// against the number of already decoded vertices.
pub fn decode_stream(
    reader: &mut CellReader,
    maximum_cells: usize,
) -> Result<Vec<Record>, StreamFault> {
    let mut records = Vec::new();
    let mut used = 0;
    let mut vertices = 0;
    while used < maximum_cells {
        let first = reader.tick()?;
        used += 1;
        match first & 3 {
            VERTEX_TAG => {
                if used + 2 > maximum_cells || vertices >= 64 {
                    return Err(StreamFault::Truncated);
                }
                let second = reader.tick()?;
                let third = reader.tick()?;
                used += 2;
                let xyz10 = [
                    ((first >> 2) & POSITION_MASK) as u16,
                    ((first >> 12) & POSITION_MASK) as u16,
                    ((first >> 22) & POSITION_MASK) as u16,
                ];
                let normal = [
                    decode_normal(second & NORMAL_MASK),
                    decode_normal((second >> 8) & NORMAL_MASK),
                    decode_normal((second >> 16) & NORMAL_MASK),
                ];
                records.push(Record::Vertex {
                    xyz10,
                    normal,
                    uv12: [
                        ((second >> 24) | ((third & 15) << 8)) as u16,
                        ((third >> 4) & UV_MASK) as u16,
                    ],
                    color565: (third >> 16) as u16,
                });
                vertices += 1;
            }
            TRIANGLE_TAG => {
                if first >> 20 != 0 {
                    return Err(StreamFault::ReservedBits);
                }
                let refs = [
                    ((first >> 2) & 63) as u8,
                    ((first >> 8) & 63) as u8,
                    ((first >> 14) & 63) as u8,
                ];
                if refs.iter().any(|x| usize::from(*x) >= vertices) {
                    return Err(StreamFault::ForwardReference);
                }
                records.push(Record::Triangle(refs));
            }
            END_TAG => {
                if first != END_TAG {
                    return Err(StreamFault::ReservedBits);
                }
                if used != maximum_cells {
                    return Err(StreamFault::TrailingData);
                }
                records.push(Record::End);
                return Ok(records);
            }
            _ => return Err(StreamFault::BadTag),
        }
    }
    Err(StreamFault::Truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_cells_match_the_offline_packer_golden() {
        let record = Record::Vertex {
            xyz10: [1, 2, 3],
            normal: [
                Q14::from_raw(-16384).unwrap(),
                Q14::from_raw(0).unwrap(),
                Q14::from_raw(16384).unwrap(),
            ],
            uv12: [0xabc, 0x123],
            color565: 0xf81f,
        };
        assert_eq!(
            encode_record(&record).unwrap(),
            [0x00c0_2004, 0xbc7f_0080, 0xf81f_123a]
        );
    }

    #[test]
    fn interleaved_records_cross_64_bit_boundaries_without_padding() {
        let vertex = |x| Record::Vertex {
            xyz10: [x, 1023 - x, x + 7],
            normal: [
                Q14::from_raw(-16384).unwrap(),
                Q14::from_raw(0).unwrap(),
                Q14::from_raw(16384).unwrap(),
            ],
            uv12: [0xaaa, 0x555],
            color565: 0xa55a,
        };
        let stream = [
            vertex(1),
            vertex(2),
            vertex(3),
            Record::Triangle([0, 1, 2]),
            Record::End,
        ];
        let words = encode_stream(&stream).unwrap();
        assert_eq!(words.len(), 6); // 11 useful cells, one final pad cell.
        let mut reader = CellReader::new(words);
        let parsed = decode_stream(&mut reader, 11).unwrap();
        assert_eq!(reader.edges(), 11);
        assert_eq!(reader.reads(), 6);
        assert_eq!(parsed.len(), 5);
        assert_eq!(parsed[3], Record::Triangle([0, 1, 2]));
        if let Record::Vertex { normal, .. } = &parsed[0] {
            assert_eq!(normal[0].raw(), -16384);
            assert_eq!(normal[2].raw(), 16256); // Q1.7 +1 saturation is explicit.
        } else {
            panic!("first record should be a vertex");
        }
    }

    #[test]
    fn malformed_forward_reference_stops_before_end() {
        let words = encode_stream(&[Record::Triangle([0, 0, 0]), Record::End]).unwrap();
        let mut reader = CellReader::new(words);
        assert_eq!(
            decode_stream(&mut reader, 2),
            Err(StreamFault::ForwardReference)
        );
        assert_eq!(reader.edges(), 1);
    }

    #[test]
    fn signed_half_ties_and_invalid_tags_are_checked_at_format_boundary() {
        let vertex = Record::Vertex {
            xyz10: [0, 0, 0],
            normal: [
                Q14::from_raw(64).unwrap(),
                Q14::from_raw(192).unwrap(),
                Q14::from_raw(-192).unwrap(),
            ],
            uv12: [4095, 0x123],
            color565: 0xf81f,
        };
        let words = encode_stream(&[vertex, Record::End]).unwrap();
        let mut reader = CellReader::new(words.clone());
        let decoded = decode_stream(&mut reader, 4).unwrap();
        match &decoded[0] {
            Record::Vertex {
                normal,
                uv12,
                color565,
                ..
            } => {
                assert_eq!(normal.map(|value| value.raw()), [0, 256, -256]);
                assert_eq!(*uv12, [4095, 0x123]);
                assert_eq!(*color565, 0xf81f);
            }
            _ => panic!("first record should be a vertex"),
        }
        let mut damaged = words;
        damaged[1] |= 1_u64 << 32;
        let mut reader = CellReader::new(damaged);
        assert_eq!(decode_stream(&mut reader, 4), Err(StreamFault::BadTag));
        assert_eq!(reader.edges(), 4);
        assert_eq!(
            normal8(Q14::from_raw(16385).unwrap()),
            Err(StreamFault::Numeric(NumericFault::OutOfRange))
        );
    }
}
