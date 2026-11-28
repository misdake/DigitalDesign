//! Authored pickup bindings and live flight, inventory and shared light state.
use super::pickup_snapshot;
use serde_json::{Value, json};
use tankracer_game::{
    actor_startup::Actor, object_query::ObstacleId, vehicle_runtime::SinglePlayer,
};
pub fn configuration(world: &SinglePlayer) -> Vec<Value> {
    let mut out = Vec::new();
    for id in 0..world.objects.world.colliders.len() {
        let Some(Actor::Powerup(_)) = world.objects.components.actors.get(ObstacleId(id)) else {
            continue;
        };
        let o = world.objects.components.objects[id].as_ref().unwrap();
        let config = o.data.auxiliary.as_ref().unwrap();
        let mut gains = Vec::new();
        for g in [
            config.bounce,
            config.friction,
            config.push,
            config.velocity,
            config.spin,
        ] {
            gains.extend([g.multiplier, u16::from(g.shift)]);
        }
        out.push(json!({"object":id,"kind":3,"name":config.shape_name,"gravity":config.gravity,"gains":gains,
            "current_mesh":o.data.mesh.map_or(-1,|m|m.index() as i32),"private_mesh":o.auxiliary_mesh.map_or(-1,|m|m.index() as i32),
            "active":o.auxiliary_active,"motion_count":o.auxiliary_motion.as_ref().map_or(0,|m|m.faces.len()),"visibility":o.data.visibility.distance,"reference_alias":false,"attached_count":0}));
    }
    out
}
pub fn flights(world: &SinglePlayer) -> Vec<i32> {
    let mut out = Vec::new();
    for id in 0..world.objects.world.colliders.len() {
        let Some(Actor::Powerup(p)) = world.objects.components.actors.get(ObstacleId(id)) else {
            continue;
        };
        let f = &p.flight;
        out.extend(f.angular);
        out.extend(f.normal);
        out.extend([
            f.blink_time,
            f.blink_count as i32,
            f.delay,
            i32::from(f.disabled),
            i32::from(f.parachute.is_some()),
            p.ground,
            p.fall_range,
            p.regeneration,
        ]);
        if let Some(q) = f.parachute {
            out.extend(q.matrix.0);
            out.extend(q.position);
        } else {
            out.extend([0; 12]);
        }
        out.push(p.kind as i32);
    }
    assert_eq!(out.len(), 810);
    out
}
pub fn capture(world: &SinglePlayer) -> Vec<u16> {
    let mut values = flights(world);
    for entry in &world.vehicles.entries[..2] {
        let p = entry.as_ref().unwrap();
        values.extend([
            p.inventory.weapon as i32,
            i32::from(p.inventory.ammo),
            p.physics.contact.response.timer,
        ]);
    }
    values.extend(pickup_snapshot::values(
        &world.shared.pickup,
        &world.shared.pool,
    ));
    values
        .into_iter()
        .flat_map(|v| [v as u16, (v >> 16) as u16])
        .collect()
}
