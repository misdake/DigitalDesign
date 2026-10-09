use gpu_v2::texture::ports::*;

pub const BASE: u32 = 0x1000;
pub fn slot(n: u8, mip: bool) -> Slot {
    Slot {
        base_address: BASE,
        max_size_log2: n,
        has_full_mip: mip,
        valid: true,
    }
}
pub fn input(n: u8, filter: Filter, uv: [f64; 2]) -> QuadInput {
    QuadInput {
        quad_id: 3,
        mask: 15,
        uv: [uv; 4],
        slot: 0,
        material_size_log2: n,
        filter,
        lod_bias: 0.0,
    }
}
/// Independent periodic RAW565 asset producer. This is external input stimulus;
/// it does not reuse sampler address, bank or color expansion helpers.
pub fn asset(slot: Slot, color: impl Fn(u8, usize, usize) -> u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    let first = if slot.has_full_mip {
        0
    } else {
        slot.max_size_log2
    };
    for n in first..=slot.max_size_log2 {
        let logical = 1_usize << n;
        let side = logical.div_ceil(8);
        for ty in 0..side {
            for tx in 0..side {
                for y in 0..8 {
                    for x in 0..8 {
                        bytes.extend(
                            color(n, (tx * 8 + x) % logical, (ty * 8 + y) % logical).to_le_bytes(),
                        );
                    }
                }
            }
        }
    }
    bytes
}
pub fn pattern(n: u8, x: usize, y: usize) -> u16 {
    (((x * 3 + y * 7 + usize::from(n) * 5) % 32) as u16) << 11
        | (((y * 5 + x * 11 + usize::from(n) * 13) % 64) as u16) << 5
        | ((x * 13 + y * 3 + usize::from(n) * 17) % 32) as u16
}
pub struct Image {
    pub bytes: Vec<u8>,
    pub requests: Vec<(u64, usize)>,
}
impl MemoryPort for Image {
    fn read_dma(&mut self, address: u64, bytes: usize) -> Result<Vec<u64>, String> {
        if address & 127 != 0 || bytes != 128 {
            return Err("fixture expects one RAW565 tile".into());
        }
        self.requests.push((address, bytes));
        let start = usize::try_from(
            address
                .checked_sub(u64::from(BASE))
                .ok_or("image underflow")?,
        )
        .map_err(|_| "image offset")?;
        let data = self
            .bytes
            .get(start..start + bytes)
            .ok_or("image missing tile")?;
        Ok(data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect())
    }
}
