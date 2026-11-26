//! Moving-car sweeps in original racer-slot order, retaining local/world hits.
use crate::dsl_rt::*;
use crate::{attitude, collision, integer, object_query};
pub struct Car {
    pub entity: fix32,
    pub present: bool,
    pub disabled: bool,
    pub visible: bool,
    pub ai_disabled: bool,
    pub position: Array<u16>,
    pub basis: Array<u16>,
    pub turret: Array<u16>,
    pub velocity: Array<u16>,
    pub bounds: Array<u16>,
    pub radius: fix32,
}
pub struct Cars {
    pub records: Array<Car>,
    pub count: u16,
    pub slots: Array<u16>,
    pub present: bool,
}
pub struct Hit {
    pub vehicle: fix32,
    pub world: object_query::SegmentHit,
    pub local: object_query::SegmentHit,
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
fn abs(v: fix32) -> fix32 {
    if v < n(0) {
        return -v;
    }
    v
}
pub fn zero() -> Hit {
    Hit {
        vehicle: n(-1),
        world: object_query::SegmentHit {
            point: Buf::new([0; 6]),
            normal: Buf::new([0; 6]),
        },
        local: object_query::SegmentHit {
            point: Buf::new([0; 6]),
            normal: Buf::new([0; 6]),
        },
    }
}
pub fn find(cars: &Cars, entity: fix32) -> u16 {
    if !cars.present {
        return 65535;
    }
    let mut i = 0u16;
    while i < cars.count {
        if cars.records[i].present && cars.records[i].entity == entity {
            return i;
        }
        i += 1;
    }
    65535
}
pub fn query(old: Array<u16>, current: Array<u16>, dt: fix32, car: &Car, out: &mut Hit) -> fix32 {
    let delta: Buf<u16, 6> = Buf::new([0; 6]);
    let mut i = 0u16;
    while i < 3 {
        p(delta.as_array(), i, g(current, i) - g(old, i));
        i += 1;
    }
    let length = collision::distance(delta.as_array());
    i = 0;
    while i < 3 {
        if abs(g(car.position, i) - g(old, i)) - length >= car.radius {
            return n(0);
        }
        i += 1;
    }
    let a: Buf<u16, 6> = Buf::new([0; 6]);
    let b: Buf<u16, 6> = Buf::new([0; 6]);
    let relative: Buf<u16, 6> = Buf::new([0; 6]);
    i = 0;
    while i < 3 {
        p(relative.as_array(), i, g(old, i) - g(car.position, i));
        i += 1;
    }
    collision::transform(a.as_array(), car.basis, relative.as_array(), true);
    i = 0;
    while i < 3 {
        p(
            relative.as_array(),
            i,
            g(current, i)
                - integer::sar(fmul_lo32(g(car.velocity, i), dt), 14)
                - g(car.position, i),
        );
        i += 1;
    }
    collision::transform(b.as_array(), car.basis, relative.as_array(), true);
    let mut hit = object_query::SegmentHit {
        point: Buf::new([0; 6]),
        normal: Buf::new([0; 6]),
    };
    let mut code = object_query::segment_box(a.as_array(), b.as_array(), car.bounds, &mut hit);
    if code == 3 {
        return n(-2);
    }
    if code == 2 {
        let direction: Buf<u16, 6> = Buf::new([0; 6]);
        i = 0;
        while i < 3 {
            p(
                direction.as_array(),
                i,
                g(b.as_array(), i) - g(a.as_array(), i),
            );
            i += 1;
        }
        if collision::distance(direction.as_array()) > n(2) {
            collision::normalize(direction.as_array());
            let behind: Buf<u16, 6> = Buf::new([0; 6]);
            i = 0;
            while i < 3 {
                p(
                    behind.as_array(),
                    i,
                    g(a.as_array(), i)
                        - fmul_lo32(g(direction.as_array(), i), integer::sar(car.radius, 11)),
                );
                i += 1;
            }
            code = object_query::segment_box(behind.as_array(), a.as_array(), car.bounds, &mut hit);
            if code == 3 {
                return n(-2);
            }
        }
    }
    if code != 1 {
        return n(0);
    }
    let normal: Buf<u16, 6> = Buf::new([0; 6]);
    collision::transform(normal.as_array(), car.basis, hit.normal.as_array(), false);
    collision::normalize(delta.as_array());
    let mut dot = n(0);
    i = 0;
    while i < 3 {
        dot = dot
            + integer::trunc_shift(
                fmul_lo32(g(delta.as_array(), i), g(normal.as_array(), i)),
                12,
            );
        i += 1;
    }
    if dot >= n(0) {
        return n(0);
    }
    collision::transform(
        out.world.point.as_array(),
        car.basis,
        hit.point.as_array(),
        false,
    );
    i = 0;
    while i < 3 {
        p(
            out.world.point.as_array(),
            i,
            g(out.world.point.as_array(), i) + g(car.position, i),
        );
        p(out.world.normal.as_array(), i, g(normal.as_array(), i));
        p(out.local.point.as_array(), i, g(hit.point.as_array(), i));
        p(out.local.normal.as_array(), i, g(hit.normal.as_array(), i));
        i += 1;
    }
    n(1)
}
// The owner is optional; owner ID zero is a valid distinct value when present.
pub fn cars(
    cars: &Cars,
    owner: fix32,
    has_owner: bool,
    old: Array<u16>,
    current: Array<u16>,
    frame: &mut Frame,
) -> fix32 {
    if !cars.present {
        return n(0);
    }
    let mut slot = 0u16;
    while slot < 9 {
        let id = cars.slots[slot];
        if id == 65535 {
            slot += 1;
            continue;
        }
        if id >= cars.count || !cars.records[id].present {
            return n(-4);
        }
        let car = &cars.records[id];
        if (has_owner && car.entity == owner) || car.disabled {
            slot += 1;
            continue;
        }
        let status = query(old, current, frame.dt, car, &mut frame.hit[0u16]);
        if status != n(0) {
            if status == n(1) {
                frame.hit[0u16].vehicle = fix32::from_words(id, 0);
            }
            return status;
        }
        slot += 1;
    }
    n(0)
}
pub struct Frame {
    pub dt: fix32,
    pub hit: Array<Hit>,
}
