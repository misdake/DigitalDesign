//! Independent integer semantics for the final-color contract.
//!
//! The two rounding steps are re-expressed with bounded shifts and adds so the
//! audited graph, the RTL and the emulator can share them without a divider.
//! They are cross-checked against true division (and a rational ties-even
//! reference) in the tests.

use super::Input;

/// `RNE(tint * texture / 255)`.
///
/// The reciprocal `1/255 = (1 + 2^-8) / 256 + O(2^-16)`, so the standard
/// `(t + (t >> 8)) >> 8` correction with the `+128` half-ulp is exact over the
/// full u8*u8 domain. `/255` has no integer half-way case, so nearest-even and
/// nearest-up coincide here.
pub fn base_unorm255(tint: u8, texture: u8) -> u8 {
    let product = u32::from(tint) * u32::from(texture);
    let t = product + 128;
    ((t + (t >> 8)) >> 8) as u8
}

/// `RNE(sum / 256)`, ties to even. `sum` is a legal `base*g + specular*h`.
pub fn rne_div256(sum: u32) -> u32 {
    let floor = sum >> 8;
    let remainder = sum & 255;
    floor + u32::from(remainder > 128 || (remainder == 128 && floor & 1 != 0))
}

/// One saturated UNORM8 channel.
pub fn channel(tint: u8, texture: u8, g: u16, h: u16, specular: u8) -> u8 {
    let base = u32::from(base_unorm255(tint, texture));
    let sum = base * u32::from(g) + u32::from(specular) * u32::from(h);
    rne_div256(sum).min(255) as u8
}

/// All three channels of one input.
pub fn reference_rgb(input: &Input) -> [u8; 3] {
    std::array::from_fn(|c| {
        channel(
            input.tint[c],
            input.texture[c],
            input.g,
            input.h,
            input.specular[c],
        )
    })
}

/// Exhaustively checkable reference that uses true integer division and an
/// explicit rational ties-even rule. It is not the production path.
pub fn channel_true_division(tint: u8, texture: u8, g: u16, h: u16, specular: u8) -> u8 {
    let product = u32::from(tint) * u32::from(texture);
    // Exact nearest with denominator 255; the remainder is never 127.5.
    let floor255 = product / 255;
    let rem255 = product % 255;
    let base = floor255 + u32::from(rem255 * 2 > 255 || (rem255 * 2 == 255 && floor255 & 1 != 0));
    let sum = base * u32::from(g) + u32::from(specular) * u32::from(h);
    let floor = sum / 256;
    let remainder = sum % 256;
    let rounded = floor + u32::from(remainder > 128 || (remainder == 128 && floor & 1 != 0));
    rounded.min(255) as u8
}

/// Largest legal `base*g + specular*h`, used to bound the tests and the graph.
pub const MAX_SUM: u32 = 255 * 511 + 255 * 256;
