//! Original Bounce methods, using each authored ENGLAND configuration profile.
use super::*;
use tankracer_game::{
    bounce, lifecycle, obstacle_collision as obstacle,
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
        panic!("Bounce hit has no sound")
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
pub(super) fn row(state: &bounce::State, activity: i32) -> Vec<i32> {
    let mut out = state.velocity.to_vec();
    out.extend([
        state.spin,
        state.ground,
        i32::from(state.timer),
        i32::from(state.age),
        i32::from(state.airborne),
        i32::from(state.reverse_spin),
        i32::from(state.special),
        i32::from(state.raised),
        activity,
    ]);
    out
}
pub(super) fn config(c: bounce::Config) -> [i32; 10] {
    [
        i32::from(c.elasticity),
        i32::from(c.friction),
        i32::from(c.height_factor),
        i32::from(c.hit_spin.multiplier as i16),
        i32::from(c.hit_spin.shift),
        i32::from(c.slow_spin.multiplier as i16),
        i32::from(c.slow_spin.shift),
        i32::from(c.deflection),
        c.gravity,
        i32::from(c.height_period),
    ]
}
pub(super) fn append(cases: &mut Vec<Value>, root: &std::path::Path) {
    let constructed = constructed_world(root);
    let (mut session, _) = constructed.into_session().unwrap();
    let world = session.world_mut_at_load().unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let mut profiles = Vec::new();
    for (id, c) in world.objects.components.bounces.iter().enumerate() {
        let Some(c) = c else { continue };
        if seen.insert(format!("{:?}", c.config)) {
            profiles.push((id, *c));
        }
    }
    assert!(!profiles.is_empty());
    let mut edge = profiles[0].1;
    edge.config.elasticity = i16::MIN;
    edge.config.height_factor = i8::MIN;
    edge.config.deflection = i16::MIN;
    edge.config.hit_spin.multiplier = u16::MAX;
    edge.config.hit_spin.shift = 63;
    profiles.push((profiles[0].0, edge));
    let tuning = world.objects.components.fall_tuning;
    for (profile, (id, component)) in profiles.into_iter().enumerate() {
        let original = world.objects.world.colliders[id];
        let entity = original.entity;
        assert!(entity.0 < 512 && entity.0 != 0);
        let class = world.objects.world.objects[id].class;
        assert_eq!(
            world.objects.world.classes[class.0].methods[4],
            Some(tankracer_game::breakable::ENABLE_TICK)
        );
        for basis_mode in 0..4 {
            for forward in [-400000, 250000, 250001, i32::MIN] {
                for activity in [0, 1] {
                    for limit in [0, 10000] {
                        let mut initial = original;
                        initial.external_basis = match basis_mode {
                            0 => None,
                            1 => Some(Matrix::IDENTITY),
                            _ => Some(Matrix::from_angles([7000, 5000, 3000])),
                        };
                        world.objects.world.colliders[id] = initial;
                        world.objects.world.objects[id].activity = activity;
                        let s = bounce::State {
                            velocity: [-1, -2, -3],
                            spin: -777,
                            ground: component.state.ground,
                            timer: u16::MAX,
                            age: 1234,
                            reverse_spin: 1,
                            special: component.state.special,
                            raised: component.state.raised,
                            ..Default::default()
                        };
                        world.objects.components.bounces[id] = Some(bounce::Component {
                            config: component.config,
                            state: s,
                            reference_alias: basis_mode != 2,
                        });
                        let missing = profile == 0
                            && basis_mode == 0
                            && activity == 0
                            && forward == 250001
                            && limit != 0;
                        let flag_count = if missing { 1 } else { 512 };
                        let enabled = basis_mode == 3;
                        world.identity.ticks = lifecycle::Ticks {
                            flags: vec![0; flag_count],
                            order: vec![query::EntityId(0)],
                            next: None,
                        };
                        if enabled {
                            world.identity.ticks.flags[entity.0 as usize] = 16;
                            world.identity.ticks.order.push(entity);
                        }
                        world.identity.queues = lifecycle::Queues::default();
                        world.identity.current_slot = 24;
                        let mut vehicle = obstacle::State {
                            entity: query::EntityId(99),
                            ..Default::default()
                        };
                        vehicle.motion.poses.base_position = std::array::from_fn(|i| {
                            initial.pose.position[i].wrapping_sub([123, 77, -345][i])
                        });
                        vehicle.motion.poses.reference = Matrix::from_angles([3000, 13000, 8000]);
                        vehicle.motion.velocity = if profile == 0 && forward == i32::MIN {
                            [i32::MIN, i32::MAX, -1]
                        } else {
                            [250000, -123000, 500000]
                        };
                        vehicle.powertrain.local_velocity[2] = forward;
                        vehicle.pose_input.external = if basis_mode & 1 != 0 {
                            tankracer_game::pose::ExternalBasis::Custom(Matrix::from_angles([
                                8192, 12000, 0,
                            ]))
                        } else {
                            tankracer_game::pose::ExternalBasis::None
                        };
                        let seed = 0x12345678 ^ profile as u32;
                        let mut input = row(&s, activity);
                        input.extend(collider_row(&initial));
                        input.extend(config(component.config));
                        input.extend(vehicle.motion.poses.base_position);
                        input.extend(vehicle.motion.velocity);
                        input.push(forward);
                        input.extend(vehicle.motion.poses.reference.0);
                        input.extend(vehicle.external_basis().unwrap_or(Matrix::IDENTITY).0);
                        input.extend([
                            i32::from(vehicle.external_basis().is_some()),
                            i32::from(basis_mode != 2),
                            entity.0 as i32,
                            flag_count as i32,
                            i32::from(enabled),
                            limit as i32,
                            seed as i32,
                        ]);
                        assert_eq!(input.len(), 84);
                        let mut random = seed;
                        let mut expected = Vec::new();
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
                                    tankracer_game::scene_objects::BOUNCE_HIT,
                                    obstacle::Invocation {
                                        dt: 33,
                                        target: query::ObstacleId(id),
                                        vehicle: &mut vehicle,
                                        world: &mut world.objects.world,
                                        random: &mut random,
                                        current_slot,
                                        result: &mut result,
                                    },
                                    world.registry.hit_environment(tuning, false, limit),
                                    &mut Devices,
                                    true,
                                )
                                .err()
                                .unwrap_or(0);
                            expected.extend(row(
                                &world.objects.components.bounces[id].unwrap().state,
                                world.objects.world.objects[id].activity,
                            ));
                            expected.extend(collider_row(&world.objects.world.colliders[id]));
                            expected.extend([
                                code,
                                result as i32,
                                random as i32,
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
                            expected
                                .extend(world.identity.ticks.order.iter().map(|id| id.0 as i32));
                            expected.extend(std::iter::repeat_n(
                                -1,
                                4 - world.identity.ticks.order.len(),
                            ));
                        }
                        assert_eq!(expected.len(), 156);
                        cases.push(json!({"kind":26,"input":input,"status":0,"expected":expected}));
                    }
                }
            }
        }
    }
}
