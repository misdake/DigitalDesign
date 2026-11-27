//! Canonical projectile records shared by method and continuous-world qualifiers.
use tankracer_game::{explosion::Sprite, projectile};
pub fn wide(out: &mut Vec<u16>, v: i32) {
    out.extend([v as u16, (v >> 16) as u16]);
}
pub fn ints(out: &mut Vec<u16>, v: impl IntoIterator<Item = i32>) {
    for x in v {
        wide(out, x);
    }
}
pub fn smoker(out: &mut Vec<u16>, s: &tankracer_game::actor_startup::smoker::Smoker) {
    ints(
        out,
        [
            i32::from(s.white),
            i32::from(s.billow),
            s.timer,
            i32::from(s.visible),
            s.material as i32,
        ],
    );
    for p in s.particles.iter() {
        ints(out, [i32::from(p.spin)]);
        ints(out, p.velocity);
        ints(out, p.size);
        ints(out, [i32::from(p.rotation)]);
        ints(out, p.position);
        ints(
            out,
            [
                i32::from(p.active),
                i32::from(p.clockwise),
                i32::from(p.life),
            ],
        );
    }
}
pub const FLOCK_IDS: [usize; 3] = [183, 184, 185];
pub fn flock(out: &mut Vec<u16>, f: &tankracer_game::actor_startup::flock::Flock) {
    let start = out.len();
    let t = &f.flight;
    ints(out, [i32::from(t.phase), t.stage as i32, t.timer]);
    ints(out, t.target);
    for v in t.velocities {
        ints(out, v);
    }
    ints(out, t.wings_up.map(i32::from));
    for b in f.birds {
        ints(out, b.pose.position);
        ints(out, b.pose.matrix.0);
        ints(out, [b.speed, i32::from(b.active)]);
    }
    assert_eq!(out.len() - start, 288);
}
pub const TRAIN_IDS: [usize; 2] = [125, 239];
pub fn train(out: &mut Vec<u16>, train: &tankracer_game::actor_startup::train::Train) {
    let t = &train.travel;
    ints(
        out,
        [
            i32::from(t.active),
            i32::from(t.running),
            i32::from(t.waiting),
            t.wait,
            t.angle,
        ],
    );
    ints(out, t.tilt);
    ints(out, t.angular_velocity);
    ints(out, [i32::from(t.sound_active), i32::from(t.lander_flying)]);
    ints(out, t.lander_direction);
    ints(out, t.lander_velocity);
    ints(out, [train.speed, train.range]);
    let tankracer_game::actor_startup::train::Motion::Path {
        start,
        end,
        velocity,
        ..
    } = train.motion
    else {
        panic!("first-map Train path")
    };
    ints(out, start);
    ints(out, end);
    ints(out, velocity);
}
pub const EMITTER_IDS: [usize; 5] = [81, 112, 113, 114, 115];
pub fn emitter(out: &mut Vec<u16>, e: &tankracer_game::actor_startup::emitter::Emitter) {
    ints(out, [e.timer, i32::from(e.gun), e.size]);
    ints(out, e.offset);
    ints(out, [e.pitch, e.trigger as i32]);
    ints(out, e.bounds);
    ints(out, [e.period]);
    ints(out, e.color.map(i32::from));
    ints(out, [i32::from(e.alpha)]);
    ints(out, e.glow);
    ints(
        out,
        [
            i32::from(e.asteroid),
            i32::from(e.mesh.is_some()),
            e.mesh.map_or(0, |v| v.index() as i32),
            i32::from(e.material.is_some()),
            e.material.unwrap_or(0) as i32,
        ],
    );
}
pub fn mine(out: &mut Vec<u16>, state: &tankracer_game::mine::State) {
    ints(
        out,
        [
            i32::from(state.owner.is_some()),
            state.owner.map_or(0, |v| v.0 as i32),
            state.kind as i32,
            i32::from(state.count),
            i32::from(state.repeat),
            i32::from(state.armed),
            state.born as i32,
            state.last_contact as i32,
            state.elapsed,
        ],
    );
}
pub fn ufo(out: &mut Vec<u16>, s: &tankracer_game::ufo::State) {
    let start = out.len();
    ints(out, [s.target.0 as i32, s.flags as i32]);
    ints(out, s.destination);
    ints(out, s.point);
    ints(
        out,
        [
            s.departure_speed,
            i32::from(s.arrived),
            i32::from(s.timer),
            i32::from(s.fade),
            i32::from(s.view),
            i32::from(s.reserved.is_some()),
            s.reserved.map_or(0, |v| v.0 as i32),
            i32::from(s.was_special),
            i32::from(s.was_collidable),
        ],
    );
    sprite(out, s.sprite);
    assert_eq!(out.len() - start, 56);
}
pub fn sprite(out: &mut Vec<u16>, s: Sprite) {
    ints(out, [s.material as i32]);
    ints(out, s.position);
    ints(out, [s.flags as i32, s.depth_bias]);
    ints(out, s.size);
    out.extend([
        u16::from(s.uv[0]) | (u16::from(s.uv[1]) << 8),
        u16::from(s.uv[2]) | (u16::from(s.uv[3]) << 8),
        u16::from(s.color[0]) | (u16::from(s.color[1]) << 8),
        u16::from(s.color[2]),
        s.rotation,
        0,
    ]);
}
pub fn state(out: &mut Vec<u16>, s: &projectile::State) {
    ints(
        out,
        [
            i32::from(s.owner.is_some()),
            s.owner.map_or(0, |x| x.0 as i32),
            s.kind as i32,
            s.time,
            s.flags as i32,
        ],
    );
    ints(out, s.velocity);
    ints(
        out,
        [
            i32::from(s.trail_count),
            i32::from(s.trail_cursor),
            s.trail_time,
            i32::from(s.flash_views[0]),
            i32::from(s.flash_views[1]),
            i32::from(s.flash_alpha),
            i32::from(s.target.is_some()),
            s.target.map_or(0, |x| x.0 as i32),
            s.target_height,
        ],
    );
    ints(out, s.previous);
    ints(
        out,
        [
            i32::from(s.reference.is_some()),
            s.pitch,
            i32::from(s.roll),
            i32::from(s.wobble),
            s.uv_time,
            i32::from(s.attached_ufo.is_some()),
            s.attached_ufo.map_or(0, |x| x.0 as i32),
        ],
    );
    ints(out, s.attachment);
    ints(out, s.reference.map_or([0; 9], |m| m.0));
    sprite(out, s.sprite);
    for t in s.trails {
        out.extend([u16::from(t.active), t.time as u16]);
        ints(out, t.velocity);
        wide(out, t.angular);
        sprite(out, t.sprite);
        wide(out, t.channel as i32);
    }
}
pub fn flight(out: &mut Vec<u16>, shared: &projectile::Flight) {
    ints(
        out,
        [
            shared.sequence as i32,
            shared.homing_drift,
            shared.homing_frame as i32,
            i32::from(shared.size_up),
            i32::from(shared.color_up),
        ],
    );
    sprite(out, shared.homing_glow);
    sprite(out, shared.concussion_glow);
}

