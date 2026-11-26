//! Vehicle class methods used by UFO transfer, on shared lifecycle and car views.
use crate::dsl_rt::*;
use crate::{
    attitude, entities, integer, lifecycle, scene_lifecycle, scene_query, scene_world,
    vehicle_transfer,
};
pub struct Car {
    pub present: bool,
    pub transfer: vehicle_transfer::Car,
    // Spin disabled byte, collision disabled bool, race driving bool; extended i32.
    pub flags: Array<u16>,
}
pub struct View {
    pub cars: Array<Car>,
    pub count: u16,
    pub present: bool,
    pub routes: Array<vehicle_transfer::Routes>,
    pub sine: Array<i16>,
    pub angle: Array<i16>,
    // Return value publishes on successful dispatch, including signed ground heights.
    pub result: Array<u16>,
}
pub struct Call {
    pub entity: fix32,
    pub slot: u16,
    // Value is used for payload kind 1; all other vehicle payloads select zero.
    pub payload: u16,
    pub value: fix32,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn error(status: u16) -> fix32 {
    if status == 3 {
        return n(-3);
    }
    if status == 4 {
        return n(-4);
    }
    n(-1)
}
pub fn dispatch(
    scene: &scene_query::Scene,
    store: &mut scene_lifecycle::Store,
    runtime: &mut scene_world::Runtime,
    world: &scene_world::World,
    view: &View,
    call: &Call,
) -> fix32 {
    let class = entities::class(&store.identities, call.entity);
    if class < n(0) || class >= fix32::from_words(scene.header[4u16], 0) {
        return n(-1);
    }
    let saved = g(runtime.slot, 0);
    p(runtime.slot, 0, fix32::from_words(call.slot, 0));
    let method = scene.method(class.lo_bits(), call.slot);
    if method.status == 4 {
        return n(-1);
    }
    if method.status == 2 {
        p(view.result, 0, n(0));
        return n(0);
    }
    let mut result = n(0);
    if method.status == 1 {
        let mut status = 0u16;
        if method.id == 2 {
            if store.limit == n(0) {
                return n(-1);
            }
            status = lifecycle::enable(&mut runtime.ticks, call.entity);
            if status < 2 {
                result = fix32::from_words(status, 0);
                status = 0;
            }
        } else if method.id == 35 {
            status = lifecycle::disable(&mut runtime.ticks, call.entity, store.limit);
            if status < 2 {
                result = fix32::from_words(status, 0);
                status = 0;
            }
        } else if method.id == 36 {
            status = lifecycle::queue(
                &mut runtime.queues,
                &mut runtime.ticks,
                call.entity,
                true,
                store.limit,
            );
            result = call.entity;
        } else if method.id == 37 {
            status = entities::detach(&store.identities, call.entity);
            result = n(1);
        } else if method.id == 38 {
            status = entities::release_id(&store.identities, call.entity);
            result = n(1);
        } else if method.id == 32 || method.id == 33 || method.id == 34 {
            if method.id == 33 {
                result = n(1);
            } else if method.id == 34 {
                result = call.entity;
            }
        } else {
            if entities::handle_kind(&store.identities, call.entity) != 2 {
                return n(-1);
            }
            let mut value = n(0);
            if call.payload == 1 {
                value = call.value;
            }
            if method.id == 128 {
                if value == n(1) {
                    let code = dispatch(
                        scene,
                        store,
                        runtime,
                        world,
                        view,
                        &Call {
                            entity: call.entity,
                            slot: 4,
                            payload: 0,
                            value: n(0),
                        },
                    );
                    if code != n(0) {
                        return code;
                    }
                } else {
                    status = lifecycle::queue(
                        &mut runtime.queues,
                        &mut runtime.ticks,
                        call.entity,
                        false,
                        store.limit,
                    );
                    if status != 0 {
                        return n(-3);
                    }
                }
            } else {
                let id = entities::handle(&store.identities, call.entity);
                if !view.present
                    || id.hi_bits() != 0
                    || id.lo_bits() >= view.count
                    || !view.cars[id.lo_bits()].present
                {
                    return n(-4);
                }
                let car = &view.cars[id.lo_bits()];
                if method.id == 46 {
                    let mut disabled = n(0);
                    if value == n(1) {
                        disabled = n(1);
                    }
                    p(car.flags, 0, disabled);
                } else if method.id == 48 {
                    let mut disabled = n(1);
                    if value == n(1) {
                        disabled = n(0);
                    }
                    p(car.flags, 1, disabled);
                } else if method.id == 129 {
                    let mut driving = n(0);
                    if g(car.flags, 2) == n(0) {
                        driving = n(1);
                    }
                    p(car.flags, 2, driving);
                } else if method.id == 47 {
                    let routes = &view.routes[0u16];
                    let request = vehicle_transfer::Request {
                        limit: store.limit,
                        sine: view.sine,
                        angle: view.angle,
                        height: view.result,
                    };
                    let code = vehicle_transfer::restore(
                        &car.transfer,
                        routes,
                        world,
                        &store.identities,
                        &request,
                    );
                    if code != n(0) {
                        return code;
                    }
                    result = g(view.result, 0);
                } else {
                    return n(-5);
                }
            }
        }
        if status != 0 {
            return error(status);
        }
    }
    p(view.result, 0, result);
    p(runtime.slot, 0, saved);
    n(0)
}
