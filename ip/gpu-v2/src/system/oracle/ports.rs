//! Component boundaries for functional composition, independent of cycle models.
use crate::{framebuffer::ports as fb, lighting::ports as light, triangle::ports::Sample};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Ideal,
    CompactV6,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexMath {
    Continuous,
    S16F16,
}
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub fetch: Fetch,
    pub vertex: VertexMath,
    pub vertex_normal: NormalFormat,
    pub pixel_normal: NormalFormat,
    /// fetch->vertex, vertex->setup, setup->raster, raster->shader,
    /// shader->final, final->ROP. Capacities are entries, never cycles.
    pub fifo: [usize; 6],
    pub max_steps: usize,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            fetch: Fetch::Ideal,
            vertex: VertexMath::Continuous,
            vertex_normal: NormalFormat::Q14,
            pixel_normal: NormalFormat::Q14,
            fifo: [2; 6],
            max_steps: 1_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MeshVertex {
    pub position: [f64; 3],
    pub normal: [f64; 3],
    pub uv: [f64; 2],
    pub tint: [f64; 3],
}
#[derive(Clone, Debug)]
pub struct Scene {
    pub vertices: Vec<MeshVertex>,
    pub triangles: Vec<[usize; 3]>,
    pub mvp: [[f64; 4]; 4],
    pub normal_matrix: [[f64; 3]; 3],
    pub compact_base: [i32; 3],
    pub compact_grid_shift: u8,
    pub light: light::Light,
    pub material: light::Material,
    pub projection: light::Projection,
    pub rop: fb::Context,
    pub width: u16,
    pub height: u16,
    pub textured: bool,
}
#[derive(Clone, Debug)]
pub struct FetchedTriangle {
    pub id: u32,
    pub vertices: [MeshVertex; 3],
}
#[derive(Clone, Debug)]
pub struct TransformedTriangle {
    pub id: u32,
    pub clip: [[f64; 4]; 3],
    /// UV, tint RGB, unnormalized view-space normal.
    pub attributes: [[f64; 8]; 3],
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
#[derive(Clone, Debug)]
pub struct ShadedQuad {
    pub header: fb::Header,
    pub depth: [u16; 4],
    pub tint: [[u8; 3]; 4],
    pub texture: [[u8; 3]; 4],
    pub lighting: [light::LightingOutput; 4],
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub triangles: usize,
    pub quads: usize,
    pub fragments: usize,
    pub normal_clips: usize,
    pub color_saturations: usize,
    pub helper_fallbacks: usize,
    pub texture_refills: usize,
    pub framebuffer_refills: usize,
    pub framebuffer_writebacks: usize,
    pub fifo_peak: [usize; 6],
    pub pump_steps: usize,
}
pub struct Frame {
    /// Materialized RGB565 expanded and converted from linear to sRGB for display.
    pub rgba: Vec<u8>,
    pub color: Vec<u16>,
    pub depth: Vec<u16>,
    /// Final successful ROP fragment's g/h, in integer Q8 codes.
    pub lighting: Vec<[u16; 2]>,
    pub stats: Stats,
    pub vertex_boundaries: Vec<TransformedTriangle>,
}
