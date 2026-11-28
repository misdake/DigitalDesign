//! Normal-loader qualification of real queries, responses and plain Fall methods.
use super::*;
use std::collections::BTreeSet;
use tankracer_game::{
    breakable,
    dispatch::MethodId,
    obstacle_collision as obstacle,
    scene_objects::collision::{Collision, Services, Sound},
};
struct Devices;
impl Services for Devices {
    fn sound(
        &mut self,
        _: Sound,
        _: &mut Collision<'_>,
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        Ok(())
    }
    fn feedback(
        &mut self,
        _: i32,
        _: &mut Collision<'_>,
        _: obstacle::Invocation<'_>,
    ) -> Result<(), i32> {
        panic!("feedback disabled")
    }
}
struct Methods<'a> {
    collision: Collision<'a>,
    environment: breakable::Environment,
    calls: usize,
}
impl obstacle::Effects for Methods<'_> {
    fn method(&mut self, method: MethodId, call: obstacle::Invocation<'_>) -> Result<(), i32> {
        assert_eq!(method, breakable::BREAKABLE_HIT);
        self.calls += 1;
        self.collision
            .method(method, call, self.environment, &mut Devices, true)
    }
    fn feedback(&mut self, _: i32, _: &mut obstacle::State, _: &mut u32) -> Result<(), i32> {
        panic!("feedback disabled")
    }
    fn impact(
        &mut self,
        _: &mut obstacle::State,
        _: usize,
        _: i32,
        _: &mut u32,
    ) -> Result<(), i32> {
        panic!("unexpected impacts")
    }
}
fn fall_row(s: &tankracer_game::fall::State) -> [i32; 5] {
    [
        s.angular_velocity,
        s.rotation,
        s.turn_rate,
        s.slope_limit,
        s.last_vehicle.map_or(-1, |v| v.0 as i32),
    ]
}
pub(super) fn export(
    constructed: Constructed,
    segments: &[(usize, [i32; 3], [i32; 3])],
    bounce_segments: &[(usize, [i32; 3], [i32; 3])],
    out: &Path,
) -> Result<(usize, usize, usize), Box<dyn Error>> {
    assert_eq!(segments.len(), 4);
    let (mut session, _) = constructed.into_session()?;
    let world = session.world_mut_at_load().unwrap();
    let mut lists = vec![
        world.objects.components.ordinary.len() as u16,
        world.objects.components.special.len() as u16,
    ];
    assert!(lists[0] <= 256 && lists[1] <= 256);
    for list in [
        &world.objects.components.ordinary,
        &world.objects.components.special,
    ] {
        for id in list {
            append_i32(&mut lists, id.0 as i32);
        }
    }
    binary(&out.join("runtime-lists.bin"), &lists)?;
    let entity = world.vehicles.entries[world.player.0]
        .as_ref()
        .unwrap()
        .physics
        .entity;
    let flags = &world.identity.ticks.flags;
    let order = &world.identity.ticks.order;
    assert!(flags.len() <= 512 && order.len() + 4 <= 256);
    let hit_env =
        world
            .registry
            .hit_environment(world.objects.components.fall_tuning, false, 10000);
    let mut initial = vec![
        0x4846,
        2,
        4,
        flags.len() as u16,
        order.len() as u16,
        world.registry.fall.0 as u16,
    ];
    for v in [
        entity.0 as i32,
        hit_env.minimum_turn_rate,
        hit_env.maximum_turn_rate,
        world.shared.random as i32,
        world.identity.current_slot as i32,
    ] {
        append_i32(&mut initial, v);
    }
    assert_eq!(initial.len(), 16);
    initial.extend(world.shared.noise);
    let tuning = world.objects.components.fall_tuning;
    for v in [
        tuning.squash_angle,
        tuning.gravity,
        tuning.elasticity_multiplier,
        tuning.elasticity_shift as i32,
    ] {
        append_i32(&mut initial, v);
    }
    for v in world.vehicles.entries[world.player.0]
        .as_ref()
        .unwrap()
        .physics
        .motion
        .poses
        .reference
        .0
    {
        append_i32(&mut initial, v);
    }
    assert_eq!(initial.len(), 58);
    let notifications = &world.identity.queues.notifications;
    let destruction = &world.identity.queues.destruction;
    println!(
        "Initial scene queues: {} notifications, {} destruction",
        notifications.len(),
        destruction.len()
    );
    assert!(notifications.len() + 4 <= 32 && destruction.len() + 4 <= 32);
    initial.extend([notifications.len() as u16, destruction.len() as u16]);
    for &v in flags {
        append_i32(&mut initial, v as i32);
    }
    for &v in order {
        append_i32(&mut initial, v.0 as i32);
    }
    for list in [notifications, destruction] {
        for v in list {
            append_i32(&mut initial, v.0 as i32);
        }
    }
    for &(id, _, _) in segments {
        initial.push(id as u16);
        for v in fall_row(world.objects.components.falls[id].as_ref().unwrap()) {
            append_i32(&mut initial, v);
        }
        let cell = world.objects.components.objects[id]
            .as_ref()
            .unwrap()
            .data
            .cell;
        initial.extend(cell.map_or([0, 0, 0], |c| [c.x as u16, c.z, 1]));
        let class = world.objects.world.objects[id].class;
        assert_eq!(
            world.objects.world.classes[class.0].methods[24],
            Some(breakable::BREAKABLE_HIT)
        );
        assert_eq!(
            world.objects.world.classes[class.0].methods[4],
            Some(breakable::ENABLE_TICK)
        );
    }
    binary(&out.join("hit-initial.bin"), &initial)?;
    let mut vehicle = obstacle::State {
        entity,
        ..Default::default()
    };
    let env = obstacle::Environment {
        mode: 0,
        dt: 33,
        feedback: false,
        player_class: vehicle.class,
        breakable_class: world.registry.fall,
        obstacle_slot: 24,
        deformable_slot: 21,
        tuning: obstacle::Tuning::default(),
    };
    let mut records = Vec::new();
    for repeat in 0..3 {
        for (index, &(id, a, b)) in segments.iter().enumerate() {
            let velocity = [-400000, -100000, 200000];
            let forward = if repeat == 1 { -300000 } else { 300000 };
            vehicle.motion.poses.physical_position = a;
            vehicle.motion.previous_position =
                std::array::from_fn(|i| a[i].wrapping_mul(2).wrapping_sub(b[i]));
            vehicle.motion.velocity = velocity;
            vehicle.powertrain.local_velocity[2] = forward;
            let mut hit = Hit::default();
            let w = &world.objects.world;
            assert!(query::obstacles(
                &query::Grid {
                    origin: w.grid.origin,
                    shifts: w.grid.shifts,
                    cells: w.grid.cells.each_ref().map(Vec::as_slice)
                },
                &w.colliders,
                entity,
                a,
                b,
                &mut hit
            )
            .map_err(|e| format!("Fall query {id}: {e:?}"))?);
            assert_eq!(hit.object, Some(ObstacleId(id)));
            let mut control = vec![index as u16];
            for v in [33, 10000]
                .into_iter()
                .chain(a)
                .chain(b)
                .chain(velocity)
                .chain([forward])
            {
                append_i32(&mut control, v);
            }
            assert_eq!(control.len(), 25);
            let (lifecycle, current_slot) = world.identity.split();
            let mut methods = Methods {
                collision: Collision {
                    components: &mut world.objects.components,
                    lifecycle,
                    environment: world.registry.environment(10000),
                    cars: None,
                },
                environment: hit_env,
                calls: 0,
            };
            obstacle::obstacles(
                &mut world.objects.world,
                &mut vehicle,
                env,
                obstacle::Globals {
                    random: &mut world.shared.random,
                    current_slot,
                },
                &mut methods,
            )
            .map_err(|e| format!("Fall contact {id}: {e:?}"))?;
            let status = if methods.calls == 0 { 2 } else { 1 };
            assert_eq!(status, 1, "selected Fall threshold must admit the hit");
            let mut expected = vec![
                status,
                hit.entity.map_or(-1, |v| v.0 as i32),
                hit.object.map_or(-1, |v| v.0 as i32),
                hit.code,
            ];
            expected.extend(hit.point);
            expected.extend(hit.normal);
            expected.extend(vehicle.motion.velocity);
            expected.extend([
                vehicle.collision.impact_accumulator,
                vehicle.collision.collision_size as i32,
            ]);
            expected.extend(fall_row(
                world.objects.components.falls[id].as_ref().unwrap(),
            ));
            let p = world.objects.world.objects[id];
            expected.extend([
                p.class.0 as i32,
                p.weight,
                p.threshold,
                p.activity,
                p.flags as i32,
            ]);
            let ticks = &world.identity.ticks;
            expected.extend([
                world.identity.current_slot as i32,
                world.shared.random as i32,
                ticks.flags[hit.entity.unwrap().0 as usize] as i32,
                ticks.order.len() as i32,
                0,
            ]);
            assert_eq!(expected.len(), 30);
            assert_eq!(p.activity, 1);
            expected.extend(ticks.order.iter().map(|v| v.0 as i32));
            expected.extend(std::iter::repeat_n(-1, 256 - ticks.order.len()));
            for v in expected {
                append_i32(&mut control, v);
            }
            assert_eq!(control.len(), 597);
            records.extend(control);
        }
    }
    binary(&out.join("hits.bin"), &records)?;
    let mut cells = Vec::new();
    for cell in 0..64 {
        cells.extend(terrain_pages::cell_bytes(world.terrain(), cell));
    }
    fs::write(out.join("cells.bin"), cells)?;
    let mut ticks = Vec::new();
    let mut changed = 0;
    let mut retired = BTreeSet::new();
    let mut sampled = 0;
    for dt in [
        1000, 1000, 1000, 1000, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33,
    ] {
        for (index, &(id, _, _)) in segments.iter().enumerate() {
            let entity = world.objects.world.colliders[id].entity;
            let before = world.objects.world.colliders[id].external_basis;
            let aligning = world.objects.world.objects[id].flags & 2 != 0;
            world.objects.components.dt = dt;
            let result = world.identity.context().dispatch(
                entity,
                tankracer_game::scene_loading::TICK,
                None,
                world.registry.environment(10000),
                &mut world
                    .objects
                    .updates_with_vehicles(&mut world.shared.random, &mut world.vehicles),
            );
            assert!(result.is_ok(), "real Fall tick {id}: {result:?}");
            let mut row = vec![index as u16];
            append_i32(&mut row, dt);
            let mut expected = vec![0];
            expected.extend(fall_row(
                world.objects.components.falls[id].as_ref().unwrap(),
            ));
            let p = world.objects.world.objects[id];
            expected.extend([
                p.class.0 as i32,
                p.weight,
                p.threshold,
                p.activity,
                p.flags as i32,
            ]);
            let c = world.objects.world.colliders[id];
            for v in record(&world.objects.world, id)[..60].as_chunks::<2>().0 {
                expected.push(i32::from(v[0]) | (i32::from(v[1]) << 16));
            }
            let t = &world.identity.ticks;
            expected.extend([
                world.identity.current_slot as i32,
                world.shared.random as i32,
                t.flags[entity.0 as usize] as i32,
                t.order.len() as i32,
            ]);
            assert_eq!(expected.len(), 45);
            expected.extend(t.order.iter().map(|v| v.0 as i32));
            expected.extend(std::iter::repeat_n(-1, 256 - t.order.len()));
            for list in [
                &world.identity.queues.notifications,
                &world.identity.queues.destruction,
            ] {
                assert!(list.len() <= 32);
                expected.push(list.len() as i32);
                expected.extend(list.iter().map(|v| v.0 as i32));
                expected.extend(std::iter::repeat_n(-1, 32 - list.len()));
            }
            let grid = &world.objects.world.grid;
            expected.extend(grid.cells.iter().map(|v| v.len() as i32));
            let count: usize = grid.cells.iter().map(Vec::len).sum();
            assert!(count <= 256);
            expected.extend(grid.cells.iter().flatten().map(|v| v.0 as i32));
            expected.extend(std::iter::repeat_n(-1, 256 - count));
            assert_eq!(expected.len(), 687);
            for v in expected {
                append_i32(&mut row, v);
            }
            assert_eq!(row.len(), 1377);
            ticks.extend(row);
            changed += usize::from(c.external_basis != before);
            if p.activity == 0 {
                retired.insert(id);
            }
            sampled += usize::from(aligning && p.flags & 2 == 0);
        }
    }
    assert!(changed > 0);
    assert!(
        !retired.is_empty(),
        "the connected sequence must reach a real Fall retirement"
    );
    assert!(
        sampled > 0,
        "the connected sequence must query the real terrain"
    );
    binary(&out.join("fall-ticks.bin"), &ticks)?;
    println!(
        "Fall tick transitions: {changed} changed, {} retired, {sampled} terrain queries",
        retired.len()
    );
    let bounce_count = scene_bounce::export(world, bounce_segments, out)?;
    Ok((records.len() / 597, ticks.len() / 1377, bounce_count))
}
