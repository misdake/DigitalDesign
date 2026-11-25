//! Real car deformable query/response and the synchronous post-method sparks.
use crate::dsl_rt::*;
use crate::{
    attitude, collision, deformation, impact, integer, object_query, random, scene_contact,
    scene_query, scene_world,
};
pub struct Component {
    pub object: u16,
    pub state: deformation::HitState,
    pub config: deformation::HitConfig,
    pub mesh: deformation::Mesh,
    pub sampling: deformation::Surface,
}
fn field(row: Array<u16>, word: u16) -> fix32 {
    fix32::from_words(row[word], row[word + 1])
}
// Converter rows are followed by mutable vertices and flags widened to u16.
// Hit and sampling views address the same vertex storage.
pub fn component(
    data: Array<u16>,
    world: &scene_world::World,
    index: u16,
    records: Array<deformation::Record>,
) -> Component {
    let row = unsafe { data.as_ptr().add((index * 40) as i16).as_u16_array() };
    let object = row[0u16];
    let collider = unsafe {
        world
            .colliders
            .as_ptr()
            .add((object * 60) as i16)
            .as_u16_array()
    };
    let vertices = unsafe { data.as_ptr().add(row[3u16] as i16).as_i16_array() };
    Component {
        object: object,
        state: deformation::HitState {
            dirty: field(row, 30),
            last_vehicle: field(row, 32),
            sound_timer: field(row, 34),
        },
        config: deformation::HitConfig {
            floor: field(row, 5),
            compression: field(row, 7),
            drift_shift: row[9u16],
            material: row[10u16] as i16,
            has_sound: row[11u16] != 0,
        },
        mesh: deformation::Mesh {
            vertices: vertices,
            vertex_count: row[1u16],
            flags: unsafe { data.as_ptr().add(row[4u16] as i16).as_u16_array() },
            flag_count: row[2u16],
        },
        sampling: deformation::Surface {
            enabled: true,
            origin: unsafe { collider.as_ptr().add(2).as_u16_array() },
            basis: unsafe { collider.as_ptr().add(8).as_u16_array() },
            bounds: unsafe { row.as_ptr().add(12).as_u16_array() },
            edges: unsafe { row.as_ptr().add(22).as_u16_array() },
            cap: field(row, 20),
            vertices: vertices,
            vertex_count: row[1u16],
            records: records,
            record_count: row[38u16],
            attached: row[37u16] != 0,
            dirty: row[36u16],
        },
    }
}
pub fn find(components: Array<Component>, count: u16, object: u16) -> u16 {
    let mut i = 0u16;
    while i < count {
        if components[i].object == object {
            return i;
        }
        i += 1;
    }
    65535
}
pub fn attached(
    components: Array<Component>,
    count: u16,
    world: &scene_world::World,
    entity: fix32,
) -> u16 {
    let mut i = 0u16;
    while i < count {
        if attitude::get(world.colliders, components[i].object * 30) == entity {
            return i;
        }
        i += 1;
    }
    65535
}
pub struct Vehicle {
    pub position: Array<u16>,
    pub base_position: Array<u16>,
    pub previous: Array<u16>,
    pub velocity: Array<u16>,
    pub material: Array<u16>,
    pub attached: Array<u16>,
    pub accumulator: Array<u16>,
    pub corners: Array<u16>,
    pub corner_stride: u16,
    pub player: bool,
    pub mode: fix32,
    pub dt: fix32,
    pub current_slot: Array<u16>,
    pub impact: impact::Vehicle,
}
fn n(value: i16) -> fix32 {
    integer::from_i16(value)
}
// A fresh zero hit is required, as in the original car entry. Return 0 for a
// miss, 1 for immediate method dispatch, -2 arithmetic or -1 bad scene data.
pub fn begin(
    vehicle: &mut Vehicle,
    world: &scene_world::World,
    env: &scene_contact::Environment,
    hit: &mut object_query::Hit,
) -> fix32 {
    attitude::put(vehicle.material, 0, n(0));
    let endpoint: Buf<u16, 6> = Buf::new([0; 6]);
    let end = endpoint.as_array();
    let mut i = 0u16;
    while i < 3 {
        attitude::put(
            end,
            i,
            fmul_lo32(attitude::get(vehicle.position, i), n(2))
                - attitude::get(vehicle.previous, i),
        );
        i += 1;
    }
    let query = object_query::Query {
        start: vehicle.position,
        end: end,
        self_entity: vehicle.impact.entity,
        object_count: world.count,
        deformable: true,
    };
    let status = scene_world::query(world, &query, hit);
    if status == 3 {
        return n(-2);
    }
    if status >= 3 {
        return n(-1);
    }
    attitude::put(vehicle.attached, 0, hit.entity);
    if status == 0 {
        return n(0);
    }
    let saved: Buf<u16, 6> = Buf::new([0; 6]);
    let old = saved.as_array();
    i = 0;
    while i < 3 {
        attitude::put(old, i, attitude::get(vehicle.velocity, i));
        i += 1;
    }
    if scene_contact::reflect(vehicle.velocity, hit.normal.as_array(), env) != 0 {
        return n(-2);
    }
    let properties = unsafe {
        world
            .properties
            .as_ptr()
            .add((hit.object.lo_bits() * 16) as i16)
            .as_u16_array()
    };
    scene_contact::blend(vehicle.velocity, old, attitude::get(properties, 1));
    n(1)
}
pub fn inside(collider: &object_query::Collider, bounds: Array<i16>, point: Array<u16>) -> bool {
    let scratch: Buf<u16, 6> = Buf::new([0; 6]);
    let local = scratch.as_array();
    let mut i = 0u16;
    while i < 3 {
        attitude::put(
            local,
            i,
            attitude::get(point, i) - attitude::get(collider.position.as_array(), i),
        );
        i += 1;
    }
    collision::transform(local, collider.basis.as_array(), local, true);
    let x = attitude::get(local, 0);
    let z = attitude::get(local, 2);
    x > fmul_lo32(n(bounds[0u16]), n(8))
        && x < fmul_lo32(n(bounds[3u16]), n(8))
        && z > fmul_lo32(n(bounds[2u16]), n(8))
        && z < fmul_lo32(n(bounds[5u16]), n(8))
}
fn effect_status(status: u16) -> fix32 {
    if status == 2 {
        return n(-2);
    }
    if status == 3 {
        return n(-1);
    }
    n(-4)
}
// Dispatch through the live class table. Slot restoration follows the original
// success boundary; exceeding maximum_slot deliberately leaves slot 21 set.
pub fn respond(
    vehicle: &mut Vehicle,
    scene: &scene_query::Scene,
    world: &scene_world::World,
    mut components: Array<Component>,
    impacts: &mut impact::State,
    context: &mut impact::Context,
) -> fix32 {
    let env = scene_contact::Environment {
        mode: vehicle.mode,
        breakable_class: n(-1),
        sine: context.sine,
        angle: context.angle,
    };
    let mut hit = object_query::hit_zero();
    let status = begin(vehicle, world, &env, &mut hit);
    if status != n(1) {
        return status;
    }
    let index = hit.object.lo_bits();
    let properties = unsafe {
        world
            .properties
            .as_ptr()
            .add((index * 16) as i16)
            .as_u16_array()
    };
    let class = attitude::get(properties, 0);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = attitude::get(vehicle.current_slot, 0);
    attitude::put(vehicle.current_slot, 0, n(21));
    let method = scene.method(class.lo_bits(), 21);
    if method.status == 4 {
        return n(-1);
    }
    let mut collider = object_query::collider_zero();
    let mut raw = unsafe { view_of(&mut collider).as_ptr().as_u16_array() };
    let source = unsafe {
        world
            .colliders
            .as_ptr()
            .add((index * 60) as i16)
            .as_u16_array()
    };
    let mut i = 0u16;
    while i < 60 {
        raw[i] = source[i];
        i += 1;
    }
    if method.status == 1 {
        if method.id != 0 {
            return n(-5);
        }
        let component = find(components, scene.header[9u16], index);
        if component == 65535 {
            return n(-1);
        }
        let mut view = deformation::Vehicle {
            entity: vehicle.impact.entity,
            position: vehicle.base_position,
            material: vehicle.material,
            seed: context.seed,
            dt: vehicle.dt,
        };
        let c = &mut components[component];
        if deformation::hit(&mut c.state, &c.config, &collider, &c.mesh, &mut view) >= 2 {
            return n(-1);
        }
    }
    if method.status != 2 {
        attitude::put(vehicle.current_slot, 0, saved);
    }
    sparks(
        vehicle,
        &collider,
        unsafe { properties.as_ptr().add(10).as_i16_array() },
        impacts,
        context,
    )
}
// Invoke only after the selected class method succeeds, on the same live seed.
// The accumulator advances even when a spark's cooldown suppresses emission.
pub fn sparks(
    vehicle: &mut Vehicle,
    collider: &object_query::Collider,
    bounds: Array<i16>,
    impacts: &mut impact::State,
    context: &mut impact::Context,
) -> fix32 {
    let mut i = 0u16;
    while i < 4 {
        let point = unsafe {
            vehicle
                .impact
                .contacts
                .as_ptr()
                .add((i * vehicle.impact.contact_stride * 2) as i16)
                .as_u16_array()
        };
        if inside(collider, bounds, point)
            && vehicle.player
            && collision::distance(vehicle.velocity) > fix32::from_words(3392, 3)
            && vehicle.mode != n(2)
            && vehicle.mode != n(3)
        {
            let seed = random::next(attitude::get(context.seed, 0));
            attitude::put(context.seed, 0, seed);
            if seed.lo_bits() & 48 == 0 {
                let mut first = 0u16;
                if vehicle.impact.forward >= n(0) {
                    first = 1;
                }
                let corner = attitude::get(vehicle.corners, first * vehicle.corner_stride);
                let status = impact::emit(impacts, &mut vehicle.impact, corner, n(5), context);
                if status >= 2 {
                    return effect_status(status);
                }
                let corner = attitude::get(vehicle.corners, (first + 2) * vehicle.corner_stride);
                let status = impact::emit(impacts, &mut vehicle.impact, corner, n(6), context);
                if status >= 2 {
                    return effect_status(status);
                }
                attitude::put(
                    vehicle.accumulator,
                    0,
                    attitude::get(vehicle.accumulator, 0) + vehicle.dt,
                );
            }
        }
        i += 1;
    }
    n(0)
}
