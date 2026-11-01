//! Numerical raster/attribute boundary to the finite quad ingress. This adapts
//! the existing triangle oracle's samples; it is not a live rasterizer RTL.
use super::dispatch::{ContextId, Dispatcher, Input};
use super::Basic;
use crate::{
    framebuffer::ports::Header, lighting::ports::CompactPixelInput, triangle::ports::Sample,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalFormat {
    Q14,
    S12F10,
    Snorm12,
}
impl NormalFormat {
    pub fn quantize(self, normal: [f64; 3]) -> Result<[f64; 3], String> {
        if normal.iter().any(|v| !v.is_finite()) {
            return Err("nonfinite normal".into());
        }
        let (scale, lo, hi) = match self {
            Self::Q14 => (16384.0, -32768.0, 32767.0),
            Self::S12F10 => (1024.0, -2048.0, 2047.0),
            Self::Snorm12 => (2047.0, -2047.0, 2047.0),
        };
        // Explicit representable-range clamp; rare large/extrapolated values
        // remain observable through per-stage saturation counters.
        Ok(normal.map(|v| (v * scale).round_ties_even().clamp(lo, hi) / scale))
    }
    pub fn clipped(self, normal: [f64; 3]) -> bool {
        let (scale, lo, hi) = match self {
            Self::Q14 => (16384.0, -32768.0, 32767.0),
            Self::S12F10 => (1024.0, -2048.0, 2047.0),
            Self::Snorm12 => (2047.0, -2047.0, 2047.0),
        };
        normal.iter().any(|v| {
            let code = (v * scale).round_ties_even();
            code < lo || code > hi
        })
    }
}

#[derive(Clone, Debug)]
pub struct RasterQuad {
    pub triangle: u32,
    pub xy: [u16; 2],
    pub mask: u8,
    /// Includes uncovered helper lanes; derivatives must not use zero fill.
    pub samples: [Sample; 4],
    /// Uncovered lanes whose projective extrapolation has no positive finite W.
    /// Sampling conservatively selects the coarsest mip for this quad.
    pub invalid_helpers: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quantization {
    /// Covered, lit normals outside S2.10 are explicitly saturated, as in the
    /// numerical comparison chain; this counter keeps rare clipping observable.
    pub normal_clips: usize,
}

/// Consumes the raster attributes, retaining only queue-owned compact inputs.
/// Helpers retain unwrapped S(18,16). Nonprojectable/out-of-range uncovered
/// helpers explicitly force coarsest LOD before a benign replacement is stored.
pub fn convert(
    dispatcher: &Dispatcher,
    context: ContextId,
    quad: RasterQuad,
    viewport: [u16; 2],
) -> Result<(Input, Quantization), String> {
    let [width, height] = viewport;
    if width == 0 || height == 0 || width > 512 || height > 256 {
        return Err("raster adapter viewport bounds".into());
    }
    if quad.xy[0] & 1 != 0
        || quad.xy[1] & 1 != 0
        || quad.xy[0] >= width
        || quad.xy[1] >= height
        || quad.mask > 15
        || quad.invalid_helpers & quad.mask != 0
    {
        return Err("raster adapter coverage/header".into());
    }
    let material = dispatcher.context(context)?;
    let mut input = Input {
        context,
        header: Header {
            x: quad.xy[0],
            y: quad.xy[1] as u8,
            mask: quad.mask,
        },
        basic: [Basic::default(); 4],
        light: [CompactPixelInput {
            normal: [0; 3],
            ndc: [0; 2],
        }; 4],
        uv_q16: [[0; 2]; 4],
        force_coarsest: false,
    };
    if quad.mask != 0 {
        if let Some(sample) = material.sample {
            let source = crate::texture::ports::QuadInput {
                quad_id: 0,
                mask: quad.mask,
                uv: quad.samples.clone().map(|s| s.uv),
                slot: sample.slot,
                material_size_log2: sample.size_log2,
                filter: sample.filter,
                lod_bias: f64::from(sample.bias_q8) / 256.0,
                force_coarsest: quad.invalid_helpers != 0,
            };
            (input.uv_q16, input.force_coarsest) = crate::texture::ports::capture_uv(&source)?;
        }
    }
    let mut quantization = Quantization::default();
    for (lane, sample) in quad.samples.into_iter().enumerate() {
        if quad.mask & (1 << lane) != 0 {
            if sample.rgb.iter().any(|v| !v.is_finite()) {
                return Err("raster adapter nonfinite RGB".into());
            }
            input.basic[lane] = Basic {
                tint: sample
                    .rgb
                    .map(|v| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8),
                depth: sample.quantized.depth,
            };
            if !material.lighting.material.unlit {
                quantization.normal_clips +=
                    usize::from(NormalFormat::S12F10.clipped(sample.normal));
                let normal = NormalFormat::S12F10.quantize(sample.normal)?;
                let ndc = crate::lighting::ports::pixel_center_ndc(
                    quad.xy[0] + (lane % 2) as u16,
                    quad.xy[1] + (lane / 2) as u16,
                    width,
                    height,
                )
                .map_err(|e| format!("raster adapter pixel center: {e:?}"))?;
                input.light[lane] = CompactPixelInput {
                    normal: normal.map(|v| (v * 1024.0).round_ties_even() as i16),
                    ndc,
                };
                input.light[lane]
                    .validate()
                    .map_err(|e| format!("raster adapter normal/NDC: {e:?}"))?;
            }
        }
    }
    Ok((input, quantization))
}
