use crate::lighting::ports::LightingOutput;

/// UNORM8 tint/texture -> U(9,8) g/h -> saturated linear UNORM8.
/// This is numerical execution, not a certified final arithmetic calendar.
pub fn final_rgb(
    tint: [u8; 3],
    texture: [u8; 3],
    light: LightingOutput,
    specular: [u8; 3],
) -> Result<[u8; 3], String> {
    if light.g > 511 || light.h > 256 {
        return Err("final light range".into());
    }
    Ok(std::array::from_fn(|channel| {
        let product = u32::from(tint[channel]) * u32::from(texture[channel]);
        let t = product + 128;
        let base = (t + (t >> 8)) >> 8;
        let sum = base * u32::from(light.g) + u32::from(specular[channel]) * u32::from(light.h);
        let floor = sum >> 8;
        let remainder = sum & 255;
        let rounded = floor + u32::from(remainder > 128 || remainder == 128 && floor & 1 != 0);
        rounded.min(255) as u8
    }))
}
