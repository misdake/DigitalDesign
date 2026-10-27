//! CPU-side preparation candidates for a single-source base color.
//!
//! These routines price uniform work separately from pixel arithmetic. They do
//! not change the selected scalar Lighting or post-quad RTL. Colored light,
//! gray-light folding and moving directional intensity into Ks are explicit
//! numerical candidates, not hidden reinterpretations of the current context.

/// Source of the one selected albedo, independent of a vertex transport format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorSource {
    #[default]
    VertexColor,
    Texture,
}

/// Numerical source albedo; this does not prescribe a hardware record layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Albedo {
    Color([u8; 3]),
    Texture([u16; 2]),
}

/// The two endpoint-exact UNORM8 uniform colors; no pixel-owned copy is needed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialColors {
    pub source: ColorSource,
    pub diffuse_tint: [u8; 3],
    pub specular: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TintStrategy {
    Identity,
    BlackDiffuse,
    GrayLightFold(u8),
    VertexPreparation,
    TextureAssetOrGainTable,
}

impl MaterialColors {
    /// NoLight bypasses Ia/Id. The existing h also uses Id, so folding diffuse
    /// tint into Id is safe only when specular is disabled. A future pure-power
    /// h protocol must make its independent intensity ownership explicit.
    pub fn tint_strategy(self, lighting_enabled: bool) -> TintStrategy {
        match self.diffuse_tint {
            [255, 255, 255] => TintStrategy::Identity,
            [0, 0, 0] => TintStrategy::BlackDiffuse,
            [r, g, b] if lighting_enabled && self.specular == [0; 3] && r == g && g == b => {
                TintStrategy::GrayLightFold(r)
            }
            _ if self.source == ColorSource::VertexColor => TintStrategy::VertexPreparation,
            _ => TintStrategy::TextureAssetOrGainTable,
        }
    }
}

/// Independent nearest-integer UNORM8 multiplication, evaluated at draw/asset
/// preparation. An odd divisor has no integer halfway case.
pub fn uniform_product(a: u8, b: u8) -> u8 {
    ((u32::from(a) * u32::from(b) + 127) / 255) as u8
}

pub fn prepare_vertex_color(rgb: [u8; 3], tint: [u8; 3]) -> [u8; 3] {
    std::array::from_fn(|c| uniform_product(rgb[c], tint[c]))
}

/// Combine two draw constants once. The existing selected light has scalar
/// intensity, so only [255;3] reproduces its current white-light semantics.
pub fn prepare_specular_color(specular: [u8; 3], light_rgb: [u8; 3]) -> [u8; 3] {
    std::array::from_fn(|c| uniform_product(specular[c], light_rgb[c]))
}

fn rne_div256(value: u32) -> u32 {
    let q = value >> 8;
    let r = value & 255;
    q + u32::from(r > 128 || (r == 128 && q & 1 != 0))
}

/// Explicit candidate: h becomes the unscaled power p; Id is applied here once.
/// Feeding this Ks alongside the old h=Id*p would incorrectly apply Id twice.
pub fn prepare_specular_intensity(specular: [u8; 3], id: u16) -> Result<[u8; 3], &'static str> {
    if id > 256 {
        return Err("directional intensity outside U9F8 unit interval");
    }
    Ok(specular.map(|s| rne_div256(u32::from(s) * u32::from(id)) as u8))
}

/// Candidate constant fold for neutral tint. It changes the rounding order and
/// is only compared numerically here; it does not alter the live Lighting input.
/// Current h shares Id, so use only without specular; a separate pure-power h
/// protocol must retain original Id in prepared Ks before folding diffuse Id.
pub fn prepare_gray_intensities(gray: u8, ia: u16, id: u16) -> Result<[u16; 2], &'static str> {
    if ia > 256 || id > 256 {
        return Err("light intensity outside U9F8 unit interval");
    }
    Ok([ia, id].map(|v| ((u32::from(v) * u32::from(gray) + 127) / 255) as u16))
}

/// Draw-prepared coefficients indexed by the existing scalar g. A 512x27
/// native bank fits one BSRAM by capacity; actual fill/read ports are unbound.
/// The CPU table contains three u16 containers, not a six-byte hardware row.
pub fn gain_table(tint: [u8; 3]) -> [[u16; 3]; 512] {
    std::array::from_fn(|g| tint.map(|t| ((g as u32 * u32::from(t) + 127) / 255) as u16))
}

