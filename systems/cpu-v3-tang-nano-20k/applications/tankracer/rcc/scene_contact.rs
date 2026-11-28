//! Immediate scenery response between a live query and ordinary method dispatch.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, object_query, scene_world};
pub struct Object {
    pub class: fix32,
    pub weight: fix32,
    pub threshold: fix32,
    pub activity: fix32,
    pub flags: fix32,
    pub radius: fix32,
}
pub struct Environment {
    pub mode: fix32,
    pub breakable_class: fix32,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
}
// Complete ordinary-object query and response. The owner invokes the selected
// gameplay method immediately after status 1, using hit.object and live class.
// 0 miss, 1 method boundary, 2 threshold-only, 3 arithmetic, 4 invalid object.
pub fn obstacles(
    world: &scene_world::World,
    q: &object_query::Query,
    velocity: Array<u16>,
    state: Array<u16>,
    env: &Environment,
    hit: &mut object_query::Hit,
) -> u16 {
    let status = scene_world::query(world, q, hit);
    if status != 1 {
        return status;
    }
    let index = hit.object.lo_bits();
    if index < world.fixed_count {
        let fixed_properties = unsafe {
            world
                .properties
                .as_ptr()
                .add((index * 16) as i16)
                .as_u16_array()
        };
        let fixed_collider = unsafe {
            world
                .colliders
                .as_ptr()
                .add((index * 60) as i16)
                .as_u16_array()
        };
        let object: Object = Object {
            class: attitude::get(fixed_properties, 0),
            weight: attitude::get(fixed_properties, 1),
            threshold: attitude::get(fixed_properties, 2),
            activity: attitude::get(fixed_properties, 3),
            flags: attitude::get(fixed_properties, 4),
            radius: attitude::get(fixed_collider, 28),
        };
        let response = obstacle(velocity, state, hit.normal.as_array(), &object, env);
        if response == 0 {
            return 2;
        }
        if response == 2 {
            return 3;
        }
        return 1;
    }
    let properties: Buf<u16, 16> = Buf::new([0; 16]);
    let collider: Buf<u16, 60> = Buf::new([0; 60]);
    if !scene_world::load_properties(world, index, properties.as_array())
        || !scene_world::load_collider(world, index, collider.as_array())
    {
        return 4;
    }
    let object: Object = Object {
        class: attitude::get(properties.as_array(), 0),
        weight: attitude::get(properties.as_array(), 1),
        threshold: attitude::get(properties.as_array(), 2),
        activity: attitude::get(properties.as_array(), 3),
        flags: attitude::get(properties.as_array(), 4),
        radius: attitude::get(collider.as_array(), 28),
    };
    let response = obstacle(velocity, state, hit.normal.as_array(), &object, env);
    if response == 0 {
        return 2;
    }
    if response == 2 {
        return 3;
    }
    1
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
pub fn reflect(velocity: Array<u16>, normal: Array<u16>, env: &Environment) -> u16 {
    let buffer: Buf<u16, 12> = Buf::new([0; 12]);
    let input = buffer.as_array();
    let config: Buf<u16, 16> = Buf::new([1, 0, 1, 0, 1, 0, 1, 0, 1024, 0, 10, 0, 0, 0, 0, 0]);
    attitude::put(config.as_array(), 6, env.mode);
    let output: Buf<u16, 16> = Buf::new([0; 16]);
    let mut i = 0u16;
    while i < 3 {
        attitude::put(input, i, attitude::get(normal, i));
        attitude::put(input, 3 + i, attitude::get(velocity, i));
        i += 1;
    }
    let status = collision::respond(
        input,
        config.as_array(),
        env.sine,
        env.angle,
        output.as_array(),
    );
    if status != 0 {
        return status;
    }
    i = 0;
    while i < 3 {
        attitude::put(velocity, i, attitude::get(output.as_array(), 3 + i));
        i += 1;
    }
    0
}
pub fn blend(velocity: Array<u16>, old: Array<u16>, weight: fix32) {
    let mut i = 0u16;
    while i < 3 {
        attitude::put(
            velocity,
            i,
            integer::trunc_shift(
                fmul_lo32(weight, attitude::get(velocity, i))
                    + fmul_lo32(n(128) - weight, attitude::get(old, i)),
                7,
            ),
        );
        i += 1;
    }
}
// Collision state is impact accumulator, collision size. Returns 0 after the
// threshold-only reflection, 1 to invoke the selected method, 2 on arithmetic.
// This is the car runtime's path, including impact accumulation for every car.
pub fn obstacle(
    velocity: Array<u16>,
    state: Array<u16>,
    normal: Array<u16>,
    object: &Object,
    env: &Environment,
) -> u16 {
    let old_buffer: Buf<u16, 6> = Buf::new([0; 6]);
    let old = old_buffer.as_array();
    let mut i = 0u16;
    while i < 3 {
        attitude::put(old, i, attitude::get(velocity, i));
        i += 1;
    }
    let status = reflect(velocity, normal, env);
    if status != 0 {
        return status;
    }
    let speed = collision::distance(old);
    if object.threshold != n(0) && speed < object.threshold {
        return 0;
    }
    blend(velocity, old, object.weight);
    if object.weight != n(128)
        && speed > fix32::from_words(3392, 3)
        && (object.class != env.breakable_class || object.activity == n(0) || object.flags == n(0))
    {
        attitude::put(state, 0, attitude::get(state, 0) + n(70));
        attitude::put(
            state,
            1,
            attitude::get(state, 1) + integer::sar(object.radius, 5),
        );
    }
    1
}
