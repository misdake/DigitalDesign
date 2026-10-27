//! Final U5F3 LOD decoding. No derivative, logarithm or bias is evaluated here.
use super::emu::lod::CoordinateContext;

/// `lod` already includes texture size, bias and the producer clamp.
/// Values beyond this slot's mip count saturate to its coarsest layer.
pub fn decode(lod: u8, max_n: u8, has_mip: bool, filter: u8) -> Result<CoordinateContext, String> {
    if max_n > 10 || filter > 2 {
        return Err("direct LOD configuration range".into());
    }
    let distance = (lod >> 3).min(max_n);
    let n = if has_mip { max_n - distance } else { max_n };
    let fraction = if has_mip && distance < max_n {
        lod & 7
    } else {
        0
    };
    // Exactly the selected /511 parent boundary, including the half-way code.
    let coarse = if filter == 2 {
        (u16::from(fraction) * 511 + 4) >> 3
    } else {
        0
    };
    let physical = n.max(1);
    Ok(CoordinateContext {
        shift: i32::from(physical) - 8,
        nearest: filter == 0,
        halve: n > 1,
        side: [
            1_i16 << physical,
            if n > 1 { 1_i16 << (physical - 1) } else { 2 },
        ],
        parents: [511 - coarse, coarse],
        levels: [n, n.saturating_sub(1)],
        last_fine: coarse == 0,
    })
}
