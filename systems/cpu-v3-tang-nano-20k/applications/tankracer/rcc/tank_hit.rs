//! Projectile contact impulse, hit counters, damage smoke and private mesh dents.
use crate::dsl_rt::*;
use crate::{attitude, collision, damage, integer, projectile_sweep, temporary};

pub struct Part {
    pub present: bool,
    pub target: damage::Target,
    // Four little-endian u16 words hold the presentation revision.
    pub revision: Array<u16>,
}
pub struct Vehicle {
    pub player: bool,
    pub visible: bool,
    pub inventory: Array<u16>,
    pub timer: Array<u16>,
    pub velocity: Array<u16>,
    pub angular: Array<u16>,
    pub external: Array<u16>,
    pub has_external: bool,
    pub collision_size: Array<u16>,
    // Twelve byte-valued counts, six lower then six upper.
    pub hit_counts: Array<u16>,
    // Damage severity[2], then authored socket indices[2].
    pub smoke: Array<u16>,
    // Body then turret; each damage target borrows the same one-shot flags.
    pub parts: Array<Part>,
    pub base_position: Array<u16>,
    // Sidegrip multiplier/shift, then the matching live drive-config columns.
    pub grip: Array<u16>,
    // Extended signed-word timer and slot, from the existing vehicle prefix.
    pub temporary: Array<u16>,
}
pub struct Shot {
    pub hit: Array<projectile_sweep::Hit>,
    pub velocity: Array<u16>,
    pub kind: u16,
    pub age: fix32,
    pub time: fix32,
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
fn revision(mut words: Array<u16>) {
    let mut i = 0u16;
    while i < 4 {
        words[i] += 1;
        if words[i] != 0 {
            return;
        }
        i += 1;
    }
}
// Shell: player turn, AI turn[3], player/AI speed, player/AI push, slow-after.
// Homer: player/AI turn, player/AI speed. Gains are multiplier/shift u16 pairs.
// Output: tilt, yaw, push, slow pairs and the slow-after word.
pub fn coefficients(
    mut out: Array<u16>,
    kind: u16,
    player: bool,
    cup: u16,
    shell: Array<u16>,
    homer: Array<u16>,
) {
    let mut speed = 8u16;
    let mut push = 12u16;
    if !player {
        speed = 10;
        push = 14;
    }
    let mut yaw_multiplier = 1u16;
    let mut yaw_shift = 0u16;
    if kind == 2 {
        let mut turn = 0u16;
        let mut slow = 4u16;
        if !player {
            turn = 2;
            slow = 6;
        }
        out[0u16] = 1;
        if !player {
            out[0u16] = 2;
        }
        out[1u16] = 0;
        yaw_multiplier = homer[turn];
        yaw_shift = homer[turn + 1];
        out[6u16] = homer[slow];
        out[7u16] = homer[slow + 1];
    } else {
        if kind == 0 {
            let mut turn = 0u16;
            if !player {
                turn = 2 + cup * 2;
            }
            yaw_multiplier = shell[turn];
            yaw_shift = shell[turn + 1];
        }
        out[0u16] = yaw_multiplier;
        out[1u16] = yaw_shift;
        out[6u16] = shell[speed];
        out[7u16] = shell[speed + 1];
    }
    out[2u16] = yaw_multiplier;
    out[3u16] = yaw_shift;
    out[4u16] = shell[push];
    out[5u16] = shell[push + 1];
    out[8u16] = shell[16u16];
}
pub fn contact(
    car: &Vehicle,
    hit: &projectile_sweep::Hit,
    velocity: Array<u16>,
    age: fix32,
    c: Array<u16>,
) -> fix32 {
    let weapon = g(car.inventory, 0);
    if weapon != n(1)
        && weapon != n(2)
        && weapon != n(7)
        && weapon != n(8)
        && g(car.timer, 0) < n(900)
    {
        p(car.timer, 0, n(900));
    }
    let incoming: Buf<u16, 6> = Buf::new([0; 6]);
    let mut i = 0u16;
    while i < 3 {
        p(incoming.as_array(), i, -g(velocity, i));
        i += 1;
    }
    collision::normalize(incoming.as_array());
    let mut amount = n(0);
    i = 0;
    while i < 3 {
        amount = amount
            + integer::trunc_shift(
                fmul_lo32(g(hit.world.normal.as_array(), i), g(incoming.as_array(), i)),
                12,
            );
        i += 1;
    }
    let normal = hit.local.normal.as_array();
    let point = hit.local.point.as_array();
    let mut axis = 0u16;
    let mut kind = 0u16;
    let mut turn = n(0);
    if g(normal, 0) != n(0) {
        axis = 2;
        kind = 1;
        turn = g(point, 2);
        if g(normal, 0) < n(0) {
            kind = 2;
            turn = -turn;
        }
    } else if g(normal, 1) != n(0) {
        kind = 3;
        if g(normal, 1) < n(0) {
            kind = 4;
        }
    } else if g(normal, 2) != n(0) {
        axis = 1;
        kind = 5;
        turn = -g(point, 0);
        if g(normal, 2) < n(0) {
            kind = 6;
            turn = g(point, 0);
        }
    }
    if axis != 0 && car.visible {
        let value = integer::trunc_shift(
            fmul_lo32(fmul_lo32(amount, fix32::from_words(c[0u16], 0)), n(200)),
            c[1u16] & 255,
        );
        let mut index = 0u16;
        if axis == 2 {
            index = 2;
        }
        if kind == 2 || kind == 5 {
            p(car.angular, index, g(car.angular, index) + value);
        } else {
            p(car.angular, index, g(car.angular, index) - value);
        }
    }
    turn = integer::trunc_shift(
        fmul_lo32(fmul_lo32(turn, fix32::from_words(c[2u16], 0)), n(110)),
        c[3u16] & 255,
    );
    p(car.angular, 1, g(car.angular, 1) + turn);
    if age > fix32::from_words(c[8u16], 0) || car.player {
        i = 0;
        while i < 3 {
            p(
                car.velocity,
                i,
                integer::trunc_shift(
                    fmul_lo32(g(car.velocity, i), fix32::from_words(c[6u16], 0)),
                    c[7u16] & 255,
                ),
            );
            i += 1;
        }
    }
    if !car.has_external {
        return n(-4);
    }
    let local: Buf<u16, 6> = Buf::new([0; 6]);
    collision::transform(local.as_array(), car.external, velocity, true);
    let impulse = integer::sar(
        fmul_lo32(g(local.as_array(), 0), fix32::from_words(c[4u16], 0)),
        c[5u16] & 255,
    );
    let scaled: Buf<u16, 6> = Buf::new([0; 6]);
    i = 0;
    while i < 3 {
        p(scaled.as_array(), i, integer::sar(g(car.velocity, i), 6));
        i += 1;
    }
    collision::transform(local.as_array(), car.external, scaled.as_array(), true);
    p(local.as_array(), 0, g(local.as_array(), 0) + impulse);
    collision::transform(scaled.as_array(), car.external, local.as_array(), false);
    i = 0;
    while i < 3 {
        p(car.velocity, i, fmul_lo32(g(scaled.as_array(), i), n(64)));
        i += 1;
    }
    if kind != 0 {
        let mut upper = 0u16;
        if g(point, 1) > n(1024) {
            upper = 1;
        }
        let mut counts = car.hit_counts;
        let index = upper * 6 + kind - 1;
        if counts[index] < 10 {
            let count = counts[index] + 1;
            counts[index] = count;
            let smoke = 1 - upper;
            let mut s = car.smoke;
            if count > 6 && count > s[smoke] {
                s[smoke] = count;
                s[smoke + 2] = kind - 1;
            }
            if count > 1 && car.player {
                let mut part = car.parts[upper];
                if !part.present || !part.target.mesh.present {
                    return n(-4);
                }
                let changed = damage::hit(&mut part.target, hit.world.point.as_array(), upper == 1);
                if changed > 1 {
                    return n(-4);
                }
                if changed == 1 {
                    revision(part.revision);
                }
            }
        }
    }
    if car.player {
        p(car.collision_size, 0, n(128));
    }
    n(0)
}
// The scene owner handles AI notifications at their separate dispatch boundary.
pub fn apply(
    car: &Vehicle,
    shot: &Shot,
    c: Array<u16>,
    shared: &temporary::State,
    ctx: &temporary::Context,
) -> fix32 {
    let status = contact(car, &shot.hit[0u16], shot.velocity, shot.age, c);
    if status != n(0) {
        return status;
    }
    if shot.kind == 5 {
        if !car.player {
            p(car.grip, 0, n(1));
            p(car.grip, 1, n(30));
            if shot.age > fix32::from_words(c[8u16], 0) {
                let mut i = 0u16;
                while i < 3 {
                    p(
                        car.velocity,
                        i,
                        integer::trunc_shift(
                            fmul_lo32(g(car.velocity, i), fix32::from_words(c[6u16], 0)),
                            c[7u16] & 255,
                        ),
                    );
                    i += 1;
                }
            }
        } else {
            let shift = g(car.grip, 1).lo_bits() + 1;
            p(car.grip, 1, fix32::from_words(shift, 0));
        }
        p(car.grip, 2, g(car.grip, 0));
        p(car.grip, 3, g(car.grip, 1));
        let timer = g(car.temporary, 0).lo_bits() as i16;
        let slot = unsafe { car.temporary.as_ptr().add(2).as_u16_array() };
        let status = temporary::start(shared, timer, slot, car.base_position, shot.time, ctx);
        if status != n(0) {
            return status;
        }
        p(car.temporary, 0, n(6000));
    }
    n(0)
}
