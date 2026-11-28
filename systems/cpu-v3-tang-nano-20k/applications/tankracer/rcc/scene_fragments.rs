//! Actual auxiliary-update dispatch on shared scene and resource views.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, ground, integer, mesh_break, scene_lifecycle, scene_meshes, scene_query,
    scene_world, spatial,
};

pub struct Object {
    pub id: u16,
    pub collider: Array<u16>,
    // Original i16 active word and optional mesh index (-1 for absent).
    pub active: Array<u16>,
    pub current_mesh: Array<u16>,
    pub private_mesh: fix32,
    pub motion: mesh_break::State,
    pub motion_capacity: u16,
    // 65535 selects caller-owned CPU memory; other values select one DSEG3 bank.
    pub motion_page: u16,
    pub index_page: u16,
    pub mesh: mesh_break::Mesh,
    pub config: mesh_break::Config,
    // Four little-endian words retain the full wrapping resource revision.
    pub version: Array<u16>,
    pub resource_present: bool,
    pub geometry_present: bool,
}
// One level-owned external bank, packed in authored object order. The directory
// keeps offsets; only the selected object's header/faces need a CPU window.
pub struct MotionBank {
    pub page: u16,
    pub used: u16,
}
pub fn reserve(bank: &mut MotionBank, faces: u16) -> u16 {
    // Check before multiplying or committing the directory's next offset.
    if bank.page == 65535 || bank.used > 16384 || faces > (16384 - bank.used) / 16 {
        return 65535;
    }
    let size = 8 + faces * 16;
    if size > 16384 - bank.used {
        return 65535;
    }
    let offset = bank.used;
    let saved = read_dseg_page(3);
    unsafe {
        write_dseg_page(3, bank.page);
    }
    let mut data = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    let mut i = 0u16;
    while i < size {
        data[offset + i] = 0;
        i += 1;
    }
    unsafe {
        write_dseg_page(3, saved);
    }
    bank.used += size;
    offset
}
pub fn motion(offset: u16, count: u16) -> mesh_break::State {
    let data = unsafe { Ptr::from_addr(0xc000).as_u16_array() };
    mesh_break::State {
        header: at(data, offset),
        faces: at(data, offset + 8),
        count: count,
    }
}
fn bind_motion(object: &Object) {
    if object.motion_page != 65535 && read_dseg_page(3) != object.motion_page {
        unsafe {
            write_dseg_page(3, object.motion_page);
        }
    }
}
pub struct Frame {
    pub entity: fix32,
    pub dt: fix32,
    pub mode: fix32,
    pub sine: Array<i16>,
    pub result: Array<u16>,
}
pub struct Hit {
    // The same eight-word lifetime row used by dispatch and retirement.
    pub row: Array<u16>,
    pub limit: fix32,
    pub powerup: bool,
    pub name: u16,
    // External model resolution: zero is present, positive is absent, negative
    // is the original I/O error. The entry reads it only when preparation is due.
    pub model_status: fix32,
    pub model: scene_meshes::Model,
    pub has_terrain: bool,
    pub vehicle: fix32,
    pub velocity: Array<u16>,
    pub paired_tail: u16,
    pub has_paired_tail: bool,
    pub result: Array<u16>,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
fn at(data: Array<u16>, words: u16) -> Array<u16> {
    unsafe { data.as_ptr().add(words as i16).as_u16_array() }
}
fn touch(mut version: Array<u16>) {
    let mut i = 0u16;
    while i < 4 {
        version[i] += 1;
        if version[i] != 0 {
            return;
        }
        i += 1;
    }
}
// Direct AUXILIARY_HIT execution after the caller has selected the class method.
// The loaded catalog supplies bounded geometry and enough per-face state.
pub fn activate(
    resources: &mut scene_meshes::Store,
    object: &mut Object,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    map: &mut ground::World,
    call: &Hit,
) -> fix32 {
    let saved = read_dseg_page(3);
    bind_motion(object);
    let status = activate_inner(resources, object, runtime, grid, map, call);
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
fn activate_inner(
    resources: &mut scene_meshes::Store,
    object: &mut Object,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    map: &mut ground::World,
    call: &Hit,
) -> fix32 {
    if call.limit == n(0) {
        return n(-1);
    }
    let mut row = call.row;
    if row[7u16] & 1 == 0 {
        return n(-1);
    }
    let entity = attitude::get(object.collider, 0);
    if object.active[0u16] != 0 {
        attitude::put(call.result, 0, entity);
        return n(0);
    }
    let prepared = row[7u16] & 32 != 0 && attitude::get(object.motion.header, 2) != n(0);
    if !prepared {
        if row[7u16] & 4 == 0 {
            return n(-1);
        }
        if call.model_status < n(0) {
            return call.model_status;
        }
        if call.model_status != n(0) {
            return n(-4);
        }
        let shared = scene_meshes::bind(resources, call.name, &call.model);
        if shared < n(0) {
            return shared;
        }
        let private = scene_meshes::private_copy(resources, shared.lo_bits());
        if private < n(0) {
            return n(-4);
        }
        if row[7u16] & 16 != 0 {
            if object.private_mesh.hi_bits() != 0
                || !scene_meshes::release_private(resources, object.private_mesh.lo_bits())
            {
                return n(-1);
            }
        }
        object.private_mesh = private;
        row[7u16] |= 16;
        let view = scene_meshes::view(resources, private.lo_bits(), false);
        if view.page != 65535 && view.page != object.motion_page {
            halt(0x7f82);
        }
        object.mesh = view.mesh;
        object.index_page = view.index_page;
        object.version = view.revision;
        object.resource_present = true;
        object.geometry_present = view.valid;
        let count = call.model.geometry[1u16];
        if count > object.motion_capacity {
            return n(-4);
        }
        object.motion.count = count;
        let mut header = object.motion.header;
        let mut faces = object.motion.faces;
        let mut i = 0u16;
        while i < 8 {
            header[i] = 0;
            i += 1;
        }
        attitude::put(header, 3, n(2000));
        i = 0;
        while i < count * 16 {
            faces[i] = 0;
            i += 1;
        }
        row[7u16] |= 32;
    }
    if call.powerup {
        if row[7u16] & 32 == 0 {
            return n(-1);
        }
        attitude::put(object.motion.header, 3, n(1500));
    }
    if !call.has_terrain {
        return n(-1);
    }
    let x = attitude::get(object.collider, 1);
    let z = attitude::get(object.collider, 3);
    map.bind(x, z);
    let height = map.terrain.height(x, z);
    if height.status == 2 {
        return n(-2);
    }
    if height.status > 2 {
        return n(-1);
    }
    // height() selected a collision cell in the same mapping window.
    bind_motion(object);
    if row[7u16] & 16 == 0 || row[7u16] & 32 == 0 || object.private_mesh.hi_bits() != 0 {
        return n(-1);
    }
    let view = scene_meshes::view(resources, object.private_mesh.lo_bits(), true);
    if view.page != 65535 && view.page != object.motion_page {
        halt(0x7f82);
    }
    object.mesh = view.mesh;
    object.index_page = view.index_page;
    object.version = view.revision;
    object.resource_present = true;
    object.geometry_present = view.valid;
    if !view.valid || row[7u16] & 4 == 0 {
        return n(-1);
    }
    let mut hit = mesh_break::Hit {
        velocity: call.velocity,
        has_velocity: call.vehicle != n(0),
        basis: at(object.collider, 26),
        has_basis: attitude::get(object.collider, 29) != n(0),
        height: height.height,
        paired_tail: call.paired_tail,
        has_paired_tail: call.has_paired_tail,
    };
    // The collider basis belongs to the world window. Capture it before
    // borrowing that window for immutable indices; motion stays in DSEG3.
    let velocity: Buf<u16, 6> = Buf::new([0; 6]);
    let basis: Buf<u16, 18> = Buf::new([0; 18]);
    let gains: Buf<u16, 10> = Buf::new([0; 10]);
    let mut config = mesh_break::Config {
        gravity: object.config.gravity,
        gains: object.config.gains,
    };
    let saved2 = read_dseg_page(2);
    if object.index_page != 65535 {
        let mut i = 0u16;
        let mut local = velocity.as_array();
        if hit.has_velocity {
            while i < 6 {
                local[i] = hit.velocity[i];
                i += 1;
            }
            hit.velocity = local;
        }
        i = 0;
        local = basis.as_array();
        if hit.has_basis {
            while i < 18 {
                local[i] = hit.basis[i];
                i += 1;
            }
            hit.basis = local;
        }
        i = 0;
        local = gains.as_array();
        while i < 10 {
            local[i] = config.gains[i];
            i += 1;
        }
        config.gains = local;
        unsafe {
            write_dseg_page(2, object.index_page);
        }
    }
    let status = mesh_break::activate(
        &object.motion,
        &object.mesh,
        &config,
        &hit,
        runtime.seed,
        call.limit,
    );
    if object.index_page != 65535 {
        unsafe {
            write_dseg_page(2, saved2);
        }
    }
    if status != n(0) {
        return status;
    }
    attitude::put(object.current_mesh, 0, object.private_mesh);
    let mut active = object.active;
    active[0u16] = 1;
    row[7u16] |= 128;
    row[3u16] |= 2;
    attitude::put(object.collider, 28, n(5120));
    let cell = spatial::Cell {
        x: row[4u16] as i16,
        z: row[5u16],
        present: row[6u16] != 0,
    };
    let status = spatial::remove(grid, fix32::from_words(object.id, 0), &cell, call.limit);
    if status == 3 {
        return n(-3);
    }
    if status != 0 {
        return n(-1);
    }
    attitude::put(call.result, 0, entity);
    n(0)
}
// The resource view is resolved from the object's stable private MeshId. Mesh
// slots persist after release; geometry_present describes the live payload.
pub fn tick(
    scene: &scene_query::Scene,
    lifetime: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    object: &Object,
    frame: &Frame,
) -> fix32 {
    let saved = read_dseg_page(3);
    bind_motion(object);
    let status = tick_inner(scene, lifetime, runtime, grid, object, frame);
    unsafe {
        write_dseg_page(3, saved);
    }
    status
}
fn tick_inner(
    scene: &scene_query::Scene,
    lifetime: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    grid: &mut spatial::Grid,
    object: &Object,
    frame: &Frame,
) -> fix32 {
    let class = entities::class(&lifetime.identities, frame.entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(runtime.slot, 0);
    attitude::put(runtime.slot, 0, n(22));
    let method = scene.method(class.lo_bits(), 22);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        attitude::put(frame.result, 0, n(0));
        return n(0);
    }
    if method.status != 1 {
        attitude::put(frame.result, 0, n(0));
        attitude::put(runtime.slot, 0, saved);
        return n(0);
    }
    if method.id != 68 {
        return n(-5);
    }
    let kind = entities::handle_kind(&lifetime.identities, frame.entity);
    if kind == 2 {
        return n(-5);
    }
    if kind != 1 {
        return n(-1);
    }
    let id = entities::handle(&lifetime.identities, frame.entity);
    if id.hi_bits() != 0 || id.lo_bits() >= lifetime.object_count || id.lo_bits() != object.id {
        return n(-1);
    }
    if lifetime.limit == n(0) {
        return n(-1);
    }
    let mut row = at(lifetime.records, object.id * 8);
    if row[7u16] & 1 == 0 || row[7u16] & 16 == 0 || row[7u16] & 32 == 0 {
        return n(-1);
    }
    if !object.resource_present {
        return n(-1);
    }
    touch(object.version);
    if !object.geometry_present || row[7u16] & 4 == 0 {
        return n(-1);
    }
    let mut step = mesh_break::Step {
        position: at(object.collider, 2),
        dt: frame.dt,
        limit: lifetime.limit,
        sine: frame.sine,
    };
    let position: Buf<u16, 6> = Buf::new([0; 6]);
    let gains: Buf<u16, 10> = Buf::new([0; 10]);
    let mut config = mesh_break::Config {
        gravity: object.config.gravity,
        gains: object.config.gains,
    };
    let saved2 = read_dseg_page(2);
    if object.index_page != 65535 {
        let mut local = position.as_array();
        let mut i = 0u16;
        while i < 6 {
            local[i] = step.position[i];
            i += 1;
        }
        step.position = local;
        local = gains.as_array();
        i = 0;
        while i < 10 {
            local[i] = config.gains[i];
            i += 1;
        }
        config.gains = local;
        unsafe {
            write_dseg_page(2, object.index_page);
        }
    }
    let status = mesh_break::tick(&object.motion, &object.mesh, &config, &step);
    if object.index_page != 65535 {
        unsafe {
            write_dseg_page(2, saved2);
        }
    }
    if status < n(0) {
        return status;
    }
    if status == n(1) {
        let mut active = object.active;
        active[0u16] = 0;
        attitude::put(object.current_mesh, 0, n(-1));
        row[7u16] &= 65407;
        row[3u16] &= 65533;
        attitude::put(object.collider, 28, n(0));
        let mut slot = 27u16;
        if frame.mode == n(0) || frame.mode == n(1) {
            slot = 8;
        }
        let status = scene_lifecycle::dispatch(scene, lifetime, runtime, grid, frame.entity, slot);
        if status != n(0) {
            return status;
        }
    }
    attitude::put(frame.result, 0, frame.entity);
    attitude::put(runtime.slot, 0, saved);
    n(0)
}
