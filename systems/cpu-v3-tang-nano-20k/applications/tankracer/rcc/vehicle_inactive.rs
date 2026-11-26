//! Non-AI middle-distance Tank update over caller-owned vehicle fields.
use crate::dsl_rt::*;
use crate::{
    attitude, car_effects, collision, drive, ground, impact, integer, pickup_light, prefix, spin,
    temporary, terrain, track_effects, vehicle_debris, vehicle_pose, vehicle_shield,
};
// These views bind the same fields used by the full physical update. The
// contact and suspension strides allow either packed rows or the live owner.
pub struct State {
    pub poses: vehicle_pose::State,
    pub previous: Array<u16>,
    pub velocity: Array<u16>,
    pub angular: Array<u16>,
    pub acceleration: Array<u16>,
    pub pivot: Array<u16>,
    pub rates: Array<u16>,
    pub travel: Array<u16>,
    pub travel_stride: u16,
    pub hull: Array<u16>,
    pub sample: Array<u16>,
    pub local_velocity: Array<u16>,
    pub track_speed: Array<u16>,
    pub load: Array<u16>,
    pub vertical_delta: Array<u16>,
    pub boost: Array<u16>,
    pub core: Array<u16>,
    pub spin: Array<u16>,
    pub boost_control: Array<u16>,
    pub pose_config: Array<u16>,
    pub pose_input: Array<u16>,
    pub stored_input: Array<u16>,
    pub height: Array<u16>,
    pub spray: Array<u16>,
    pub smoke: Array<u16>,
    pub lights: Array<u16>,
    pub debris: Array<u16>,
    pub cooldown: Array<u16>,
    pub contacts: Array<u16>,
    pub contact_stride: u16,
    pub reverse_braking: Array<u16>,
    pub disabled: Array<u16>,
}
pub struct Input {
    pub entity: fix32,
    pub player: bool,
    // The instance drive config stays writable: temporary expiry restores grip.
    pub drive: Array<u16>,
    // Left track present, its max Z, geometry min/max X and min/max Z.
    pub hull: Array<u16>,
    pub pickup: Array<u16>,
    pub light_offset: Array<u16>,
    pub light_material: fix32,
    pub numerics: u16,
    pub dt: fix32,
    pub number: fix32,
    pub mode: fix32,
    // Device/frame controls are distinct from the car's stored drive input.
    pub throttle: fix32,
    pub brake: fix32,
}
fn n(v: i16) -> fix32 {
    integer::from_i16(v)
}
fn at(v: Array<u16>, word: u16) -> Array<u16> {
    unsafe { v.as_ptr().add(word as i16).as_u16_array() }
}
fn g(v: Array<u16>, i: u16) -> fix32 {
    attitude::get(v, i)
}
fn p(v: Array<u16>, i: u16, x: fix32) {
    attitude::put(v, i, x);
}
fn copy(mut dst: Array<u16>, src: Array<u16>, count: u16) {
    let mut i = 0u16;
    while i < count {
        dst[i] = src[i];
        i += 1;
    }
}
fn min(a: fix32, b: fix32) -> fix32 {
    if a < b {
        return a;
    }
    b
}
fn max(a: fix32, b: fix32) -> fix32 {
    if a > b {
        return a;
    }
    b
}
fn power(state: &State, c: Array<u16>, dt: fix32) {
    let s = state.local_velocity;
    let input = state.stored_input;
    let angular = state.angular;
    let rates = state.rates;
    let oldx = g(s, 0);
    let oldz = g(s, 2);
    let force = drive::gain(g(c, 41), c, 100);
    let force = fmul_lo32(
        integer::trunc_shift(
            fmul_lo32(fmul_lo32(force, g(state.load, 0)), g(c, 18)),
            g(c, 19).lo_bits() & 255,
        ),
        n(3),
    );
    p(s, 0, drive::damp_force(oldx, force, c, 41, dt));
    p(s, 1, drive::rate(g(s, 1), g(c, 32), dt));
    p(
        s,
        2,
        drive::damp_force(oldz, drive::gain(g(c, 20), c, 96), c, 20, dt),
    );
    p(angular, 0, drive::damp(g(angular, 0), c, 38, dt));
    p(angular, 2, drive::damp(g(angular, 2), c, 38, dt));
    p(angular, 1, drive::damp(g(angular, 1), c, 35, dt));
    p(rates, 2, g(s, 0));
    let mut track = n(0);
    if g(rates, 3) != n(0) {
        track = drive::damp(g(state.track_speed, 0), c, 29, dt);
    } else {
        track = drive::damp_force(
            g(state.track_speed, 0),
            drive::gain(g(c, 23), c, 96),
            c,
            23,
            dt,
        );
    }
    let target = g(s, 2);
    let budget = drive::gain(fmul_lo32(dt, g(c, 17)), c, 98);
    if target > n(0) && target > track {
        track = track + min(target - track, budget);
    } else if target < n(0) && -track < -target {
        track = track - min(track - target, budget);
    }
    let term = integer::sar(track, 12);
    let boosted = g(state.boost, 3) >= n(2501);
    let mut coefficient = g(c, 4);
    if boosted {
        coefficient = g(c, 48);
    }
    let mut cap = g(c, 3) - fmul_lo32(coefficient, fmul_lo32(term, term));
    if boosted {
        cap = cap + g(state.boost, 0);
    }
    cap = min(max(cap, n(0)), g(c, 1));
    let term = fmul_lo32(
        fmul_lo32(integer::sar(g(input, 0), 8), integer::sar(cap, 8)),
        dt,
    );
    track = track + integer::sar(term, 15);
    let term = integer::trunc_shift(
        fmul_lo32(
            fmul_lo32(
                fmul_lo32(n(4), integer::sar(g(c, 5), 23)),
                integer::sar(g(input, 1), 8),
            ),
            dt,
        ),
        2,
    );
    let before = track;
    if track < n(1) {
        track = track + term;
    } else {
        track = track - term;
    }
    if (before < n(1) && track > n(0)) || (before > n(0) && track < n(0)) {
        track = n(0);
    }
    track = min(max(track, -g(c, 0)), g(c, 0));
    p(state.track_speed, 0, track);
    let delta = track - target;
    let budget = drive::gain(fmul_lo32(dt, g(c, 16)), c, 98);
    let mut applied = delta;
    if delta > budget {
        applied = budget;
        p(rates, 3, delta - budget);
    } else if delta < -budget {
        applied = -budget;
        p(rates, 3, delta + budget);
    } else {
        p(rates, 3, n(0));
    }
    p(s, 2, target + applied);
    p(state.acceleration, 0, g(s, 0) - oldx);
    p(state.acceleration, 1, g(s, 2) - oldz);
    p(state.load, 0, integer::trunc_shift(g(state.load, 0), 8));
    let turn = integer::trunc_shift(fmul_lo32(g(c, 2), g(input, 2)), 8);
    let turn = drive::gain(fmul_lo32(turn, g(state.load, 0)), c, 6);
    p(angular, 1, g(angular, 1) - fmul_lo32(dt, turn));
    let term = fmul_lo32(g(c, 10), g(s, 0));
    let mut correction = -integer::sar(term, g(c, 11).lo_bits() & 255);
    if -term >= n(0) {
        correction = integer::sar(-term, g(c, 11).lo_bits() & 255);
    }
    p(angular, 1, g(angular, 1) + correction);
}
// The original kind-4 recovery requires an AI route owner. Non-AI vehicles
// return -1 there, after reflection and before integration, preserving writes.
pub fn step(
    state: &State,
    input: &Input,
    world: &mut ground::World,
    prepared: &prefix::Shared,
    shared: &mut car_effects::Shared,
    context: &mut impact::Context,
) -> fix32 {
    let core = state.core;
    let pose = state.pose_config;
    let sample = state.sample;
    let dt = input.dt;
    let player = input.player;
    context.frame = input.number;
    prefix::decay(state.boost, state.boost_control, dt, player);
    let spin = state.spin;
    if g(spin, 7) > n(0) {
        let tuning: Buf<u16, 12> =
            Buf::new([27955, 0, 16307, 0, 931, 0, 1, 2049, 1, 2305, 5, 3585]);
        let status = spin::step(
            spin,
            dt,
            tuning.as_array(),
            fix32::from_words(0, 1),
            prepared.spin,
            context.seed,
        );
        if status != n(0) {
            return status;
        }
    }
    let timer = n(g(core, 4).lo_bits() as i16);
    if timer != n(0) {
        if dt < timer {
            p(core, 4, n((timer - dt).lo_bits() as i16));
        } else {
            p(core, 4, n(0));
            p(core, 8, g(core, 6));
            p(core, 9, g(core, 7));
            p(input.drive, 18, g(core, 6));
            p(input.drive, 19, g(core, 7));
            let status = prefix::expire(prepared, n(g(core, 5).lo_bits() as i16));
            if status != n(0) {
                return status;
            }
        }
    }
    let vector: Buf<u16, 6> = Buf::new([0; 6]);
    let v = vector.as_array();
    collision::transform(v, state.poses.tracks, sample, false);
    p(sample, 3, g(state.poses.body_position, 0));
    p(sample, 4, g(v, 1) + g(state.poses.tracks, 10));
    p(sample, 5, g(state.poses.body_position, 2));
    p(state.pivot, 0, n(0));
    p(state.pivot, 1, n(0));
    p(state.pivot, 2, n(0));
    let mut hit = ground::zero_hit();
    let found = world.query(g(sample, 3), g(sample, 5), &mut hit);
    if found > 1 {
        return n(-2);
    }
    let mut height = g(state.height, 0);
    p(v, 0, n(0));
    p(v, 1, n(0));
    p(v, 2, n(0));
    if found == 1 {
        height = hit.height;
        p(v, 0, hit.nx);
        p(v, 1, hit.ny);
        p(v, 2, hit.nz);
        p(state.height, 0, height);
        p(sample, 6, n(1));
        p(sample, 90, n(((hit.special.lo_bits() & 1) * 2) as i16));
    } else {
        let mut normal = terrain::Normal {
            x: g(sample, 12),
            y: g(sample, 13),
            z: g(sample, 14),
        };
        let found = world.fallback(g(sample, 3), g(sample, 4), g(sample, 5), 0, &mut normal);
        if found > 1 {
            return n(-2);
        }
        p(sample, 12, normal.x);
        p(sample, 13, normal.y);
        p(sample, 14, normal.z);
        let mut kind = n(3);
        if found == 1 {
            kind = n(4);
        }
        p(sample, 6, kind);
    }
    p(sample, 9, height);
    p(state.poses.physical_position, 1, height + n(1400));
    collision::transform(v, state.poses.body_basis, v, true);
    let x = attitude::angle(context.angle, g(v, 1), g(v, 2));
    if x == n(-1) {
        return n(-2);
    }
    let z = attitude::angle(context.angle, -g(v, 0), g(v, 1));
    if z == n(-1) {
        return n(-2);
    }
    attitude::rotate(
        state.poses.body_basis,
        0,
        0,
        min(max(x - n(16384), n(-800)), n(800)),
        context.sine,
    );
    attitude::rotate(
        state.poses.body_basis,
        0,
        2,
        min(max(-z, n(-800)), n(800)),
        context.sine,
    );
    p(state.rates, 1, n(0));
    p(sample, 92, n(0));
    p(sample, 89, n(0));
    p(state.load, 0, n(14500));
    p(sample, 91, n(0));
    p(state.velocity, 1, n(0));
    p(sample, 98, n(1));
    p(sample, 99, n(1));
    let large = drive::large(state.velocity);
    drive::transform(
        state.velocity,
        state.poses.reference,
        state.local_velocity,
        false,
        large,
    );
    power(state, input.drive, dt);
    drive::transform(
        state.local_velocity,
        state.poses.reference,
        state.velocity,
        true,
        large,
    );
    p(
        state.velocity,
        1,
        g(state.velocity, 1) + g(state.vertical_delta, 0),
    );
    let kind = g(sample, 6);
    if kind == n(3) {
        p(state.velocity, 0, -g(state.velocity, 0));
        p(state.velocity, 2, -g(state.velocity, 2));
    }
    if kind == n(4) {
        let values: Buf<u16, 12> = Buf::new([0; 12]);
        copy(values.as_array(), at(sample, 24), 6);
        copy(at(values.as_array(), 6), state.velocity, 6);
        let tuning: Buf<u16, 16> = Buf::new([0; 16]);
        p(tuning.as_array(), 6, input.mode);
        let response: Buf<u16, 16> = Buf::new([0; 16]);
        if collision::respond(
            values.as_array(),
            tuning.as_array(),
            context.sine,
            context.angle,
            response.as_array(),
        ) != 0
        {
            return n(-2);
        }
        copy(at(sample, 24), response.as_array(), 6);
        copy(state.velocity, at(response.as_array(), 6), 6);
        return n(-1);
    }
    copy(state.previous, state.poses.physical_position, 6);
    let mut i = 0u16;
    while i < 3 {
        p(
            state.poses.physical_position,
            i,
            g(state.poses.physical_position, i)
                + integer::trunc_shift(fmul_lo32(g(state.velocity, i), dt), 14),
        );
        i += 1;
    }
    // Attitude is a source-local transaction; failed normalization retains only
    // the earlier ground alignment and integrated physical position.
    let attitude_state: Buf<u16, 58> = Buf::new([0; 58]);
    let a = attitude_state.as_array();
    copy(a, state.poses.reference, 18);
    copy(at(a, 18), state.poses.body_basis, 24);
    copy(at(a, 42), state.angular, 6);
    let controls: Buf<u16, 3> = Buf::new([0; 3]);
    let mut c = controls.as_array();
    p(c, 0, dt);
    // 65536 is 6 modulo 10; both u16 halves retain the full frame's phase.
    c[2u16] = (input.number.lo_bits() % 10 + (input.number.hi_bits() % 10) * 6) % 10;
    let flags: Buf<u16, 1> = Buf::new([0; 1]);
    let tuning: Buf<u16, 5> = Buf::new([0, 0, 0, 0, input.numerics]);
    if !attitude::step(
        a,
        tuning.as_array(),
        c,
        context.sine,
        context.angle,
        flags.as_array(),
    ) {
        return n(-2);
    }
    copy(state.poses.body_basis, at(a, 18), 24);
    copy(state.angular, at(a, 42), 6);
    copy(state.poses.body_position, state.poses.physical_position, 6);
    copy(state.poses.reference, state.poses.body_basis, 18);
    copy(state.poses.base_position, state.poses.physical_position, 6);
    let mut sum = n(0);
    i = 0;
    while i < 4 {
        sum = sum + drive::gain(g(state.travel, i * state.travel_stride), pose, 0);
        i += 1;
    }
    p(
        state.poses.base_position,
        1,
        g(state.poses.physical_position, 1) + integer::sar(sum, 3),
    );
    // Refresh the render parts from the current body without suspension wobble,
    // recoil, turret yaw, articulation or the full pose update's other stores.
    i = 0;
    while i < 6 {
        let mut part = state.poses.tracks;
        if i == 1 {
            part = at(part, 24);
        } else if i == 2 {
            part = state.poses.turret;
        } else if i == 3 {
            part = state.poses.gun;
        } else if i >= 4 {
            part = at(state.poses.attachments, (i - 4) * 24);
        }
        let origin = state.poses.body_position;
        copy(part, state.poses.body_basis, 18);
        collision::transform(at(part, 18), part, at(pose, 6 + i * 6), false);
        let mut axis = 0u16;
        while axis < 3 {
            let mut offset = g(origin, axis);
            if i == 3 {
                offset = g(at(state.poses.turret, 18), axis);
            }
            p(part, 9 + axis, g(part, 9 + axis) + offset);
            axis += 1;
        }
        i += 1;
    }
    collision::transform(
        state.poses.bottom_position,
        state.poses.body_basis,
        at(pose, 42),
        false,
    );
    i = 0;
    while i < 3 {
        p(
            state.poses.bottom_position,
            i,
            g(state.poses.bottom_position, i) + g(state.poses.body_position, i),
        );
        i += 1;
    }
    let spray = state.spray;
    if g(spray, 2) != n(0) {
        i = 0;
        while i < 8 {
            p(spray, 3 + i * 12 + 6, n(0));
            i += 1;
        }
        p(spray, 2, n(0));
    }
    let mut quad_states = shared.quads;
    let quads = &mut quad_states[0u16];
    let status = pickup_light::fade(
        &shared.pickup,
        &pickup_light::Frame {
            target: input.entity,
            position: state.poses.base_position,
            number: input.number,
            dt: dt,
        },
        input.pickup,
        quads,
        &context.pool,
    );
    if status != n(0) {
        return status;
    }
    let external = state.pose_input;
    let mut basis = state.poses.reference;
    if g(external, 8) == n(2) {
        basis = at(external, 18);
    }
    let status = track_effects::lights(
        state.lights,
        &track_effects::LightInput {
            player: player,
            disabled: g(state.disabled, 0) != n(0),
            active: true,
            visible: n(0),
            queued: false,
            reverse_braking: g(state.reverse_braking, 0) != n(0),
            material: input.light_material,
            throttle: input.throttle,
            brake: input.brake,
            forward: g(state.local_velocity, 2),
            position: state.poses.physical_position,
            basis: basis,
            has_basis: g(external, 8) != n(0),
            materials: at(sample, 196),
            heights: at(sample, 18),
            height_stride: 17,
            offset: input.light_offset,
        },
        quads,
        &context.pool,
    );
    if status != n(0) {
        return status;
    }
    let status = temporary::tick(
        &shared.temporary,
        g(core, 4).lo_bits() as i16,
        g(core, 5).lo_bits() as i16,
        state.poses.base_position,
        &temporary::Context {
            pool: view_of(&mut context.pool),
            seed: context.seed,
            sine: context.sine,
        },
    );
    if status != n(0) {
        return status;
    }
    vehicle_debris::clear(shared.debris, player, state.debris);
    let mut vehicle = impact::Vehicle {
        entity: input.entity,
        cooldown: state.cooldown,
        contacts: state.contacts,
        contact_stride: state.contact_stride,
        forward: g(state.local_velocity, 2),
        basis: basis,
        has_basis: g(external, 8) != n(0),
    };
    let mut states = shared.impacts;
    let status =
        car_effects::impact_status(impact::tick(&mut states[0u16], &mut vehicle, dt, context));
    if status != n(0) {
        return status;
    }
    let smoke = state.smoke;
    if g(smoke, 3) != n(0) {
        i = 0;
        while i < 15 {
            p(smoke, 10 + i * 11 + 6, n(0));
            i += 1;
        }
        p(smoke, 3, n(0));
    }
    let mut bank = n(0);
    if player {
        bank = n(1);
    }
    let status = vehicle_shield::tick(
        shared.shield,
        &vehicle_shield::Input {
            enabled: g(sample, 82) != n(0),
            bank: bank,
            dt: dt,
            position: state.poses.base_position,
            basis: basis,
            has_basis: g(external, 8) != n(0),
        },
        context.seed,
        context.sine,
    );
    if status != n(0) {
        return status;
    }
    p(state.hull, 0, g(state.poses.physical_position, 0));
    p(state.hull, 1, g(sample, 9));
    p(state.hull, 2, g(state.poses.physical_position, 2));
    copy(at(state.hull, 6), state.poses.reference, 18);
    if g(input.hull, 0) == n(0) {
        return n(-4);
    }
    let half = g(input.hull, 1);
    let mut vertices = at(state.hull, 24);
    i = 0;
    while i < 4 {
        let mut x = g(input.hull, 2);
        if i == 1 || i == 2 {
            x = g(input.hull, 3);
        }
        let mut z = n(-8);
        if i >= 2 {
            z = n(8);
        }
        vertices[i * 3] = integer::sar(x, 3).lo_bits();
        vertices[i * 3 + 1] = 0;
        vertices[i * 3 + 2] = integer::sar(fmul_lo32(half, z), 3).lo_bits();
        i += 1;
    }
    let rx = max(g(input.hull, 2).abs(), g(input.hull, 3).abs());
    let rz = max(g(input.hull, 4).abs(), g(input.hull, 5).abs());
    let mut radius = integer::sar(rx, 1) + rz;
    if rx > rz {
        radius = integer::sar(rz, 1) + rx;
    }
    let mut bytes = state.hull;
    bytes[36u16] = integer::sar(radius, 1).lo_bits();
    n(0)
}