/// Arithmetic of the selected integer Final: each diffuse/specular term loses
/// its low eight product bits separately, then the sum saturates to RGB8.
pub fn shade_floor8(
    base: [u8; 3],
    g: [u16; 3],
    specular: [u8; 3],
    h: u16,
) -> Result<[u8; 3], &'static str> {
    if g.iter().any(|&x| x > 511) || h > 256 {
        return Err("final lighting intensity outside its code domain");
    }
    Ok(std::array::from_fn(|c| {
        let diffuse = (u32::from(base[c]) * u32::from(g[c])) >> 8;
        let specular = (u32::from(specular[c]) * u32::from(h)) >> 8;
        (diffuse + specular).min(255) as u8
    }))
}

/// Independent full-rounding reference; deliberately separate from Floor8.
pub fn shade_rne(
    base: [u8; 3],
    g: [u16; 3],
    specular: [u8; 3],
    h: u16,
) -> Result<[u8; 3], &'static str> {
    if g.iter().any(|&x| x > 511) || h > 256 {
        return Err("final lighting intensity outside its code domain");
    }
    Ok(std::array::from_fn(|c| {
        let sum = u32::from(base[c]) * u32::from(g[c]) + u32::from(specular[c]) * u32::from(h);
        rne_div256(sum).min(255) as u8
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_preserve_endpoints_and_black_diffuse_keeps_specular() {
        for code in 0..=255_u8 {
            assert_eq!(uniform_product(code, 255), code);
            assert_eq!(uniform_product(code, 0), 0);
            assert_eq!(prepare_specular_intensity([code; 3], 256), Ok([code; 3]));
        }
        assert_eq!(
            shade_floor8([0; 3], [511; 3], [80, 40, 20], 256),
            Ok([80, 40, 20])
        );
        assert_eq!(prepare_specular_color([80, 40, 20], [255; 3]), [80, 40, 20]);
        assert!(prepare_specular_intensity([255; 3], 257).is_err());
        assert!(prepare_gray_intensities(128, 257, 0).is_err());
        assert!(shade_floor8([255; 3], [512; 3], [0; 3], 0).is_err());
    }

    #[test]
    fn tint_policy_and_table_domains_are_explicit() {
        let material = |source, diffuse_tint| MaterialColors {
            source,
            diffuse_tint,
            specular: [0; 3],
        };
        assert_eq!(
            material(ColorSource::VertexColor, [255; 3]).tint_strategy(false),
            TintStrategy::Identity
        );
        assert_eq!(
            material(ColorSource::Texture, [0; 3]).tint_strategy(false),
            TintStrategy::BlackDiffuse
        );
        assert_eq!(
            material(ColorSource::Texture, [128; 3]).tint_strategy(true),
            TintStrategy::GrayLightFold(128)
        );
        assert_eq!(
            material(ColorSource::VertexColor, [90, 160, 255]).tint_strategy(false),
            TintStrategy::VertexPreparation
        );
        assert_eq!(
            material(ColorSource::Texture, [90, 160, 255]).tint_strategy(true),
            TintStrategy::TextureAssetOrGainTable
        );
        assert_eq!(
            material(ColorSource::VertexColor, [128; 3]).tint_strategy(false),
            TintStrategy::VertexPreparation
        );
        assert_eq!(
            material(ColorSource::Texture, [128; 3]).tint_strategy(false),
            TintStrategy::TextureAssetOrGainTable
        );
        for (source, strategy) in [
            (ColorSource::VertexColor, TintStrategy::VertexPreparation),
            (ColorSource::Texture, TintStrategy::TextureAssetOrGainTable),
        ] {
            assert_eq!(
                MaterialColors {
                    source,
                    diffuse_tint: [128; 3],
                    specular: [80, 40, 20],
                }
                .tint_strategy(true),
                strategy,
                "gray diffuse tint must preserve independently colored highlights"
            );
        }
        let identity = gain_table([255; 3]);
        for (g, row) in identity.iter().enumerate() {
            assert_eq!(*row, [g as u16; 3]);
        }
        assert_eq!(gain_table([0; 3]), [[0; 3]; 512]);
    }
}
