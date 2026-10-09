use gpu_v2::triangle::{
    ports::{Config, FieldOrigin, Interpolation},
    sim::oracle::{Edge, Report},
};

pub const WORDS: usize = 51;
const MASK: u64 = (1 << 36) - 1;
pub fn profile() -> Config {
    Config {
        width: 32,
        height: 32,
        field_bits: Some(36),
        interpolation: Interpolation::Planes,
        attribute_fraction: 18,
        max_samples: 4096,
        ..Default::default()
    }
}
fn signed(n: i128, bits: u32) -> Result<u64, String> {
    if n < -(1_i128 << (bits - 1)) || n >= 1_i128 << (bits - 1) {
        return Err(format!("signed{bits} field overflow: {n}"));
    }
    Ok((n as u128 & ((1_u128 << bits) - 1)) as u64)
}
fn extend(n: u64, bits: u32) -> i64 {
    ((n << (64 - bits)) as i64) >> (64 - bits)
}
fn exponent(v: f64, low: i32, high: i32) -> Result<i32, String> {
    if !v.is_finite() || v <= 0.0 {
        return Err("nonfinite/nonpositive power-of-two scale".into());
    }
    let e = v.log2();
    if e != e.round() || e < f64::from(low) || e > f64::from(high) {
        return Err("power-of-two exponent range".into());
    }
    let e = e as i32;
    if 2.0_f64.powi(e).to_bits() != v.to_bits() {
        return Err("inexact power-of-two representation".into());
    }
    Ok(e)
}
#[derive(Clone, Debug)]
pub struct Encoded(pub [u64; WORDS]);
impl Encoded {
    /// Producer-only oracle encoding; not part of the consumer data path.
    pub fn from_report(r: &Report, fan: usize) -> Result<Self, String> {
        let c = r.config;
        if c.width != 32
            || c.height != 32
            || c.field_bits != Some(36)
            || c.field_origin != FieldOrigin::Local
            || c.interpolation != Interpolation::Planes
            || c.attribute_fraction != 18
            || c.subpixel_bits != 4
            || c.rgb_affine
        {
            return Err("unsupported record fixture profile".into());
        }
        let t = r.triangles.get(fan).ok_or("fan index")?;
        if t.flip {
            return Err("fixture requires front-facing fan".into());
        }
        let s = r.source.as_ref().ok_or("missing oracle source")?;
        if s.cache.coefficients.len() != 9 {
            return Err("nine fields required".into());
        }
        let mut w = [0_u64; WORDS];
        let [x0, y0, x1, y1] = t.bbox;
        if x0 > x1 || y0 > y1 || x1 >= 32 || y1 >= 32 {
            return Err("fixture bbox".into());
        }
        w[0] = u64::from(x0) | (u64::from(y0) << 9) | (u64::from(x1) << 17) | (u64::from(y1) << 26);
        w[1] = signed(i128::from(t.origin[0]), 18)? | (signed(i128::from(t.origin[1]), 18)? << 18);
        w[2] = 1 | (4 << 4);
        for (i, edge) in t.edges.iter().enumerate() {
            w[2] |= u64::from(edge.top_left) << (9 + i);
            w[5 + 2 * i] = signed(edge.a, 18)? | (signed(edge.b, 18)? << 18);
            w[6 + 2 * i] = signed(edge.c, 36)?;
        }
        let anchor: [u64; 2] = std::array::from_fn(|i| (s.cache.anchor[i] * 2.0) as u64);
        if anchor[0] >= 1 << 10
            || anchor[1] >= 1 << 9
            || (0..2).any(|i| anchor[i] as f64 / 2.0 != s.cache.anchor[i])
        {
            return Err("noncanonical doubled local anchor".into());
        }
        let re = exponent(s.cache.radius, 0, 8)?;
        let se = exponent(s.cache.scale, -1022, 1023)?;
        w[3] = anchor[0]
            | (anchor[1] << 10)
            | ((re as u64) << 19)
            | (signed(i128::from(se), 12)? << 23);
        w[4] = 32 | (32 << 9) | (36 << 17) | (18 << 23) | (1 << 28);
        for (f, field) in s.cache.coefficients.iter().enumerate() {
            for (i, &v) in field.iter().enumerate() {
                let m = v / s.cache.scale * 2.0_f64.powi(34);
                if !m.is_finite() || m != m.round() || m.abs() >= 2.0_f64.powi(35) {
                    return Err("mantissa noninteger/overflow".into());
                }
                let word = signed(m as i128, 36)?;
                let decoded = extend(word, 36) as f64 * s.cache.scale / 2.0_f64.powi(34);
                if decoded.to_bits() != v.to_bits() {
                    return Err(format!("coefficient bit roundtrip failure {f}/{i}"));
                }
                w[11 + f * 3 + i] = word;
            }
        }
        let det = s.determinant as u128;
        for i in 0..4 {
            w[38 + i] = ((det >> (36 * i)) & u128::from(MASK)) as u64;
        }
        for (i, v) in r.input.vertices.iter().enumerate() {
            if v.uv.iter().any(|&uv| uv > 4095) {
                return Err("source attribute UV width".into());
            }
            let p = u128::from(v.normal[0] as u16)
                | (u128::from(v.normal[1] as u16) << 16)
                | (u128::from(v.normal[2] as u16) << 32)
                | (u128::from(v.uv[0]) << 48)
                | (u128::from(v.uv[1]) << 60)
                | (u128::from(v.rgb565) << 72);
            for j in 0..3 {
                w[42 + 3 * i + j] = ((p >> (36 * j)) & u128::from(MASK)) as u64;
            }
        }
        let d = Decoded::from_words(&w)?;
        for (decoded, original) in d
            .coefficients
            .iter()
            .flatten()
            .zip(s.cache.coefficients.iter().flatten())
        {
            if decoded.to_bits() != original.to_bits() {
                return Err("decoded coefficient identity".into());
            }
        }
        if d.constants != s.constant_channels || d.determinant != s.determinant {
            return Err("constant bypass/determinant identity".into());
        }
        Ok(Self(w))
    }
}

