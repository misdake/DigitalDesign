//! Pure public value construction, independent of GPU state and submission.
use crate::dsl_rt::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;
mod gpu_scene_transforms;
use gpu_scene_transforms::*;
mod gpu_ui_grid;
use gpu_ui_grid::*;

impl CellRect {
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub const fn full_grid() -> Self {
        Self::new(0, 0, 50, 15)
    }
}

impl Vec3 {
    pub const fn new(x: fix32, y: fix32, z: fix32) -> Self {
        Self { x, y, z }
    }
    pub const fn zero() -> Self {
        Self::splat(fix32::zero())
    }
    pub const fn one() -> Self {
        Self::splat(fix32::one())
    }
    pub const fn splat(value: fix32) -> Self {
        Self::new(value, value, value)
    }
    pub const fn unit_x() -> Self {
        Self::new(fix32::one(), fix32::zero(), fix32::zero())
    }
    pub const fn unit_y() -> Self {
        Self::new(fix32::zero(), fix32::one(), fix32::zero())
    }
    pub const fn unit_z() -> Self {
        Self::new(fix32::zero(), fix32::zero(), fix32::one())
    }
}
impl Vec4 {
    pub const fn new(x: fix32, y: fix32, z: fix32, w: fix32) -> Self {
        Self { x, y, z, w }
    }
    pub const fn zero() -> Self {
        Self::splat(fix32::zero())
    }
    pub const fn splat(value: fix32) -> Self {
        Self::new(value, value, value, value)
    }
}
impl Mat3x3 {
    pub const fn from_rows(row0: Vec3, row1: Vec3, row2: Vec3) -> Self {
        Self { row0, row1, row2 }
    }
    pub const fn identity() -> Self {
        Self::from_rows(Vec3::unit_x(), Vec3::unit_y(), Vec3::unit_z())
    }
}
impl Mat3x4 {
    pub const fn from_rows(row0: Vec4, row1: Vec4, row2: Vec4) -> Self {
        Self { row0, row1, row2 }
    }
    pub const fn identity() -> Self {
        Self::from_rows(
            Vec4::new(fix32::one(), fix32::zero(), fix32::zero(), fix32::zero()),
            Vec4::new(fix32::zero(), fix32::one(), fix32::zero(), fix32::zero()),
            Vec4::new(fix32::zero(), fix32::zero(), fix32::one(), fix32::zero()),
        )
    }
}
impl Pose {
    /// Source-world TRS; angles are radians and normal matrices remain derived.
    pub const fn new(position: Vec3, angles: Vec3, scale: Vec3) -> Self {
        Self {
            position,
            angles,
            scale,
        }
    }
    pub const fn identity() -> Self {
        Self::new(Vec3::zero(), Vec3::zero(), Vec3::one())
    }
    pub const fn translation(position: Vec3) -> Self {
        Self::new(position, Vec3::zero(), Vec3::one())
    }
    pub const fn rotation(angles: Vec3) -> Self {
        Self::new(Vec3::zero(), angles, Vec3::one())
    }
    pub const fn uniform_scale(scale: fix32) -> Self {
        Self::new(Vec3::zero(), Vec3::zero(), Vec3::splat(scale))
    }
    pub const fn with_position(self, position: Vec3) -> Self {
        Self::new(position, self.angles, self.scale)
    }
    pub const fn with_rotation(self, angles: Vec3) -> Self {
        Self::new(self.position, angles, self.scale)
    }
    pub const fn with_scale(self, scale: Vec3) -> Self {
        Self::new(self.position, self.angles, scale)
    }
}
impl CameraDesc {
    pub const fn new(eye: Vec3, angles: Vec3, projection: Projection) -> Self {
        Self {
            eye,
            angles,
            projection,
        }
    }
}
impl Rgb8 {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
    pub const fn gray(value: u8) -> Self {
        Self::new(value, value, value)
    }
    pub const fn black() -> Self {
        Self::gray(0)
    }
    pub const fn white() -> Self {
        Self::gray(255)
    }
    pub const fn with_alpha(self, a: u8) -> Rgba8 {
        Rgba8::new(self.r, self.g, self.b, a)
    }
}
impl Rgba8 {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }
    pub const fn gray(value: u8) -> Self {
        Self::rgb(value, value, value)
    }
    pub const fn white() -> Self {
        Self::gray(255)
    }
    pub const fn transparent() -> Self {
        Self::new(0, 0, 0, 0)
    }
    pub const fn with_alpha(self, a: u8) -> Self {
        Self::new(self.r, self.g, self.b, a)
    }
    /// Native UI R15:11/G10:6/B5:1/A0; alpha uses its high bit.
    pub const fn to_rgba5551(self) -> u16 {
        (((self.r as u16) >> 3) << 11)
            | (((self.g as u16) >> 3) << 6)
            | (((self.b as u16) >> 3) << 1)
            | ((self.a as u16) >> 7)
    }
}
impl MaterialDesc {
    pub const fn unlit(color: Rgba8) -> Self {
        Self {
            uniform_color: color,
            specular: Rgb8::black(),
            ambient_response: fix32::one(),
            directional_response: fix32::zero(),
            shininess: Shininess::S4,
            unlit: true,
        }
    }
    pub const fn diffuse(color: Rgba8) -> Self {
        Self {
            uniform_color: color,
            specular: Rgb8::black(),
            ambient_response: fix32::one(),
            directional_response: fix32::one(),
            shininess: Shininess::S4,
            unlit: false,
        }
    }
    pub const fn lit(color: Rgba8, specular: Rgb8, shininess: Shininess) -> Self {
        Self {
            uniform_color: color,
            specular,
            ambient_response: fix32::one(),
            directional_response: fix32::one(),
            shininess,
            unlit: false,
        }
    }
    pub const fn with_response(self, ambient: fix32, directional: fix32) -> Self {
        Self {
            uniform_color: self.uniform_color,
            specular: self.specular,
            ambient_response: ambient,
            directional_response: directional,
            shininess: self.shininess,
            unlit: self.unlit,
        }
    }
    pub const fn with_unlit(self, unlit: bool) -> Self {
        Self {
            uniform_color: self.uniform_color,
            specular: self.specular,
            ambient_response: self.ambient_response,
            directional_response: self.directional_response,
            shininess: self.shininess,
            unlit,
        }
    }
}
impl Lighting {
    pub const fn ambient(intensity: fix32) -> Self {
        Self {
            direction: Vec3::unit_z(),
            ambient: intensity,
            directional: fix32::zero(),
        }
    }
    pub const fn directional(direction: Vec3, ambient: fix32, intensity: fix32) -> Self {
        Self {
            direction,
            ambient,
            directional: intensity,
        }
    }
}
impl RenderStateDesc {
    /// Common Less/write/Replace state. Sorting and pass ownership stay explicit.
    pub const fn opaque(lighting: Lighting) -> Self {
        Self {
            depth: DepthFunc::Less,
            depth_write: true,
            blend: BlendMode::Replace,
            filter: TextureFilter::Trilinear,
            lighting,
        }
    }
    /// Common source-over state; tests depth but does not write it.
    pub const fn transparent(lighting: Lighting) -> Self {
        Self {
            depth: DepthFunc::Less,
            depth_write: false,
            blend: BlendMode::SrcOver,
            filter: TextureFilter::Trilinear,
            lighting,
        }
    }
    pub const fn with_depth(self, depth: DepthFunc, write: bool) -> Self {
        Self {
            depth,
            depth_write: write,
            blend: self.blend,
            filter: self.filter,
            lighting: self.lighting,
        }
    }
    pub const fn with_filter(self, filter: TextureFilter) -> Self {
        Self {
            depth: self.depth,
            depth_write: self.depth_write,
            blend: self.blend,
            filter,
            lighting: self.lighting,
        }
    }
    pub const fn with_blend(self, blend: BlendMode) -> Self {
        Self {
            depth: self.depth,
            depth_write: self.depth_write,
            blend,
            filter: self.filter,
            lighting: self.lighting,
        }
    }
}
