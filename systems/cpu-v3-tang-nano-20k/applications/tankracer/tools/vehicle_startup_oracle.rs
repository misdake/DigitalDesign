//! Real Player::construct from assembled original vehicle models.
use super::{car_motion_oracle, ints, track_effects_oracle, vehicle_pose_oracle};
use serde_json::json;
use std::{fs, path::Path};
use tankracer_game::{
    dispatch::ClassId,
    math::Matrix,
    object_query::EntityId,
    pose,
    vehicle_runtime::SinglePlayer,
    vehicle_startup::{Environment, Player},
};

pub(super) fn snapshot(car: &Player) -> Vec<u16> {
    let v = &car.physics;
    let mut out = car_motion_oracle::snapshot(&v.motion, &[[0; 3]; 6]);
    out.truncate(339);
    let p = &v.powertrain;
    ints(&mut out, p.local_velocity);
    ints(
        &mut out,
        [
            p.track_speed,
            p.boost_target,
            p.engine_display,
            i32::from(p.was_active),
            i32::from(p.gear),
            p.load,
        ],
    );
    assert_eq!(out.len(), 357);
    let s = &v.contact.response;
    for p in s.points {
        ints(
            &mut out,
            [p.kind, p.gap, p.corner as i32, p.travel, p.rate, p.output],
        );
    }
    ints(&mut out, s.angular);
    ints(&mut out, s.tilt);
    ints(
        &mut out,
        [
            s.penetrating_input_sum,
            s.travel_sum,
            s.vertical_delta,
            s.world_vy,
            s.position_y,
            s.timer,
        ],
    );
    ints(&mut out, s.auxiliary_type);
    ints(&mut out, s.auxiliary_cached);
    ints(&mut out, [s.max_penetration, s.last_penetration]);
    assert_eq!(out.len(), 437);
    let s = &v.contact.sampling;
    for (i, p) in s.primary.points.iter().enumerate() {
        ints(&mut out, p.local);
        ints(&mut out, p.world);
        ints(
            &mut out,
            [
                p.kind,
                p.gap,
                p.previous_gap,
                p.raw_height,
                p.travel,
                p.flags as i32,
            ],
        );
        ints(&mut out, p.normal);
        assert!(s.primary.nodes[i].is_none());
        ints(&mut out, [s.primary.surfaces[i], -1]);
    }
    for a in &s.auxiliary {
        ints(&mut out, a.world);
        ints(&mut out, [a.kind, a.surface, a.cached]);
    }
    let e = &s.effect;
    ints(
        &mut out,
        [
            e.timer,
            e.restore,
            e.active,
            e.phase,
            e.impulse[0],
            e.impulse[1],
            e.pose,
            i32::from(e.clear_player_slot),
        ],
    );
    let p = &s.primary;
    ints(
        &mut out,
        [
            p.load,
            p.airborne_count,
            p.special_count,
            p.flagged_count,
            i32::from(p.has_extra),
        ],
    );
    ints(&mut out, p.extra);
    ints(
        &mut out,
        [
            p.flagged_ground_count,
            s.materials[0],
            s.materials[1],
            s.fallback_mode,
            i32::from(s.feedback_required),
            s.clear_player_slot.map_or(-1, i32::from),
        ],
    );
    assert_eq!(out.len(), 643);
    let s = &car.prefix;
    let b = s.boost;
    ints(
        &mut out,
        [b.extra_speed, b.power, b.power_loss, i32::from(b.timer)],
    );
    ints(
        &mut out,
        [
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
        ],
    );
    let t = s.spin;
    ints(
        &mut out,
        [
            i32::from(t.flags),
            i32::from(t.visible),
            t.hold,
            t.color_timer,
            t.color as i32,
            t.target_color as i32,
            t.angle,
            t.speed,
            i32::from(t.disabled),
        ],
    );
    ints(
        &mut out,
        [
            b.camera_velocity,
            b.camera_timer,
            b.target.0 as i32,
            i32::from(b.active),
            i32::from(b.applied),
            i32::from(b.phase),
        ],
    );
    assert_eq!(out.len(), 701);
    ints(
        &mut out,
        [
            v.entity.0 as i32,
            v.class.0 as i32,
            i32::from(v.player),
            i32::from(car.slot),
            i32::from(car.rank),
            i32::from(car.place),
            car.weather as i32,
            i32::from(car.threatened),
            car.inactive_height,
            i32::from(car.horn_down),
            i32::from(car.horn_latched),
            i32::from(car.horn_timer),
            i32::from(car.visible),
            i32::from(car.ai.is_some()),
            car.tracks.travel[0] as i32,
            car.tracks.travel[1] as i32,
        ],
    );
    ints(
        &mut out,
        car.hit_counts.into_iter().flatten().map(i32::from),
    );
    ints(&mut out, car.damage.body.map(i32::from));
    ints(&mut out, car.damage.turret.map(i32::from));
    ints(&mut out, car.damage.tracks.map(i32::from));
    let d = car.input.drive;
    ints(
        &mut out,
        [
            d.throttle,
            d.brake,
            d.steering,
            d.action_flags as i32,
            i32::from(d.alternate),
            i32::from(d.alternate_lateral),
            i32::from(d.alternate_rotation),
            i32::from(car.input.start_held),
            i32::from(car.input.fast_start),
            i32::from(car.brake.saved_turn),
            i32::from(car.brake.reverse_braking),
        ],
    );
    let r = car.route;
    ints(
        &mut out,
        [
            i32::from(r.checkpoint.is_some()),
            r.checkpoint.map_or(0, |id| id.0 as i32),
            r.passed as i32,
            r.completed,
            r.records[0],
            r.records[1],
            r.records[2],
            r.score,
            r.previous_score,
            i32::from(r.wrong_way),
            i32::from(r.warning_timer),
        ],
    );
    ints(
        &mut out,
        [
            car.race.elapsed,
            i32::from(car.race.laps),
            i32::from(car.race.driving),
        ],
    );
    let t = car.turret;
    ints(
        &mut out,
        [
            t.velocity,
            t.cooldown,
            i32::from(t.centering),
            i32::from(t.moving),
            i32::from(t.center_lock),
            i32::from(t.pressed[0]),
            i32::from(t.pressed[1]),
            car.inventory.weapon as i32,
            i32::from(car.inventory.ammo),
            v.yaw_lock,
            v.impact_cooldown as i32,
        ],
    );
    let c = &v.collision;
    ints(
        &mut out,
        [
            i32::from(c.disabled),
            c.timer,
            c.impact_accumulator,
            c.collision_size as i32,
            c.material,
            i32::from(c.deformable.is_some()),
            c.deformable.map_or(0, |id| id.0 as i32),
            c.obstacle_impulse,
        ],
    );
    ints(&mut out, c.tank_delta);
    ints(&mut out, c.ground_delta);
    let b = &v.blast;
    ints(
        &mut out,
        [
            b.health,
            i32::from(b.health_timer),
            i32::from(b.health_color),
            b.protection_timer,
            i32::from(b.angular_enabled),
            i32::from(b.hit_axes[0]),
            i32::from(b.hit_axes[1]),
        ],
    );
    assert_eq!(out.len(), 891);
    out.resize(921, 0);
    let tracks =
        track_effects_oracle::snapshot(&car.spray, &car.marks, &car.lights, &Default::default());
    out.extend_from_slice(&tracks[..374]);
    let s = &car.smoke;
    ints(&mut out, [s.timer, s.side as i32, s.force, s.count as i32]);
    ints(&mut out, s.damage.map(i32::from));
    ints(&mut out, s.sockets.map(i32::from));
    ints(&mut out, s.damage_timers);
    for p in s.particles {
        ints(&mut out, p.position);
        ints(&mut out, p.velocity);
        ints(
            &mut out,
            [
                i32::from(p.life),
                i32::from(p.size),
                i32::from(p.rotation),
                i32::from(p.direction),
                i32::from(p.expanding),
            ],
        );
    }
    out.extend_from_slice(&tracks[374..384]);
    ints(
        &mut out,
        [
            car.debris.active as i32,
            car.debris.timer,
            i32::from(car.debris.previous_plane),
        ],
    );
    assert_eq!(out.len(), 1661);
    out.extend(vehicle_pose_oracle::config(&v.pose));
    let e = v.pose_input;
    let (kind, basis) = match e.external {
        pose::ExternalBasis::None => (0, [0; 9]),
        pose::ExternalBasis::Body => (1, [0; 9]),
        pose::ExternalBasis::Custom(m) => (2, m.0),
    };
    ints(
        &mut out,
        [
            i32::from(e.timer),
            i32::from(e.ambient_wobble),
            i32::from(e.session_active),
            e.ambient_phase as i32,
            e.recoil_mode,
            i32::from(e.turret_yaw),
            v.motion.articulation.recoil,
            v.motion.articulation.pitch,
            kind,
        ],
    );
    ints(&mut out, basis);
    assert_eq!(out.len(), 1751);
    out.resize(1760, 0);
    out
}

