//! Source-world camera/model preparation. Native reflection occurs at draw only.
use crate::dsl_rt::*;
mod gpu_schema;
use gpu_schema::*;
mod gpu_scene_objects;
use gpu_scene_objects::*;
mod gpu_scene_math;
use gpu_scene_math::*;
mod gpu_depth;
use gpu_depth::*;

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
pub struct Transform {
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
    depth_shift: u16,
}
impl Projection {
    pub fn focal_pixels(&self) -> fix32 {
        self.focal_pixels
    }
    pub fn depth_shift(&self) -> u16 {
        self.depth_shift
    }
    pub fn set_depth_shift(&mut self, shift: u16) -> bool {
        if !gpu_depth_valid(self.far.lo_bits(), self.far.hi_bits(), shift) {
            return false;
        }
        self.depth_shift = shift;
        true
    }
}
#[derive(Copy, Clone)]
pub struct CameraDesc {
    pub eye: Vec3,
    pub angles: Vec3,
    pub projection: Projection,
}

pub fn gpu_scene_projection_focal(mut out: Array<u16>, focal: fix32, far: fix32) -> bool {
    // Below 100 pixels |ray_x| > 1, outside native Q14's narrower admissible
    // interval. Reject before scaling to Q14, which could overflow fix32.
    if focal < fix32::from_int(100) || far <= fix32::zero() || far >= fix32::from_int(200) {
        return false;
    }
    let inverse = frcp(focal * fix32::from_words(256, 0));
    let scale = fix32::from_int(16384);
    let ray_x = (-fix32::from_words(25600, 0) * inverse * scale)
        .round()
        .to_int();
    let ray_y = (-fix32::from_words(15360, 0) * inverse * scale)
        .round()
        .to_int();
    if ray_x < -12288 || ray_x > 12288 || ray_y < -12288 || ray_y > 12288 {
        return false;
    }
    gpu_scene_fix_write(out, 0, focal);
    gpu_scene_fix_write(out, 2, far);
    out[4u16] = ray_x as u16;
    out[5u16] = ray_y as u16;
    true
}

pub fn gpu_scene_projection_value(input: Array<u16>) -> Projection {
    Projection {
        focal_pixels: gpu_scene_fix_read(input, 0),
        far: gpu_scene_fix_read(input, 2),
        ray_x: input[4u16],
        ray_y: input[5u16],
        depth_shift: 0,
    }
}

pub fn gpu_scene_projection_fov(
    out: Array<u16>,
    angle: fix32,
    far: fix32,
    horizontal: bool,
) -> bool {
    if angle <= fix32::zero() || angle >= fix32::from_words(9279, 3) {
        return false;
    }
    let pair = fsincos(angle * fix32::from_words(32768, 0));
    let mut pixels = fix32::from_int(120);
    if horizontal {
        pixels = fix32::from_int(200);
    }
    let numerator = pixels * pair.y();
    let max_focal = fix32::from_words(65535, 32767);
    if pair.x() <= fix32::zero() || numerator <= fix32::zero() || numerator > max_focal * pair.x() {
        return false;
    }
    gpu_scene_projection_focal(out, numerator * frcp(pair.x()), far)
}

