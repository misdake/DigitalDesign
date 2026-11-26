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
