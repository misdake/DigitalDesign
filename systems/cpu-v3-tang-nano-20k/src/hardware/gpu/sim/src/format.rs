//! Temporary 8-byte record headers for the first microkernel experiment.
//! Temporary expanded and compact vertex wire formats for the GPU v2 cmodel.

use crate::fixed::{NumericFault, Q14, Q16};

pub const UNIFORM_BYTES: usize = 128;
pub const VERTEX_BYTES: usize = 48;
pub const STREAM_BYTES: usize = 3 * VERTEX_BYTES + 16;
pub const VERTEX_PAYLOAD_BYTES: u16 = 40;
pub const COMPACT_PAYLOAD_BYTES: u16 = 8;
const NORMAL_ROM: &[u8; 2560] = include_bytes!("../../assets/normal_rom.bin");

fn normal_rom_word(id: usize) -> u64 {
    let mut word = [0_u8; 8];
    word[..5].copy_from_slice(&NORMAL_ROM[id * 5..id * 5 + 5]);
    u64::from_le_bytes(word)
}

pub fn normal_rom_words() -> Vec<u64> {
    (0..512).map(normal_rom_word).collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompactGrid {
    pub origin: [i32; 3],
    pub base_step: u32,
    pub meshlet_base: [i32; 3],
    pub level: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeshletBounds {
    pub min: [Q16; 3],
    pub max: [Q16; 3],
}

impl MeshletBounds {
    pub const BYTES: usize = 24;

    pub fn decode(words: [u64; 3]) -> Result<Self, FormatError> {
        let mut min = [Q16::from_raw(0).unwrap(); 3];
        let mut max = min;
        for axis in 0..3 {
            let pair = words[axis].to_le_bytes();
            let lo = i32::from_le_bytes(pair[..4].try_into().unwrap());
            let hi = i32::from_le_bytes(pair[4..].try_into().unwrap());
            if lo > hi {
                return Err(FormatError::InvalidRecord);
            }
            min[axis] = Q16::from_raw(i128::from(lo))?;
            max[axis] = Q16::from_raw(i128::from(hi))?;
        }
        Ok(Self { min, max })
    }

    pub fn contains(self, position: [Q16; 4]) -> bool {
        (0..3).all(|axis| self.min[axis] <= position[axis] && position[axis] <= self.max[axis])
    }
}

impl CompactGrid {
    pub fn valid(self) -> bool {
        self.base_step.is_power_of_two()
            && self.level <= 24
            && self
                .meshlet_base
                .iter()
                .all(|&base| i64::from(base).rem_euclid(1_i64 << self.level) == 0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatError {
    InvalidHeader,
    InvalidLength,
    InvalidRecord,
    Numeric(NumericFault),
}

impl From<NumericFault> for FormatError {
    fn from(value: NumericFault) -> Self {
        Self::Numeric(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Uniform {
    pub mvp: [[Q16; 4]; 4],
    /// Trial 3x3 Q2.14 normal transform; no normalization in this experiment.
    pub normal: [[Q14; 3]; 3],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputVertex {
    pub position: [Q16; 4],
    pub normal: [Q14; 3],
    pub rgba: [u8; 4],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Header {
    Vertex { id: u8 },
    CompactVertex { id: u8 },
    Triangle { refs: [u8; 3] },
    End,
}

impl Header {
    pub fn decode(word: u64) -> Result<Self, FormatError> {
        let bytes = word.to_le_bytes();
        let length = u16::from_le_bytes([bytes[2], bytes[3]]);
        match bytes[0] {
            1 if length == VERTEX_PAYLOAD_BYTES && bytes[1] < 64 && bytes[4..] == [0; 4] => {
                Ok(Self::Vertex { id: bytes[1] })
            }
            4 if length == COMPACT_PAYLOAD_BYTES && bytes[1] < 64 && bytes[4..] == [0; 4] => {
                Ok(Self::CompactVertex { id: bytes[1] })
            }
            2 if length == 0 && bytes[1] == 0 && bytes[7] == 0 => Ok(Self::Triangle {
                refs: [bytes[4], bytes[5], bytes[6]],
            }),
            3 if bytes[1..] == [0; 7] => Ok(Self::End),
            1..=4 => Err(FormatError::InvalidLength),
            _ => Err(FormatError::InvalidHeader),
        }
    }

    pub fn encode(self) -> u64 {
        let mut bytes = [0_u8; 8];
        match self {
            Self::Vertex { id } => {
                bytes[0] = 1;
                bytes[1] = id;
                bytes[2..4].copy_from_slice(&VERTEX_PAYLOAD_BYTES.to_le_bytes());
            }
            Self::CompactVertex { id } => {
                bytes[0] = 4;
                bytes[1] = id;
                bytes[2..4].copy_from_slice(&COMPACT_PAYLOAD_BYTES.to_le_bytes());
            }
            Self::Triangle { refs } => {
                bytes[0] = 2;
                bytes[4..7].copy_from_slice(&refs);
            }
            Self::End => bytes[0] = 3,
        }
        u64::from_le_bytes(bytes)
    }
}

impl Uniform {
    pub fn encode(self) -> [u8; UNIFORM_BYTES] {
        let mut bytes = [0; UNIFORM_BYTES];
        for row in 0..4 {
            for column in 0..4 {
                let offset = (row * 4 + column) * 4;
                bytes[offset..offset + 4]
                    .copy_from_slice(&(self.mvp[row][column].raw() as i32).to_le_bytes());
            }
        }
        for row in 0..3 {
            for column in 0..3 {
                let offset = 64 + (row * 3 + column) * 2;
                bytes[offset..offset + 2]
                    .copy_from_slice(&(self.normal[row][column].raw() as i16).to_le_bytes());
            }
        }
        bytes
    }

    pub fn decode(words: [u64; 16]) -> Result<Self, FormatError> {
        let bytes = words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        Ok(Self {
            mvp: std::array::from_fn(|row| {
                std::array::from_fn(|column| {
                    let offset = (row * 4 + column) * 4;
                    Q16::from_raw(i128::from(i32::from_le_bytes(
                        bytes[offset..offset + 4].try_into().unwrap(),
                    )))
                    .unwrap()
                })
            }),
            normal: std::array::from_fn(|row| {
                std::array::from_fn(|column| {
                    let offset = 64 + (row * 3 + column) * 2;
                    Q14::from_raw(i128::from(i16::from_le_bytes(
                        bytes[offset..offset + 2].try_into().unwrap(),
                    )))
                    .unwrap()
                })
            }),
        })
    }

    /// Read only the three normal-matrix words into local staging. MVP stays
    /// in scratchpad and is streamed through the wide DSP per vertex.
    pub fn decode_normal_words(words: [u64; 3]) -> Result<[[Q14; 3]; 3], FormatError> {
        let bytes = words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        Ok(std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                let offset = (row * 3 + column) * 2;
                Q14::from_raw(i128::from(i16::from_le_bytes(
                    bytes[offset..offset + 2].try_into().unwrap(),
                )))
                .unwrap()
            })
        }))
    }
}

impl InputVertex {
    pub fn decode_compact(word: u64, grid: CompactGrid) -> Result<Self, FormatError> {
        let direction_id = ((word >> 30) & 511) as usize;
        Self::decode_compact_with_normal_word(word, grid, normal_rom_word(direction_id))
    }

    pub fn decode_compact_with_normal_word(
        word: u64,
        grid: CompactGrid,
        magnitude_word: u64,
    ) -> Result<Self, FormatError> {
        if !grid.valid() {
            return Err(FormatError::InvalidRecord);
        }
        let mut position = [Q16::from_raw(0).unwrap(); 4];
        for (axis, component) in position.iter_mut().enumerate().take(3) {
            let delta = i128::from((word >> (axis * 10)) & 1023);
            let canonical = i128::from(grid.meshlet_base[axis]) + (delta << grid.level);
            let raw = i128::from(grid.origin[axis]) + canonical * i128::from(grid.base_step);
            *component = Q16::from_raw(raw).map_err(FormatError::Numeric)?;
        }
        position[3] = Q16::from_raw(1 << 16).unwrap();
        let normal_code = (word >> 30) & 4095;
        let mut normal = [Q14::from_raw(0).unwrap(); 3];
        for (axis, component) in normal.iter_mut().enumerate() {
            let magnitude = i128::from(((magnitude_word >> (axis * 12)) & 4095) << 2);
            let signed = if normal_code & (1 << (9 + axis)) != 0 {
                -magnitude
            } else {
                magnitude
            };
            *component = Q14::from_raw(signed).map_err(FormatError::Numeric)?;
        }
        let attributes = (word >> 42) as u32;
        let rgb565 = attributes as u16;
        let alpha6 = (attributes >> 16) as u8;
        let red5 = ((rgb565 >> 11) & 31) as u8;
        let green6 = ((rgb565 >> 5) & 63) as u8;
        let blue5 = (rgb565 & 31) as u8;
        let rgba = [
            (red5 << 3) | (red5 >> 2),
            (green6 << 2) | (green6 >> 4),
            (blue5 << 3) | (blue5 >> 2),
            (alpha6 << 2) | (alpha6 >> 4),
        ];
        Ok(Self {
            position,
            normal,
            rgba,
        })
    }

    pub fn encode_payload(self) -> [u8; VERTEX_PAYLOAD_BYTES as usize] {
        let mut bytes = [0; VERTEX_PAYLOAD_BYTES as usize];
        for column in 0..4 {
            bytes[column * 4..column * 4 + 4]
                .copy_from_slice(&(self.position[column].raw() as i32).to_le_bytes());
        }
        for column in 0..3 {
            bytes[16 + column * 2..18 + column * 2]
                .copy_from_slice(&(self.normal[column].raw() as i16).to_le_bytes());
        }
        bytes[22..26].copy_from_slice(&self.rgba);
        bytes
    }

    pub fn decode_payload(words: [u64; 5]) -> Result<Self, FormatError> {
        let bytes = words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>();
        if bytes[26..].iter().any(|&byte| byte != 0) {
            return Err(FormatError::InvalidRecord);
        }
        Ok(Self {
            position: std::array::from_fn(|column| {
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
        })
    }
}

pub fn encode_stream(vertices: [InputVertex; 3]) -> [u8; STREAM_BYTES] {
    let mut bytes = [0; STREAM_BYTES];
    for (index, vertex) in vertices.into_iter().enumerate() {
        let offset = index * VERTEX_BYTES;
        bytes[offset..offset + 8]
            .copy_from_slice(&Header::Vertex { id: index as u8 }.encode().to_le_bytes());
        bytes[offset + 8..offset + VERTEX_BYTES].copy_from_slice(&vertex.encode_payload());
    }
    bytes[144..152].copy_from_slice(&Header::Triangle { refs: [0, 1, 2] }.encode().to_le_bytes());
    bytes[152..160].copy_from_slice(&Header::End.encode().to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_headers_reject_length_and_reserved_bits() {
        assert_eq!(
            Header::decode(Header::Triangle { refs: [0, 1, 2] }.encode()),
            Ok(Header::Triangle { refs: [0, 1, 2] })
        );
        assert_eq!(
            Header::decode(Header::End.encode() | (1 << 8)),
            Err(FormatError::InvalidLength)
        );
        assert_eq!(
            Header::decode(Header::Vertex { id: 0 }.encode() ^ (1 << 16)),
            Err(FormatError::InvalidLength)
        );
        assert_eq!(
            Header::decode(Header::CompactVertex { id: 7 }.encode()),
            Ok(Header::CompactVertex { id: 7 })
        );
        assert_eq!(
            Header::decode(Header::CompactVertex { id: 7 }.encode() | (1 << 32)),
            Err(FormatError::InvalidLength)
        );
    }

    #[test]
    fn compact_xyz10_grid_normal_rom_and_color_decode() {
        let grid = CompactGrid {
            origin: [-100, 200, 300],
            base_step: 2,
            meshlet_base: [8, 16, -24],
            level: 2,
        };
        let position30 = 3_u64 | (4 << 10) | (5 << 20);
        let normal12 = 1_u64 << 9;
        let payload = position30 | (normal12 << 30) | (0x3fffff_u64 << 42);
        let decoded = InputVertex::decode_compact(payload, grid).unwrap();
        assert_eq!(decoded.position.map(Q16::raw), [-60, 264, 292, 65_536]);
        assert_eq!(decoded.rgba, [255; 4]);
        assert!(decoded.normal[0].raw() < 0);
        assert!(decoded.normal[1].raw() > 0 && decoded.normal[2].raw() > 0);
        assert_eq!(
            InputVertex::decode_compact(
                payload,
                CompactGrid {
                    base_step: 3,
                    ..grid
                }
            ),
            Err(FormatError::InvalidRecord)
        );
    }

    #[test]
    fn uniform_and_vertex_byte_offsets_are_explicit() {
        let mut uniform = Uniform {
            mvp: [[Q16::from_raw(0).unwrap(); 4]; 4],
            normal: [[Q14::from_raw(0).unwrap(); 3]; 3],
        };
        uniform.mvp[0][0] = Q16::from_raw(0x1234_5678).unwrap();
        uniform.normal[0][0] = Q14::from_raw(-2).unwrap();
        let encoded = uniform.encode();
        assert_eq!(&encoded[0..4], &[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(&encoded[64..66], &[0xfe, 0xff]);
        let words = std::array::from_fn(|index| {
            u64::from_le_bytes(encoded[index * 8..index * 8 + 8].try_into().unwrap())
        });
        assert_eq!(Uniform::decode(words), Ok(uniform));
        assert_eq!(
            Uniform::decode_normal_words(words[8..11].try_into().unwrap()),
            Ok(uniform.normal)
        );
        let mut future_uniform = encoded;
        future_uniform[82] = 0xa5;
        future_uniform[87] = 0x5a;
        future_uniform[127] = 0x3c;
        let future_words = std::array::from_fn(|index| {
            u64::from_le_bytes(future_uniform[index * 8..index * 8 + 8].try_into().unwrap())
        });
        assert_eq!(Uniform::decode(future_words), Ok(uniform));
        assert_eq!(
            Uniform::decode_normal_words(future_words[8..11].try_into().unwrap()),
            Ok(uniform.normal)
        );

        let vertex = InputVertex {
            position: [Q16::from_raw(-2).unwrap(); 4],
            normal: [Q14::from_raw(0x1234).unwrap(); 3],
            rgba: [0x12, 0x34, 0x56, 0x78],
        };
        let payload = vertex.encode_payload();
        assert_eq!(&payload[0..4], &[0xfe, 0xff, 0xff, 0xff]);
        assert_eq!(&payload[16..18], &[0x34, 0x12]);
        assert_eq!(&payload[22..26], &[0x12, 0x34, 0x56, 0x78]);
        let words = std::array::from_fn(|index| {
            u64::from_le_bytes(payload[index * 8..index * 8 + 8].try_into().unwrap())
        });
        assert_eq!(InputVertex::decode_payload(words), Ok(vertex));
    }
}
