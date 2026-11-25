//! Source-world camera/model preparation. Native reflection occurs at draw only.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;
mod gpu_scene_math;
use gpu_scene_math::*;

#[derive(Copy, Clone)]
pub struct Vec4 {
    pub x: fix32,
    pub y: fix32,
    pub z: fix32,
    pub w: fix32,
}
#[derive(Copy, Clone)]
pub struct Mat3x4 {
    pub row0: Vec4,
    pub row1: Vec4,
    pub row2: Vec4,
}
#[derive(Copy, Clone)]
pub struct Mat3x3 {
    pub row0: Vec3,
    pub row1: Vec3,
    pub row2: Vec3,
}
#[derive(Copy, Clone)]
pub struct Pose {
    pub position: Vec3,
    pub angles: Vec3,
    pub scale: Vec3,
}
#[derive(Copy, Clone)]
pub struct Projection {
    focal_pixels: fix32,
    far: fix32,
    ray_x: u16,
    ray_y: u16,
}
impl Projection {
    pub const fn focal_pixels(self) -> fix32 {
        self.focal_pixels
    }
}
#[derive(Copy, Clone)]
pub struct CameraDesc {
    pub eye: Vec3,
    pub angles: Vec3,
    pub projection: Projection,
}

#[derive(Copy, Clone)]
struct ProjectionResult {
    valid: bool,
    value: Projection,
}
const fn gpu_scene_projection_invalid(focal: fix32, far: fix32) -> ProjectionResult {
    ProjectionResult {
        valid: false,
        value: Projection {
            focal_pixels: focal,
            far,
            ray_x: 0,
            ray_y: 0,
        },
    }
}
const fn gpu_scene_projection_focal_desc(focal: fix32, far: fix32) -> ProjectionResult {
    if focal.less_than(fix32::from_int(100))
        || !far.greater_than(fix32::zero())
        || !far.less_than(fix32::from_int(200))
    {
        gpu_scene_projection_invalid(focal, far)
    } else {
        let inverse = frcp(focal.wrapping_mul(fix32::from_words(256, 0)));
        let scale = fix32::from_int(16384);
        let ray_x = fix32::from_words(25600, 0)
            .wrapping_neg()
            .wrapping_mul(inverse)
            .wrapping_mul(scale)
            .round()
            .to_int();
        let ray_y = fix32::from_words(15360, 0)
            .wrapping_neg()
            .wrapping_mul(inverse)
            .wrapping_mul(scale)
            .round()
            .to_int();
        ProjectionResult {
            valid: ray_x >= -12288 && ray_x <= 12288 && ray_y >= -12288 && ray_y <= 12288,
            value: Projection {
                focal_pixels: focal,
                far,
                ray_x: ray_x as u16,
                ray_y: ray_y as u16,
            },
        }
    }
}
const fn gpu_scene_projection_fov_desc(
    angle: fix32,
    far: fix32,
    horizontal: bool,
) -> ProjectionResult {
    if !angle.greater_than(fix32::zero()) || !angle.less_than(fix32::from_words(9279, 3)) {
        gpu_scene_projection_invalid(fix32::zero(), far)
    } else {
        let pair = fsincos(angle.wrapping_mul(fix32::from_words(32768, 0)));
        let pixels = if horizontal {
            fix32::from_int(200)
        } else {
            fix32::from_int(120)
        };
        let numerator = pixels.wrapping_mul(pair.y());
        if !pair.x().greater_than(fix32::zero())
            || !numerator.greater_than(fix32::zero())
            || numerator.greater_than(fix32::from_words(65535, 32767).wrapping_mul(pair.x()))
        {
            gpu_scene_projection_invalid(fix32::zero(), far)
        } else {
            gpu_scene_projection_focal_desc(numerator.wrapping_mul(frcp(pair.x())), far)
        }
    }
}
fn gpu_scene_projection_write(mut out: Array<u16>, result: ProjectionResult) -> bool {
    if !result.valid {
        return false;
    }
    gpu_scene_fix_write(out, 0, result.value.focal_pixels);
    gpu_scene_fix_write(out, 2, result.value.far);
    out[4u16] = result.value.ray_x;
    out[5u16] = result.value.ray_y;
    true
}
pub fn gpu_scene_projection_focal(out: Array<u16>, focal: fix32, far: fix32) -> bool {
    gpu_scene_projection_write(out, gpu_scene_projection_focal_desc(focal, far))
}
pub fn gpu_scene_projection_value(input: Array<u16>) -> Projection {
    Projection {
        focal_pixels: gpu_scene_fix_read(input, 0),
        far: gpu_scene_fix_read(input, 2),
        ray_x: input[4u16],
        ray_y: input[5u16],
    }
}
pub fn gpu_scene_projection_fov(
    out: Array<u16>,
    angle: fix32,
    far: fix32,
    horizontal: bool,
) -> bool {
    gpu_scene_projection_write(out, gpu_scene_projection_fov_desc(angle, far, horizontal))
}

