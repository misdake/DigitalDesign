//! Persistent two-car trajectories through the original private collision entries.
mod complete_driving_step;
mod complete_scene_assets;
mod debris_scene_assets;
mod deform_assets;
mod driving_capture;
mod driving_devices;
mod driving_step;
mod identity_assets;
mod impact_snapshot;
#[allow(dead_code)]
#[path = "terrain_assets.rs"]
mod pages;
mod parent_scene_assets;
mod pickup_scene_assets;
mod pickup_snapshot;
mod prefix_scene_assets;
#[allow(dead_code, clippy::needless_range_loop, clippy::manual_range_contains)]
#[path = "vehicle_oracle.rs"]
mod reference;
mod reference_world;
mod scene_export;
mod scenery_assets;
mod scenery_frame;
mod vehicle_startup_assets;
use serde_json::{Value, json};
use std::{fs, path::Path};
use tankracer_assets::{config::Part, terrain::TerrainData};
use tankracer_game::{
    attitude, body, contact, contact_pipeline as ground, drive, effect_pool, impact_particles,
    math::Matrix,
    motion, pose, powertrain,
    sampling::Transform,
    suspension, terrain, vehicle,
    vehicle_runtime::{SinglePlayer, rcc_car_probe, rcc_probe},
    vehicle_startup::Player,
    velocity,
};

fn core(p: &Player, data: &TerrainData, stage: &[i32]) -> Vec<i32> {
    let v = &p.physics;
    let mut out =
        driving_capture::state(&v.motion, &v.powertrain, &p.prefix.boost, &v.contact, data);
    out[73..89].copy_from_slice(stage);
    let gun = &p.model.parts[Part::Gun.slot()]
        .as_ref()
        .unwrap()
        .geometry()
        .vertices;
    out.extend(driving_capture::body_state(&v.motion, &v.pose, gun));
    assert_eq!(out.len(), 295);
    out
}
fn mesh_state(out: &mut Vec<i32>, p: &Player) {
    out.extend(p.damage.body.map(i32::from));
    out.extend(p.damage.turret.map(i32::from));
    out.extend(p.damage.tracks.map(i32::from));
    let mesh = p.model.parts[Part::Body.slot()]
        .as_ref()
        .unwrap()
        .geometry();
    out.extend(mesh.vertices.iter().flatten().copied().map(i32::from));
    out.extend(mesh.triangles.iter().flat_map(|t| t.normal.map(i32::from)));
}
fn snapshot(world: &SinglePlayer, stages: &[Vec<i32>; 2]) -> Vec<i32> {
    let a = world.vehicles.entries[0].as_ref().unwrap();
    let b = world.vehicles.entries[1].as_ref().unwrap();
    let data = world.terrain();
    let mut out = core(a, data, &stages[0]);
    out.extend(impact_snapshot::snapshot(
        [0; 4],
        &world.shared.impacts,
        &a.physics,
        &world.shared.pool,
        world.shared.random,
    ));
    out.extend(a.damage.body.map(i32::from));
    out.extend(a.damage.turret.map(i32::from));
    out.extend(a.damage.tracks.map(i32::from));
    out.extend(a.physics.collision.ground_delta);
    let mesh = a.model.parts[Part::Body.slot()]
        .as_ref()
        .unwrap()
        .geometry();
    out.extend(mesh.vertices.iter().flatten().copied().map(i32::from));
    out.extend(mesh.triangles.iter().flat_map(|t| t.normal.map(i32::from)));
    out.push(a.physics.collision.timer);
    out.extend(a.physics.collision.tank_delta);
    out.extend(core(b, data, &stages[1]));
    out.extend([b.physics.impact_cooldown as i32, b.physics.collision.timer]);
    out.extend(b.physics.collision.tank_delta);
    out.extend(b.physics.collision.ground_delta);
    mesh_state(&mut out, b);
    assert_eq!(out.len(), 2279);
    for p in [a, b] {
        out.extend([
            p.physics.collision.material,
            p.physics.collision.deformable.map_or(-1, |e| e.0 as i32),
            p.physics.collision.impact_accumulator,
        ]);
    }
    out.push(world.identity.current_slot as i32);
    for p in [a, b] {
        out.extend([
            p.physics.collision.collision_size as i32,
            p.physics.collision.obstacle_impulse,
        ]);
    }
    out
}

