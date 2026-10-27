//! Independent finite post-quad stimulus and scalar goldens.
//! No DUT queues, calendars, cache timing or branch completion answers are used.
//! SourceCursor advances only after the real eight-row ingress accepts a beat.
#![allow(dead_code)]

pub const QUADS: usize = 256;
pub const TEX_BASE: u32 = 0x1000;
pub const MAX_N: u8 = 6;
pub const COLOR_BASE: u32 = 0x100000;
pub const DEPTH_BASE: u32 = 0x140000;
pub const WIDTH: usize = 512;
pub const HEIGHT: usize = 256;
pub const WALL_LIMIT: u64 = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    Bare,
    Texture,
    Lighting,
    Full,
}
impl Feature {
    pub fn textured(self) -> bool {
        matches!(self, Self::Texture | Self::Full)
    }
    pub fn lit(self) -> bool {
        matches!(self, Self::Lighting | Self::Full)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Footprint {
    NearestSingle,
    BilinearSingle,
    TrilinearTwoMip,
    TrilinearSeamWrap,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Temperature {
    ColdStart,
    HotResident,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub feature: Feature,
    pub footprint: Footprint,
    pub texture: Temperature,
    pub framebuffer: Temperature,
}
impl Profile {
    pub fn filter(self) -> u8 {
        match self.footprint {
            Footprint::NearestSingle => 0,
            Footprint::BilinearSingle => 1,
            _ => 2,
        }
    }
    pub fn bias_q8(self) -> i16 {
        if self.filter() == 2 {
            128
        } else {
            0
        }
    }
    pub fn sample_config27(self) -> u32 {
        u32::from(self.bias_q8() as u16)
            | (u32::from(MAX_N) << 20)
            | (1 << 24)
            | (u32::from(self.filter()) << 25)
    }
    pub fn alpha(self) -> u8 {
        if self.feature == Feature::Full {
            128
        } else {
            255
        }
    }
    pub fn depth_func(self) -> u8 {
        if self.feature == Feature::Full {
            3
        } else {
            7
        }
    }
    pub fn blend(self) -> u8 {
        u8::from(self.feature == Feature::Full)
    }
    /// Exact +Z light/view/normal endpoint. Full and Lighting exercise specular.
    pub fn light_output(self) -> [u16; 2] {
        if self.feature.lit() {
            [224, 192]
        } else {
            [256, 0]
        }
    }
    pub fn specular(self) -> [u8; 3] {
        if self.feature.lit() {
            [24, 40, 56]
        } else {
            [0; 3]
        }
    }
    /// Four real UniformRows, with material mode prepared at the draw boundary.
    pub fn light_uniforms(self) -> [u64; 4] {
        let mode = if self.feature.lit() { 3_u64 } else { 0 };
        [
            0,
            16384 | (32 << 16) | (192 << 25),
            0xe000 | (0xe000 << 16),
            8192 | (mode << 16) | (8 << 18),
        ]
    }
}

/// ColdStart means cold at measurement start, not a miss on every later access.
/// HotResident is a claim to verify from actual measured refill counters.
pub fn matrix() -> Vec<Profile> {
    let mut out = Vec::new();
    for feature in [
        Feature::Bare,
        Feature::Texture,
        Feature::Lighting,
        Feature::Full,
    ] {
        for footprint in [
            Footprint::NearestSingle,
            Footprint::BilinearSingle,
            Footprint::TrilinearTwoMip,
            Footprint::TrilinearSeamWrap,
        ] {
            if !feature.textured() && footprint != Footprint::NearestSingle {
                continue;
            }
            for texture in [Temperature::ColdStart, Temperature::HotResident] {
                if !feature.textured() && texture == Temperature::HotResident {
                    continue;
                }
                for framebuffer in [Temperature::ColdStart, Temperature::HotResident] {
                    out.push(Profile {
                        feature,
                        footprint,
                        texture,
                        framebuffer,
                    });
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quad {
    pub x: u16,
    pub y: u8,
    pub mask: u8,
    pub tint: [[u8; 3]; 4],
    pub depth: [u16; 4],
    pub uv_q16: [[i32; 2]; 4],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomRow {
    pub begin: u32,
    pub basic: u32,
    pub lighting: u64,
    pub uv: u64,
}
impl Quad {
    pub fn rows(self, p: Profile) -> [RomRow; 8] {
        assert!(self.x <= 510 && self.x & 1 == 0 && self.y & 1 == 0 && self.mask <= 15);
        let origin = u32::from(self.x / 2) | (u32::from(self.y / 2) << 8);
        let begin = u32::from(self.mask)
            | (u32::from(!p.feature.lit()) << 5)
            | (u32::from(!p.feature.textured()) << 6)
            | (origin << 8);
        std::array::from_fn(|row| {
            let lane = row / 2;
            let odd = row & 1 != 0;
            let c = self.tint[lane];
            let uv = self.uv_q16[lane];
            assert!(uv.iter().all(|v| (-131072..=131071).contains(v)));
            RomRow {
                begin,
                basic: if odd {
                    u32::from(self.depth[lane])
                } else {
                    u32::from(c[0]) | (u32::from(c[1]) << 8) | (u32::from(c[2]) << 16)
                },
                lighting: if odd { 0 } else { 1024_u64 << 24 },
                uv: if odd {
                    (uv[0] as u64 & 0x3ffff) | ((uv[1] as u64 & 0x3ffff) << 18)
                } else {
                    0
                },
            }
        })
    }
}

pub fn quads(p: Profile) -> Vec<Quad> {
    (0..QUADS)
        .map(|i| {
            let [u, v] = match p.footprint {
                Footprint::NearestSingle | Footprint::BilinearSingle => [
                    ((2 + i % 3) * 1024 + 256) as i32,
                    ((2 + i / 3 % 3) * 1024 + 768) as i32,
                ],
                Footprint::TrilinearTwoMip => [3840, 3840],
                Footprint::TrilinearSeamWrap => {
                    if i & 1 == 0 {
                        [8448, 8448]
                    } else {
                        [-256, -256]
                    }
                }
            };
            Quad {
                x: ((i % 8) * 2) as u16,
                y: ((i / 8 % 8) * 2) as u8,
                mask: 15,
                tint: std::array::from_fn(|lane| {
                    [
                        (i * 23 + lane * 17) as u8,
                        (181 + i * 3 + lane * 11) as u8,
                        (i * 7 + lane * 37) as u8,
                    ]
                }),
                depth: std::array::from_fn(|lane| (60000 - i * 13 - lane) as u16),
                uv_q16: std::array::from_fn(|lane| {
                    [u + (lane % 2) as i32 * 1024, v + (lane / 2) as i32 * 1024]
                }),
            }
        })
        .collect()
}

/// Uniform 64x16 texture-space footprint, including every fine/coarse tile
/// phase. This is independent of framebuffer positions and DUT membership.
pub fn spatial_uv(quads: &mut [Quad]) {
    for (i, q) in quads.iter_mut().enumerate() {
        let u = ((i % 32) * 2 * 1024 + 256) as i32;
        let v = ((i / 32 % 8) * 2 * 1024 + 256) as i32;
        q.uv_q16 = std::array::from_fn(|lane| {
            [u + (lane % 2) as i32 * 1024, v + (lane / 2) as i32 * 1024]
        });
    }
}

/// Use the same ROM attributes for primers at a disjoint framebuffer tile.
/// Each epoch has immutable pins and must drain before changing its profile.
pub fn workloads(p: Profile, count: usize, origin_x: u16, depth_start: u16) -> Vec<Quad> {
    assert!(count <= QUADS && origin_x & 15 == 0 && origin_x <= 496);
    assert!(usize::from(depth_start) >= count.saturating_sub(1) * 13 + 3);
    quads(p)
        .into_iter()
        .take(count)
        .enumerate()
        .map(|(i, mut q)| {
            q.x += origin_x;
            q.depth = std::array::from_fn(|lane| depth_start - (i * 13 + lane) as u16);
            q
        })
        .collect()
}

/// Priming is real quad work, outside the measured 256-quad interval.
/// Mask/full attributes remain genuine; depth65535 keeps measured Full eligible.
pub fn primers(p: Profile) -> Vec<(Profile, Vec<Quad>)> {
    let mut epochs = Vec::new();
    if p.framebuffer == Temperature::HotResident {
        let bare = Profile {
            feature: Feature::Bare,
            ..p
        };
        let mut qs = workloads(bare, 64, 0, 65535);
        for q in &mut qs {
            q.depth = [65535; 4];
        }
        epochs.push((bare, qs));
    }
    if p.feature.textured() && p.texture == Temperature::HotResident {
        let texture = Profile {
            feature: Feature::Texture,
            ..p
        };
        let x = if p.framebuffer == Temperature::ColdStart {
            32
        } else {
            0
        };
        let mut qs = workloads(texture, 64, x, 65535);
        for q in &mut qs {
            q.depth = [65535; 4];
        }
        epochs.push((texture, qs));
    }
    epochs
}

pub struct SourceCursor {
    quad: usize,
    row: usize,
}
impl SourceCursor {
    pub fn new() -> Self {
        Self { quad: 0, row: 0 }
    }
    pub fn offered(&self, rom: &[Quad], p: Profile) -> Option<RomRow> {
        rom.get(self.quad).map(|q| q.rows(p)[self.row])
    }
    pub fn position(&self) -> (usize, usize) {
        (self.quad, self.row)
    }
    pub fn accepted(&mut self) {
        assert!(self.quad < QUADS);
        self.row += 1;
        if self.row == 8 {
            self.row = 0;
            self.quad += 1;
        }
    }
}

/// RAW565 external data, independent of sampler bank/address helpers.
pub fn pattern(n: u8, x: usize, y: usize) -> u16 {
    ((((x * 3 + y * 7 + usize::from(n) * 5) % 32) as u16) << 11)
        | ((((y * 5 + x * 11 + usize::from(n) * 13) % 64) as u16) << 5)
        | ((x * 13 + y * 3 + usize::from(n) * 17) % 32) as u16
}
pub fn texture_asset() -> Vec<u8> {
    let mut bytes = Vec::new();
    for n in 0..=MAX_N {
        let logical = 1_usize << n;
        for ty in 0..logical.div_ceil(8) {
            for tx in 0..logical.div_ceil(8) {
                for y in 0..8 {
                    for x in 0..8 {
                        bytes.extend(
                            pattern(n, (tx * 8 + x) % logical, (ty * 8 + y) % logical)
                                .to_le_bytes(),
                        );
                    }
                }
            }
        }
    }
    bytes
}
pub fn texture_memory_beat(asset: &[u8], address: u32) -> Result<u64, String> {
    if address & 7 != 0 {
        return Err("texture beat alignment".into());
    }
    let offset = address
        .checked_sub(TEX_BASE)
        .ok_or("texture beat below asset")? as usize;
    let bytes = asset
        .get(offset..offset + 8)
        .ok_or("texture beat beyond asset")?;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}
pub fn expand565(w: u16) -> [u8; 3] {
    let [r, g, b] = [(w >> 11) as u8, ((w >> 5) & 63) as u8, (w & 31) as u8];
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}
fn nearest_even(value: u64, denominator: u64) -> u64 {
    let q = value / denominator;
    let r = value % denominator;
    q + u64::from(r * 2 > denominator || (r * 2 == denominator && q & 1 != 0))
}
fn layer_sum(uv: [i32; 2], n: u8, parent: u64, nearest: bool) -> [u64; 3] {
    let a = 1_i64 << n.max(1);
    let coord = uv.map(|v| i64::from(v).rem_euclid(65536) * a - if nearest { 0 } else { 32768 });
    let integer = coord.map(|v| v.div_euclid(65536));
    let frac = coord.map(|v| v.rem_euclid(65536) as u64 / 256);
    let weights = if nearest {
        [parent, 0, 0, 0]
    } else {
        let bottom = parent * frac[1] / 256;
        let top = parent - bottom;
        let tr = top * frac[0] / 256;
        let br = bottom * frac[0] / 256;
        [top - tr, tr, bottom - br, br]
    };
    std::array::from_fn(|c| {
        (0..4)
            .map(|tap| {
                let x = (integer[0] + (tap % 2) as i64).rem_euclid(a) as usize;
                let y = (integer[1] + (tap / 2) as i64).rem_euclid(a) as usize;
                u64::from(expand565(pattern(n, x, y))[c]) * weights[tap]
            })
            .sum()
    })
}
/// This finite ROM has exact rho=1. Trilinear bias0.5 gives parent255/256;
/// single-mip profiles have LOD0. No floating log/ROM answer is borrowed.
pub fn sample_rgb(q: &Quad, lane: usize, p: Profile) -> [u8; 3] {
    assert!(lane < 4);
    if !p.feature.textured() {
        return [255; 3];
    }
    let tri = p.filter() == 2;
    let fine = layer_sum(
        q.uv_q16[lane],
        MAX_N,
        if tri { 255 } else { 511 },
        p.filter() == 0,
    );
    let coarse = if tri {
        layer_sum(q.uv_q16[lane], MAX_N - 1, 256, false)
    } else {
        [0; 3]
    };
    std::array::from_fn(|c| nearest_even(fine[c] + coarse[c], 511) as u8)
}
pub fn final_rgb(q: &Quad, lane: usize, p: Profile) -> [u8; 3] {
    let tex = sample_rgb(q, lane, p);
    let [g, h] = p.light_output();
    let spec = p.specular();
    std::array::from_fn(|c| {
        let base = nearest_even(u64::from(q.tint[lane][c]) * u64::from(tex[c]), 255);
        nearest_even(base * u64::from(g) + u64::from(spec[c]) * u64::from(h), 256).min(255) as u8
    })
}
pub fn quantize565(rgb: [u8; 3]) -> u16 {
    let code = std::array::from_fn::<_, 3, _>(|c| {
        nearest_even(u64::from(rgb[c]) * [31, 63, 31][c], 255) as u16
    });
    (code[0] << 11) | (code[1] << 5) | code[2]
}
/// Sequential framebuffer oracle for Always/Replace or LessEqual/SrcOver.
/// The caller owns initial/primer framebuffer contents and true final visibility.
pub fn apply_pixel(old: [u16; 2], q: &Quad, lane: usize, p: Profile) -> [u16; 2] {
    if q.mask & (1 << lane) == 0 || (p.depth_func() == 3 && q.depth[lane] > old[1]) {
        return old;
    }
    let rgb = final_rgb(q, lane, p);
    let blended = if p.blend() == 0 {
        rgb
    } else {
        let dst = expand565(old[0]);
        let alpha = u64::from(p.alpha());
        std::array::from_fn(|c| {
            nearest_even(
                alpha * u64::from(rgb[c]) + (255 - alpha) * u64::from(dst[c]),
                255,
            ) as u8
        })
    };
    [quantize565(blended), q.depth[lane]]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub first: usize,
    pub end: usize,
}
impl Window {
    pub fn middle() -> Self {
        Self {
            first: 64,
            end: 192,
        }
    }
    /// Per-quad steady intervals only; wall timestamps include genuine stalls.
    pub fn interval(self, timestamps: &[u64]) -> (u64, usize) {
        assert!(self.end <= timestamps.len() && self.end > self.first + 1);
        (
            timestamps[self.end - 1] - timestamps[self.first],
            self.end - self.first - 1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_source_holds_real_rows_and_exact_rho() {
        assert_eq!(matrix().len(), 36);
        for p in matrix() {
            let rom = quads(p);
            let mut cursor = SourceCursor::new();
            for q in &rom {
                for lane in 0..4 {
                    assert_eq!(
                        q.uv_q16[lane][0] - q.uv_q16[lane ^ 1][0],
                        if lane & 1 == 0 { -1024 } else { 1024 }
                    );
                    assert_eq!(
                        q.uv_q16[lane][1] - q.uv_q16[lane ^ 2][1],
                        if lane & 2 == 0 { -1024 } else { 1024 }
                    );
                }
                for expected in q.rows(p) {
                    assert_eq!(cursor.offered(&rom, p), Some(expected));
                    assert_eq!(cursor.offered(&rom, p), Some(expected));
                    cursor.accepted();
                }
            }
            assert!(cursor.offered(&rom, p).is_none());
        }
    }
    #[test]
    fn independent_asset_and_scalar_endpoints() {
        let asset = texture_asset();
        assert_eq!(asset.len(), 88 * 128);
        assert_eq!(
            texture_memory_beat(&asset, TEX_BASE).unwrap() as u16,
            pattern(0, 0, 0)
        );
        for rgb in [
            [0, 0, 0],
            [255, 255, 255],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
        ] {
            assert_eq!(quantize565(expand565(quantize565(rgb))), quantize565(rgb));
        }
        for p in matrix() {
            for q in quads(p) {
                for lane in 0..4 {
                    let out = apply_pixel([0, 65535], &q, lane, p);
                    assert_eq!(out[1], q.depth[lane]);
                    assert_eq!(
                        apply_pixel(
                            [out[0], 0],
                            &q,
                            lane,
                            Profile {
                                feature: Feature::Full,
                                ..p
                            }
                        ),
                        [out[0], 0]
                    );
                }
            }
        }
        assert_eq!(nearest_even(128, 256), 0);
        assert_eq!(nearest_even(384, 256), 2);
    }
}