// Source matrices are borrowed CPU snapshots. Import/export preserves their
// exact Q16.16 words and avoids by-value row temporaries between private helpers.
fn gpu_scene_matrix_write(out: Array<u16>, at: u16, input: Array<Mat3x4>) {
    let source = input.as_ptr();
    let target = out.as_ptr().add(at as i16);
    vec4::export(vec4::import(source), target);
    vec4::export(vec4::import(source.add(8)), target.add(8));
    vec4::export(vec4::import(source.add(16)), target.add(16));
}
fn gpu_scene_row_read(data: Array<u16>, at: u16) -> Vec4 {
    Vec4 {
        x: gpu_scene_fix_read(data, at),
        y: gpu_scene_fix_read(data, at + 2),
        z: gpu_scene_fix_read(data, at + 4),
        w: gpu_scene_fix_read(data, at + 6),
    }
}
fn gpu_scene_rotation(out: Array<u16>, angles: Vec3) {
    let target = out.as_ptr();
    let zero = fix32::zero();
    if angles.x == zero && angles.y == zero && angles.z == zero {
        let one = fix32::from_int(1);
        vec4::export(vec4::new(one, zero, zero, zero), target);
        vec4::export(vec4::new(zero, one, zero, zero), target.add(8));
        vec4::export(vec4::new(zero, zero, one, zero), target.add(16));
        return;
    }
    let x = fsincos(angles.x);
    let y = fsincos(angles.y);
    let z = fsincos(angles.z);
    let sx = x.x();
    let cx = x.y();
    let sy = y.x();
    let cy = y.y();
    let sz = z.x();
    let cz = z.y();
    let cz_sy = cz * sy;
    let sz_sy = sz * sy;
    vec4::export(
        vec4::new(cz * cy, cz_sy * sx - sz * cx, cz_sy * cx + sz * sx, zero),
        target,
    );
    vec4::export(
        vec4::new(sz * cy, sz_sy * sx + cz * cx, sz_sy * cx - cz * sx, zero),
        target.add(8),
    );
    vec4::export(vec4::new(-sy, cy * sx, cy * cx, zero), target.add(16));
}

pub fn gpu_scene_camera_projection(mut out: Array<u16>, projection: Projection) -> bool {
    gpu_scene_fix_write(out, GPU_CAMERA_FOCAL, projection.focal_pixels);
    gpu_scene_fix_write(out, GPU_CAMERA_FAR, projection.far);
    out[GPU_CAMERA_RAY] = projection.ray_x;
    out[GPU_CAMERA_RAY + 1] = projection.ray_y;
    out[GPU_CAMERA_RAY + 2] = 8192;
    true
}

