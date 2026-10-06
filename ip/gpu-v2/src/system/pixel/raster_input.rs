//! Numerical raster/attribute boundary to the finite quad ingress. This adapts
//! the existing triangle oracle's samples; it is not a live rasterizer RTL.
use super::dispatch::{ContextId, Dispatcher, Input};
use super::Basic;
use crate::{
    framebuffer::ports::Header,
    lighting::ports::CompactPixelInput,
    system::oracle::ports::{NormalFormat, RasterQuad},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quantization {
    /// Covered, lit normals outside S2.10 are explicitly saturated, as in the
    /// numerical comparison chain; this counter keeps rare clipping observable.
    pub normal_clips: usize,
}

/// Consumes the raster attributes, retaining only queue-owned compact inputs.
/// UV helper lanes remain unwrapped S40F18; they are never coverage-zero-filled.
/// A nonprojectable helper currently fails explicitly for textured work. Its
/// conservative coarsest-LOD override is not yet in the Runtime ingress ABI.
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
    if quad.mask != 0 && material.sample.is_some() && quad.invalid_helpers != 0 {
        return Err("textured invalid helper requires explicit coarsest-LOD contract".into());
    }
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
        uv_q18: [[0; 2]; 4],
    };
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
                let ndc = [
                    sample.position[0] * 2.0 / f64::from(width) - 1.0,
                    1.0 - sample.position[1] * 2.0 / f64::from(height),
                ];
                if ndc
                    .iter()
                    .any(|v| !v.is_finite() || !(-2.0..2.0).contains(v))
                {
                    return Err("raster adapter NDC S2.16 range".into());
                }
                input.light[lane] = CompactPixelInput {
                    normal: normal.map(|v| (v * 1024.0).round_ties_even() as i16),
                    ndc: ndc.map(|v| (v * 65536.0).round_ties_even() as i32),
                };
                input.light[lane]
                    .validate()
                    .map_err(|e| format!("raster adapter normal/NDC: {e:?}"))?;
            }
        }
        if quad.mask != 0 && material.sample.is_some() {
            for axis in 0..2 {
                let raw = (sample.uv[axis] * 262144.0).round_ties_even();
                if !raw.is_finite() || raw.abs() > (1_u64 << 38) as f64 {
                    return Err("raster adapter helper UV range".into());
                }
                input.uv_q18[lane][axis] = raw as i64;
            }
        }
    }
    Ok((input, quantization))
}
