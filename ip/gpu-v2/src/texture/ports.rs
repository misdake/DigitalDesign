// Reuse the existing GPU-owned functional memory facade. Vendor Service adapters
// remain at the test/composition boundary, never in this IP library.
pub use crate::frontend::ports::MemoryPort;

/// Cycle projection of the existing SDRAM Service facade. Composition adapters
/// forward submit/step directly; this interface supplies no latency model.
pub trait RefillPort {
    fn submit_read(&mut self, address: u64, bytes: usize) -> Result<u64, String>;
    /// The committed return channel cannot be backpressured by the sampler.
    fn step(&mut self) -> Result<Vec<RefillEvent>, String>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RefillEvent {
    Started {
        id: u64,
    },
    Beat {
        id: u64,
        index: usize,
        data: u64,
        last: bool,
    },
    Complete {
        id: u64,
    },
}

pub const TILE_BYTES: usize = 128;
pub const MAX_SIZE_LOG2: u8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub base_address: u32,
    pub has_full_mip: bool,
    pub max_size_log2: u8,
    pub valid: bool,
}
impl Slot {
    pub fn validate(self) -> Result<(), String> {
        if !self.valid || self.max_size_log2 > MAX_SIZE_LOG2 || self.base_address & 127 != 0 {
            return Err("texture slot validity/size/alignment".into());
        }
        // Reject overflowing allocations, even when the current sample is tiny.
        let tiles = if self.has_full_mip {
            (0..=self.max_size_log2).map(tile_count).sum::<u32>()
        } else {
            tile_count(self.max_size_log2)
        };
        if u64::from(self.base_address) + u64::from(tiles) * TILE_BYTES as u64 > 1_u64 << 32 {
            return Err("texture allocation exceeds 32-bit byte address space".into());
        }
        Ok(())
    }
}
pub(crate) fn tile_count(n: u8) -> u32 {
    1 << (2 * n.saturating_sub(3))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TileKey {
    pub slot: u8,
    pub n: u8,
    pub x: u8,
    pub y: u8,
}
impl TileKey {
    pub fn set(self) -> usize {
        usize::from((self.y & 3) << 2 | (self.x & 3))
    }
    pub fn address(self, slots: &[Slot]) -> Result<u64, String> {
        let slot = *slots
            .get(usize::from(self.slot))
            .ok_or("texture slot index")?;
        slot.validate()?;
        if self.n > slot.max_size_log2 {
            return Err("texture tile level outside slot".into());
        }
        let side = 1_u32 << self.n.saturating_sub(3);
        if (!slot.has_full_mip && self.n != slot.max_size_log2)
            || u32::from(self.x) >= side
            || u32::from(self.y) >= side
        {
            return Err("texture tile key outside slot/layer".into());
        }
        let offset = if slot.has_full_mip {
            layer_offset(self.n)?
        } else {
            0
        };
        Ok(u64::from(slot.base_address)
            + u64::from(offset + u32::from(self.y) * side + u32::from(self.x)) * 128)
    }
}
/// Small fixed lookup, in 128 B tiles; no per-request mip summation.
pub fn layer_offset(n: u8) -> Result<u32, String> {
    const OFFSETS: [u32; 11] = [0, 1, 2, 3, 4, 8, 24, 88, 344, 1368, 5464];
    OFFSETS
        .get(usize::from(n))
        .copied()
        .ok_or("texture mip level bound".into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Bilinear,
    Trilinear,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MipSelection {
    Floor,
    /// Nearest layer, with half-way values selecting the coarser layer.
    Nearest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LodMethod {
    Exact,
    /// Leading exponent and floor-indexed 64-entry log2 mantissa table.
    Table64,
    /// RNE mantissa grid; index 64 carries into the exponent.
    Table64Nearest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoefficientEncoding {
    /// Binary weights divided by 2^F; unity needs F+1 storage bits.
    FixedPoint,
    /// Weights divided by 2^B-1; both endpoints fit in B storage bits.
    Unorm,
}

#[derive(Clone, Debug)]
pub struct QuadInput {
    pub quad_id: u8,
    pub mask: u8,
    /// Row-major 2x2 helper UV, unwrapped, including uncovered lanes.
    pub uv: [[f64; 2]; 4],
    pub slot: u8,
    pub material_size_log2: u8,
    pub filter: Filter,
    /// In mip levels, applied before clamping.
    pub lod_bias: f64,
}

/// Oracle experiment controls, not frozen counted/RTL formats.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub uv_fraction: Option<u8>,
    pub coordinate_fraction: u8,
    pub coefficient_fraction: u8,
    pub coefficient_encoding: CoefficientEncoding,
    pub lod_fraction: u8,
    pub lod_method: LodMethod,
    pub mip_selection: MipSelection,
    /// Absolute unwrapped UV differences above this force maximum available LOD.
    pub derivative_limit: f64,
    pub max_quads: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            uv_fraction: Some(17),
            coordinate_fraction: 8,
            coefficient_fraction: 8,
            coefficient_encoding: CoefficientEncoding::FixedPoint,
            lod_fraction: 8,
            lod_method: LodMethod::Table64,
            mip_selection: MipSelection::Nearest,
            derivative_limit: 2.0,
            max_quads: 4096,
        }
    }
}
impl Config {
    /// Frozen step-1 contract; historical default remains an oracle experiment.
    pub fn counted() -> Self {
        Self {
            uv_fraction: Some(18),
            coefficient_fraction: 9,
            coefficient_encoding: CoefficientEncoding::Unorm,
            lod_method: LodMethod::Table64Nearest,
            mip_selection: MipSelection::Floor,
            ..Self::default()
        }
    }
    /// coefficient_fraction is F for FixedPoint, or storage width B for UNORM.
    pub fn coefficient_scale(self) -> u32 {
        (1_u32 << self.coefficient_fraction)
            - u32::from(self.coefficient_encoding == CoefficientEncoding::Unorm)
    }

    pub fn validate(self) -> Result<(), String> {
        if self.uv_fraction.is_some_and(|f| f > 30)
            || !(1..=16).contains(&self.coordinate_fraction)
            || !(1..=16).contains(&self.coefficient_fraction)
            || !(1..=16).contains(&self.lod_fraction)
            || !self.derivative_limit.is_finite()
            || self.derivative_limit <= 0.0
            || self.max_quads == 0
            || self.max_quads > 1_000_000
        {
            return Err("texture oracle configuration bounds".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group4 {
    pub key: TileKey,
    pub top_left_local: [u8; 2],
    pub coefficients: [u32; 4],
    pub first: bool,
    pub last: bool,
    pub quad_id: u8,
    pub lane: u8,
}
impl Group4 {
    /// Runtime decode of the UNORM9 packet consumed by cache and color.
    pub fn unpack72(payload: i128) -> Result<Self, String> {
        if !(0..1_i128 << 72).contains(&payload) {
            return Err("Group4 payload width".into());
        }
        let w = payload as u128;
        let g = Self {
            key: TileKey {
                slot: (w & 15) as u8,
                n: ((w >> 4) & 15) as u8,
                x: ((w >> 8) & 127) as u8,
                y: ((w >> 15) & 127) as u8,
            },
            top_left_local: [((w >> 22) & 7) as u8, ((w >> 25) & 7) as u8],
            coefficients: std::array::from_fn(|j| ((w >> (28 + 9 * j)) & 511) as u32),
            first: w >> 64 & 1 != 0,
            last: w >> 65 & 1 != 0,
            quad_id: ((w >> 66) & 15) as u8,
            lane: (w >> 70) as u8,
        };
        g.pack72()?;
        Ok(g)
    }
    /// Canonical bit order for inspection only. ABI bit positions are not frozen.
    pub fn pack72(&self) -> Result<u128, String> {
        if self.key.slot > 15
            || self.key.n > 10
            || self.key.x > 127
            || self.key.y > 127
            || self.top_left_local.iter().any(|&v| v > 7)
            || self.coefficients.iter().any(|&v| v > 511)
            || self.quad_id > 15
            || self.lane > 3
        {
            return Err("Group4 cannot be represented in 72 bits".into());
        }
        let fields = [
            (u128::from(self.key.slot), 4),
            (u128::from(self.key.n), 4),
            (u128::from(self.key.x), 7),
            (u128::from(self.key.y), 7),
            (u128::from(self.top_left_local[0]), 3),
            (u128::from(self.top_left_local[1]), 3),
            (u128::from(self.coefficients[0]), 9),
            (u128::from(self.coefficients[1]), 9),
            (u128::from(self.coefficients[2]), 9),
            (u128::from(self.coefficients[3]), 9),
            (u128::from(self.first), 1),
            (u128::from(self.last), 1),
            (u128::from(self.quad_id), 4),
            (u128::from(self.lane), 2),
        ];
        let mut word = 0;
        let mut shift = 0;
        for (value, width) in fields {
            word |= value << shift;
            shift += width;
        }
        Ok(word)
    }

    /// Experimental binary 9-fraction encoding: w=1..511 keeps its value;
    /// w=512 uses code 0. Four appended mask bits distinguish zero from unity
    /// (both have code 0). This costs 76 bits, not the baseline 72 bits.
    pub fn pack76_zero_mask(&self) -> Result<u128, String> {
        let mut encoded = self.clone();
        let mut zero_mask = 0_u128;
        for (tap, weight) in encoded.coefficients.iter_mut().enumerate() {
            if *weight > 512 {
                return Err("zero-mask coefficient exceeds 512".into());
            }
            if *weight == 0 {
                zero_mask |= 1 << tap;
            }
            *weight &= 511;
        }
        Ok(encoded.pack72()? | (zero_mask << 72))
    }
}