struct PhysicalFrame<'a, 'b> {
    world: &'a mut SinglePlayer,
    setup: &'a driving_step::Setup<'b>,
    input: drive::Input,
    number: u32,
    result: Option<driving_step::Step>,
    scenery_ticks: usize,
}
impl tankracer_game::entity_factory::Methods<tankracer_game::scene_entity::Handle>
    for PhysicalFrame<'_, '_>
{
    fn invoke(
        &mut self,
        method: tankracer_game::dispatch::MethodId,
        mut call: tankracer_game::entity_factory::Invocation<
            '_,
            tankracer_game::scene_entity::Handle,
        >,
        env: tankracer_game::entity_factory::Environment<'_>,
    ) -> Result<(), i32> {
        if method.0 == 74 {
            assert_eq!(
                call.entity,
                self.world.vehicles.entries[0]
                    .as_ref()
                    .unwrap()
                    .physics
                    .entity
            );
            self.result = Some(driving_step::step(
                self.world,
                self.setup,
                0,
                self.input,
                self.number,
                Some(&mut call.context),
            ));
            return Ok(());
        }
        if matches!(method.0, 65 | 80 | 83 | 114) {
            self.scenery_ticks += 1;
        }
        tankracer_game::entity_factory::Methods::invoke(
            &mut tankracer_game::scene_objects::View {
                execution: tankracer_game::scene_objects::Execution::Simulation {
                    random: &mut self.world.shared.random,
                },
                world: &mut self.world.objects.world,
                components: &mut self.world.objects.components,
                vehicles: Some(&mut self.world.vehicles),
                effect_pool: Some(&mut self.world.shared.pool),
                temporary: Some(&mut self.world.shared.temporary),
            },
            method,
            call,
            env,
        )
    }
}
fn ordered_frame(
    world: &mut SinglePlayer,
    setup: &driving_step::Setup<'_>,
    input: drive::Input,
    number: u32,
    event: Option<usize>,
) -> driving_step::Step {
    use tankracer_game::{entity_factory as factory, scene_loading};
    let mut identity = std::mem::replace(&mut world.identity, factory::State::new(2).unwrap());
    let classes = world.objects.world.classes.clone();
    let template = world.registry.environment(100000);
    let env = factory::Environment {
        classes: &classes,
        ..template
    };
    world.objects.components.dt = 33;
    let target = event.map(|id| world.objects.world.colliders[id].entity);
    let mut methods = PhysicalFrame {
        world,
        setup,
        input,
        number,
        result: None,
        scenery_ticks: 0,
    };
    let mut context = identity.context();
    if let Some(target) = target {
        context
            .queues
            .queue(context.ticks, target, true, 100000)
            .unwrap();
    }
    context
        .tick(scene_loading::TICK, env, &mut methods)
        .unwrap();
    context
        .drain(scene_loading::STOP, env, &mut methods)
        .unwrap();
    let mut result = methods.result.take().unwrap();
    result.scenery_ticks = methods.scenery_ticks;
    world.identity = identity;
    result
}
fn obstacle_segment(world: &SinglePlayer, target: usize) -> ([i32; 3], [i32; 3]) {
    use tankracer_game::object_query;
    let grid = &world.objects.world.grid;
    for axis in [0, 2] {
        let mut a = world.objects.world.colliders[target].pose.position;
        let mut b = a;
        let reach = if target == 181 { 2000 } else { 10000 };
        a[axis] += reach;
        b[axis] -= reach;
        let mut hit = object_query::Hit::default();
        object_query::obstacles(
            &object_query::Grid {
                origin: grid.origin,
                shifts: grid.shifts,
                cells: grid.cells.each_ref().map(Vec::as_slice),
            },
            &world.objects.world.colliders,
            world.vehicles.entries[0].as_ref().unwrap().physics.entity,
            a,
            b,
            &mut hit,
        )
        .unwrap();
        if hit.object == Some(object_query::ObstacleId(target)) {
            return (a, b);
        }
    }
    panic!("missing ordinary segment {target}");
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let root = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    let steps: usize = args[2].parse().unwrap();
    let template_world = reference_world::two_cars(root, 1);
    let originals = [
        template_world.vehicles.entries[0].as_ref().unwrap().clone(),
        template_world.vehicles.entries[1].as_ref().unwrap().clone(),
    ];
    let terrain = template_world.terrain().clone();
    let surfaces = template_world.surfaces().clone();
    let common: Value =
        serde_json::from_slice(&fs::read(out.join("driving.json")).unwrap()).unwrap();
    let env = &common["cases"][0]["environment"];
    let model = &originals[0].model;
    let tank = &model.config.tank;
    let tuning = drive::Tuning::default();
    let contact = contact::Config::try_from(&tank.suspension).unwrap();
    let bounds = model.geometry_bounds;
    let layout = [
        bounds.min[0],
        bounds.min[1],
        bounds.min[2],
        bounds.max[0],
        bounds.max[2],
    ];
    let mut exported = Vec::<Value>::new();
    let mut cells = std::collections::BTreeSet::new();
    let (mut asset_session, resources) = reference_world::constructed(root).into_session().unwrap();
    let authored = asset_session.world_mut_at_load().unwrap();
    let bindings = parent_scene_assets::configuration(authored);
    let pickup_bindings = pickup_scene_assets::configuration(authored);
    fs::write(
        out.join("pickup-bindings.json"),
        serde_json::to_vec(&pickup_bindings).unwrap(),
    )
    .unwrap();
    let geometry = *authored.objects.components.actors.pickup_geometry();
    let pickup_tuning =
        rcc_car_probe::pickup_tuning(authored, tankracer_game::object_query::TankId(0));
    let mut pickup_config = vec![
        geometry.separation,
        geometry.diameter as i32,
        pickup_tuning.outer_growth,
        pickup_tuning.inner_growth,
        pickup_tuning.flare_growth,
    ];
    pickup_config.extend(pickup_tuning.palette.into_iter().flatten().map(i32::from));
    pickup_config.extend(
        authored
            .objects
            .components
            .actors
            .pickup_amounts()
            .map(i32::from),
    );
    fs::write(
        out.join("pickup-config.json"),
        serde_json::to_vec(&pickup_config).unwrap(),
    )
    .unwrap();
    fs::write(
        out.join("parent-bindings.json"),
        serde_json::to_vec(&bindings).unwrap(),
    )
    .unwrap();
    for (name, words) in [
        ("debris.bin", debris_scene_assets::encode(authored)),
        ("symbols.bin", debris_scene_assets::bindings(authored)),
    ] {
        fs::write(
            out.join(name),
            words
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    let mut models = Vec::new();
    for row in bindings.iter().chain(&pickup_bindings) {
        let name = row["name"].as_str().unwrap();
        if !name.is_empty() && !models.iter().any(|(key, _)| key == name) {
            models.push((name.to_string(), resources.auxiliary_model(name).unwrap()));
        }
    }
    for (name, velocities, separation, object, ordinary, retire, transient) in [
        ("complete-startup", [0, 0], 1600, None, false, false, false),
        (
            "complete-fast-controls",
            [600000, 0],
            16000,
            None,
            false,
            false,
            false,
        ),
        (
            "complete-controls",
            [600000, 0],
            16000,
            None,
            false,
            false,
            false,
        ),
        (
            "complete-visibility",
            [100000, 0],
            1600,
            None,
            false,
            false,
            false,
        ),
        (
            "complete-effect",
            [100000, 0],
            1600,
            None,
            false,
            false,
            false,
        ),
        (
            "complete-prepare",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-drive",
            [100000, 0],
            1600,
            None,
            false,
            false,
            false,
        ),
        (
            "complete-pair",
            [300000, -300000],
            1000,
            None,
            false,
            false,
            false,
        ),
        (
            "complete-wall",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "pickup-186",
            [300000, 0],
            1600,
            Some(186),
            true,
            false,
            false,
        ),
        (
            "pickup-187",
            [300000, 0],
            1600,
            Some(187),
            true,
            false,
            false,
        ),
        (
            "pickup-189",
            [300000, 0],
            1600,
            Some(189),
            true,
            false,
            false,
        ),
        (
            "prefix-boost",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        ("pair-chase", [100000, 0], 1600, None, false, false, false),
        (
            "debris-169",
            [300000, 0],
            1600,
            Some(169),
            true,
            false,
            false,
        ),
        (
            "debris-170",
            [300000, 0],
            1600,
            Some(170),
            true,
            false,
            false,
        ),
        (
            "debris-171",
            [300000, 0],
            1600,
            Some(171),
            true,
            false,
            false,
        ),
        (
            "debris-179",
            [300000, 0],
            1600,
            Some(179),
            true,
            false,
            false,
        ),
        (
            "debris-180",
            [300000, 0],
            1600,
            Some(180),
            true,
            false,
            false,
        ),
        (
            "debris-181",
            [300000, 0],
            1600,
            Some(181),
            true,
            false,
            false,
        ),
        (
            "debris-peer-blast",
            [300000, 0],
            1600,
            Some(181),
            true,
            false,
            false,
        ),
        (
            "debris-retire",
            [300000, 0],
            1600,
            Some(170),
            true,
            true,
            false,
        ),
        (
            "pair-head-on",
            [300000, -300000],
            1600,
            None,
            false,
            false,
            false,
        ),
        (
            "deform-wide",
            [300000, 0],
            1600,
            Some(124),
            false,
            false,
            false,
        ),
        (
            "deform-reverse",
            [-300000, 0],
            1600,
            Some(173),
            false,
            false,
            false,
        ),
        (
            "scenery-fall",
            [300000, 0],
            1600,
            Some(60),
            true,
            false,
            false,
        ),
        (
            "scenery-bounce",
            [300000, 0],
            1600,
            Some(58),
            true,
            false,
            false,
        ),
        (
            "retire-fall",
            [300000, 0],
            1600,
            Some(60),
            true,
            true,
            false,
        ),
        (
            "retire-bounce",
            [300000, 0],
            1600,
            Some(58),
            true,
            true,
            true,
        ),
        (
            "auxiliary-bench",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "auxiliary-gate",
            [300000, 0],
            1600,
            Some(141),
            true,
            false,
            false,
        ),
        (
            "auxiliary-closer",
            [300000, 0],
            1600,
            Some(172),
            true,
            false,
            false,
        ),
        (
            "auxiliary-bench-peer",
            [300000, 0],
            1600,
            Some(70),
            true,
            false,
            false,
        ),
        (
            "auxiliary-gate-peer",
            [300000, 0],
            1600,
            Some(142),
            true,
            false,
            false,
        ),
        (
            "auxiliary-gate-north",
            [300000, 0],
            1600,
            Some(237),
            true,
            false,
            false,
        ),
        (
            "auxiliary-gate-south",
            [300000, 0],
            1600,
            Some(238),
            true,
            false,
            false,
        ),
        (
            "auxiliary-retire-bench",
            [300000, 0],
            1600,
            Some(69),
            true,
            true,
            false,
        ),
        (
            "auxiliary-retire-closer",
            [300000, 0],
            1600,
            Some(172),
            true,
            true,
            false,
        ),
    ] {
        let mut session = reference_world::complete(root);
        let mut empty = reference_world::two_cars(root, 1);
        let world = if object.is_some() {
            session.world_mut_at_load().unwrap()
        } else {
            &mut empty
        };
        let entity = world.vehicles.entries[0].as_ref().unwrap().physics.entity;
        let segment = if ordinary {
            object.map(|index| {
                if name.starts_with("pickup-") {
                    let mut a = world.objects.world.colliders[index].pose.position;
                    let mut b = a;
                    a[0] += 500;
                    b[0] -= 500;
                    (a, b)
                } else {
                    obstacle_segment(world, index)
                }
            })
        } else {
            None
        };
        let start = object.map(|index| {
            if let Some((a, _)) = segment {
                return a;
            }
            let c = &world.objects.world.colliders[index];
            let component = world.objects.components.surfaces.entries[index]
                .as_ref()
                .unwrap();
            let vertex = world
                .objects
                .components
                .surfaces
                .meshes
                .get(component.mesh)
                .unwrap()
                .vertices[0]
                .map(|v| i32::from(v) * 8);
            let mut point = c.pose.matrix.local_to_world(vertex);
            for i in 0..3 {
                point[i] += c.pose.position[i];
            }
            point
        });
        if object.is_some() {
            let (scene, _) = scene_export::encode(&world.objects.world);
            let config = deform_assets::encode(world);
            let bytes = |words: &[u16]| {
                words
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>()
            };
            if out.join("scene.bin").exists() {
                assert_eq!(fs::read(out.join("scene.bin")).unwrap(), bytes(&scene));
            }
            fs::write(out.join("scene.bin"), bytes(&scene)).unwrap();
            fs::write(out.join("deform.bin"), bytes(&config)).unwrap();
            fs::write(
                out.join("deform-bound.bin"),
                bytes(&deform_assets::encode_bound(world)),
            )
            .unwrap();
            let mut mesh_ids = Vec::new();
            for id in &world.objects.world.deformables {
                let surface = world.objects.components.surfaces.entries[id.0]
                    .as_ref()
                    .unwrap();
                mesh_ids.push(surface.mesh.index() as u16);
            }
            let encoded = bytes(&mesh_ids);
            let path = out.join("deform-mesh-ids.bin");
            if path.exists() {
                assert_eq!(fs::read(&path).unwrap(), encoded);
            }
            fs::write(path, encoded).unwrap();
        }
        let mut startup_models = Vec::new();
        for (i, original) in originals.iter().enumerate() {
            let mut p = original.clone();
            let v = &mut p.physics;
            v.motion = motion::State::default();
            v.powertrain = powertrain::State::default();
            v.contact = ground::Pipeline::default();
            v.collision = Default::default();
            v.impact_cooldown = if object.is_some() { 500 } else { 0 };
            if object.is_some() && i == 0 {
                v.entity = entity;
            }
            v.player = i == 0;
            p.slot = i as u8;
            p.visible = true;
            p.damage = Default::default();
            p.prefix.boost = Default::default();
            v.pose = p.model.pose_config([0; 3]);
            let z = start.map_or(9072, |p| p[2]) + i as i32 * separation;
            let x = start.map_or(-235, |p| p[0]);
            let mut height = 0;
            let sampled = terrain::height(&terrain, x, z, &mut height).unwrap();
            assert!(ordinary || sampled);
            v.motion.poses.physical_position = [
                x,
                height - p.model.primary_local[0][1] - v.pose.track_offsets[0][1],
                z,
            ];
            if let Some((a, b)) = segment {
                if i == 0 || name == "debris-peer-blast" {
                    v.motion.poses.physical_position = a;
                    if i == 1 {
                        v.motion.poses.physical_position[2] += 1600;
                    }
                    v.motion.previous_position =
                        std::array::from_fn(|axis| a[axis].wrapping_mul(2).wrapping_sub(b[axis]));
                    // The post-contact car center is above this low barrier.
                    // A descending swept segment reaches its actual hit volume.
                    if object == Some(169) {
                        v.motion.previous_position[1] += 2000;
                    }
                }
            }
            v.motion.poses.body.matrix = Matrix::IDENTITY;
            v.motion.poses.reference = Matrix::IDENTITY;
            v.motion
                .poses
                .update(
                    &v.pose,
                    [0; 4],
                    &pose::Environment {
                        timer: 0,
                        ambient_wobble: false,
                        session_active: true,
                        ambient_phase: 0,
                        recoil_mode: 0,
                        turret_yaw: 0,
                        recoil: 0,
                        gun_pitch: 0,
                        external: pose::ExternalBasis::Body,
                    },
                )
                .unwrap();
            v.motion.velocity = [0, 0, velocities[i]];
            if name == "complete-fast-controls" {
                v.powertrain.local_velocity = v.motion.velocity;
            }
            if ordinary && i == 0 {
                v.motion.velocity = [-400000, -100000, 200000];
            }
            if name.starts_with("pickup-") {
                v.motion.velocity = [0; 3];
            }
            v.pose_input.external = pose::ExternalBasis::Body;
            for j in 0..4 {
                v.contact.sampling.primary.points[j].local = p.model.primary_local[j];
                v.contact.response.points[j].corner = j as u32;
            }
            if name == "complete-startup" {
                let spawn = pose::Pose {
                    matrix: Matrix::IDENTITY,
                    position: v.motion.poses.physical_position,
                };
                let environment = tankracer_game::vehicle_startup::Environment {
                    ambient_wobble: false,
                    session_active: true,
                    ambient_phase: 0,
                    gun_pivot: [0; 3],
                };
                startup_models.extend(vehicle_startup_assets::encode(
                    &p.model,
                    spawn,
                    environment,
                    v.entity,
                    v.class,
                    v.player,
                ));
                p = Player::construct(v.entity, v.class, p.model, spawn, environment, v.player)
                    .unwrap();
                p.slot = i as u8;
            }
            world.vehicles.entries[i] = Some(p);
        }
        if !startup_models.is_empty() {
            assert_eq!(startup_models.len(), vehicle_startup_assets::WORDS * 2);
            fs::write(
                out.join("startup-models.bin"),
                startup_models
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        world.vehicles.racers = [None; 9];
        world.vehicles.racers[0] = Some(tankracer_game::object_query::TankId(0));
        world.vehicles.racers[1] = Some(tankracer_game::object_query::TankId(1));
        if object.is_some() && name != "debris-peer-blast" {
            world.vehicles.racers[1] = None;
        }
        world.shared.random = 0x1234abcd;
        world.shared.noise = ground::initialize_noise(&mut world.shared.random);
        world.shared.pool = effect_pool::Pool::default();
        world.shared.impacts = impact_particles::State::default();
        if transient {
            let id = object.unwrap();
            world.objects.components.objects[id]
                .as_mut()
                .unwrap()
                .transient = true;
            let entity = world.objects.world.colliders[id].entity;
            world.identity.ticks.flags[entity.0 as usize] &= !8;
        }
        if ordinary {
            let retained = object
                .filter(|id| {
                    (name.starts_with("auxiliary-") && *id == 172) || name.starts_with("pickup-")
                })
                .and_then(|id| {
                    let entity = world.objects.world.colliders[id].entity;
                    (world.identity.ticks.flags[entity.0 as usize] & 16 != 0).then_some(entity)
                });
            // The physical qualifier freezes unrelated actors in its initial
            // snapshot; live car activation still follows the real scheduler.
            for flags in &mut world.identity.ticks.flags {
                *flags &= !16;
            }
            world.identity.ticks.order.clear();
            world.identity.ticks.enable(entity).unwrap();
            if let Some(entity) = retained {
                world.identity.ticks.enable(entity).unwrap();
            }
            world.identity.ticks.next = None;
            let live = scenery_frame::initial(world);
            let bytes = live
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            if name == "scenery-fall" {
                fs::write(out.join("scenery.bin"), &bytes).unwrap();
            }
            fs::write(out.join(format!("scenery-initial-{name}.bin")), &bytes).unwrap();
            for (filename, words) in [
                (
                    "identity.bin",
                    identity_assets::encode(&world.identity.entities),
                ),
                ("lifetime.bin", scenery_frame::initial_lists(world)),
            ] {
                let bytes = words
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>();
                if out.join(filename).exists() {
                    assert_eq!(fs::read(out.join(filename)).unwrap(), bytes);
                }
                fs::write(out.join(filename), bytes).unwrap();
            }
        }
        if name == "complete-visibility" {
            for car in world.vehicles.entries[..2].iter_mut().flatten() {
                car.input.drive.throttle = 12000;
                car.input.drive.steering = -4000;
            }
        }
        if name == "complete-effect" {
            for car in world.vehicles.entries[..2].iter_mut().flatten() {
                car.physics.contact.sampling.effect.timer = 1;
                car.physics.contact.sampling.effect.active = 1;
                car.physics.contact.sampling.effect.restore = car.physics.pose.turret_offset[2];
            }
            world.shared.shield.banks[0].tail = 0x12345678;
            world.shared.shield.banks[1].tail = 0x87654321;
        }
        let initial_seed = world.shared.random;
        let pickup_initial = if name.starts_with("pickup-") {
            pickup_scene_assets::capture(world)
        } else {
            Vec::new()
        };
        let mut prefix_initial = Vec::new();
        if name.starts_with("prefix-") || name == "complete-prepare" {
            let s = &mut world.vehicles.entries[0].as_mut().unwrap().prefix;
            s.boost.start(entity, false, 0, &mut world.shared.boost);
            s.boost.camera_timer = 199;
            world.shared.camera = tankracer_game::vehicle_prefix::boost::Camera {
                pitch: -1638,
                height: 5500,
                distance: -7000,
            };
            s.temporary_timer = 33;
            s.temporary_slot = 0;
            world.shared.temporary.timestamps[0] = 1;
            world.shared.boost.source_uv = [0, 65535];
            world.shared.view.enabled = true;
            world.shared.view.position = [800, 1600, 2400];
            prefix_initial = prefix_scene_assets::encode(world);
            world.vehicles.entries[0]
                .as_mut()
                .unwrap()
                .prefix
                .spin
                .start(true, 0, &mut world.shared.spin, &mut world.shared.random);
            let cfg = rcc_car_probe::prefix_config(world, tankracer_game::object_query::TankId(0));
            fs::write(
                out.join("prefix-config.bin"),
                cfg.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>(),
            )
            .unwrap();
        }
        if name.starts_with("complete-") && name != "complete-prepare" {
            prefix_initial = prefix_scene_assets::encode(world);
            let cfg = rcc_car_probe::prefix_config(world, tankracer_game::object_query::TankId(0));
            fs::write(
                out.join("prefix-config.bin"),
                cfg.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>(),
            )
            .unwrap();
        }
        let noise = world.shared.noise;
        let mut stages = std::array::from_fn(|i| {
            let p = world.vehicles.entries[i].as_ref().unwrap();
            reference::drive_fixture::flatten(&p.physics.powertrain.drive_state(&p.physics.motion))
        });
        let initial = snapshot(&world, &stages);
        let complete_initial = if name.starts_with("complete-") {
            complete_scene_assets::capture(world)
        } else {
            Vec::new()
        };
        let car_bindings = debris_scene_assets::cars(world);
        let racer_slots = world
            .vehicles
            .racers
            .map(|v| v.map_or(65535, |v| v.0 as u16));
        let entities: Vec<_> = world.vehicles.entries[..2]
            .iter()
            .map(|p| p.as_ref().unwrap().physics.entity.0)
            .collect();
        let mut frames = Vec::new();
        let mut hits = 0;
        let mut deform_hits = 0;
        let mut sampled_points = 0;
        let mut spark_events = 0;
        let mut ordinary_hits = 0;
        let mut scenery_ticks = 0;
        let mut activated = false;
        let before_mesh = deform_assets::capture(world);
        let baseline = (0..world.objects.world.objects.len())
            .map(|i| scene_export::record(&world.objects.world, i))
            .collect::<Vec<_>>();
        let actors = if object.is_some() { 1 } else { 2 };
        let mut visited = std::collections::BTreeSet::new();
        if name.starts_with("auxiliary-")
            || name.starts_with("debris-")
            || name.starts_with("prefix-")
            || name.starts_with("pickup-")
        {
            let origin = world.objects.world.colliders[object.unwrap()].pose.position;
            let x = ((origin[0] >> 3) - terrain.origin[0]) >> terrain.shifts[0];
            let z = ((origin[2] >> 3) - terrain.origin[1]) >> terrain.shifts[1];
            assert!((0..8).contains(&x) && (0..8).contains(&z));
            visited.insert((x + z * 8) as usize);
        }
        let setup = driving_step::Setup {
            complete: name.starts_with("complete-"),
            sample_controls: if name == "complete-controls" {
                1
            } else if name == "complete-fast-controls" {
                2
            } else {
                0
            },
            terrain: &terrain,
            surfaces: &surfaces,
            tank,
            tuning: &tuning,
            contact: &contact,
            layout,
            environment: env,
            deform: object.is_some(),
            ordinary,
            prepare: name.starts_with("prefix-") || name.starts_with("complete-"),
            pickups: name.starts_with("pickup-"),
            models: if name.starts_with("auxiliary-")
                || name.starts_with("debris-")
                || name.starts_with("prefix-")
                || name.starts_with("pickup-")
                || name == "complete-wall"
                || name == "complete-prepare"
            {
                &models
            } else {
                &[]
            },
        };
        let case_steps = if name == "complete-fast-controls" {
            steps.clamp(2, 3)
        } else if name == "complete-controls" {
            steps.max(6)
        } else if name == "complete-visibility" {
            steps.max(4)
        } else if retire {
            steps.clamp(3, 4)
        } else if name.starts_with("pickup-") {
            steps.clamp(2, 4)
        } else if ordinary {
            steps.min(4)
        } else {
            steps
        };
        for frame in 0..case_steps {
            let number = frame as u32 + 1;
            world.frame = number;
            for own in 0..actors {
                if name == "complete-visibility" {
                    world.vehicles.entries[own].as_mut().unwrap().visible =
                        !((frame == 1 && own == 0) || (frame == 2 && own == 1));
                }
                let input = drive::Input {
                    throttle: if name.starts_with("pickup-") {
                        0
                    } else if velocities[own] < 0 {
                        -65535
                    } else {
                        65535
                    },
                    ..Default::default()
                };
                let result = if ordinary {
                    ordered_frame(
                        world,
                        &setup,
                        input,
                        number,
                        if retire && frame == 1 { object } else { None },
                    )
                } else {
                    driving_step::step(world, &setup, own, input, number, None)
                };
                if ordinary
                    && !name.starts_with("auxiliary-")
                    && !name.starts_with("debris-")
                    && !name.starts_with("prefix-")
                    && !name.starts_with("pickup-")
                    && frame == 0
                {
                    assert_eq!(result.scenery_ticks, 0);
                }
                if ordinary
                    && !name.starts_with("auxiliary-")
                    && !name.starts_with("debris-")
                    && !name.starts_with("prefix-")
                    && !name.starts_with("pickup-")
                    && frame == 1
                {
                    assert_eq!(result.scenery_ticks, 1);
                }
                if name.starts_with("auxiliary-") {
                    activated |= world.objects.components.objects[object.unwrap()]
                        .as_ref()
                        .is_some_and(|o| {
                            o.auxiliary_mesh.is_some() && o.auxiliary_motion.is_some()
                        });
                }
                let control = result.control;
                stages[own] = result.stage;
                visited.extend(result.cells);
                hits += result.tank_hits;
                deform_hits += result.deform_hits;
                sampled_points += result.sampled_points;
                spark_events += result.spark_events;
                ordinary_hits += result.ordinary_hits;
                scenery_ticks += result.scenery_ticks;
                let mut expected = snapshot(&world, &stages);
                expected.push(world.shared.random as i32);
                let surface = if object.is_some() {
                    deform_assets::capture(world)
                } else {
                    Vec::new()
                };
                let mut resource_versions = Vec::new();
                if object.is_some() {
                    for id in &world.objects.world.deformables {
                        let component = world.objects.components.surfaces.entries[id.0]
                            .as_ref()
                            .unwrap();
                        let version = world
                            .objects
                            .components
                            .surfaces
                            .meshes
                            .info(component.mesh)
                            .unwrap()
                            .version;
                        for shift in [0, 16, 32, 48] {
                            resource_versions.push((version >> shift) as u16);
                        }
                    }
                }
                let live = if ordinary {
                    scenery_frame::capture(world, &baseline)
                } else {
                    Vec::new()
                };
                frames.push(
                    json!({"control":control,"expected":expected,"surface":surface,"scenery":live,"resource_versions":resource_versions,
                        "complete":if setup.complete {complete_scene_assets::capture(world)}else{Vec::new()},
                        "prefix": if setup.prepare {prefix_scene_assets::encode(world)}else{Vec::new()},
                        "pickup":if name.starts_with("pickup-"){pickup_scene_assets::capture(world)}else{Vec::new()},
                        "debris": if name.starts_with("debris-") {debris_scene_assets::capture(world)} else {Vec::new()},
                        "parents": if name.starts_with("auxiliary-") || name.starts_with("debris-") || name.starts_with("prefix-") || name.starts_with("pickup-") || name=="complete-wall" || name=="complete-prepare" { parent_scene_assets::capture(world, &bindings) } else { Vec::new() }}),
                );
                if retire && frame >= 1 {
                    let id = object.unwrap();
                    assert!(world.objects.components.objects[id].is_none());
                    assert!(
                        world
                            .identity
                            .entities
                            .record(world.objects.world.colliders[id].entity)
                            .is_none()
                    );
                    assert!(
                        world
                            .objects
                            .world
                            .grid
                            .cells
                            .iter()
                            .all(|cell| !cell
                                .contains(&tankracer_game::object_query::ObstacleId(id)))
                    );
                }
            }
        }
        let after_mesh = deform_assets::capture(world);
        let mut changed_vertices = 0;
        let mut cursor = 0;
        for id in &world.objects.world.deformables {
            let component = world.objects.components.surfaces.entries[id.0]
                .as_ref()
                .unwrap();
            cursor += 5;
            let count = world
                .objects
                .components
                .surfaces
                .meshes
                .get(component.mesh)
                .unwrap()
                .vertices
                .len();
            for _ in 0..count {
                changed_vertices +=
                    usize::from(before_mesh[cursor..cursor + 3] != after_mesh[cursor..cursor + 3]);
                cursor += 3;
            }
        }
        assert!(
            hits > 0
                || (deform_hits > 0 && changed_vertices > 0)
                || ordinary_hits > 0
                || name.starts_with("pickup-")
                || name == "complete-controls"
                || name == "complete-fast-controls",
            "paired trajectories must exercise real car collisions: {name}, ordinary {ordinary_hits}"
        );
        if name == "complete-controls" {
            assert_eq!(
                frames[2]["complete"][10416].as_u64(),
                Some(1),
                "late countdown press must arm fast start"
            );
            assert_eq!(
                frames[4]["complete"][7723].as_u64(),
                Some(65535),
                "right turret key must publish a negative signed angle"
            );
            assert_eq!(
                frames[6]["complete"][7758].as_u64(),
                Some(1),
                "auxiliary brake must publish reverse braking"
            );
        }
        if name == "complete-fast-controls" {
            assert_eq!(
                frames[0]["complete"][10368].as_u64(),
                Some(1),
                "fast auxiliary override must activate"
            );
            assert_eq!(
                frames[0]["complete"][10420].as_u64(),
                Some(310),
                "fast brake must publish the live model turn limit"
            );
            assert_eq!(
                frames[2]["complete"][10370].as_u64(),
                Some(frames[0]["complete"][10370].as_u64().unwrap() + 66),
                "release must advance the surface override restoration"
            );
        }
        if object.is_some() && !ordinary && steps >= 2 {
            assert!(
                sampled_points > 0,
                "deformed height must reach next-frame contacts"
            );
        }
        if name.starts_with("auxiliary-") {
            assert!(
                activated,
                "{name}: expected actual mesh fragment activation"
            );
            assert!(scenery_ticks > 0, "{name}: expected actual parent ticks");
        }
        if name.starts_with("debris-") {
            if !retire {
                assert_ne!(
                    world.objects.components.objects[object.unwrap()]
                        .as_ref()
                        .unwrap()
                        .data
                        .debris
                        .as_ref()
                        .unwrap()
                        .flags
                        & 1,
                    0,
                    "{name}: debris never activated"
                );
            }
            if object == Some(169) {
                assert!(
                    world.objects.components.objects[172]
                        .as_ref()
                        .unwrap()
                        .auxiliary_mesh
                        .is_some(),
                    "attached binding must activate the real Closer"
                );
            }
            if object == Some(181) {
                assert!(
                    world.vehicles.entries[0]
                        .as_ref()
                        .unwrap()
                        .physics
                        .collision
                        .collision_size
                        >= 300,
                    "current car must receive a blast in actual force range"
                );
            }
            if name == "debris-peer-blast" {
                let peer = &world.vehicles.entries[1].as_ref().unwrap().physics;
                assert_ne!(
                    peer.motion.angular, [0; 3],
                    "peer must receive the real blast"
                );
            }
        }
        if name == "complete-effect" {
            assert_eq!(world.shared.shield.banks[0].tail, 0);
            assert_eq!(world.shared.shield.banks[1].tail, 0);
            assert!(
                world.vehicles.entries[..2].iter().flatten().all(|c| c
                    .physics
                    .contact
                    .sampling
                    .effect
                    .timer
                    <= 0)
            );
        }
        if name.starts_with("prefix-") || name == "complete-prepare" {
            let s = &world.vehicles.entries[0].as_ref().unwrap().prefix;
            assert_eq!(
                s.boost.applied, 1,
                "boost slot must apply to the live player"
            );
            assert!(
                s.boost.timer > 0 && s.boost.extra_speed > 0,
                "boost must reach power integration"
            );
            assert_eq!(
                s.temporary_timer, 0,
                "temporary effect must expire before physics"
            );
            assert_eq!(world.shared.temporary.available, 0);
            assert!(
                s.spin.speed > 0 && world.shared.random != initial_seed,
                "shared prefix RNG must advance"
            );
            if name == "complete-prepare" && steps >= 3 {
                let p = world.vehicles.entries[0].as_ref().unwrap();
                assert!(
                    p.smoke.particles.iter().any(|p| p.life > 0),
                    "continuous driving must emit smoke after its timer threshold"
                );
                assert!(
                    world.shared.impacts.batches.iter().any(|b| b.active),
                    "traction smoke must emit its synchronous impact batch"
                );
            }
        }
        if name.starts_with("pickup-") {
            let p = world.vehicles.entries[0].as_ref().unwrap();
            let Some(tankracer_game::actor_startup::Actor::Powerup(pickup)) = world
                .objects
                .components
                .actors
                .get(tankracer_game::object_query::ObstacleId(object.unwrap()))
            else {
                panic!("pickup actor")
            };
            assert_eq!(
                p.inventory.weapon, pickup.kind,
                "pickup must collect its authored inventory"
            );
            assert!(
                world.shared.pool.quad_count > 0,
                "collection must allocate its live lights"
            );
        }
        cells.extend(visited.iter().copied());
        exported.push(json!({"name":name,"complete_initial":complete_initial,"cars":actors,"car_bindings":car_bindings,"prefix_initial":prefix_initial,"pickup_initial":pickup_initial,"racer_slots":racer_slots,"entities":entities,"initial":initial,"seed_after_noise":initial_seed,"noise":noise,
            "frames":frames,"cells":visited,"fallback_contacts":0,"tank_contacts":hits,"deform_contacts":deform_hits,
            "deformed_vertices":changed_vertices,"surface_sample_points":sampled_points,"deform_spark_events":spark_events,"scene":object.is_some(),"scenery":ordinary,"ordinary_contacts":ordinary_hits,"scenery_ticks":scenery_ticks,
            "retirement":retire,"transient":transient,"retire_object":if retire {object.unwrap()+1} else {0},
            "retire_frame":if retire {1} else {65535},"scenery_initial":format!("scenery-initial-{name}.bin")}));
    }
    fs::write(
        out.join("paired.json"),
        serde_json::to_vec(&json!({"cases":exported,"cells":cells})).unwrap(),
    )
    .unwrap();
    let mut collision = Vec::new();
    for cell in &cells {
        collision.extend(pages::cell_bytes(&terrain, *cell));
    }
    fs::write(out.join("paired-cells.bin"), collision).unwrap();
}
