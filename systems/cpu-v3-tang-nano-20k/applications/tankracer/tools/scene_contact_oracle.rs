//! Original obstacle query/reflection/blend up to the immediate method boundary.
use super::*;
use tankracer_game::{
    dispatch::{Class, ClassId, MethodId},
    obstacle_collision as obstacle,
};
struct Probe {
    calls: i32,
}
struct MutatingProbe {
    calls: i32,
}
impl obstacle::Effects for MutatingProbe {
    fn method(&mut self, method: MethodId, call: obstacle::Invocation<'_>) -> Result<(), i32> {
        assert_eq!(method, MethodId(111));
        self.calls += 1;
        let object = &mut call.world.objects[call.target.0];
        if self.calls == 1 {
            object.activity = 1;
            object.flags = 1;
        } else if self.calls == 2 {
            object.weight = 128;
        } else if self.calls == 3 {
            object.threshold = i32::MAX;
        }
        Ok(())
    }
    fn feedback(&mut self, _: i32, _: &mut obstacle::State, _: &mut u32) -> Result<(), i32> {
        panic!("live obstacle feedback")
    }
    fn impact(
        &mut self,
        _: &mut obstacle::State,
        _: usize,
        _: i32,
        _: &mut u32,
    ) -> Result<(), i32> {
        panic!("live obstacle particles")
    }
}
// Exercise the shared query/response phase. The explicit method probe changes
// properties so that later calls must read live rows rather than startup data.
pub(super) fn append_live(cases: &mut Vec<Value>) {
    for weight in [-64, 0, 64, 192] {
        for mode in [0, 4] {
            let c = query::Collider {
                entity: query::EntityId(10),
                pose: Pose {
                    position: [0; 3],
                    matrix: Matrix::IDENTITY,
                },
                external_basis: Some(Matrix::IDENTITY),
                bounds: geometry::Bounds {
                    min: [-100; 3],
                    max: [100; 3],
                },
                radius: 64,
            };
            let object = obstacle::Object {
                class: ClassId(1),
                weight,
                threshold: 0,
                activity: 0,
                flags: 0,
                mesh_bounds: [[i16::MIN, -1, 0], [1, 100, i16::MAX]],
            };
            let class = std::sync::Arc::new(Class {
                maximum_slot: 1,
                methods: vec![None, Some(MethodId(111))],
            });
            let mut world = obstacle::World {
                colliders: vec![c],
                objects: vec![object],
                classes: vec![std::sync::Arc::clone(&class), class],
                grid: obstacle::Grid {
                    origin: [0; 2],
                    shifts: [12; 2],
                    cells: std::array::from_fn(|_| vec![query::ObstacleId(0)]),
                },
                deformables: Vec::new(),
            };
            let mut input = collider_row(&c);
            input.extend([1, weight, 0, 0, 0]);
            input.extend(object.mesh_bounds.into_iter().flatten().map(i32::from));
            input.push(mode);
            let mut car = obstacle::State {
                entity: query::EntityId(99),
                class: ClassId(0),
                ..Default::default()
            };
            car.motion.poses.physical_position = [500, 0, 0];
            car.motion.previous_position = [1500, 0, 0];
            let env = obstacle::Environment {
                mode,
                dt: 33,
                feedback: false,
                player_class: ClassId(0),
                breakable_class: ClassId(1),
                obstacle_slot: 1,
                deformable_slot: 1,
                tuning: obstacle::Tuning::default(),
            };
            let mut shared = obstacle::Shared {
                random: 7,
                current_slot: 777,
            };
            let mut probe = MutatingProbe { calls: 0 };
            let mut expected = Vec::new();
            for step in 0..6 {
                car.motion.velocity = [-250001, 0, 0];
                if step == 4 {
                    world.objects[0].threshold = 0;
                    world.objects[0].class = ClassId(0);
                }
                if step == 5 {
                    world.colliders[0].pose.position[1] = 10000;
                }
                let mut hit = query::Hit::default();
                let found = query::obstacles(
                    &query::Grid {
                        origin: world.grid.origin,
                        shifts: world.grid.shifts,
                        cells: world.grid.cells.each_ref().map(Vec::as_slice),
                    },
                    &world.colliders,
                    car.entity,
                    [500, 0, 0],
                    [-500, 0, 0],
                    &mut hit,
                )
                .unwrap();
                let before = probe.calls;
                obstacle::obstacles(&mut world, &mut car, env, &mut shared, &mut probe).unwrap();
                let status = if !found {
                    0
                } else if probe.calls == before {
                    2
                } else {
                    1
                };
                expected.push(status);
                expected.extend(obstacle_hit(&hit));
                expected.extend(car.motion.velocity);
                expected.extend([
                    car.collision.impact_accumulator,
                    car.collision.collision_size as i32,
                    probe.calls,
                ]);
                let p = world.objects[0];
                expected.extend([
                    p.class.0 as i32,
                    p.weight,
                    p.threshold,
                    p.activity,
                    p.flags as i32,
                ]);
                expected.extend(p.mesh_bounds.into_iter().flatten().map(i32::from));
            }
            assert_eq!(shared.random, 7);
            assert_eq!(shared.current_slot, 777);
            assert_eq!(probe.calls, 4);
            assert_eq!(input.len(), 42);
            assert_eq!(expected.len(), 162);
            cases.push(json!({"kind":28,"input":input,"status":0,"expected":expected}));
        }
    }
}
impl obstacle::Effects for Probe {
    fn method(&mut self, method: MethodId, _: obstacle::Invocation<'_>) -> Result<(), i32> {
        assert_eq!(method, MethodId(111));
        self.calls += 1;
        Ok(())
    }
    fn feedback(&mut self, _: i32, _: &mut obstacle::State, _: &mut u32) -> Result<(), i32> {
        panic!("obstacle feedback")
    }
    fn impact(
        &mut self,
        _: &mut obstacle::State,
        _: usize,
        _: i32,
        _: &mut u32,
    ) -> Result<(), i32> {
        panic!("obstacle particles")
    }
}
pub(super) fn append(cases: &mut Vec<Value>) {
    for variant in 0..16 {
        for speed in [199999_i32, 200000, 200001, i32::MAX] {
            for threshold in [0, 200000, 300000] {
                for mode in [0, 4] {
                    let c = query::Collider {
                        entity: query::EntityId(10),
                        pose: Pose {
                            position: [0; 3],
                            matrix: Matrix::IDENTITY,
                        },
                        external_basis: Some(Matrix::IDENTITY),
                        bounds: geometry::Bounds {
                            min: [-100; 3],
                            max: [100; 3],
                        },
                        radius: if variant == 15 { u16::MAX } else { 64 },
                    };
                    let object = obstacle::Object {
                        class: ClassId((variant & 1) as usize),
                        weight: [-64, 0, 64, 128, 192, i32::MIN, i32::MAX, 127]
                            [(variant >> 1) as usize],
                        threshold,
                        activity: i32::from(variant & 4 != 0),
                        flags: u32::from(variant & 8 != 0),
                        mesh_bounds: [[0; 3]; 2],
                    };
                    let class = std::sync::Arc::new(Class {
                        maximum_slot: 1,
                        methods: vec![None, Some(MethodId(111))],
                    });
                    let mut world = obstacle::World {
                        colliders: vec![c],
                        objects: vec![object],
                        classes: vec![std::sync::Arc::clone(&class), class],
                        grid: obstacle::Grid {
                            origin: [0; 2],
                            shifts: [12; 2],
                            cells: std::array::from_fn(|_| vec![query::ObstacleId(0)]),
                        },
                        deformables: Vec::new(),
                    };
                    let mut vehicle = obstacle::State {
                        entity: query::EntityId(99),
                        class: ClassId(0),
                        ..Default::default()
                    };
                    vehicle.motion.poses.physical_position = [500, 0, 0];
                    vehicle.motion.previous_position = [1500, 0, 0];
                    vehicle.motion.velocity = [speed.wrapping_neg(), 0, 0];
                    vehicle.collision.impact_accumulator = i32::MAX - 50;
                    vehicle.collision.collision_size = u32::MAX - 1;
                    let mut hit = query::Hit::default();
                    query::obstacles(
                        &query::Grid {
                            origin: world.grid.origin,
                            shifts: world.grid.shifts,
                            cells: world.grid.cells.each_ref().map(|x| x.as_slice()),
                        },
                        &world.colliders,
                        vehicle.entity,
                        [500, 0, 0],
                        [-500, 0, 0],
                        &mut hit,
                    )
                    .unwrap();
                    assert_eq!(hit.object, Some(query::ObstacleId(0)));
                    let mut input = hit.normal.to_vec();
                    input.extend(vehicle.motion.velocity);
                    input.extend([
                        object.class.0 as i32,
                        object.weight,
                        object.threshold,
                        object.activity,
                        object.flags as i32,
                        i32::from(c.radius),
                        mode,
                        1,
                        vehicle.collision.impact_accumulator,
                        vehicle.collision.collision_size as i32,
                    ]);
                    assert_eq!(input.len(), 16);
                    let env = obstacle::Environment {
                        mode,
                        dt: 33,
                        feedback: false,
                        player_class: ClassId(0),
                        breakable_class: ClassId(1),
                        obstacle_slot: 1,
                        deformable_slot: 1,
                        tuning: obstacle::Tuning::default(),
                    };
                    let mut shared = obstacle::Shared {
                        random: 7,
                        current_slot: 777,
                    };
                    let mut probe = Probe { calls: 0 };
                    let mut expected = Vec::new();
                    for _ in 0..3 {
                        assert_eq!(
                            obstacle::obstacles(
                                &mut world,
                                &mut vehicle,
                                env,
                                &mut shared,
                                &mut probe
                            )
                            .unwrap(),
                            Some(query::ObstacleId(0))
                        );
                        expected.extend(vehicle.motion.velocity);
                        expected.extend([
                            vehicle.collision.impact_accumulator,
                            vehicle.collision.collision_size as i32,
                            probe.calls,
                        ]);
                    }
                    assert_eq!(shared.random, 7);
                    cases.push(json!({"kind":24,"input":input,"status":0,"expected":expected}));
                }
            }
        }
    }
}
