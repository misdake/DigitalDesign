//! Private CPU scene caches. Camera zero looks along source-world -Z.
//! Native view is diag(-1,1,1)*R_camera^T for position, normal and light.
use crate::dsl_rt::*;
mod gpu_assets;
use gpu_assets::*;
mod gpu_encode;
use gpu_encode::*;
mod boot_resources;
use boot_resources::*;
const SCENE_CAP: u16 = 4;
const SCENE_CAMERA_WORDS: u16 = 80;
const SCENE_MODEL_WORDS: u16 = 116;
static GPU_SCENE_CAMERAS: Buf<u16, 320> = Buf::new([0; 320]);
static GPU_SCENE_MODELS: Buf<u16, 464> = Buf::new([0; 464]);
static GPU_SCENE_VIEW: Buf<u16, 80> = Buf::new([0; 80]);
static GPU_SCENE_VIEW_FLAGS: Buf<u16, 1> = Buf::new([0; 1]);
// Camera rotation/translation/rays/light, model rotation, final position/normal,
// immutable directory validations. Diagnostic counts do not drive invalidation.
static GPU_SCENE_WORK: Buf<u16, 8> = Buf::new([0; 8]);
static GPU_SCENE_MESHES: Buf<u16, 96> = Buf::new([0; 96]);
static GPU_SCENE_MATERIALS: Buf<u16, 64> = Buf::new([0; 64]);
static GPU_SCENE_ENTRIES: Buf<u16, 32> = Buf::new([0; 32]);