pub fn export(out: &Path, world: &SinglePlayer) {
    let models: Vec<_> = world.vehicles.entries.iter().flatten().collect();
    let mut cases = Vec::new();
    for branch in 0..40 {
        let mut model = models[branch % models.len()].model.clone();
        let player = branch & 1 != 0;
        let environment = Environment {
            ambient_wobble: branch & 2 != 0,
            session_active: branch & 4 != 0,
            ambient_phase: if branch & 8 != 0 {
                u32::MAX - branch as u32
            } else {
                197 + branch as u32
            },
            gun_pivot: [branch as i32 * 19 - 300, 200, branch as i32 * -13],
        };
        model.alternate_control = [
            (branch as i16 * 31).wrapping_sub(600),
            if branch & 2 != 0 { i16::MIN } else { i16::MAX },
        ];
        model.reset_sidegrip.multiplier = 257 + branch as u16;
        if branch == 35 {
            if model.shadow.edit(|g| g.vertices.truncate(3)).is_none() {
                std::sync::Arc::make_mut(&mut model.shadow.source)
                    .geometry
                    .vertices
                    .truncate(3);
            }
        }
        if branch >= 36 {
            model.track_dimensions[2] = i32::MIN;
        }
        let spawn = pose::Pose {
            matrix: if branch & 8 != 0 {
                Matrix::IDENTITY
            } else {
                Matrix::from_angles([1234 + branch as u32 * 67, 30500, 1700])
            },
            position: [-500000 + branch as i32 * 713, 1700, -90000],
        };
        let entity = EntityId(if branch & 16 != 0 {
            u32::MAX - branch as u32
        } else {
            branch as u32 + 7
        });
        let class = ClassId(branch * 7 + 3);
        let pose = vehicle_pose_oracle::config(&model.pose_config([0; 3]));
        let mut contacts = Vec::new();
        ints(&mut contacts, model.primary_local.into_iter().flatten());
        let shadow: Vec<_> = model
            .shadow
            .geometry()
            .vertices
            .iter()
            .take(4)
            .flatten()
            .map(|v| *v as u16)
            .collect();
        assert!(shadow.len() <= 12);
        let control = [
            i32::from(model.reset_sidegrip.multiplier),
            i32::from(model.reset_sidegrip.shift),
            i32::from(model.config.tank.drive.sidegrip_fraction.multiplier),
            i32::from(model.config.tank.drive.sidegrip_fraction.shift),
            i32::from(model.alternate_control[0]),
            i32::from(model.alternate_control[1]),
        ];
        let mut spawn_words = Vec::new();
        ints(&mut spawn_words, spawn.matrix.0);
        ints(&mut spawn_words, spawn.position);
        let input = [
            entity.0 as i32,
            class.0 as i32,
            i32::from(player),
            i32::from(environment.ambient_wobble),
            i32::from(environment.session_active),
            environment.ambient_phase as i32,
            environment.gun_pivot[0],
            environment.gun_pivot[1],
            environment.gun_pivot[2],
        ];
        let count = model.shadow.geometry().vertices.len() as u16;
        let radius = model.shadow.radius;
        let result = Player::construct(entity, class, model, spawn, environment, player);
        let (status, state) = match result {
            Ok(car) => (0, snapshot(&car)),
            Err(tankracer_game::vehicle_startup::Error::ShadowGeometry) => (-4, Vec::new()),
            Err(tankracer_game::vehicle_startup::Error::Pose(_)) => (-2, Vec::new()),
        };
        cases.push(json!({"tag":format!("model-{}-branch-{branch}",branch%models.len()),"status":status,"state":state,"pose":pose,"contacts":contacts,"shadow":shadow,"shadow_count":count,"shadow_radius":radius,"control":control,"spawn":spawn_words,"input":input}));
    }
    fs::write(out.join("vehicle-startup.json"),serde_json::to_vec_pretty(&json!({"scope":"Original Player::construct on actual FullRace-loaded vehicle models: entire fresh physical/contact/prefix/player/effect state, authored shadow hull, immutable model inputs and the one/two original pose updates. Entity allocation, resource loading and full world startup remain separate.","cases":cases})).unwrap()).unwrap();
}