fn gpu_scene_row_write(out: Array<u16>, at: u16, row: Vec4) {
    gpu_scene_fix_write(out, at, row.x);
    gpu_scene_fix_write(out, at + 2, row.y);
    gpu_scene_fix_write(out, at + 4, row.z);
    gpu_scene_fix_write(out, at + 6, row.w);
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
    let zero = fix32::zero();
    if angles.x == zero && angles.y == zero && angles.z == zero {
        gpu_scene_zero(out, 18);
        gpu_scene_fix_write(out, 0, fix32::from_int(1));
        gpu_scene_fix_write(out, 8, fix32::from_int(1));
        gpu_scene_fix_write(out, 16, fix32::from_int(1));
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
    gpu_scene_fix_write(out, 0, cz * cy);
    gpu_scene_fix_write(out, 2, cz * sy * sx - sz * cx);
    gpu_scene_fix_write(out, 4, cz * sy * cx + sz * sx);
    gpu_scene_fix_write(out, 6, sz * cy);
    gpu_scene_fix_write(out, 8, sz * sy * sx + cz * cx);
    gpu_scene_fix_write(out, 10, sz * sy * cx - cz * sx);
    gpu_scene_fix_write(out, 12, -sy);
    gpu_scene_fix_write(out, 14, cy * sx);
    gpu_scene_fix_write(out, 16, cy * cx);
}

pub fn gpu_scene_camera_projection(mut out: Array<u16>, projection: Projection) -> bool {
    if !gpu_depth_valid(
        projection.far.lo_bits(),
        projection.far.hi_bits(),
        projection.depth_shift,
    ) {
        return false;
    }
    gpu_scene_fix_write(out, GPU_CAMERA_FOCAL, projection.focal_pixels);
    gpu_scene_fix_write(out, GPU_CAMERA_FAR, projection.far);
    out[GPU_CAMERA_RAY] = projection.ray_x;
    out[GPU_CAMERA_RAY + 1] = projection.ray_y;
    out[GPU_CAMERA_RAY + 2] = 8192;
    out[GPU_CAMERA_DEPTH_SHIFT] = projection.depth_shift;
    true
}

pub fn gpu_scene_camera_pose(out: Array<u16>, eye: Vec3, angles: Vec3) -> bool {
    let mut rotation: Buf<u16, 18> = Buf::new([0; 18]);
    gpu_scene_rotation(rotation.as_array(), angles);
    let origin = vec3::new(eye.x, eye.y, eye.z);
    let mut row: u16 = 0;
    while row < 3 {
        let basis = vec3::new(
            gpu_scene_fix_read(rotation.as_array(), row * 2),
            gpu_scene_fix_read(rotation.as_array(), 6 + row * 2),
            gpu_scene_fix_read(rotation.as_array(), 12 + row * 2),
        );
        gpu_scene_vec_write(out, GPU_CAMERA_VIEW + row * 8, basis);
        gpu_scene_fix_write(out, GPU_CAMERA_VIEW + row * 8 + 6, -fdot(basis, origin));
        row += 1;
    }
    true
}

pub fn gpu_scene_camera_view(out: Array<u16>, view: Mat3x4) -> bool {
    gpu_scene_row_write(out, GPU_CAMERA_VIEW, view.row0);
    gpu_scene_row_write(out, GPU_CAMERA_VIEW + 8, view.row1);
    gpu_scene_row_write(out, GPU_CAMERA_VIEW + 16, view.row2);
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

pub fn gpu_scene_model_init(mut out: Array<u16>, transform: Transform) -> bool {
    if transform.scale.x == fix32::zero()
        || transform.scale.y == fix32::zero()
        || transform.scale.z == fix32::zero()
    {
        return false;
    }
    let mut rotation: Buf<u16, 18> = Buf::new([0; 18]);
    gpu_scene_rotation(rotation.as_array(), transform.angles);
    let ix = frcp(transform.scale.x);
    let iy = frcp(transform.scale.y);
    let iz = frcp(transform.scale.z);
    let mut row: u16 = 0;
    while row < 3 {
        let mut col: u16 = 0;
        while col < 3 {
            let mut scale = transform.scale.x;
            let mut inverse = ix;
            if col == 1 {
                scale = transform.scale.y;
                inverse = iy;
            }
            if col == 2 {
                scale = transform.scale.z;
                inverse = iz;
            }
            let value = gpu_scene_fix_read(rotation.as_array(), (row * 3 + col) * 2);
            gpu_scene_fix_write(out, GPU_MODEL_WORLD + row * 8 + col * 2, value * scale);
            gpu_scene_fix_write(out, GPU_MODEL_NORMAL + (row * 3 + col) * 2, value * inverse);
            col += 1;
        }
        let mut translation = transform.position.x;
        if row == 1 {
            translation = transform.position.y;
        }
        if row == 2 {
            translation = transform.position.z;
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

pub fn gpu_scene_model_matrices(mut out: Array<u16>, world: Mat3x4, normal: Mat3x3) -> bool {
    gpu_scene_row_write(out, GPU_MODEL_WORLD, world.row0);
    gpu_scene_row_write(out, GPU_MODEL_WORLD + 8, world.row1);
    gpu_scene_row_write(out, GPU_MODEL_WORLD + 16, world.row2);
    gpu_scene_vec_write(
        out,
        GPU_MODEL_NORMAL,
        vec3::new(normal.row0.x, normal.row0.y, normal.row0.z),
    );
    gpu_scene_vec_write(
        out,
        GPU_MODEL_NORMAL + 6,
        vec3::new(normal.row1.x, normal.row1.y, normal.row1.z),
    );
    gpu_scene_vec_write(
        out,
        GPU_MODEL_NORMAL + 12,
        vec3::new(normal.row2.x, normal.row2.y, normal.row2.z),
    );
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