fn scene_mesh(slot: u16) -> Array<u16> {
    unsafe {
        GPU_SCENE_MESHES
            .as_ptr()
            .add(((slot - 1) * 24) as i16)
            .as_u16_array()
    }
}
fn scene_material(slot: u16) -> Array<u16> {
    unsafe {
        GPU_SCENE_MATERIALS
            .as_ptr()
            .add(((slot - 1) * 16) as i16)
            .as_u16_array()
    }
}
fn scene_mesh_value(slot: u16) -> GpuMesh {
    let m = scene_mesh(slot);
    GpuMesh {
        directory_sector: m[1u16],
        directory_word: m[2u16],
        count: m[3u16],
        first_sector: m[4u16],
        sectors: m[5u16],
    }
}
fn scene_mesh_bounds(slot: u16) -> Array<u16> {
    unsafe { scene_mesh(slot).as_ptr().add(6).as_u16_array() }
}
fn scene_mesh_new(root: Array<GpuMesh>, minimum: vec3, maximum: vec3) -> u16 {
    if !gpu_mesh_root_valid(root)
        || !scene_bounded(minimum, fix32::from_int(100))
        || !scene_bounded(maximum, fix32::from_int(100))
        || minimum.x() > maximum.x()
        || minimum.y() > maximum.y()
        || minimum.z() > maximum.z()
    {
        return 0;
    }
    let mut slot: u16 = 1;
    while slot <= SCENE_CAP {
        let mut m = scene_mesh(slot);
        if m[0u16] == 0 {
            let mut i: u16 = 0;
            while i < root[0u16].count {
                let mut n: u16 = root[0u16].count - i;
                if n > 16 {
                    n = 16;
                }
                if !gpu_mesh_batch(root, i, GPU_SCENE_ENTRIES.as_array(), n)
                    || !gpu_mesh_entries_valid(root, GPU_SCENE_ENTRIES.as_array(), n)
                {
                    return 0;
                }
                i += n;
            }
            m[1u16] = root[0u16].directory_sector;
            m[2u16] = root[0u16].directory_word;
            m[3u16] = root[0u16].count;
            m[4u16] = root[0u16].first_sector;
            m[5u16] = root[0u16].sectors;
            scene_store_vec(m, 6, minimum);
            scene_store_vec(m, 12, maximum);
            m[0u16] = 1;
            GPU_SCENE_WORK[7u16] += 1;
            return slot;
        }
        slot += 1;
    }
    0
}
fn scene_mesh_release(slot: u16) {
    scene_mesh(slot)[0u16] = 0;
}
fn scene_texture() -> GpuTexture {
    GpuTexture {
        sector: GPU_RENDERER_TEXTURE,
        sectors: GPU_RENDERER_TEXTURE_SECTORS,
        size_log2: GPU_RENDERER_TEXTURE_LOG2,
        full_mip: GPU_RENDERER_TEXTURE_MIP,
    }
}
fn scene_render(slot: u16) -> GpuRender {
    let m = scene_material(slot);
    let texture = scene_texture();
    let mut depth: u16 = 1;
    let mut blend: u16 = 0;
    if m[4u16] != 255 {
        depth = 0;
        blend = 1;
    }
    GpuRender {
        texture_sector: texture.sector,
        texture_sectors: texture.sectors,
        size_log2: texture.size_log2,
        full_mip: texture.full_mip,
        filter: 2,
        specular_low: m[6u16] | (m[7u16] << 8),
        specular_high: m[8u16],
        depth_func: 1,
        depth_write: depth,
        blend: blend,
        unlit: m[5u16],
        lod_bias: 0,
    }
}
fn scene_global(slot: u16) -> GpuGlobal {
    let m = scene_material(slot);
    let c = GPU_SCENE_VIEW.as_array();
    GpuGlobal {
        far_low: c[16u16],
        far_high: c[17u16],
        light_x: c[69u16] as i16,
        light_y: c[70u16] as i16,
        light_z: c[71u16] as i16,
        ray_x: c[66u16] as i16,
        ray_y: c[67u16] as i16,
        ray_k: c[68u16],
        ambient: m[9u16],
        directional: m[10u16],
        mode: 3,
        shininess: m[12u16],
        epoch: 0,
    }
}
fn scene_material_color(slot: u16, red: u16, green: u16, blue: u16, alpha: u16) -> bool {
    if red > 255 || green > 255 || blue > 255 || alpha > 255 {
        return false;
    }
    let mut m = scene_material(slot);
    m[1u16] = red;
    m[2u16] = green;
    m[3u16] = blue;
    m[4u16] = alpha;
    m[13u16] = red | (green << 8);
    m[14u16] = blue | (alpha << 8);
    true
}
fn scene_material_lighting(slot: u16, ambient: u16, directional: u16, shininess: u16) -> bool {
    if ambient > 256 || directional > 256 || shininess > 16 {
        return false;
    }
    let mut m = scene_material(slot);
    m[9u16] = ambient;
    m[10u16] = directional;
    m[12u16] = shininess;
    true
}
fn scene_material_specular(slot: u16, red: u16, green: u16, blue: u16) -> bool {
    if red > 255 || green > 255 || blue > 255 {
        return false;
    }
    let mut m = scene_material(slot);
    m[6u16] = red;
    m[7u16] = green;
    m[8u16] = blue;
    true
}
fn scene_material_new(red: u16, green: u16, blue: u16, lit: bool) -> u16 {
    let mut slot: u16 = 1;
    while slot <= SCENE_CAP {
        let mut m = scene_material(slot);
        if m[0u16] == 0 {
            if !scene_material_color(slot, red, green, blue, 255) {
                return 0;
            }
            m[5u16] = 1;
            if lit {
                m[5u16] = 0;
            }
            m[6u16] = 160;
            m[7u16] = 160;
            m[8u16] = 160;
            m[9u16] = 64;
            m[10u16] = 192;
            m[12u16] = 12;
            let mut render = scene_render(slot);
            if !gpu_asset_extent(render.texture_sector, render.texture_sectors)
                || !gpu_render_valid(view_of(&mut render))
            {
                return 0;
            }
            m[0u16] = 1;
            return slot;
        }
        slot += 1;
    }
    0
}
fn scene_material_release(slot: u16) {
    scene_material(slot)[0u16] = 0;
}

