//! Quantized component ports. These are pixel ports, with no quad ownership.

/// Unnormalized S(16,14) normal and S(18,16) NDC pixel center from raster.
#[derive(Clone, Copy, Debug)]
pub struct PixelInput {
    pub normal: [i16; 3],
    pub ndc: [i32; 2],
}

/// Candidate local buffer layout, independent of external GPU memory/command ABI.
/// Row 0: normal X/Y; row 1: normal Z and NDC X; row 2: NDC Y.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRows(pub [u64; 3]);
impl PixelRows {
    pub fn encode(pixel: PixelInput) -> Result<Self, InputError> {
        if pixel.ndc.iter().any(|&v| !(-65536..=65536).contains(&v)) {
            return Err(InputError::ViewRay);
        }
        Ok(Self([
            u64::from(pixel.normal[0] as u16) | (u64::from(pixel.normal[1] as u16) << 16),
            u64::from(pixel.normal[2] as u16) | (((pixel.ndc[0] as u64) & 0x3ffff) << 16),
            (pixel.ndc[1] as u64) & 0x3ffff,
        ]))
    }
    pub fn decode(self) -> Result<PixelInput, InputError> {
        if self.0[0] >> 32 != 0 || self.0[1] >> 34 != 0 || self.0[2] >> 18 != 0 {
            return Err(InputError::ViewRay);
        }
        let sign18 = |v: u64| ((v as i32) << 14) >> 14;
        let pixel = PixelInput {
            normal: [self.0[0] as i16, (self.0[0] >> 16) as i16, self.0[1] as i16],
            ndc: [sign18(self.0[1] >> 16), sign18(self.0[2])],
        };
        Self::encode(pixel)?;
        Ok(pixel)
    }
}

/// Four candidate 36-bit rows loaded once per uniform batch, then latched.
/// Light XY; light Z/Ia/Id; projection XY; projection K/mode/shininess.
/// Mode is CPU-prepared: 0 unlit, 1 ambient, 2 diffuse, 3 full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UniformRows(pub [u64; 4]);
impl UniformRows {
    pub fn encode(
        material: Material,
        light: Light,
        projection: Projection,
    ) -> Result<Self, InputError> {
        validate(
            PixelInput {
                normal: [0; 3],
                ndc: [0; 2],
            },
            material,
            light,
            projection,
        )?;
        let mode = if material.unlit {
            0
        } else if light.directional == 0 {
            1
        } else if material.specular_color == [0; 3] {
            2
        } else {
            3
        };
        Ok(Self([
            u64::from(light.direction[0] as u16) | (u64::from(light.direction[1] as u16) << 16),
            u64::from(light.direction[2] as u16)
                | (u64::from(light.ambient) << 16)
                | (u64::from(light.directional) << 25),
            u64::from(projection.ray_scale[0] as u16)
                | (u64::from(projection.ray_scale[1] as u16) << 16),
            u64::from(projection.k as u16)
                | (mode << 16)
                | (u64::from(material.shininess_code) << 18),
        ]))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub unlit: bool,
    /// Used only to choose diffuse-only versus full. Color application is final's job.
    pub specular_color: [u8; 3],
    /// 0..16, selecting the generated exponent table. Default 8 means s=16.
    pub shininess_code: u8,
}
impl Default for Material {
    fn default() -> Self {
        Self {
            unlit: false,
            specular_color: [255; 3],
            shininess_code: 8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Light {
    /// Unit view-space vector from surface to light, S(16,14).
    pub direction: [i16; 3],
    /// U(9,8), 0..=256.
    pub ambient: u16,
    pub directional: u16,
}

/// CPU-prepared -k/fx, -k/fy and k, each S(16,14).
#[derive(Clone, Copy, Debug)]
pub struct Projection {
    pub ray_scale: [i16; 2],
    pub k: i16,
}
impl Default for Projection {
    fn default() -> Self {
        Self {
            ray_scale: [-8192; 2],
            k: 8192,
        }
    }
}
impl Default for Light {
    fn default() -> Self {
        Self {
            direction: [0, 0, 16384],
            ambient: 32,
            directional: 224,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightingOutput {
    /// U(9,8), at most 511; 256 represents one.
    pub g: u16,
    /// U(9,8), at most 256.
    pub h: u16,
}

/// Immutable register context. Updates are accepted only after pipeline drain.
#[derive(Clone, Copy, Debug)]
pub struct LightingContext {
    pub material: Material,
    pub light: Light,
    pub projection: Projection,
    pub epoch: u16,
}
impl LightingContext {
    pub fn validate(self) -> Result<(), InputError> {
        validate(
            PixelInput {
                normal: [0; 3],
                ndc: [0; 2],
            },
            self.material,
            self.light,
            self.projection,
        )
    }
    pub fn mode(self) -> u8 {
        if self.material.unlit {
            0
        } else if self.light.directional == 0 {
            1
        } else if self.material.specular_color == [0; 3] {
            2
        } else {
            3
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LightingRequest {
    pub pixel: PixelInput,
    pub id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightingResult {
    pub output: LightingOutput,
    pub id: u32,
    pub epoch: u16,
}

/// Clock inputs. Reset has priority over CE; transfers require CE.
#[derive(Clone, Copy, Debug)]
pub struct LightingTick {
    pub reset: bool,
    pub ce: bool,
    pub context: Option<LightingContext>,
    pub input: Option<LightingRequest>,
    pub output_ready: bool,
}

/// Signals immediately before an edge. Output is held through CE/ready stalls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightingSignals {
    pub context_ready: bool,
    pub input_ready: bool,
    pub output: Option<LightingResult>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    Shininess,
    LightIntensity,
    LightDirection,
    ViewRay,
    Configuration,
}

pub fn validate(
    pixel: PixelInput,
    material: Material,
    light: Light,
    projection: Projection,
) -> Result<(), InputError> {
    if material.shininess_code > 16 {
        return Err(InputError::Shininess);
    }
    if light.ambient > 256 || light.directional > 256 {
        return Err(InputError::LightIntensity);
    }
    let squared: i64 = light.direction.iter().map(|&v| i64::from(v).pow(2)).sum();
    // Quantizing an exact unit vector to Q14 can change squared length by < 32768.
    if (squared - (1_i64 << 28)).abs() > 32768 {
        return Err(InputError::LightDirection);
    }
    if !(8192..=12288).contains(&projection.k)
        || projection
            .ray_scale
            .iter()
            .any(|&v| i32::from(v).abs() > 12288)
        || pixel.ndc.iter().any(|&v| !(-65536..=65536).contains(&v))
    {
        return Err(InputError::ViewRay);
    }
    Ok(())
}
