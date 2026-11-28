//! Read-only finish-driving data from the constructed game's route and tuning.
use tankracer_game::vehicle_startup::Player;
use tankracer_game::{symbols::Binding, vehicle_runtime::SinglePlayer, waypoint::Point};

pub fn point(point: &Point) -> Vec<i32> {
    let mut out = vec![1];
    for binding in &point.next {
        out.extend(match binding {
            Binding::Direct(None) => [0, 0],
            Binding::Direct(Some(entity)) => [1, entity.0 as i32],
            Binding::Named(_) => [2, 0],
        });
    }
    out.extend([
        i32::from(point.activate_lap),
        i32::from(point.braking_range),
        point.gate_size,
        point.max_speed,
        point.width,
        i32::from(point.lap),
    ]);
    assert_eq!(out.len(), 11);
    out
}

pub fn tuning(world: &SinglePlayer) -> Vec<i32> {
    let config = world.vehicles.ai_config[world.vehicles.cup * 5];
    let global = world.vehicles.ai_settings;
    let gain = global.damage_model[world.vehicles.cup];
    let mut out = vec![
        config.acceleration,
        config.braking_power,
        config.look_ahead,
        gain.multiplier,
        gain.shift as i32,
        global.reverse_time,
    ];
    for rules in [
        &config.steering[..],
        &config.inner_steering[..],
        &config.speed_steering[..],
    ] {
        for rule in rules {
            out.extend([rule.threshold, rule.multiplier, rule.shift as i32]);
        }
    }
    out.extend([
        config.max_speed[world.vehicles.difficulty],
        global.debunch_accel.multiplier,
        global.debunch_accel.shift as i32,
        global.debunch_slow.multiplier,
        global.debunch_slow.shift as i32,
    ]);
    assert_eq!(out.len(), 62);
    out
}

pub fn encode(world: &SinglePlayer) -> Vec<u16> {
    let points = &world.objects.components.waypoints.points;
    let count = world.objects.world.colliders.len();
    assert!(points.len() <= count && count <= 240);
    let mut out = vec![0u16; 16 + count];
    out[16..].fill(65535);
    let mut compact = 0u16;
    for (id, value) in points.iter().enumerate() {
        if let Some(value) = value {
            out[16 + id] = compact;
            for value in point(value) {
                out.extend([value as u16, (value >> 16) as u16]);
            }
            compact += 1;
        }
    }
    let config_offset = out.len();
    for value in tuning(world) {
        out.extend([value as u16, (value >> 16) as u16]);
    }
    let header = [
        0x4644,
        1,
        out.len() as u16,
        count as u16,
        compact,
        16,
        (16 + count) as u16,
        config_offset as u16,
        124,
        world.vehicles.cup as u16,
        world.vehicles.difficulty as u16,
        world.vehicles.ai_config[world.vehicles.cup * 5].on_rails_speed[world.vehicles.difficulty]
            as u16,
        (world.vehicles.ai_config[world.vehicles.cup * 5].on_rails_speed[world.vehicles.difficulty]
            >> 16) as u16,
    ];
    out[..header.len()].copy_from_slice(&header);
    out
}

pub fn state(car: &Player) -> Vec<i32> {
    let a = car.ai.unwrap_or_default();
    let mut out = vec![
        i32::from(a.vehicle),
        i32::from(a.colour),
        a.flags as i32,
        a.spacing as i32,
    ];
    out.extend(a.look_position);
    out.extend([
        i32::from(a.look_checkpoint.is_some()),
        a.look_checkpoint.map_or(0, |e| e.0 as i32),
        i32::from(a.drive_checkpoint.is_some()),
        a.drive_checkpoint.map_or(0, |e| e.0 as i32),
        a.look_branch as i32,
        a.drive_branch as i32,
        a.distance,
        a.turn_time,
        a.reverse_time,
        a.stuck_time,
        a.stuck_distance,
        a.confusion,
        a.anger,
        i32::from(a.target.is_some()),
        a.target.map_or(0, |e| e.0 as i32),
        a.awareness,
        a.aim_time,
        a.warn_time,
        a.mine_time,
        a.view_timer,
        a.view_span,
        a.behind_span,
        a.shell_error,
    ]);
    assert_eq!(out.len(), 30);
    out
}