fn scene_load(a: Array<u16>, at: u16) -> fix32 {
    fix32::from_words(a[at], a[at + 1])
}
fn scene_store(mut a: Array<u16>, at: u16, v: fix32) {
    a[at] = v.lo_bits();
    a[at + 1] = v.hi_bits();
}
fn scene_vec(a: Array<u16>, at: u16) -> vec3 {
    vec3::new(
        scene_load(a, at),
        scene_load(a, at + 2),
        scene_load(a, at + 4),
    )
}
fn scene_store_vec(a: Array<u16>, at: u16, v: vec3) {
    scene_store(a, at, v.x());
    scene_store(a, at + 2, v.y());
    scene_store(a, at + 4, v.z());
}
fn scene_equal(a: vec3, b: vec3) -> bool {
    a.x() == b.x() && a.y() == b.y() && a.z() == b.z()
}
fn scene_bounded(v: vec3, limit: fix32) -> bool {
    v.x() >= -limit
        && v.x() <= limit
        && v.y() >= -limit
        && v.y() <= limit
        && v.z() >= -limit
        && v.z() <= limit
}
fn scene_camera(slot: u16) -> Array<u16> {
    unsafe {
        GPU_SCENE_CAMERAS
            .as_ptr()
            .add(((slot - 1) * SCENE_CAMERA_WORDS) as i16)
            .as_u16_array()
    }
}
fn scene_model(slot: u16) -> Array<u16> {
    unsafe {
        GPU_SCENE_MODELS
            .as_ptr()
            .add(((slot - 1) * SCENE_MODEL_WORDS) as i16)
            .as_u16_array()
    }
}

