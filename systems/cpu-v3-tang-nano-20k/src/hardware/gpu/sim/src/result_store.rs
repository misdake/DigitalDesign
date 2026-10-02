//! A separate transformed store, two 512x36 BSRAMs in parallel (72-bit rows).
//! Three 72-bit rows hold one 27-byte vertex. The setup queue only receives
//! references after all three rows have been committed.

use crate::fixed::{Q14, Q16};

pub const RESULT_ROWS: usize = 512;
pub const ROWS_PER_VERTEX: usize = 3;
const MASK36: u128 = (1_u128 << 36) - 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransformedVertex {
    pub clip: [Q16; 4],
    pub normal: [Q14; 3],
    pub rgba: [u8; 4],
}

impl TransformedVertex {
    pub fn rows(self) -> [u128; ROWS_PER_VERTEX] {
        let mut bytes = [0_u8; 27];
        for column in 0..4 {
            bytes[column * 4..column * 4 + 4]
                .copy_from_slice(&(self.clip[column].raw() as i32).to_le_bytes());
        }
        for column in 0..3 {
            bytes[16 + column * 2..18 + column * 2]
                .copy_from_slice(&(self.normal[column].raw() as i16).to_le_bytes());
        }
        bytes[22..26].copy_from_slice(&self.rgba);
        std::array::from_fn(|row| {
            bytes[row * 9..row * 9 + 9]
                .iter()
                .enumerate()
                .fold(0_u128, |word, (byte, value)| {
                    word | (u128::from(*value) << (byte * 8))
                })
        })
    }

    pub fn from_rows(rows: [u128; ROWS_PER_VERTEX]) -> Self {
        let mut bytes = [0_u8; 27];
        for row in 0..3 {
            for byte in 0..9 {
                bytes[row * 9 + byte] = (rows[row] >> (byte * 8)) as u8;
            }
        }
        Self {
            clip: std::array::from_fn(|column| {
                Q16::from_raw(i128::from(i32::from_le_bytes(
                    bytes[column * 4..column * 4 + 4].try_into().unwrap(),
                )))
                .unwrap()
            }),
            normal: std::array::from_fn(|column| {
                Q14::from_raw(i128::from(i16::from_le_bytes(
                    bytes[16 + column * 2..18 + column * 2].try_into().unwrap(),
                )))
                .unwrap()
            }),
            rgba: bytes[22..26].try_into().unwrap(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultRead {
    Data(u128),
    Collision,
}

#[derive(Clone, Debug)]
pub struct ResultStore {
    lo: [u64; RESULT_ROWS],
    hi: [u64; RESULT_ROWS],
    output: ResultRead,
    valid: [bool; RESULT_ROWS / ROWS_PER_VERTEX],
}

impl ResultStore {
    pub fn new(fill: u128) -> Self {
        assert!(fill < (1_u128 << 72));
        Self {
            lo: [(fill & MASK36) as u64; RESULT_ROWS],
            hi: [(fill >> 36) as u64; RESULT_ROWS],
            output: ResultRead::Data(0),
            valid: [false; RESULT_ROWS / ROWS_PER_VERTEX],
        }
    }

    pub fn output(&self) -> ResultRead {
        self.output
    }

    pub fn inspect_row(&self, row: usize) -> u128 {
        assert!(row < RESULT_ROWS);
        u128::from(self.lo[row]) | (u128::from(self.hi[row]) << 36)
    }

    pub fn vertex(&self, id: usize) -> Option<TransformedVertex> {
        if id >= self.valid.len() || !self.valid[id] {
            return None;
        }
        Some(TransformedVertex::from_rows(std::array::from_fn(|row| {
            self.inspect_row(id * 3 + row)
        })))
    }

    pub fn publish(&mut self, id: usize) {
        assert!(id < self.valid.len());
        assert!(!self.valid[id], "overwriting an owned transformed slot");
        self.valid[id] = true;
    }

    pub fn tick(&mut self, read: Option<usize>, write: Option<(usize, u128)>) {
        if let Some(row) = read {
            assert!(row < RESULT_ROWS);
            self.output = if write.is_some_and(|(other, _)| other == row) {
                ResultRead::Collision
            } else {
                ResultRead::Data(self.inspect_row(row))
            };
        }
        if let Some((row, value)) = write {
            assert!(row < RESULT_ROWS);
            assert!(value < (1_u128 << 72));
            self.lo[row] = (value & MASK36) as u64;
            self.hi[row] = (value >> 36) as u64;
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TriangleRef {
    pub vertices: [u8; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_packing_preserves_signed_values_and_guard() {
        let value = TransformedVertex {
            clip: [
                Q16::from_raw(-1).unwrap(),
                Q16::from_raw(i32::MIN.into()).unwrap(),
                Q16::from_raw(0x12345678).unwrap(),
                Q16::from_raw(0).unwrap(),
            ],
            normal: [
                Q14::from_raw(i16::MIN.into()).unwrap(),
                Q14::from_raw(-7).unwrap(),
                Q14::from_raw(i16::MAX.into()).unwrap(),
            ],
            rgba: [1, 127, 254, 255],
        };
        let guard = 0x5a5a5a5a5a5a5a5a5a_u128;
        let mut store = ResultStore::new(guard);
        for (row, data) in value.rows().into_iter().enumerate() {
            store.tick(None, Some((row, data)));
        }
        assert_eq!(store.vertex(0), None);
        store.publish(0);
        assert_eq!(store.vertex(0), Some(value));
        assert_eq!(store.inspect_row(3), guard);
    }
}
