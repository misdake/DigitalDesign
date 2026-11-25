//! Real plain Fall contact through original activation and persistent retries.
use super::*;
use tankracer_game::{
    fall, lifecycle, obstacle_collision as obstacle,
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
pub(super) fn append(cases: &mut Vec<Value>, root: &std::path::Path) {
    let constructed = constructed_world(root);
    let (mut session, _) = constructed.into_session().unwrap();
    let world = session.world_mut_at_load().unwrap();
    let id = world
        .objects
        .components
        .falls
        .iter()
        .enumerate()
        .find_map(|(i, s)| {
            let f = s.as_ref()?;
            let o = world.objects.components.objects[i].as_ref()?;
            (o.data.auxiliary.is_none() && o.data.debris.is_none() && f.attached.is_none())
                .then_some(i)
        })
        .unwrap();
    let entity = world.objects.world.colliders[id].entity;
    let class = world.objects.world.objects[id].class;
    assert_eq!(
        world.objects.world.classes[class.0].methods[4],
        Some(tankracer_game::breakable::ENABLE_TICK)
    );
    assert!(entity.0 < 512 && entity.0 != 0);
    let tuning = world.objects.components.fall_tuning;
    for variant in 0..9 {
        for forward in [-400000, 0, 400000] {
            for limit in [0, 1, 10000] {
                let s = fall::State {
                    angular_velocity: 123,
                    rotation: 77,
                    turn_rate: 999,
                    slope_limit: -32,
                    ..Default::default()
                };
                world.objects.components.falls[id] = Some(s.clone());
                let p = &mut world.objects.world.objects[id];
                p.activity = i32::from(variant == 2);
                p.flags = 0x1234001c | u32::from(variant == 1);
                let flag_count = if variant == 4 { 1 } else { 512 };
                world.identity.ticks = lifecycle::Ticks {
                    flags: vec![0; flag_count],
                    order: vec![query::EntityId(0)],
                    next: None,
                };
                if variant == 3 {
                    world.identity.ticks.flags[entity.0 as usize] = 16;
                    world.identity.ticks.order.push(entity);
                }
                world.identity.queues = lifecycle::Queues::default();
                world.identity.current_slot = 24;
                let mut vehicle = obstacle::State {
                    entity: query::EntityId(99),
                    ..Default::default()
                };
                vehicle.motion.velocity = if variant == 5 {
                    [i32::MIN, i32::MAX, -777]
                } else if variant == 7 {
                    [0; 3]
                } else {
                    [250000, -150000, 500000]
                };
                vehicle.powertrain.local_velocity[2] =
                    if variant == 6 { i32::MIN } else { forward };
                let mut env = world.registry.hit_environment(tuning, false, limit);
                if variant == 8 {
                    env.minimum_turn_rate = 2000;
                    env.maximum_turn_rate = 1000;
                }
                let mut input = vec![
                    s.angular_velocity,
                    s.rotation,
                    s.turn_rate,
                    s.slope_limit,
                    p.flags as i32,
                    p.activity,
                    -1,
                ];
                input.extend(vehicle.motion.velocity);
                input.extend([
                    vehicle.powertrain.local_velocity[2],
                    env.minimum_turn_rate,
                    env.maximum_turn_rate,
                    entity.0 as i32,
                    flag_count as i32,
                    i32::from(variant == 3),
                    limit as i32,
                ]);
                assert_eq!(input.len(), 17);
                let mut expected = Vec::new();
                let mut seed = 7;
                for _ in 0..3 {
                    let (lifecycle, current_slot) = world.identity.split();
                    let mut collision = Collision {
                        components: &mut world.objects.components,
                        lifecycle,
                        environment: world.registry.environment(limit),
                        cars: None,
                    };
                    let mut result = 0x7182;
                    let code = collision
                        .method(
                            tankracer_game::breakable::BREAKABLE_HIT,
                            obstacle::Invocation {
                                dt: 33,
                                target: query::ObstacleId(id),
                                vehicle: &mut vehicle,
                                world: &mut world.objects.world,
                                random: &mut seed,
                                current_slot,
                                result: &mut result,
                            },
                            env,
                            &mut Devices,
                            true,
                        )
                        .err()
                        .unwrap_or(0);
                    let s = world.objects.components.falls[id].as_ref().unwrap();
                    let p = world.objects.world.objects[id];
                    expected.extend([
                        s.angular_velocity,
                        s.rotation,
                        s.turn_rate,
                        s.slope_limit,
                        p.flags as i32,
                        p.activity,
                        s.last_vehicle.map_or(-1, |id| id.0 as i32),
                        code,
                        result as i32,
                        seed as i32,
                        world.identity.current_slot as i32,
                        world
                            .identity
                            .ticks
                            .flags
                            .get(entity.0 as usize)
                            .copied()
                            .unwrap_or(0) as i32,
                        world.identity.ticks.order.len() as i32,
                    ]);
                    expected.extend(world.identity.ticks.order.iter().map(|id| id.0 as i32));
                    expected.extend(std::iter::repeat_n(
                        -1,
                        4 - world.identity.ticks.order.len(),
                    ));
                }
                assert_eq!(expected.len(), 51);
                cases.push(json!({"kind":25,"input":input,"status":0,"expected":expected}));
            }
        }
    }
}
