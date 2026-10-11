//! Closed LOD owner codec. No numeric fraction or coordinate is stored here.
use super::Owner;
pub const OWNER_BITS: u32 = 24;
pub const OWNER_SDP4_CELLS: u64 = 6;
pub const SDP4_CELLS: u64 = OWNER_SDP4_CELLS + 9;
pub fn pack(o: Owner) -> Result<u32, String> {
    o.pack()?;
    if o.levels[0] > 10
        || o.levels[1] != o.levels[0].saturating_sub(1)
        || o.parents[0] != 511 - o.parents[1]
        || o.last_fine != (o.parents[1] == 0)
    {
        return Err("Coordinate canonical LOD owner contract".into());
    }
    Ok(u32::from(o.parents[1])
        | (u32::from(o.nearest) << 9)
        | (u32::from(o.levels[0]) << 10)
        | (u32::from(o.slot) << 14)
        | (u32::from(o.key) << 18))
}
pub fn unpack(raw: u32) -> Owner {
    let p1 = (raw & 511) as u16;
    let n = ((raw >> 10) & 15) as u8;
    Owner {
        parents: [511 ^ p1, p1],
        nearest: raw >> 9 & 1 != 0,
        levels: [n, n.saturating_sub(1)],
        slot: ((raw >> 14) & 15) as u8,
        key: ((raw >> 18) & 63) as u8,
        last_fine: p1 == 0,
    }
}