pub fn metadata(world: &tankracer_game::vehicle_runtime::SinglePlayer, id: usize) -> [u16; 30] {
    let object = world.objects.components.objects[id].as_ref().unwrap();
    let mut meta = [0u16; 30];
    meta[0] = (id + 1) as u16;
    meta[1] = object.data.sort_position as u16;
    meta[5] = object.data.visibility.distance as u16;
    if let Some(emitter) = object.emitter {
        meta[7] = 1;
        meta[8] = emitter.0 as u16;
        meta[9] = (emitter.0 >> 16) as u16;
    }
    if let Some(mesh) = object.data.mesh {
        meta[2] = 1;
        meta[3] = mesh.index() as u16;
        meta[4] = (mesh.index() >> 16) as u16;
    }

    meta
}

pub fn explosion(out: &mut Vec<u16>, s: &tankracer_game::explosion::State) {
    let start = out.len();
    ints(
        out,
        [
            i32::from(s.slot),
            s.timer,
            s.angular_velocity,
            i32::from(s.visible),
            s.smoke_timer,
            i32::from(s.smoke_count),
            s.band as i32,
            s.ring as i32,
            i32::from(s.owner.is_some()),
            s.owner.map_or(0, |v| v.0 as i32),
        ],
    );
    ints(out, s.velocity);
    ints(out, s.attachment);
    ints(
        out,
        [
            i32::from(s.fragments_enabled),
            i32::from(s.fragment_count),
            s.fragment_time,
            i32::from(s.fragment_vehicle.is_some()),
            s.fragment_vehicle.map_or(0, |v| v.0 as i32),
        ],
    );
    out.extend([
        u16::from(s.fragment_uv[0]) | (u16::from(s.fragment_uv[1]) << 8),
        u16::from(s.fragment_uv[2]) | (u16::from(s.fragment_uv[3]) << 8),
    ]);
    sprite(out, s.sprite);
    for puff in s.smoke {
        sprite(out, puff.sprite);
        ints(out, puff.velocity);
        ints(out, [puff.lift, puff.angular_velocity]);
        out.extend([puff.active, 0]);
    }
    for f in s.fragments {
        ints(out, f.position);
        ints(out, f.velocity);
        out.extend(f.size.map(|v| v as u16));
        ints(out, [f.angular]);
        out.extend([f.rotation, 0]);
    }
    assert_eq!(out.len() - start, 676);
}
