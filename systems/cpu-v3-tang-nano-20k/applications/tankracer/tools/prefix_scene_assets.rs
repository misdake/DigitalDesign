//! Compact prefix rows shared with the continuous driving owner.
use tankracer_game::{vehicle_runtime::SinglePlayer, vehicle_startup::Player};
fn wide(out: &mut Vec<u16>, value: i32) {
    out.extend([value as u16, (value >> 16) as u16]);
}
pub fn car(out: &mut Vec<u16>, player: &Player) {
    let s = &player.prefix;
    for value in [
        s.alternating_sign,
        s.countdown_throttle,
        s.countdown_rpm,
        s.target,
        i32::from(s.temporary_timer),
        i32::from(s.temporary_slot),
        i32::from(s.restore_sidegrip.multiplier),
        i32::from(s.restore_sidegrip.shift),
        i32::from(s.sidegrip.multiplier),
        i32::from(s.sidegrip.shift),
    ] {
        wide(out, value);
    }
    let spin = &s.spin;
    for value in [
        i32::from(spin.flags),
        i32::from(spin.visible),
        spin.hold,
        spin.color_timer,
        spin.color as i32,
        spin.target_color as i32,
        spin.angle,
        spin.speed,
        i32::from(spin.disabled),
    ] {
        wide(out, value);
    }
    let b = &s.boost;
    for value in [
        b.camera_velocity,
        b.camera_timer,
        b.target.0 as i32,
        i32::from(b.active),
        i32::from(b.applied),
        i32::from(b.phase),
    ] {
        wide(out, value);
    }
}
pub fn encode(world: &SinglePlayer) -> Vec<u16> {
    let mut out = Vec::new();
    for player in world.vehicles.entries[..2].iter().flatten() {
        car(&mut out, player);
    }
    assert_eq!(out.len(), 100);
    let shared = &world.shared;
    let v = &shared.view;
    for value in [i32::from(v.enabled), i32::from(v.visible)]
        .into_iter()
        .chain(v.position)
        .chain([v.angular_rate])
        .chain(v.matrix.0)
    {
        wide(&mut out, value);
    }
    for value in [
        shared.camera.pitch,
        shared.camera.height,
        shared.camera.distance,
    ] {
        wide(&mut out, value);
    }
    out.extend([u16::from(shared.spin.count), u16::from(shared.spin.pulse)]);
    for value in [
        shared.boost.texture_frame as i32,
        i32::from(shared.boost.texture_delta),
    ]
    .into_iter()
    .chain(shared.boost.source_uv.map(i32::from))
    .chain(shared.boost.current_uv.map(i32::from))
    .chain(shared.boost.feedback)
    {
        wide(&mut out, value);
    }
    wide(&mut out, shared.temporary.available as i32);
    for value in shared.temporary.timestamps {
        wide(&mut out, value as i32);
    }
    for value in shared.temporary.lines.iter().flatten() {
        wide(&mut out, value.0 as i32);
    }
    assert_eq!(out.len(), 238);
    out
}
