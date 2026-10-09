#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedVertex(pub [u32; 3]);
impl PackedVertex {
    pub fn encode(
        xyz: [u16; 3],
        normal: [i8; 3],
        uv: [u16; 2],
        rgb565: u16,
    ) -> Result<Self, String> {
        if xyz.iter().any(|&x| x > 1023) || uv.iter().any(|&x| x > 4095) {
            return Err("v6 coordinate/UV width".into());
        }
        let mut bits = 0_u128;
        for i in 0..3 {
            bits |= u128::from(xyz[i]) << (2 + i * 10);
            bits |= u128::from(normal[i] as u8) << (32 + i * 8);
        }
        bits |= u128::from(uv[0]) << 56;
        bits |= u128::from(uv[1]) << 68;
        bits |= u128::from(rgb565) << 80;
        Ok(Self([
            bits as u32,
            (bits >> 32) as u32,
            (bits >> 64) as u32,
        ]))
    }
    pub fn bits(self) -> u128 {
        u128::from(self.0[0]) | (u128::from(self.0[1]) << 32) | (u128::from(self.0[2]) << 64)
    }
}
#[derive(Clone, Debug)]
pub struct Context {
    pub mvp: [[i32; 4]; 4],
    pub normal_matrix: [[i16; 3]; 3],
    pub base: [i32; 3],
    /// Raw Q16.16 position spacing is exactly 1 << grid_shift.
    pub grid_shift: u8,
}
impl Default for Context {
    fn default() -> Self {
        let mut mvp = [[0; 4]; 4];
        for (i, row) in mvp.iter_mut().enumerate() {
            row[i] = 65536;
        }
        Self {
            mvp,
            normal_matrix: [[16384, 0, 0], [0, 16384, 0], [0, 0, 16384]],
            base: [0; 3],
            grid_shift: 6,
        }
    }
}
impl Context {
    pub fn validate(&self) -> Result<(), String> {
        if self.grid_shift > 30 {
            return Err("grid shift > 30".into());
        }
        // Driver preflight on the actual quantized matrix, M^T M ~= I.
        for a in 0..3 {
            for b in 0..3 {
                let gram: i64 = (0..3)
                    .map(|k| {
                        i64::from(self.normal_matrix[k][a]) * i64::from(self.normal_matrix[k][b])
                    })
                    .sum();
                let expected = if a == b { 1_i64 << 28 } else { 0 };
                if (gram - expected).abs() > 32768 {
                    return Err("normal matrix violates quantized orthogonality".into());
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transformed {
    pub clip: [i32; 4],
    pub normal: [i16; 3],
    pub uv: [u16; 2],
    pub rgb565: u16,
}
impl Transformed {
    /// Seven 36-bit rows; unused upper bits are zero, signed fields are raw bits.
    pub fn rows(&self) -> [u64; 7] {
        [
            self.clip[0] as u32 as u64,
            self.clip[1] as u32 as u64,
            self.clip[2] as u32 as u64,
            self.clip[3] as u32 as u64,
            self.normal[0] as u16 as u64 | ((self.normal[1] as u16 as u64) << 16),
            self.normal[2] as u16 as u64 | (u64::from(self.uv[0]) << 16),
            u64::from(self.uv[1]) | (u64::from(self.rgb565) << 12),
        ]
    }
}