pub fn gpu_scene_camera_pose(out: Array<u16>, eye: Vec3, angles: Vec3) -> bool {
    let rotation: Buf<u16, 24> = Buf::new([0; 24]);
    gpu_scene_rotation(rotation.as_array(), angles);
    let origin = vec3::new(eye.x, eye.y, eye.z);
    let mut row: u16 = 0;
    while row < 3 {
        let basis = vec3::new(
            gpu_scene_fix_read(rotation.as_array(), row * 2),
            gpu_scene_fix_read(rotation.as_array(), 8 + row * 2),
            gpu_scene_fix_read(rotation.as_array(), 16 + row * 2),
        );
        gpu_scene_vec_write(out, GPU_CAMERA_VIEW + row * 8, basis);
        gpu_scene_fix_write(out, GPU_CAMERA_VIEW + row * 8 + 6, -fdot(basis, origin));
        row += 1;
    }
    true
}

pub fn gpu_scene_camera_view(out: Array<u16>, view: Array<Mat3x4>) -> bool {
    gpu_scene_matrix_write(out, GPU_CAMERA_VIEW, view);
    true
}

pub fn gpu_scene_camera_get_view(input: Array<u16>) -> Mat3x4 {
    Mat3x4 {
        row0: gpu_scene_row_read(input, GPU_CAMERA_VIEW),
        row1: gpu_scene_row_read(input, GPU_CAMERA_VIEW + 8),
        row2: gpu_scene_row_read(input, GPU_CAMERA_VIEW + 16),
    }
}

pub fn gpu_scene_camera_init(mut out: Array<u16>, desc: CameraDesc) -> bool {
    gpu_scene_camera_projection(out, desc.projection);
    gpu_scene_camera_pose(out, desc.eye, desc.angles);
    let mut at: u16 = 31;
    while at < GPU_API_LARGE_WORDS {
        out[at] = 0;
        at += 1;
    }
    true
}

pub fn gpu_scene_camera_look_to(out: Array<u16>, eye: Vec3, direction: Vec3, up: Vec3) -> bool {
    let backward = -gpu_scene_normalize(vec3::new(direction.x, direction.y, direction.z));
    let unit_up = gpu_scene_normalize(vec3::new(up.x, up.y, up.z));
    let right = gpu_scene_normalize(gpu_scene_cross(unit_up, backward));
    if fdot(backward, backward) == fix32::zero() || fdot(right, right) == fix32::zero() {
        return false;
    }
    let vertical = gpu_scene_cross(backward, right);
    let origin = vec3::new(eye.x, eye.y, eye.z);
    gpu_scene_vec_write(out, GPU_CAMERA_VIEW, right);
    gpu_scene_fix_write(out, GPU_CAMERA_VIEW + 6, -fdot(right, origin));
    gpu_scene_vec_write(out, GPU_CAMERA_VIEW + 8, vertical);
    gpu_scene_fix_write(out, GPU_CAMERA_VIEW + 14, -fdot(vertical, origin));
    gpu_scene_vec_write(out, GPU_CAMERA_VIEW + 16, backward);
    gpu_scene_fix_write(out, GPU_CAMERA_VIEW + 22, -fdot(backward, origin));
    true
}

pub fn gpu_scene_camera_look_at(out: Array<u16>, eye: Vec3, target: Vec3, up: Vec3) -> bool {
    let direction: Vec3 = Vec3 {
        x: target.x - eye.x,
        y: target.y - eye.y,
        z: target.z - eye.z,
    };
    gpu_scene_camera_look_to(out, eye, direction, up)
}

pub fn gpu_scene_camera_look_at_y_up(out: Array<u16>, eye: Vec3, target: Vec3) -> bool {
    let up: Vec3 = Vec3 {
        x: fix32::zero(),
        y: fix32::from_int(1),
        z: fix32::zero(),
    };
    gpu_scene_camera_look_at(out, eye, target, up)
}