#[derive(Debug)]
pub struct Decoded {
    pub bbox: [u16; 4],
    pub origin: [i64; 2],
    pub edges: [Edge; 3],
    pub anchor: [f64; 2],
    pub radius: f64,
    pub coefficients: [[f64; 3]; 9],
    pub determinant: i128,
    pub constants: [Option<f64>; 8],
    pub confirmation: u64,
}
impl Decoded {
    pub fn from_words(w: &[u64; WORDS]) -> Result<Self, String> {
        if w.iter().any(|v| v >> 36 != 0)
            || w[0] >> 34 != 0
            || w[2] >> 12 != 0
            || w[3] >> 35 != 0
            || w[41] >> 20 != 0
            || [44, 47, 50].iter().any(|&i| w[i] >> 16 != 0)
        {
            return Err("noncanonical record spare bits".into());
        }
        if w[2] & 0x1ff != 0x41 || w[4] != 32 | (32 << 9) | (36 << 17) | (18 << 23) | (1 << 28) {
            return Err("record version/profile/flip".into());
        }
        let bbox = [
            (w[0] & 511) as u16,
            ((w[0] >> 9) & 255) as u16,
            ((w[0] >> 17) & 511) as u16,
            ((w[0] >> 26) & 255) as u16,
        ];
        if bbox[0] > bbox[2] || bbox[1] > bbox[3] || bbox[2] >= 32 || bbox[3] >= 32 {
            return Err("decoded bbox".into());
        }
        let re = ((w[3] >> 19) & 15) as i32;
        let se = extend((w[3] >> 23) & 4095, 12) as i32;
        if re > 8 || !(-1022..=1023).contains(&se) {
            return Err("decoded exponent range".into());
        }
        let scale = 2.0_f64.powi(se);
        let coefficients = std::array::from_fn(|f| {
            std::array::from_fn(|i| extend(w[11 + f * 3 + i], 36) as f64 * scale / 2.0_f64.powi(34))
        });
        if coefficients.iter().flatten().any(|v| !v.is_finite()) {
            return Err("decoded coefficient overflow".into());
        }
        let det = (0..4).fold(0_u128, |p, i| p | (u128::from(w[38 + i]) << (i * 36))) as i128;
        if det == 0 {
            return Err("zero determinant".into());
        }
        let attrs: [[f64; 8]; 3] = std::array::from_fn(|i| {
            let p = (0..3).fold(0_u128, |p, j| {
                p | (u128::from(w[42 + i * 3 + j]) << (j * 36))
            });
            let rgb = (p >> 72) as u16;
            [
                ((p >> 48) & 4095) as f64 / 4095.0,
                ((p >> 60) & 4095) as f64 / 4095.0,
                f64::from(rgb >> 11) / 31.0,
                f64::from((rgb >> 5) & 63) / 63.0,
                f64::from(rgb & 31) / 31.0,
                f64::from(p as u16 as i16) / 16384.0,
                f64::from((p >> 16) as u16 as i16) / 16384.0,
                f64::from((p >> 32) as u16 as i16) / 16384.0,
            ]
        });
        Ok(Self {
            bbox,
            origin: [extend(w[1] & ((1 << 18) - 1), 18), extend(w[1] >> 18, 18)],
            edges: std::array::from_fn(|i| Edge {
                a: i128::from(extend(w[5 + i * 2] & ((1 << 18) - 1), 18)),
                b: i128::from(extend(w[5 + i * 2] >> 18, 18)),
                c: i128::from(extend(w[6 + i * 2], 36)),
                top_left: w[2] >> (9 + i) & 1 != 0,
            }),
            anchor: [
                (w[3] & 1023) as f64 / 2.0,
                ((w[3] >> 10) & 511) as f64 / 2.0,
            ],
            radius: 2.0_f64.powi(re),
            coefficients,
            determinant: det,
            constants: std::array::from_fn(|k| {
                (attrs[0][k] == attrs[1][k] && attrs[0][k] == attrs[2][k]).then_some(attrs[0][k])
            }),
            confirmation: w[50],
        })
    }
    /// Atomic host point query of the returned nine fields, not arithmetic emu.
    pub fn evaluate(&self, x: f64, y: f64) -> Result<Values, String> {
        if [x, y].iter().any(|v| !v.is_finite() || v.abs() > 4096.0) {
            return Err("helper position range".into());
        }
        let p = [
            (x - self.anchor[0]) / self.radius,
            (y - self.anchor[1]) / self.radius,
            1.0,
        ];
        let v: [f64; 9] = self.coefficients.map(|f| (0..3).map(|i| f[i] * p[i]).sum());
        let inv = 1.0 / v[0];
        let a = std::array::from_fn(|i| self.constants[i].unwrap_or(v[i + 1] * inv));
        let w = self.determinant as f64 * inv / 65536.0;
        if !w.is_finite() || w <= 0.0 || a.iter().any(|v| !v.is_finite()) {
            return Err("invalid helper W/attributes".into());
        }
        Ok(Values { a, w })
    }
}
#[derive(Debug)]
pub struct Values {
    pub a: [f64; 8],
    pub w: f64,
}
impl Values {
    pub fn uv(&self) -> Result<[f64; 2], String> {
        let mut result = [0.0; 2];
        for (i, value) in result.iter_mut().enumerate() {
            let raw = (self.a[i] * 262144.0).round_ties_even();
            if !raw.is_finite()
                || raw.abs() > (1_u64 << 38) as f64
                || raw < -(1_i64 << 39) as f64
                || raw >= (1_i64 << 39) as f64
            {
                return Err("helper UV ingress overflow".into());
            }
            *value = raw / 262144.0;
        }
        Ok(result)
    }
    pub fn normal(&self) -> Result<[i16; 3], String> {
        let mut result = [0_i16; 3];
        for (i, value) in result.iter_mut().enumerate() {
            let raw = (self.a[i + 5] * 16384.0).round_ties_even();
            if !raw.is_finite() || raw < f64::from(i32::MIN) || raw > f64::from(i32::MAX) {
                return Err("normal i32 overflow".into());
            }
            *value = i16::try_from(raw as i32).map_err(|_| "normal i16 checked narrowing")?;
        }
        Ok(result)
    }
    pub fn tint(&self) -> [u8; 3] {
        std::array::from_fn(|i| (self.a[i + 2].clamp(0.0, 1.0) * 255.0).round_ties_even() as u8)
    }
    pub fn depth(&self, near: i32, far: i32) -> Result<u16, String> {
        if near <= 0 || far <= near {
            return Err("depth context".into());
        }
        Ok(
            (((self.w * 65536.0 - f64::from(near)) / f64::from(far - near)).clamp(0.0, 1.0)
                * 65535.0)
                .round_ties_even() as u16,
        )
    }
}
