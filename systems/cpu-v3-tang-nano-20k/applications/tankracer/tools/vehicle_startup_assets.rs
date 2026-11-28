//! Immutable assembled-model metadata and authored spawn for target construction.
use tankracer_assets::config::Part;
use tankracer_game::{
    dispatch::ClassId, object_query::EntityId, pose::Pose, vehicle_model::VehicleModel,
    vehicle_startup::Environment,
};
pub const WORDS: usize = 200;
fn ints(out: &mut Vec<u16>, values: impl IntoIterator<Item = i32>) {
    for v in values {
        out.extend([v as u16, (v >> 16) as u16]);
    }
}
pub fn encode(
    model: &VehicleModel,
    spawn: Pose,
    env: Environment,
    entity: EntityId,
    class: ClassId,
    player: bool,
) -> Vec<u16> {
    let c = model.pose_config([0; 3]);
    let mut out = Vec::new();
    ints(
        &mut out,
        [
            i32::from(c.travel_gain.multiplier),
            i32::from(c.travel_gain.shift),
            c.track_length,
        ],
    );
    ints(&mut out, c.track_offsets.into_iter().flatten());
    ints(&mut out, c.turret_offset);
    ints(&mut out, c.gun_offset);
    ints(&mut out, c.attachment_offsets.into_iter().flatten());
    ints(&mut out, c.bottom_offset);
    ints(&mut out, c.gun_pivot);
    ints(&mut out, model.primary_local.into_iter().flatten());
    assert_eq!(out.len(), 78);
    let shadow = model.shadow.geometry();
    out.extend(shadow.vertices.iter().take(4).flatten().map(|v| *v as u16));
    out.resize(90, 0);
    ints(
        &mut out,
        [
            i32::from(model.reset_sidegrip.multiplier),
            i32::from(model.reset_sidegrip.shift),
            i32::from(model.config.tank.drive.sidegrip_fraction.multiplier),
            i32::from(model.config.tank.drive.sidegrip_fraction.shift),
            i32::from(model.alternate_control[0]),
            i32::from(model.alternate_control[1]),
        ],
    );
    ints(&mut out, spawn.matrix.0);
    ints(&mut out, spawn.position);
    ints(
        &mut out,
        [
            entity.0 as i32,
            u32::try_from(class.0).unwrap() as i32,
            i32::from(player),
            i32::from(env.ambient_wobble),
            i32::from(env.session_active),
            env.ambient_phase as i32,
        ],
    );
    ints(&mut out, env.gun_pivot);
    assert_eq!(out.len(), 144);
    out.extend([
        u16::try_from(shadow.vertices.len()).unwrap(),
        model.shadow.radius,
        0,
    ]);
    if let Some(gun) = &model.parts[Part::Gun.slot()] {
        let vertices = &gun.geometry().vertices;
        out[146] = u16::try_from(vertices.len() * 3).unwrap();
        out.extend(vertices.iter().flatten().map(|v| *v as u16));
    }
    assert!(out.len() <= WORDS);
    out.resize(WORDS, 0);
    out
}