pub fn gpu_scene_camera_init_look_at(
    mut out: Array<u16>,
    eye: Vec3,
    target: Vec3,
    up: Vec3,
    projection: Projection,
) -> bool {
    if !gpu_scene_camera_look_at(out, eye, target, up) {
        return false;
    }
    gpu_scene_camera_projection(out, projection);
    let mut at: u16 = 31;
    while at < GPU_API_LARGE_WORDS {
        out[at] = 0;
        at += 1;
    }
    true
}

pub fn gpu_scene_model_init(mut out: Array<u16>, pose: Pose) -> bool {
    if pose.scale.x == fix32::zero()
        || pose.scale.y == fix32::zero()
        || pose.scale.z == fix32::zero()
    {
        return false;
    }
    let rotation: Buf<u16, 24> = Buf::new([0; 24]);
    gpu_scene_rotation(rotation.as_array(), pose.angles);
    let ix = frcp(pose.scale.x);
    let iy = frcp(pose.scale.y);
    let iz = frcp(pose.scale.z);
    let mut row: u16 = 0;
    while row < 3 {
        let mut col: u16 = 0;
        while col < 3 {
            let mut scale = pose.scale.x;
            let mut inverse = ix;
            if col == 1 {
                scale = pose.scale.y;
                inverse = iy;
            }
            if col == 2 {
                scale = pose.scale.z;
                inverse = iz;
            }
            let value = gpu_scene_fix_read(rotation.as_array(), row * 8 + col * 2);
            gpu_scene_fix_write(out, GPU_MODEL_WORLD + row * 8 + col * 2, value * scale);
            gpu_scene_fix_write(out, GPU_MODEL_NORMAL + (row * 3 + col) * 2, value * inverse);
            col += 1;
        }
        let mut translation = pose.position.x;
        if row == 1 {
            translation = pose.position.y;
        }
        if row == 2 {
            translation = pose.position.z;
        }
        gpu_scene_fix_write(out, GPU_MODEL_WORLD + row * 8 + 6, translation);
        row += 1;
    }
    let mut at = GPU_MODEL_WORDS;
    while at < GPU_API_LARGE_WORDS {
        out[at] = 0;
        at += 1;
    }
    true
}

pub fn gpu_scene_model_matrices(
    mut out: Array<u16>,
    world: Array<Mat3x4>,
    normal: Array<Mat3x3>,
) -> bool {
    gpu_scene_matrix_write(out, GPU_MODEL_WORLD, world);
    let source = normal.as_ptr();
    let target = out.as_ptr().add(GPU_MODEL_NORMAL as i16);
    vec4::export(vec4::import(source), target);
    vec4::export(vec4::import(source.add(8)), target.add(8));
    // The normal matrix has exactly eighteen words; do not overread its tail.
    unsafe {
        target.write(16, source.read(16));
        target.write(17, source.read(17));
    }
    let mut at = GPU_MODEL_WORDS;
    while at < GPU_API_LARGE_WORDS {
        out[at] = 0;
        at += 1;
    }
    true
}

pub fn gpu_scene_model_world(input: Array<u16>) -> Mat3x4 {
    Mat3x4 {
        row0: gpu_scene_row_read(input, GPU_MODEL_WORLD),
        row1: gpu_scene_row_read(input, GPU_MODEL_WORLD + 8),
        row2: gpu_scene_row_read(input, GPU_MODEL_WORLD + 16),
    }
}
pub fn gpu_scene_model_normal(input: Array<u16>) -> Mat3x3 {
    let a = gpu_scene_vec_read(input, GPU_MODEL_NORMAL);
    let b = gpu_scene_vec_read(input, GPU_MODEL_NORMAL + 6);
    let c = gpu_scene_vec_read(input, GPU_MODEL_NORMAL + 12);
    Mat3x3 {
        row0: Vec3 {
            x: a.x(),
            y: a.y(),
            z: a.z(),
        },
        row1: Vec3 {
            x: b.x(),
            y: b.y(),
            z: b.z(),
        },
        row2: Vec3 {
            x: c.x(),
            y: c.y(),
            z: c.z(),
        },
    }
}
