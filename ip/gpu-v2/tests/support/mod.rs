use gpu_v2::lighting::ports::{Light, Material, PixelInput, Projection};

pub const MAX_EVENTS: usize = 2048;

pub fn representative() -> Vec<(PixelInput, Material, Light, Projection)> {
    let normals = [
        [0, 0, 0],
        [0, 0, 3],
        [0, 0, 4],
        [1, -2, 4],
        [0, 0, 16384],
        [0, 0, -16384],
        [16384, 0, 0],
        [-32768, 0, 0],
        [32767, 32767, 32767],
        [8191, 8192, 8193],
        [63, -64, 65],
        [1, 16383, -1],
        [-8192, -8192, 8192],
    ];
    let positions = [
        [0, 0],
        [-65536, -65536],
        [65536, 65536],
        [-32768, 16384],
        [12345, -54321],
    ];
    let directions = [
        [0, 0, 16384],
        [0, 0, -16384],
        [16384, 0, 0],
        [0, 16384, 0],
        [9459, 9459, 9459],
    ];
    let mut result = Vec::new();
    for normal in normals {
        for ndc in positions {
            for direction in directions {
                for code in [0, 8, 16] {
                    result.push((
                        PixelInput { normal, ndc },
                        Material {
                            shininess_code: code,
                            ..Material::default()
                        },
                        Light {
                            direction,
                            ..Light::default()
                        },
                        Projection::default(),
                    ));
                }
            }
        }
    }
    result
}

pub struct Random(pub u64);
impl Random {
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as u32
    }
    pub fn direction(&mut self) -> [i16; 3] {
        let v = std::array::from_fn::<_, 3, _>(|_| f64::from(self.next() as i32));
        let length = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| (x / length * 16384.0).round_ties_even() as i16)
    }
}
