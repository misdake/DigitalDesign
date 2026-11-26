//! Direct first-map car/effect phase image. Every offset is in u16 words.
use super::*;
pub const WORDS: usize = 10468;
fn ints(out: &mut Vec<u16>, values: impl IntoIterator<Item = i32>) {
    for v in values {
        out.extend([v as u16, (v >> 16) as u16]);
    }
}
pub fn capture(world: &SinglePlayer) -> Vec<u16> {
    let mut out = Vec::new();
    let values = pickup_snapshot::values(&world.shared.pickup, &world.shared.pool);
    ints(&mut out, values[..3386].iter().copied());
    assert_eq!(out.len(), 6772);
    out.push(0x9a53);
    out.resize(6800, 0);
    ints(&mut out, [world.shared.pool.quad_count as i32]);
    out.resize(6810, 0);
    for i in 0..2 {
        let start = out.len();
        let car = world.vehicles.entries[i].as_ref().unwrap();
        let v = &car.physics;
        let m = &v.motion;
        for pose in [
            m.poses.turret,
            m.poses.gun,
            m.poses.attachments[0],
            m.poses.attachments[1],
        ] {
            ints(&mut out, pose.matrix.0);
            ints(&mut out, pose.position);
        }
        ints(&mut out, m.poses.bottom_position);
        ints(
            &mut out,
            [
                i32::from(m.poses.recoil_factor),
                i32::from(m.poses.recoil_shift),
            ],
        );
        assert_eq!(out.len() - start, 106);
        let s = &car.spray;
        ints(&mut out, [s.timer, s.side as i32, s.count as i32]);
        for p in s.particles {
            ints(&mut out, p.position);
            ints(&mut out, p.velocity);
            ints(
                &mut out,
                [
                    i32::from(p.life),
                    i32::from(p.size),
                    i32::from(p.floor),
                    i32::from(p.rotation),
                    i32::from(p.kind),
                    i32::from(p.side),
                ],
            );
        }
        assert_eq!(out.len() - start, 304);
        let s = &car.marks;
        ints(&mut out, s.active.map(i32::from));
        ints(&mut out, s.movement);
        ints(&mut out, [s.count as i32, s.head as i32, s.tail as i32]);
        ints(&mut out, s.endpoints.into_iter().flatten().flatten());
        ints(&mut out, s.handles.map(|h| h.0 as i32));
        assert_eq!(out.len() - start, 480);
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
        assert_eq!(out.len() - start, 830);
        let s = &car.lights;
        ints(
            &mut out,
            [
                i32::from(s.brake_mode),
                i32::from(s.active[0]),
                i32::from(s.active[1]),
                s.handles[0].0 as i32,
                s.handles[1].0 as i32,
            ],
        );
        ints(
            &mut out,
            [
                v.entity.0 as i32,
                car.debris.active as i32,
                car.debris.timer,
                i32::from(car.debris.previous_plane),
            ],
        );
        assert_eq!(out.len() - start, 848);
        let c = &v.pose;
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
                0,
                0,
                kind,
            ],
        );
        ints(&mut out, basis);
        let (_, _, _, offset, material, special) =
            rcc_car_probe::effect_config(world, tankracer_game::object_query::TankId(i));
        ints(&mut out, offset);
        ints(
            &mut out,
            [
                material as i32,
                i32::from(special),
                i32::from(car.brake.reverse_braking),
                i32::from(v.collision.disabled),
                i32::from(car.prefix.temporary_timer),
                i32::from(car.prefix.temporary_slot),
            ],
        );
        assert_eq!(out.len() - start, 956);
    }
    assert_eq!(out.len(), 8722);
    for b in &world.shared.debris.banks {
        for p in b.particles {
            ints(&mut out, p.velocity);
            ints(&mut out, p.position);
            ints(&mut out, p.color.map(i32::from));
            ints(&mut out, [i32::from(p.active), p.size as i32]);
        }
        ints(
            &mut out,
            [
                b.floor,
                i32::from(b.owner.is_some()),
                b.owner.map_or(0, |e| e.0 as i32),
            ],
        );
    }
    assert_eq!(out.len(), 9086);
    for b in &world.shared.shield.banks {
        for o in b.orbits {
            ints(
                &mut out,
                [
                    o.pitch,
                    o.pitch_target,
                    o.yaw,
                    o.yaw_rate,
                    o.yaw_rate_target,
                    o.size,
                    i32::from(o.growing),
                    i32::from(o.spin),
                ],
            );
            ints(&mut out, o.position);
        }
        ints(&mut out, [b.tail as i32]);
    }
    assert_eq!(out.len(), 9896);
    let t = &world.shared.temporary;
    ints(&mut out, [t.available as i32]);
    ints(&mut out, t.timestamps.map(|v| v as i32));
    ints(&mut out, t.lines.into_iter().flatten().map(|h| h.0 as i32));
    for row in t.rows.into_iter().flatten() {
        ints(&mut out, row.first);
        ints(&mut out, row.last);
        out.extend([
            u16::from(row.colors[0]) | (u16::from(row.colors[1]) << 8),
            u16::from(row.colors[2]) | (u16::from(row.colors[3]) << 8),
            row.phases[0],
            row.phases[1],
        ]);
    }
    assert_eq!(out.len(), 10170);
    ints(
        &mut out,
        [
            i32::from(world.shared.debris_sound.is_some()),
            world.shared.debris_sound.unwrap_or(0) as i32,
        ],
    );
    ints(&mut out, world.camera_position);
    ints(&mut out, world.camera_forward);
    assert_eq!(out.len(), 10186);
    let (spray, smoke, pickup, _, _, _) =
        rcc_car_probe::effect_config(world, tankracer_game::object_query::TankId(0));
    for s in spray.styles {
        ints(
            &mut out,
            [
                s.spin as i32,
                i32::from(s.initial_size),
                i32::from(s.gain.multiplier),
                i32::from(s.gain.shift),
            ],
        );
    }
    out.extend(spray.kinds.map(|v| v as u16));
    ints(&mut out, smoke.sockets.into_iter().flatten().flatten());
    ints(
        &mut out,
        [
            pickup.outer_growth,
            pickup.inner_growth,
            pickup.flare_growth,
        ],
    );
    out.extend(pickup.palette.into_iter().flatten().map(u16::from));
    assert_eq!(out.len(), 10358);
    for car in world.vehicles.entries[..2].iter().flatten() {
        let d = &car.input.drive;
        let track = car.model.parts[tankracer_assets::config::Part::LeftTrack.slot()].as_ref();
        let bounds = car.model.geometry_bounds;
        ints(
            &mut out,
            [
                car.inactive_height,
                d.throttle,
                d.brake,
                d.steering,
                d.action_flags as i32,
                i32::from(d.alternate),
                i32::from(d.alternate_lateral),
                i32::from(d.alternate_rotation),
                i32::from(track.is_some()),
                track.map_or(0, |p| i32::from(p.source.bounds[1][2])),
                bounds.min[0],
                bounds.max[0],
                bounds.min[2],
                bounds.max[2],
            ],
        );
    }
    assert_eq!(out.len(), 10414);
    for car in world.vehicles.entries[..2].iter().flatten() {
        let t = car.turret;
        ints(
            &mut out,
            [
                i32::from(car.input.start_held),
                i32::from(car.input.fast_start),
                i32::from(car.brake.saved_turn),
                car.model.config.tank.drive.max_turn,
                car.physics.yaw_lock,
                t.velocity,
                t.cooldown,
                i32::from(t.centering),
                i32::from(t.moving),
                i32::from(t.center_lock),
                i32::from(t.pressed[0]),
                i32::from(t.pressed[1]),
                i32::from(car.race.driving),
            ],
        );
    }
    ints(
        &mut out,
        [i32::from(world.shared.input.steering_analog_latch)],
    );
    assert_eq!(out.len(), WORDS);
    out
}