fn scene_rotation(a: Array<u16>, at: u16, angles: vec3) {
    if angles.x() == fix32::zero() && angles.y() == fix32::zero() && angles.z() == fix32::zero() {
        let mut i: u16 = 0;
        while i < 9 {
            let mut value = fix32::zero();
            if i == 0 || i == 4 || i == 8 {
                value = fix32::from_int(1);
            }
            scene_store(a, at + i * 2, value);
            i += 1;
        }
        return;
    }
    let x = fsincos(angles.x());
    let y = fsincos(angles.y());
    let z = fsincos(angles.z());
    let sx = x.x();
    let cx = x.y();
    let sy = y.x();
    let cy = y.y();
    let sz = z.x();
    let cz = z.y();
    scene_store(a, at, cz * cy);
    scene_store(a, at + 2, cz * sy * sx - sz * cx);
    scene_store(a, at + 4, cz * sy * cx + sz * sx);
    scene_store(a, at + 6, sz * cy);
    scene_store(a, at + 8, sz * sy * sx + cz * cx);
    scene_store(a, at + 10, sz * sy * cx - cz * sx);
    scene_store(a, at + 12, -sy);
    scene_store(a, at + 14, cy * sx);
    scene_store(a, at + 16, cy * cx);
}
fn scene_camera_pose(slot: u16, eye: vec3, angles: vec3) -> bool {
    if !scene_bounded(eye, fix32::from_int(100)) || !scene_bounded(angles, fix32::from_int(4)) {
        return false;
    }
    let mut c = scene_camera(slot);
    if !scene_equal(scene_vec(c, 2), eye) {
        scene_store_vec(c, 2, eye);
        c[1u16] |= 2;
    }
    if !scene_equal(scene_vec(c, 8), angles) {
        scene_store_vec(c, 8, angles);
        c[1u16] |= 1;
    }
    true
}
fn scene_camera_projection(slot: u16, focal: fix32, far: fix32) -> bool {
    if focal < fix32::from_int(144)
        || focal > fix32::from_int(256)
        || far <= fix32::zero()
        || far >= fix32::from_int(200)
    {
        return false;
    }
    let mut c = scene_camera(slot);
    if scene_load(c, 14) != focal {
        scene_store(c, 14, focal);
        c[1u16] |= 4;
    }
    if scene_load(c, 16) != far {
        scene_store(c, 16, far);
    }
    true
}
fn scene_camera_light(slot: u16, light: vec3) -> bool {
    if !scene_bounded(light, fix32::from_int(100)) || scene_equal(light, vec3::zero()) {
        return false;
    }
    let mut c = scene_camera(slot);
    if !scene_equal(scene_vec(c, 18), light) {
        scene_store_vec(c, 18, light);
        c[1u16] |= 8;
    }
    true
}
fn scene_camera_new(eye: vec3, angles: vec3, focal: fix32, far: fix32) -> u16 {
    let mut slot: u16 = 1;
    while slot <= SCENE_CAP {
        let mut c = scene_camera(slot);
        if c[0u16] == 0 {
            if !scene_camera_pose(slot, eye, angles) || !scene_camera_projection(slot, focal, far) {
                return 0;
            }
            scene_store_vec(
                c,
                18,
                vec3::new(fix32::zero(), fix32::zero(), fix32::from_int(1)),
            );
            c[1u16] = 15;
            c[0u16] = 1;
            return slot;
        }
        slot += 1;
    }
    0
}
fn scene_camera_release(slot: u16) {
    scene_camera(slot)[0u16] = 0;
}
fn scene_unit(code: u16) -> fix32 {
    fix32::from_words(code << 2, ((code as i16) >> 14) as u16)
}
fn scene_light_quantize(mut c: Array<u16>) -> bool {
    let mut largest: u16 = 69;
    let mut i: u16 = 70;
    while i < 72 {
        if fix32::from_int(c[i] as i16).abs() > fix32::from_int(c[largest] as i16).abs() {
            largest = i;
        }
        i += 1;
    }
    let mut attempt: u16 = 0;
    while attempt < 16 {
        if gpu_unit_light(c[69u16] as i16, c[70u16] as i16, c[71u16] as i16) {
            return true;
        }
        let v = vec3::new(
            scene_unit(c[69u16]),
            scene_unit(c[70u16]),
            scene_unit(c[71u16]),
        );
        let delta: i16 = if fdot(v, v) < fix32::from_int(1) {
            1
        } else {
            -1
        };
        let code = c[largest] as i16;
        if code < 0 {
            c[largest] = (code - delta) as u16;
        } else {
            c[largest] = (code + delta) as u16;
        }
        attempt += 1;
    }
    false
}
// Power-of-two scaling keeps the Q16 reciprocal square root precise at both
// large direction magnitudes and the smallest nonzero input component.
fn scene_light_normalize(raw: vec3) -> vec3 {
    let mut light = raw;
    let mut largest = raw.x().abs();
    if raw.y().abs() > largest {
        largest = raw.y().abs();
    }
    if raw.z().abs() > largest {
        largest = raw.z().abs();
    }
    let one = fix32::from_int(1);
    let half = fix32::from_words(32768, 0);
    while largest > one {
        light = light * half;
        largest = largest * half;
    }
    while largest < half {
        light = light * fix32::from_int(2);
        largest = largest * fix32::from_int(2);
    }
    light * frsqrt(fdot(light, light))
}
fn scene_camera_prepare(slot: u16) -> bool {
    let mut c = scene_camera(slot);
    let mut dirty = c[1u16];
    if dirty == 0 {
        return true;
    }
    if dirty & 1 != 0 {
        let mut rotation: Buf<u16, 18> = Buf::new([0; 18]);
        scene_rotation(rotation.as_array(), 0, scene_vec(c, 8));
        let mut row: u16 = 0;
        while row < 3 {
            let mut col: u16 = 0;
            while col < 3 {
                // Match native look-at winding and the Lighting ray basis.
                // Reflect before sharing this basis with position/normal/light.
                let mut value = scene_load(rotation.as_array(), 2 * (col * 3 + row));
                if row == 0 {
                    value = -value;
                }
                scene_store(c, 24 + 2 * (row * 3 + col), value);
                col += 1;
            }
            row += 1;
        }
        dirty |= 10;
        GPU_SCENE_WORK[0u16] += 1;
    }
    if dirty & 4 != 0 {
        // Normalize before reciprocal to retain precision for large focal values.
        let inverse = frcp(scene_load(c, 14) * fix32::from_words(256, 0));
        let scale = fix32::from_int(16384);
        c[66u16] = (-fix32::from_words(25600, 0) * inverse * scale)
            .round()
            .to_int() as u16;
        c[67u16] = (-fix32::from_words(15360, 0) * inverse * scale)
            .round()
            .to_int() as u16;
        c[68u16] = 8192;
        dirty |= 2;
        GPU_SCENE_WORK[2u16] += 1;
    }
    if dirty & 2 != 0 {
        let k = scene_load(c, 14) * fix32::from_words(256, 0);
        let eye = scene_vec(c, 2);
        let mut row: u16 = 0;
        while row < 3 {
            let mut basis = scene_vec(c, 24 + row * 6);
            let mut factor = fix32::from_int(1);
            if row == 0 {
                factor = k;
            } else {
                if row == 1 {
                    factor = -k;
                } else {
                    factor = -factor;
                }
            }
            let translation = -fdot(basis, eye) * factor;
            basis = basis * factor;
            scene_store_vec(c, 42 + row * 8, basis);
            scene_store(c, 48 + row * 8, translation);
            row += 1;
        }
        GPU_SCENE_WORK[1u16] += 1;
    }
    if dirty & 8 != 0 {
        let raw = scene_vec(c, 18);
        let light = scene_light_normalize(raw);
        let scale = fix32::from_int(16384);
        let mut row: u16 = 0;
        while row < 3 {
            c[69 + row] = (fdot(scene_vec(c, 24 + row * 6), light) * scale)
                .round()
                .to_int() as u16;
            row += 1;
        }
        if !scene_light_quantize(c) {
            return false;
        }
        GPU_SCENE_WORK[3u16] += 1;
    }
    c[1u16] = 0;
    true
}
// One immutable camera snapshot per Pass. Compare actual prepared fields so
// wrapping versions or a released/reused Camera slot cannot alias a stale view.
fn scene_view_begin(slot: u16) -> bool {
    if !scene_camera_prepare(slot) {
        return false;
    }
    let c = scene_camera(slot);
    let mut view = GPU_SCENE_VIEW.as_array();
    let mut normal_changed = GPU_SCENE_VIEW_FLAGS[0u16] == 0;
    let mut position_changed = normal_changed;
    let mut i: u16 = 24;
    while i < 66 {
        if c[i] != view[i] {
            if i < 42 {
                normal_changed = true;
            } else {
                position_changed = true;
            }
        }
        i += 1;
    }
    let mut flags: u16 = 0;
    if normal_changed {
        flags |= 8;
    }
    if position_changed {
        flags |= 4;
    }
    if flags != 0 {
        let mut n: u16 = 1;
        while n <= SCENE_CAP {
            let mut m = scene_model(n);
            if m[0u16] != 0 {
                m[1u16] |= flags;
            }
            n += 1;
        }
    }
    i = 16;
    while i < 72 {
        view[i] = c[i];
        i += 1;
    }
    GPU_SCENE_VIEW_FLAGS[0u16] = 1;
    true
}
fn scene_model_pose(slot: u16, position: vec3, angles: vec3, scale: vec3) -> bool {
    let half = fix32::from_words(32768, 0);
    let two = fix32::from_int(2);
    if !scene_bounded(position, fix32::from_int(100))
        || !scene_bounded(angles, fix32::from_int(4))
        || scale.x() < half
        || scale.x() > two
        || scale.y() < half
        || scale.y() > two
        || scale.z() < half
        || scale.z() > two
    {
        return false;
    }
    let mut m = scene_model(slot);
    if !scene_equal(scene_vec(m, 2), position) {
        scene_store_vec(m, 2, position);
        m[1u16] |= 2;
    }
    if !scene_equal(scene_vec(m, 8), angles) {
        scene_store_vec(m, 8, angles);
        m[1u16] |= 1;
    }
    if !scene_equal(scene_vec(m, 14), scale) {
        scene_store_vec(m, 14, scale);
        m[1u16] |= 2;
    }
    true
}
fn scene_model_new(position: vec3, angles: vec3, scale: vec3) -> u16 {
    let mut slot: u16 = 1;
    while slot <= SCENE_CAP {
        let mut m = scene_model(slot);
        if m[0u16] == 0 {
            if !scene_model_pose(slot, position, angles, scale) {
                return 0;
            }
            m[1u16] = 15;
            m[0u16] = 1;
            return slot;
        }
        slot += 1;
    }
    0
}
fn scene_model_release(slot: u16) {
    scene_model(slot)[0u16] = 0;
}
fn scene_numeric(matrix: Array<u16>, bounds: Array<u16>) -> bool {
    let mut corner: u16 = 0;
    while corner < 8 {
        let mut x = scene_load(bounds, 0);
        let mut y = scene_load(bounds, 2);
        let mut z = scene_load(bounds, 4);
        if corner & 1 != 0 {
            x = scene_load(bounds, 6);
        }
        if corner & 2 != 0 {
            y = scene_load(bounds, 8);
        }
        if corner & 4 != 0 {
            z = scene_load(bounds, 10);
        }
        let point = vec4::new(x, y, z, fix32::from_int(1));
        let mut row: u16 = 0;
        while row < 3 {
            let at = row * 8;
            let value = fdot(
                point,
                vec4::new(
                    scene_load(matrix, at),
                    scene_load(matrix, at + 2),
                    scene_load(matrix, at + 4),
                    scene_load(matrix, at + 6),
                ),
            );
            if value < -fix32::from_int(512) || value >= fix32::from_int(512) {
                return false;
            }
            row += 1;
        }
        corner += 1;
    }
    true
}
fn scene_model_prepare(slot: u16, bounds: Array<u16>) -> bool {
    let mut m = scene_model(slot);
    let view = GPU_SCENE_VIEW.as_array();
    let mut dirty = m[1u16];
    if dirty & 1 != 0 {
        scene_rotation(m, 20, scene_vec(m, 8));
        dirty |= 10;
        GPU_SCENE_WORK[4u16] += 1;
    }
    if dirty & 2 != 0 {
        let mut row: u16 = 0;
        while row < 3 {
            let mut col: u16 = 0;
            while col < 3 {
                scene_store(
                    m,
                    38 + row * 8 + col * 2,
                    scene_load(m, 20 + row * 6 + col * 2) * scene_load(m, 14 + col * 2),
                );
                col += 1;
            }
            scene_store(m, 44 + row * 8, scene_load(m, 2 + row * 2));
            row += 1;
        }
        dirty |= 4;
    }
    let position_changed = dirty & 4 != 0;
    if position_changed {
        let mut row: u16 = 0;
        while row < 3 {
            let at = 42 + row * 8;
            let ar = vec4::new(
                scene_load(view, at),
                scene_load(view, at + 2),
                scene_load(view, at + 4),
                scene_load(view, at + 6),
            );
            let mut col: u16 = 0;
            while col < 4 {
                let mut h = fix32::zero();
                if col == 3 {
                    h = fix32::from_int(1);
                }
                let bc = vec4::new(
                    scene_load(m, 38 + col * 2),
                    scene_load(m, 46 + col * 2),
                    scene_load(m, 54 + col * 2),
                    h,
                );
                scene_store(m, 62 + 2 * (row * 4 + col), fdot(ar, bc));
                col += 1;
            }
            row += 1;
        }
        GPU_SCENE_WORK[5u16] += 1;
    }
    if dirty & 8 != 0 {
        let mut row: u16 = 0;
        while row < 3 {
            let ar = scene_vec(view, 24 + row * 6);
            let mut col: u16 = 0;
            while col < 3 {
                let bc = vec3::new(
                    scene_load(m, 20 + col * 2),
                    scene_load(m, 26 + col * 2),
                    scene_load(m, 32 + col * 2),
                );
                scene_store(m, 86 + 2 * (row * 3 + col), fdot(ar, bc));
                col += 1;
            }
            row += 1;
        }
        GPU_SCENE_WORK[6u16] += 1;
    }
    let mut changed = position_changed;
    let mut i: u16 = 0;
    while i < 12 {
        if bounds[i] != m[104 + i] {
            changed = true;
        }
        i += 1;
    }
    if changed {
        let matrix = unsafe { m.as_ptr().add(62).as_u16_array() };
        if !scene_numeric(matrix, bounds) {
            return false;
        }
        i = 0;
        while i < 12 {
            m[104 + i] = bounds[i];
            i += 1;
        }
    }
    m[1u16] = 0;
    true
}
fn scene_matrix(slot: u16) -> Array<u16> {
    unsafe { scene_model(slot).as_ptr().add(62).as_u16_array() }
}
