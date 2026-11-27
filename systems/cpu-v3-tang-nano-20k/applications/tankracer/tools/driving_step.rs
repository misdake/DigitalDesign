//! Original physical phase calls shared by direct and ordered reference frames.
use super::*;
use std::collections::BTreeSet;
use tankracer_game::{entity_factory as factory, scene_entity::Handle};
pub struct Setup<'a> {
    pub complete: bool,
    pub sample_controls: u16,
    pub dt: i32,
    pub start_stage: i32,
    pub terrain: &'a TerrainData,
    pub surfaces: &'a tankracer_assets::config::Surfaces,
    pub tank: &'a tankracer_assets::config::Tank,
    pub tuning: &'a drive::Tuning,
    pub contact: &'a contact::Config,
    pub layout: [i32; 5],
    pub environment: &'a Value,
    pub deform: bool,
    pub ordinary: bool,
    pub prepare: bool,
    pub pickups: bool,
    pub models: &'a [(String, std::sync::Arc<tankracer_assets::model::Model>)],
}
pub struct Step {
    pub control: Vec<i32>,
    pub stage: Vec<i32>,
    pub peer_stage: Option<Vec<i32>>,
    pub cells: BTreeSet<usize>,
    pub tank_hits: usize,
    pub deform_hits: usize,
    pub sampled_points: usize,
    pub spark_events: usize,
    pub ordinary_hits: usize,
    pub scenery_ticks: usize,
}
pub fn step(
    world: &mut SinglePlayer,
    setup: &Setup<'_>,
    own: usize,
    input: drive::Input,
    number: u32,
    mut context: Option<&mut factory::Context<'_, Handle>>,
) -> Step {
    if setup.complete {
        return complete_driving_step::step(world, setup, own, input, number, context);
    }
    if setup.prepare {
        rcc_car_probe::prepare_with_context(
            world,
            tankracer_game::object_query::TankId(own),
            &tankracer_game::vehicle::Frame {
                dt: 33,
                number,
                mode: 0,
                input,
                drive: drive::Environment {
                    active: true,
                    ..Default::default()
                },
            },
            [0; 3],
            context.as_deref_mut(),
        )
        .unwrap();
    }
    let terrain = setup.terrain;
    let surfaces = setup.surfaces;
    let tank = setup.tank;
    let tuning = setup.tuning;
    let contact = setup.contact;
    let layout = setup.layout;
    let env = setup.environment;
    let noise = world.shared.noise;
    let mut visited = BTreeSet::new();
    let mut hits = 0;
    let mut deform_hits = 0;
    let mut sampled_points = 0;
    let mut spark_events = 0;
    let mut ordinary_hits = 0;
    let boost = world.vehicles.entries[own].as_ref().unwrap().prefix.boost;
    let p = world.vehicles.entries[own].as_mut().unwrap();
    let v = &mut p.physics;
    let mut deformable = rcc_probe::surface(
        &mut world.objects.components.surfaces,
        &world.objects.world.colliders,
        v.collision.deformable,
    )
    .unwrap();
    let dt = 33;
    let se = suspension::Environment {
        dt,
        is_player: v.player,
        session_active: 1,
        player_multiplier: env[11].as_u64().unwrap() as u16,
        player_shift: env[12].as_u64().unwrap() as u32,
        other_multiplier: env[13].as_u64().unwrap() as u16,
        other_shift: env[14].as_u64().unwrap() as u32,
        penetration_shift: tank.suspension.susp_to_shape.shift as u8 as u32,
        base: [
            v.motion.poses.base_position[0],
            v.motion.poses.base_position[2],
        ],
        ..Default::default()
    };
    let sampling = ground::Environment {
        tracks: v.motion.poses.tracks.map(|t| Transform {
            basis: t.matrix.0,
            origin: t.position,
        }),
        auxiliary_local: p.model.auxiliary_local,
        force_surface: false,
        effect_gain: contact::Coefficient {
            multiplier: 3,
            shift: 9,
        },
        load: (&tank.suspension).into(),
        dt,
        frame: number,
        player: u32::from(v.player),
    };
    v.contact
        .step_vehicle(
            ground::Vehicle {
                motion: &mut v.motion,
                powertrain: &mut v.powertrain,
                pose: &mut v.pose,
            },
            &ground::World {
                terrain: &terrain,
                surface_bumps: &surfaces.entries.map(|s| s.bump.into()),
                noise,
            },
            &ground::Config {
                sampling: &sampling,
                suspension: &se,
                contact: &contact,
                layout,
            },
            &mut world.shared.random,
            deformable.as_mut(),
            |_| true,
        )
        .unwrap();
    // Coverage only: sampling a cloned diagnostic state does not
    // alter the live record/dirty writes used by the next frame.
    if let Some(surface) = deformable.as_mut() {
        let mut state = surface.state.clone();
        for point in &v.contact.sampling.primary.points {
            sampled_points += usize::from(state.sample(&surface.geometry, point.world).offset != 0);
        }
    }
    for point in v
        .contact
        .sampling
        .primary
        .points
        .iter()
        .map(|p| p.world)
        .chain(
            v.contact
                .sampling
                .auxiliary
                .iter()
                .filter(|p| p.cached != 0)
                .map(|p| p.world),
        )
    {
        let x = ((point[0] >> 3) - terrain.origin[0]) >> terrain.shifts[0];
        let z = ((point[2] >> 3) - terrain.origin[1]) >> terrain.shifts[1];
        if (0..8).contains(&x) && (0..8).contains(&z) {
            visited.insert((x + z * 8) as usize);
        }
    }
    let de = drive::Environment {
        dt,
        mode: 0,
        player: v.player,
        active: true,
        launch_eligible: false,
        materials: v.contact.sampling.materials.map(|m| m as usize),
        kinds: v.contact.sampling.primary.points.each_ref().map(|p| p.kind),
        contact_y: v
            .contact
            .sampling
            .primary
            .points
            .each_ref()
            .map(|p| p.world[1]),
        airborne: v.contact.sampling.primary.airborne_count as i8,
        boost_charge: if setup.prepare { boost.timer } else { 0 },
        boost_power: if setup.prepare { boost.extra_speed } else { 0 },
        extra_drive: if setup.prepare { boost.power } else { 0 },
    };
    let mut control = reference::drive_fixture::frame(&input, &de);
    control.extend([number as i32, 0, own as i32, 0]);
    v.powertrain
        .step(
            &mut v.motion,
            &powertrain::Config {
                drive: &tank.drive,
                surfaces: &surfaces,
                tuning: &tuning,
                yaw: velocity::YawConfig::from(&tank.drive),
            },
            &powertrain::Frame {
                input: &input,
                drive: &de,
                vertical_delta: v.contact.response.vertical_delta,
                yaw_lock: 0,
            },
        )
        .unwrap();
    let stage = reference::drive_fixture::flatten(&v.powertrain.drive_state(&v.motion));
    let before_timer = v.collision.timer;
    let id = tankracer_game::object_query::TankId(own);
    if setup.ordinary {
        let before_impact = world.vehicles.entries[own]
            .as_ref()
            .unwrap()
            .physics
            .collision
            .impact_accumulator;
        let before = world.vehicles.entries[own]
            .as_ref()
            .unwrap()
            .physics
            .motion
            .velocity;
        if setup.models.is_empty() {
            rcc_probe::contacts(world, id, dt, 0, 2, 100000, context.as_deref_mut()).unwrap();
        } else {
            let mut devices = driving_devices::Devices {
                models: setup.models,
            };
            rcc_probe::contacts_with_devices(
                world,
                id,
                dt,
                0,
                2,
                100000,
                context.as_deref_mut(),
                &mut devices,
            )
            .unwrap();
        }
        let v = &mut world.vehicles.entries[own].as_mut().unwrap().physics;
        let delta = std::array::from_fn(|i| v.motion.velocity[i].wrapping_sub(before[i]));
        v.collision.obstacle_impulse = tankracer_game::geometry::approximate_distance(delta);
        ordinary_hits += usize::from(
            v.collision.obstacle_impulse != 0 || before_impact != v.collision.impact_accumulator,
        );
    }
    rcc_probe::tanks(world, id, dt, 0).unwrap();
    hits += usize::from(
        world.vehicles.entries[own]
            .as_ref()
            .unwrap()
            .physics
            .collision
            .timer
            != before_timer,
    );
    if setup.deform {
        let before = world.vehicles.entries[own]
            .as_ref()
            .unwrap()
            .physics
            .collision
            .impact_accumulator;
        rcc_probe::contacts(world, id, dt, 0, 1, 100000, context.as_deref_mut()).unwrap();
        deform_hits += usize::from(
            world.vehicles.entries[own]
                .as_ref()
                .unwrap()
                .physics
                .collision
                .deformable
                .is_some(),
        );
        spark_events += ((world.vehicles.entries[own]
            .as_ref()
            .unwrap()
            .physics
            .collision
            .impact_accumulator
            - before)
            / dt) as usize;
    }
    rcc_car_probe::ground(world, id, dt, number).unwrap();
    let pickup_tuning = rcc_car_probe::pickup_tuning(world, id);
    let p = world.vehicles.entries[own].as_mut().unwrap();
    let v = &mut p.physics;
    let indices = p.model.gun_vertices;
    let original_z = p.model.gun_original_z;
    p.model.parts[Part::Gun.slot()]
        .as_mut()
        .unwrap()
        .edit(|mesh| {
            v.motion.integrate_and_pose(
                &motion::Config {
                    attitude: &attitude::Config::from(&tank.suspension),
                    body: &body::Tuning::default(),
                    pose: &v.pose,
                },
                &motion::Frame {
                    dt,
                    number,
                    player: v.player,
                    mode: 0,
                    pose: motion::PoseInput {
                        session_active: true,
                        external: pose::ExternalBasis::Body,
                        ..Default::default()
                    },
                },
                Some(body::GunMesh {
                    vertices: &mut mesh.vertices,
                    indices,
                    original_z,
                }),
            )
        })
        .unwrap()
        .unwrap();
    if setup.pickups {
        world
            .shared
            .pickup
            .fade(
                v.entity,
                v.motion.poses.base_position,
                number,
                dt,
                &pickup_tuning,
                &mut world.shared.pool,
            )
            .unwrap();
    }
    world
        .shared
        .impacts
        .tick(
            v,
            dt,
            &mut impact_particles::Context {
                frame: number,
                random: &mut world.shared.random,
                pool: &mut world.shared.pool,
                tuning: impact_particles::Tuning::default(),
            },
        )
        .unwrap();
    v.motion.finish_frame(dt);
    Step {
        control,
        stage,
        peer_stage: None,
        cells: visited,
        tank_hits: hits,
        deform_hits,
        sampled_points,
        spark_events,
        ordinary_hits,
        scenery_ticks: 0,
    }
}
