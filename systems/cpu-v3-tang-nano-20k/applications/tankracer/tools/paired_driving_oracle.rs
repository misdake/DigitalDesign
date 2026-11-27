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
mod player_scene_assets;
mod prefix_scene_assets;
#[allow(dead_code)]
mod projectile_snapshot;
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
    peer: Option<driving_step::Step>,
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
            let own = call
                .context
                .entities
                .record(call.entity)
                .unwrap()
                .object
                .and_then(tankracer_game::scene_entity::Handle::vehicle)
                .unwrap()
                .0;
            assert!(own == 0 || self.setup.sample_controls == 9 && own == 1);
            let input = if own == 0 {
                self.input
            } else {
                self.world.vehicles.entries[own]
                    .as_ref()
                    .unwrap()
                    .input
                    .drive
            };
            let result = driving_step::step(
                self.world,
                self.setup,
                own,
                input,
                self.number,
                Some(&mut call.context),
            );
            if own == 0 {
                self.result = Some(result);
            } else {
                self.peer = Some(result);
            }
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
    world.objects.components.dt = setup.dt;
    // Complete Player/world passes publish frame context before the first actor.
    // Raw physics/pickup qualifiers retain their explicitly frozen context.
    if setup.sample_controls >= 3 {
        world.objects.components.frame_group = number;
        world.objects.components.camera_position = world.camera_position;
        world.objects.components.camera_basis = world.camera_basis;
    }
    let target = event.map(|id| world.objects.world.colliders[id].entity);
    let mut methods = PhysicalFrame {
        world,
        setup,
        input,
        number,
        result: None,
        peer: None,
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
    let mut result = methods.result.take().unwrap_or_else(|| {
        assert_eq!(setup.sample_controls, 7, "player must be scheduled");
        let car = methods.world.vehicles.entries[0].as_ref().unwrap();
        let mut control = reference::drive_fixture::frame(
            &drive::Input::default(),
            &drive::Environment {
                dt: setup.dt,
                player: true,
                active: car.race.driving,
                ..Default::default()
            },
        );
        control.extend([
            number as i32,
            256 | i32::from(car.visible),
            0,
            131072 | (4 << 9),
        ]);
        driving_step::Step {
            control,
            stage: reference::drive_fixture::flatten(
                &car.physics.powertrain.drive_state(&car.physics.motion),
            ),
            peer_stage: None,
            cells: Default::default(),
            tank_hits: 0,
            deform_hits: 0,
            sampled_points: 0,
            spark_events: 0,
            ordinary_hits: 0,
            scenery_ticks: 0,
        }
    });
    if setup.sample_controls == 9 {
        let peer = methods
            .peer
            .take()
            .expect("the authored peer Tank must be scheduled");
        result.cells.extend(peer.cells);
        result.tank_hits += peer.tank_hits;
        result.ordinary_hits += peer.ordinary_hits;
        result.deform_hits += peer.deform_hits;
        result.peer_stage = Some(peer.stage);
    }
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
fn flock_observer(world: &SinglePlayer, id: usize, terrain: &TerrainData) -> [i32; 3] {
    let p = world.objects.world.colliders[id].pose.position;
    for [dx, dz] in [[4096, -1000], [-4096, -1000], [1000, 4096], [-1000, -4096]] {
        let x = p[0] + dx;
        let z = p[2] + dz;
        let mut height = 0;
        if tankracer_game::terrain::height(terrain, x, z, &mut height).unwrap()
            && height != tankracer_game::terrain::MISS_HEIGHT
        {
            return [x, height, z];
        }
    }
    panic!("Flock {id} requires real terrain")
}
fn ambient_camera(world: &SinglePlayer, boundary: bool, frame: usize) -> [i32; 3] {
    let chimney = world.objects.world.colliders[1]
        .pose
        .position
        .map(|v| v >> 3);
    let Some(tankracer_game::actor_startup::Actor::Noisy { sounds, .. }) = world
        .objects
        .components
        .actors
        .get(tankracer_game::object_query::ObstacleId(0))
    else {
        unreachable!()
    };
    let mut sound = 0;
    if boundary {
        if frame >= 8 {
            let mut p = chimney;
            p[0] += if frame < 12 { 19200 } else { 19199 };
            return p;
        }
    } else {
        if (3..13).contains(&frame) || (17..41).contains(&frame) {
            return chimney;
        }
        if frame >= 3 && !(41..49).contains(&frame) {
            return [1000000, 0, 1000000];
        }
        if frame >= 41 {
            sound = 1;
        }
    }
    let mut camera = [
        sounds[sound].values[0] >> 3,
        0,
        sounds[sound].values[2] >> 3,
    ];
    if boundary {
        camera[0] += if frame < 4 { 10239 } else { 10240 };
    }
    camera
}
fn train_observer(world: &SinglePlayer, id: usize, terrain: &TerrainData) -> [i32; 3] {
    let Some(tankracer_game::actor_startup::Actor::Train(t)) = world
        .objects
        .components
        .actors
        .get(tankracer_game::object_query::ObstacleId(id))
    else {
        unreachable!()
    };
    let tankracer_game::actor_startup::train::Motion::Path {
        start, velocity, ..
    } = t.motion
    else {
        unreachable!()
    };
    for lead in [8, 4, 12, 0, 16] {
        for lateral in [0, 8192, -8192, 16384, -16384, 32768, -32768] {
            let x = start[0] + ((velocity[0] * 100 * lead) >> 8) + lateral;
            let z = start[2] + ((velocity[2] * 100 * lead) >> 8);
            let mut height = 0;
            if tankracer_game::terrain::height(terrain, x, z, &mut height).unwrap()
                && height != tankracer_game::terrain::MISS_HEIGHT
            {
                return [x, height, z];
            }
        }
    }
    panic!("Train {id} requires a real terrain observation point")
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
    let mut owners_written = false;
    let mut flock_initial_image: Option<Vec<u16>> = None;
    let mut deform_id_image: Option<Vec<u16>> = None;
    let mut player_deform_id_image: Option<Vec<u16>> = None;
    let mut object_mesh_image: Option<Vec<u16>> = None;
    let mut pickup_flight_image: Option<Vec<u16>> = None;
    let mut cells = std::collections::BTreeSet::new();
    let (mut asset_session, resources) =
        reference_world::constructed_pair(root, originals[1].model.clone())
            .into_session()
            .unwrap();
    let authored = asset_session.world_mut_at_load().unwrap();
    let (scene, _) = scene_export::encode(&authored.objects.world);
    fs::write(
        out.join("scene.bin"),
        scene
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
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
        (
            "complete-player",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-homing",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-warp",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-clock",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-world",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-world-lap",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-world-pickup",
            [0, 0],
            16000,
            Some(186),
            true,
            false,
            false,
        ),
        (
            "complete-player-world-startup",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-world-cold",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-horn",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-hidden",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-flock-183",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-flock-184",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-flock-185",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-lap-crush-217",
            [0, 0],
            1600,
            Some(217),
            true,
            false,
            false,
        ),
        (
            "complete-player-lap-crush-220",
            [0, 0],
            1600,
            Some(220),
            true,
            false,
            false,
        ),
        (
            "complete-player-lap-crush-230",
            [0, 0],
            1600,
            Some(230),
            true,
            false,
            false,
        ),
        (
            "complete-player-ambient-cycle",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-ambient-boundary",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-train-125",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-train-239",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-train-239-reversal",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-emitter-rain",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-emitter-gun",
            [0, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-ground",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-ground-warp",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-ground-concussion",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-hit-shell",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-hit-homing",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-hit-warp",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-hit-reflect",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-hit-body-player",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-hit-turret-player",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-dynamic",
            [300000, 0],
            1600,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-hit-ufo",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-ufo-transfer",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-ufo-object-bench",
            [0, 0],
            16000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-ufo-object-gate",
            [0, 0],
            16000,
            Some(172),
            true,
            false,
            false,
        ),
        (
            "complete-player-ufo-object-prop",
            [0, 0],
            16000,
            Some(71),
            true,
            false,
            false,
        ),
        (
            "complete-player-ufo-object-pickup",
            [0, 0],
            16000,
            Some(186),
            true,
            false,
            false,
        ),
        (
            "complete-player-mines-ordinary",
            [300000, 0],
            8000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-mines-electric",
            [300000, 0],
            8000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-mines-hit-ordinary",
            [300000, 0],
            6000,
            Some(69),
            true,
            false,
            false,
        ),
        (
            "complete-player-mines-hit-electric",
            [300000, 0],
            6000,
            Some(69),
            true,
            false,
            false,
        ),
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
        let constructed_world = reference_world::constructed_pair(root, originals[1].model.clone());
        if name == "complete-player-world-cold" {
            let camera = &constructed_world.camera;
            let mut values = Vec::new();
            values.extend(camera.pose.matrix.0);
            values.extend(camera.pose.position);
            values.extend(camera.angles.map(|v| v as i32));
            values.extend(camera.displacement);
            values.extend(camera.target_angles);
            values.push(camera.variant as i32);
            for row in &camera.resize {
                values.extend([
                    i32::from(row.rate),
                    i32::from(row.multiplier),
                    i32::from(row.shift),
                ]);
            }
            assert_eq!(values.len(), 28);
            fs::write(
                out.join("scene-boot-camera.bin"),
                values
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        let mut session = if name.starts_with("complete-player") {
            player_scene_assets::prepare(root, constructed_world).0
        } else {
            constructed_world.into_session().unwrap().0
        };
        let mut empty = reference_world::two_cars(root, 1);
        let world = if object.is_some() {
            session.world_mut_at_load().unwrap()
        } else {
            &mut empty
        };
        // Both handles were installed by the original vehicle factory, before
        // reserving the separate finish owner. Keep them with the authored pose.
        let peer_entity = world.vehicles.entries[1].as_ref().unwrap().physics.entity;
        let peer_class = world.vehicles.entries[1].as_ref().unwrap().physics.class;
        let entity = world.vehicles.entries[0].as_ref().unwrap().physics.entity;
        let world_pass = name.starts_with("complete-player-world");
        let world_pickup = name == "complete-player-world-pickup";
        let cold_scene = name == "complete-player-world-cold";
        let constructed =
            name == "complete-startup" || name == "complete-player-world-startup" || cold_scene;
        if world_pickup {
            world.camera_position = [250, 600, -800];
        }
        if world_pass && name.ends_with("-lap") {
            tankracer_game::scene_objects::rcc_lifecycle_probe::lap_step(
                world,
                1,
                false,
                [1000000, 0, 1000000],
                100000,
            )
            .unwrap();
        }
        let late_surface = name.starts_with("complete-player-lap-crush");
        if late_surface {
            let id = object.unwrap();
            let lap = world.objects.components.objects[id]
                .as_ref()
                .unwrap()
                .data
                .visibility
                .add_lap;
            assert!(lap > 0);
            for lap in 1..=lap {
                tankracer_game::scene_objects::rcc_lifecycle_probe::lap_step(
                    world,
                    lap,
                    false,
                    [1000000, 0, 1000000],
                    100000,
                )
                .unwrap();
            }
        }
        let flock_sequence = name.starts_with("complete-player-flock");
        let flock_id = if flock_sequence {
            name.rsplit('-').next().unwrap().parse::<usize>().unwrap()
        } else {
            183
        };
        if flock_sequence && flock_id == 185 {
            // The later-lap flock enters through its authored lap action.
            let lap = world.objects.components.objects[185]
                .as_ref()
                .unwrap()
                .data
                .visibility
                .add_lap;
            assert!(lap >= 0);
            tankracer_game::scene_objects::rcc_lifecycle_probe::lap_step(
                world,
                lap,
                false,
                [1000000, 0, 1000000],
                100000,
            )
            .unwrap();
        }
        let ambient_sequence = name.starts_with("complete-player-ambient");
        let train_sequence = name.starts_with("complete-player-train");
        let emitter_sequence = name.starts_with("complete-player-emitter");
        let mine_sequence = name.starts_with("complete-player-mines");
        let ufo_sequence = name.starts_with("complete-player-ufo");
        let object_ufo = name.starts_with("complete-player-ufo-object");
        let mine_contact = name.starts_with("complete-player-mines-hit");
        let race_spawn = world.vehicles.entries[0]
            .as_ref()
            .unwrap()
            .physics
            .motion
            .poses
            .physical_position;
        let segment = if ordinary
            && !mine_sequence
            && !ufo_sequence
            && !emitter_sequence
            && !train_sequence
            && !ambient_sequence
            && !flock_sequence
            && !late_surface
            && (!world_pass || world_pickup)
        {
            object.map(|index| {
                if name.starts_with("pickup-") || world_pickup {
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
        let start = if world_pickup {
            Some(segment.unwrap().0)
        } else if flock_sequence {
            Some(flock_observer(world, flock_id, &terrain))
        } else if ambient_sequence {
            Some(race_spawn)
        } else if train_sequence {
            Some(train_observer(
                world,
                if name.contains("239") { 239 } else { 125 },
                &terrain,
            ))
        } else if name == "complete-player-emitter-gun" {
            let Some(tankracer_game::actor_startup::Actor::Emitter(e)) = world
                .objects
                .components
                .actors
                .get(tankracer_game::object_query::ObstacleId(81))
            else {
                unreachable!()
            };
            Some([
                e.bounds[0] + ((e.bounds[2] - e.bounds[0]) >> 1),
                0,
                e.bounds[1] + ((e.bounds[3] - e.bounds[1]) >> 1),
            ])
        } else if emitter_sequence {
            let c = &world.objects.world.colliders[112];
            Some(c.pose.position)
        } else if mine_sequence || ufo_sequence || world_pass {
            Some(race_spawn)
        } else {
            object.map(|index| {
                if let Some((a, _)) = segment {
                    return a;
                }
                let c = &world.objects.world.colliders[index];
                let component = world.objects.components.surfaces.entries[index]
                    .as_ref()
                    .unwrap();
                let mesh = world
                    .objects
                    .components
                    .surfaces
                    .meshes
                    .get(component.mesh)
                    .unwrap();
                let mut vertex = mesh.vertices[0].map(|v| i32::from(v) * 8);
                if late_surface {
                    let mut top = i32::MIN;
                    for face in &mesh.triangles {
                        let [a, b, c] = face
                            .indices
                            .map(|v| mesh.vertices[v as usize].map(i32::from));
                        if (b[0] - a[0]) * (c[2] - a[2]) == (b[2] - a[2]) * (c[0] - a[0]) {
                            continue;
                        }
                        let y = (a[1] + b[1] + c[1]) * 8 / 3;
                        if y > top {
                            top = y;
                            vertex = [
                                (a[0] + b[0] + c[0]) * 8 / 3,
                                y,
                                (a[2] + b[2] + c[2]) * 8 / 3,
                            ];
                        }
                    }
                    assert_ne!(top, i32::MIN);
                }
                let mut point = c.pose.matrix.local_to_world(vertex);
                for i in 0..3 {
                    point[i] += c.pose.position[i];
                }
                point
            })
        };
        if object.is_some() && !name.starts_with("complete-player") {
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
            for id in deform_assets::ids(world) {
                let surface = world.objects.components.surfaces.entries[id.0]
                    .as_ref()
                    .unwrap();
                mesh_ids.push(surface.mesh.index() as u16);
            }
            let encoded = bytes(&mesh_ids);
            let path = out.join("deform-mesh-ids.bin");
            if let Some(previous) = &deform_id_image {
                assert_eq!(&mesh_ids, previous);
            } else {
                deform_id_image = Some(mesh_ids);
            }
            fs::write(path, encoded).unwrap();
        }
        let mut startup_models = Vec::new();
        for (i, original) in originals.iter().enumerate() {
            if cold_scene {
                let p = world.vehicles.entries[i].as_ref().unwrap();
                let v = &p.physics;
                let spawn = v.motion.poses.body;
                let environment = tankracer_game::vehicle_startup::Environment {
                    ambient_wobble: false,
                    session_active: false,
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
                let mut p = Player::construct(
                    v.entity,
                    v.class,
                    p.model.clone(),
                    spawn,
                    environment,
                    v.player,
                )
                .unwrap();
                p.slot = i as u8;
                world.vehicles.entries[i] = Some(p);
                continue;
            }
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
            if i == 1 {
                v.entity = peer_entity;
                v.class = peer_class;
            }
            // The extra local target covers the original player-only dent path.
            v.player = i == 0 || (i == 1 && name.ends_with("-player"));
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
            if late_surface {
                v.motion.velocity = [0, -10000, 0];
                v.motion.previous_position = v.motion.poses.physical_position;
            }
            if mine_sequence
                || ufo_sequence
                || emitter_sequence
                || train_sequence
                || ambient_sequence
                || flock_sequence
                || world_pass
            {
                v.motion.velocity = [0, 0, velocities[i]];
                if !world_pickup {
                    v.motion.previous_position = v.motion.poses.physical_position;
                }
            }
            if name.starts_with("pickup-") || name.starts_with("complete-player-hit") {
                v.motion.velocity = [0; 3];
            }
            v.pose_input.external = pose::ExternalBasis::Body;
            for j in 0..4 {
                v.contact.sampling.primary.points[j].local = p.model.primary_local[j];
                v.contact.response.points[j].corner = j as u32;
            }
            if constructed {
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
            let bytes = startup_models
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            fs::write(out.join(format!("startup-models-{name}.bin")), &bytes).unwrap();
            if name == "complete-startup" {
                fs::write(out.join("startup-models.bin"), &bytes).unwrap();
            }
        }
        world.vehicles.racers = [None; 9];
        world.vehicles.racers[0] = Some(tankracer_game::object_query::TankId(0));
        world.vehicles.racers[1] = Some(tankracer_game::object_query::TankId(1));
        if object.is_some() && name != "debris-peer-blast" && !world_pass {
            world.vehicles.racers[1] = None;
        }
        if name.starts_with("complete-player-hit") {
            world.vehicles.racers[1] = Some(tankracer_game::object_query::TankId(1));
            world.vehicles.entries[1].as_mut().unwrap().hit_counts = [[6; 6]; 2];
            if name.ends_with("-reflect") {
                world.vehicles.entries[1]
                    .as_mut()
                    .unwrap()
                    .prefix
                    .spin
                    .speed = 1;
            }
        }
        // An independent initial RNG state selects the other real endpoint branch.
        let seed_before_noise = if name.ends_with("-reversal") || cold_scene {
            0
        } else {
            0x1234abcd
        };
        if !cold_scene {
            world.shared.random = seed_before_noise;
            world.shared.noise = ground::initialize_noise(&mut world.shared.random);
        }
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
            if !world_pass {
                for flags in &mut world.identity.ticks.flags {
                    *flags &= !16;
                }
                let authored = world.identity.ticks.order.clone();
                world.identity.ticks.order.clear();
                if emitter_sequence || train_sequence || ambient_sequence || flock_sequence {
                    for current in authored {
                        if current == entity
                            || projectile_snapshot::EMITTER_IDS
                                .iter()
                                .any(|&id| world.objects.world.colliders[id].entity == current)
                            || ((ambient_sequence || flock_sequence)
                                && world.objects.world.colliders[..2]
                                    .iter()
                                    .any(|c| c.entity == current))
                            || (flock_sequence
                                && projectile_snapshot::FLOCK_IDS
                                    .iter()
                                    .any(|&id| world.objects.world.colliders[id].entity == current))
                            || ((train_sequence || ambient_sequence || flock_sequence)
                                && projectile_snapshot::TRAIN_IDS
                                    .iter()
                                    .any(|&id| world.objects.world.colliders[id].entity == current))
                        {
                            world.identity.ticks.enable(current).unwrap();
                        }
                    }
                } else {
                    world.identity.ticks.enable(entity).unwrap();
                }
                if let Some(entity) = retained {
                    world.identity.ticks.enable(entity).unwrap();
                }
                world.identity.ticks.next = None;
            }
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
                if owners_written {
                    assert_eq!(
                        fs::read(out.join(filename)).unwrap(),
                        bytes,
                        "shared owner fixture: {filename}"
                    );
                }
                fs::write(out.join(filename), bytes).unwrap();
            }
            owners_written = true;
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
        let player_sequence = name.starts_with("complete-player");
        let flight_sequence = matches!(
            name,
            "complete-player" | "complete-player-homing" | "complete-player-warp"
        ) || name.starts_with("complete-player-ground")
            || name.starts_with("complete-player-hit")
            || mine_sequence
            || ufo_sequence
            || emitter_sequence
            || train_sequence
            || ambient_sequence
            || flock_sequence
            || late_surface;
        let flight_sequence = flight_sequence || world_pass;
        let clock_sequence = name == "complete-player-clock" || ufo_sequence || world_pass;
        let mut race_clock = tankracer_game::race_clock::Clock::default();
        if ufo_sequence {
            race_clock.countdown = 300;
        }
        if player_sequence {
            let car = world.vehicles.entries[0].as_mut().unwrap();
            let vertex = car.model.gun_vertices[0] as usize;
            let gun = car.model.parts[Part::Gun.slot()].as_mut().unwrap();
            if name.ends_with("-homing") {
                gun.rcc_set_revision(65535);
            } else if name.ends_with("-warp") {
                gun.rcc_set_revision(u64::from(u32::MAX));
            } else if name.ends_with("-horn") {
                gun.edit(|geometry| {
                    geometry.vertices[vertex][2] = geometry.vertices[vertex][2].wrapping_add(1)
                });
                gun.rcc_set_revision(u64::MAX);
                car.horn_down = true;
                car.horn_latched = true;
                car.horn_timer = 34;
            }
            if name.starts_with("complete-player-ground") {
                let car = world.vehicles.entries[0].as_mut().unwrap();
                let poses = &mut car.physics.motion.poses;
                poses.turret.matrix = Matrix::from_angles([49152, 0, 0]);
                poses.gun.matrix = poses.turret.matrix;
                let mut height = 0;
                tankracer_game::terrain::height(
                    &terrain,
                    poses.gun.position[0],
                    poses.gun.position[2],
                    &mut height,
                )
                .unwrap();
                assert_ne!(height, tankracer_game::terrain::MISS_HEIGHT);
                poses.gun.position[1] = height + 512;
            }
            if name == "complete-player-hit-turret-player" {
                world.vehicles.entries[0]
                    .as_mut()
                    .unwrap()
                    .physics
                    .motion
                    .poses
                    .gun
                    .position[1] += 1536;
            }
            let inventory = &mut world.vehicles.entries[0].as_mut().unwrap().inventory;
            if ufo_sequence
                || emitter_sequence
                || train_sequence
                || ambient_sequence
                || flock_sequence
                || late_surface
            {
                inventory.weapon = 0;
                inventory.ammo = 0;
            } else if mine_sequence {
                inventory.weapon = if name.ends_with("-electric") { 2 } else { 1 };
                inventory.ammo = if mine_contact { 0 } else { 2 };
                if mine_contact {
                    inventory.weapon = 0;
                }
            } else if name == "complete-player-hit-ufo" {
                inventory.weapon = 4;
                inventory.ammo = 2;
            } else if name.ends_with("-homing") {
                inventory.weapon = 3;
                inventory.ammo = 2;
            } else if name.ends_with("-warp") {
                inventory.weapon = 10;
                inventory.ammo = 2;
            } else if name.ends_with("-concussion") {
                inventory.weapon = 5;
                inventory.ammo = 2;
            }
            let (scene, _) = scene_export::encode(&world.objects.world);
            fs::write(
                out.join(format!("player-scene-{name}.bin")),
                scene
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            world.race_active = true;
            world.objects.components.stamp_units = 100;
            world.vehicles.entries[0].as_mut().unwrap().race.driving = !clock_sequence;
            tankracer_game::vehicle_runtime::rcc_player_probe::initialize_routes(world).unwrap();
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
        let mut saw_ground_impact = false;
        let mut saw_reflection = false;
        let mut saw_mine_explosion = false;
        let mut saw_third_mine = false;
        let train_positions = if train_sequence {
            projectile_snapshot::TRAIN_IDS.map(|id| world.objects.world.colliders[id].pose.position)
        } else {
            [[0; 3]; 2]
        };
        let mut flock_initial_vertices = Vec::new();
        if flock_sequence {
            for id in projectile_snapshot::FLOCK_IDS {
                let Some(tankracer_game::actor_startup::Actor::Flock(f)) = world
                    .objects
                    .components
                    .actors
                    .get(tankracer_game::object_query::ObstacleId(id))
                else {
                    unreachable!()
                };
                for model in &f.models {
                    flock_initial_vertices.push(model.geometry.vertices.clone());
                }
            }
        }
        let mut flock_coverage = [[false; 5]; 3];
        let object_ufo_origin = object.map(|id| world.objects.world.colliders[id].pose.position);
        let mut object_ufo_coverage = [false; 4];
        let mut ambient_selected = std::collections::BTreeSet::new();
        let mut ambient_visible = Vec::new();
        let mut ambient_previous = Vec::new();
        let mut ambient_previous_sequence = 0u8;
        let mut ambient_frozen = 0usize;
        let mut saw_ambient_particle = false;
        let mut saw_ambient_reuse = false;
        let mut saw_train_running = false;
        let mut saw_train_motion = false;
        let mut saw_train_arrival = false;
        let mut saw_train_reversal = false;
        let mut saw_train_reset = false;
        let mut saw_train_wait_end = false;
        let mut was_train_waiting = [false; 2];
        let mut saw_emitter_shot = false;
        let mut saw_emitter_explosion = false;
        let mut saw_emitter_retirement = false;
        let mut previous_emitter_shots = std::collections::BTreeSet::new();
        let before_mesh = deform_assets::capture(world);
        let baseline = (0..world.objects.world.objects.len())
            .map(|i| scene_export::record(&world.objects.world, i))
            .collect::<Vec<_>>();
        if cold_scene {
            fs::write(
                out.join("scene-boot-expected.bin"),
                scenery_frame::capture(world, &baseline)
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            fs::write(
                out.join("scene-boot-debris.bin"),
                debris_scene_assets::capture(world)
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
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
        let player_initial = if player_sequence {
            player_scene_assets::capture(world)
        } else {
            Vec::new()
        };
        if player_sequence {
            let flights = pickup_scene_assets::flights(world)
                .iter()
                .flat_map(|&v| [v as u16, (v >> 16) as u16])
                .collect::<Vec<_>>();
            if let Some(previous) = &pickup_flight_image {
                assert_eq!(&flights, previous, "shared initial pickup flights: {name}");
            } else {
                fs::write(
                    out.join("player-pickup-flights.bin"),
                    flights
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                pickup_flight_image = Some(flights);
            }
            let meshes = world
                .objects
                .components
                .objects
                .iter()
                .map(|entry| {
                    entry
                        .as_ref()
                        .and_then(|o| o.data.mesh)
                        .map_or(65535, |m| u16::try_from(m.index()).unwrap())
                })
                .collect::<Vec<_>>();
            if let Some(previous) = &object_mesh_image {
                assert_eq!(&meshes, previous, "shared authored object meshes: {name}");
            } else {
                fs::write(
                    out.join("object-meshes.bin"),
                    meshes
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                object_mesh_image = Some(meshes);
            }
            let mut ids = Vec::new();
            for id in deform_assets::ids(world) {
                ids.push(
                    world.objects.components.surfaces.entries[id.0]
                        .as_ref()
                        .unwrap()
                        .mesh
                        .index() as u16,
                );
            }
            if let Some(previous) = &player_deform_id_image {
                assert_eq!(&ids, previous, "shared Player surface owners: {name}");
            } else {
                fs::write(
                    out.join("player-deform-mesh-ids.bin"),
                    ids.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>(),
                )
                .unwrap();
                player_deform_id_image = Some(ids);
            }
            let image = player_scene_assets::flock_initial(world);
            if let Some(previous) = &flock_initial_image {
                assert_eq!(&image, previous, "shared initial flock owners: {name}");
            } else {
                fs::write(
                    out.join("flock-initial.bin"),
                    image
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                flock_initial_image = Some(image);
            }
            fs::write(
                out.join(format!("player-context-{name}.bin")),
                player_scene_assets::context(world)
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        let dynamic = name == "complete-dynamic";
        let mut dynamic_entities: Vec<tankracer_game::object_query::EntityId> = Vec::new();
        let clock_initial = if clock_sequence {
            player_scene_assets::clock(&race_clock)
        } else {
            Vec::new()
        };
        let mut setup = driving_step::Setup {
            complete: name.starts_with("complete-"),
            sample_controls: if world_pass {
                9
            } else if (train_sequence && name.ends_with("125"))
                || (ambient_sequence && name.ends_with("-cycle"))
            {
                8
            } else if ufo_sequence
                || emitter_sequence
                || train_sequence
                || ambient_sequence
                || flock_sequence
                || late_surface
            {
                7
            } else if mine_contact {
                6
            } else if name.ends_with("-horn") {
                5
            } else if clock_sequence {
                4
            } else if player_sequence {
                3
            } else if name == "complete-controls" {
                1
            } else if name == "complete-fast-controls" {
                2
            } else {
                0
            },
            dt: 33,
            start_stage: 0,
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
                || dynamic
                || player_sequence
            {
                &models
            } else {
                &[]
            },
        };
        let case_steps = if world_pass {
            steps.clamp(if constructed { 4 } else { 8 }, 32)
        } else if object_ufo {
            steps.clamp(3, 80)
        } else if flock_sequence {
            steps.clamp(if flock_id == 183 { 8 } else { 3 }, 64)
        } else if ambient_sequence {
            if name.ends_with("-boundary") {
                steps.clamp(8, 16)
            } else {
                steps.clamp(8, 56)
            }
        } else if train_sequence {
            if name.ends_with("-reversal") {
                steps.clamp(10, 82)
            } else if name.contains("239") {
                steps.clamp(10, 32)
            } else {
                steps.clamp(8, 56)
            }
        } else if emitter_sequence {
            if name.ends_with("-rain") {
                steps.clamp(4, 48)
            } else {
                steps.clamp(10, 32)
            }
        } else if ufo_sequence {
            48
        } else if clock_sequence {
            5
        } else if mine_sequence && !mine_contact {
            18
        } else if name.starts_with("complete-player-hit") || mine_sequence {
            10
        } else if name.starts_with("complete-player-ground") {
            8
        } else if dynamic || player_sequence {
            3
        } else if name == "complete-fast-controls" {
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
            let raw_dt = if world_pass {
                if frame < 3 { 1000 } else { 33 }
            } else if ufo_sequence
                || emitter_sequence
                || train_sequence
                || ambient_sequence
                || flock_sequence
            {
                100
            } else if clock_sequence {
                [1000, 1000, 1000, 250, 5][frame]
            } else {
                33
            };
            if clock_sequence {
                let running = race_clock.countdown == 0;
                let launch = race_clock.tick_time(raw_dt);
                for slot in world.vehicles.racers {
                    let Some(id) = slot else { continue };
                    let racer = &mut world.vehicles.entries[id.0].as_mut().unwrap().race;
                    if running {
                        racer.elapsed = racer.elapsed.wrapping_add(raw_dt);
                    } else if launch {
                        racer.driving = true;
                    }
                }
                setup.dt = tankracer_game::race_clock::physics_dt(raw_dt);
                setup.start_stage = race_clock.display;
                world.race_active = race_clock.countdown == 0;
            }
            if ufo_sequence
                || emitter_sequence
                || train_sequence
                || ambient_sequence
                || flock_sequence
            {
                setup.dt = raw_dt;
            }
            if train_sequence || ambient_sequence || flock_sequence || late_surface {
                setup.start_stage = 4;
            }
            if flock_sequence {
                world.camera_position = world.vehicles.entries[0]
                    .as_ref()
                    .unwrap()
                    .physics
                    .motion
                    .poses
                    .body
                    .position
                    .map(|v| v >> 3);
            }
            if ambient_sequence {
                world.camera_position = ambient_camera(world, name.ends_with("-boundary"), frame);
            }
            if emitter_sequence {
                setup.start_stage = 4;
                let id = if name.ends_with("-gun") { 81 } else { 112 };
                world.camera_position = world.objects.world.colliders[id]
                    .pose
                    .position
                    .map(|v| v >> 3);
                if name.ends_with("-rain") && (12..15).contains(&frame) {
                    world.camera_position[0] += 100000;
                }
            }
            for own in 0..actors {
                let mut world_event = [0u16; 4];
                if ufo_sequence && frame == 0 {
                    let (target, destination) = if object_ufo {
                        let collider = &world.objects.world.colliders[object.unwrap()];
                        let mut destination = collider.pose.position;
                        destination[0] += 1000;
                        (collider.entity, destination)
                    } else {
                        (entity, [0; 3])
                    };
                    tankracer_game::scene_objects::rcc_lifecycle_probe::spawn_ufo(
                        world,
                        None,
                        target,
                        destination,
                        100000,
                        true,
                    )
                    .unwrap();
                    world_event = if object_ufo {
                        [6, 1, object.unwrap() as u16, 1000]
                    } else {
                        [5, 1, 0, 0]
                    };
                } else if mine_sequence && !mine_contact && frame == 17 {
                    let id = world.objects.components.mines.order[0];
                    let entity = world.objects.world.colliders[id.0].entity;
                    tankracer_game::scene_objects::rcc_lifecycle_probe::dispatch_weapon(
                        world, entity, 8, 100000,
                    )
                    .unwrap();
                    world_event = [4, 1, entity.0 as u16, 0];
                } else if mine_contact && frame == 0 {
                    let peer = &world.vehicles.entries[1].as_ref().unwrap().physics;
                    let owner = tankracer_game::mine::Owner {
                        entity: peer.entity,
                        position: peer.motion.poses.physical_position,
                    };
                    let kind = u32::from(name.ends_with("-electric"));
                    world.objects.components.frame_group = number;
                    tankracer_game::scene_objects::rcc_lifecycle_probe::spawn_mine(
                        world, owner, false, kind, 100000,
                    )
                    .unwrap();
                    world_event = [3, 1, 1, kind as u16];
                } else if dynamic {
                    if frame == 1 {
                        let first = dynamic_entities[198];
                        world_event = [2, 2, first.0 as u16, 0];
                        for index in 198..200 {
                            tankracer_game::scene_objects::rcc_lifecycle_probe::release_transient(
                                world,
                                dynamic_entities[index],
                            )
                            .unwrap();
                        }
                    } else {
                        let count = if frame == 0 { 200 } else { 2 };
                        let class = world.registry.generic;
                        world_event = [1, count, 0, class.0 as u16];
                        for _ in 0..count {
                            let entity = world.identity.entities.create_record(100000).unwrap();
                            world.identity.entities.record_mut(entity).unwrap().class = Some(class);
                            tankracer_game::scene_objects::rcc_lifecycle_probe::allocate_transient(
                                world, entity,
                            )
                            .unwrap();
                            dynamic_entities.push(entity);
                        }
                    }
                }
                if name == "complete-visibility" {
                    world.vehicles.entries[own].as_mut().unwrap().visible =
                        !((frame == 1 && own == 0) || (frame == 2 && own == 1));
                }
                if name == "complete-player-hidden" {
                    world.vehicles.entries[own].as_mut().unwrap().visible = frame != 1;
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
                let result = if ordinary && (!player_sequence || flight_sequence) {
                    ordered_frame(
                        world,
                        &setup,
                        input,
                        number,
                        if retire && frame == 1 {
                            object
                        } else if name == "complete-player-warp" && frame == 2 {
                            Some(baseline.len())
                        } else if name == "complete-player-ground-concussion" && frame == 6 {
                            Some(baseline.len() + 1)
                        } else {
                            None
                        },
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
                    && !player_sequence
                {
                    assert_eq!(result.scenery_ticks, 0);
                }
                if ordinary
                    && !name.starts_with("auxiliary-")
                    && !name.starts_with("debris-")
                    && !name.starts_with("prefix-")
                    && !name.starts_with("pickup-")
                    && frame == 1
                    && !dynamic
                    && !player_sequence
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
                let mut control = result.control;
                if clock_sequence {
                    control[0] = raw_dt;
                    control[29] &= !(15 << 9);
                }
                stages[own] = result.stage;
                if let Some(peer) = result.peer_stage {
                    stages[1] = peer;
                }
                visited.extend(result.cells);
                hits += result.tank_hits;
                deform_hits += result.deform_hits;
                sampled_points += result.sampled_points;
                spark_events += result.spark_events;
                ordinary_hits += result.ordinary_hits;
                scenery_ticks += result.scenery_ticks;
                if object_ufo {
                    let id = tankracer_game::object_query::ObstacleId(object.unwrap());
                    let component = world.objects.components.ufos.iter().flatten().next();
                    object_ufo_coverage[0] |= component.is_some_and(|u| u.flags & 2 != 0)
                        && !world.objects.components.special.contains(&id);
                    object_ufo_coverage[1] |=
                        object_ufo_coverage[0] && world.objects.components.ordinary.contains(&id);
                    object_ufo_coverage[2] |= world.objects.world.colliders[id.0].pose.position
                        != object_ufo_origin.unwrap();
                    object_ufo_coverage[3] |=
                        frame > 0 && world.objects.components.ufos.iter().all(Option::is_none);
                }
                if mine_sequence {
                    saw_mine_explosion |= world
                        .objects
                        .components
                        .explosions
                        .iter()
                        .any(Option::is_some);
                    saw_third_mine |= world.objects.components.mines.entries().len() >= 3;
                }
                if name.starts_with("complete-player-ground") {
                    saw_ground_impact |= world
                        .objects
                        .components
                        .explosions
                        .iter()
                        .any(Option::is_some);
                }
                if name.ends_with("-reflect") {
                    saw_reflection |=
                        (baseline.len()..world.objects.world.objects.len()).any(|id| {
                            world
                                .objects
                                .components
                                .projectiles
                                .get(tankracer_game::object_query::ObstacleId(id))
                                .is_some_and(|shot| shot.owner == Some(peer_entity))
                        });
                }
                if train_sequence {
                    for (i, id) in projectile_snapshot::TRAIN_IDS.into_iter().enumerate() {
                        let Some(tankracer_game::actor_startup::Actor::Train(t)) = world
                            .objects
                            .components
                            .actors
                            .get(tankracer_game::object_query::ObstacleId(id))
                        else {
                            unreachable!()
                        };
                        let tankracer_game::actor_startup::train::Motion::Path { start, .. } =
                            t.motion
                        else {
                            unreachable!()
                        };
                        let position = world.objects.world.colliders[id].pose.position;
                        saw_train_running |= t.travel.running;
                        saw_train_motion |= position != train_positions[i];
                        saw_train_arrival |= t.travel.waiting;
                        saw_train_wait_end |= was_train_waiting[i] && !t.travel.waiting;
                        was_train_waiting[i] = t.travel.waiting;
                        saw_train_reversal |= start != train_positions[i];
                        saw_train_reset |= t.travel.waiting && position == train_positions[i];
                    }
                }
                if flock_sequence {
                    for (i, id) in projectile_snapshot::FLOCK_IDS.into_iter().enumerate() {
                        let Some(tankracer_game::actor_startup::Actor::Flock(f)) = world
                            .objects
                            .components
                            .actors
                            .get(tankracer_game::object_query::ObstacleId(id))
                        else {
                            unreachable!()
                        };
                        let c = &mut flock_coverage[i];
                        c[0] |= f.flight.phase == 1;
                        c[1] |= f.flight.velocities.iter().any(|&v| v != [0; 3]);
                        c[2] |= f.flight.stage == 1;
                        c[3] |= f.flight.phase == 1
                            && world.identity.ticks.flags
                                [world.objects.world.colliders[id].entity.0 as usize]
                                & 16
                                == 0;
                        c[4] |= f.models[0].geometry.vertices != flock_initial_vertices[i * 2]
                            || f.models[1].geometry.vertices != flock_initial_vertices[i * 2 + 1];
                    }
                }
                if ambient_sequence {
                    let Some(tankracer_game::actor_startup::Actor::Noisy { selected, .. }) = world
                        .objects
                        .components
                        .actors
                        .get(tankracer_game::object_query::ObstacleId(0))
                    else {
                        unreachable!()
                    };
                    ambient_selected.insert(*selected);
                    let Some(tankracer_game::actor_startup::Actor::Smoker(s)) = world
                        .objects
                        .components
                        .actors
                        .get(tankracer_game::object_query::ObstacleId(1))
                    else {
                        unreachable!()
                    };
                    let sequence = world.objects.components.actors.smoke_sequence.0;
                    let mut current = Vec::new();
                    projectile_snapshot::smoker(&mut current, s);
                    ambient_visible.push(s.visible);
                    if !ambient_previous.is_empty() {
                        if !s.visible {
                            for j in 0..426 {
                                if j != 6 && j != 7 {
                                    assert_eq!(
                                        current[j], ambient_previous[j],
                                        "frozen smoke: {frame}/{j}"
                                    );
                                }
                            }
                            assert_eq!(sequence, ambient_previous_sequence);
                            ambient_frozen += 1;
                        } else {
                            for i in 0..16 {
                                let row = 10 + i * 26;
                                saw_ambient_reuse |= current[row + 20] != 0
                                    && ambient_previous[row + 20] != 0
                                    && current[row + 24] > ambient_previous[row + 24];
                            }
                        }
                    }
                    saw_ambient_particle |= s.particles.iter().any(|p| p.active != 0);
                    ambient_previous = current;
                    ambient_previous_sequence = sequence;
                }
                if emitter_sequence || ambient_sequence || flock_sequence {
                    saw_emitter_shot |=
                        world
                            .objects
                            .components
                            .projectiles
                            .entries()
                            .any(|(id, _)| {
                                world.objects.components.objects[id.0]
                                    .as_ref()
                                    .is_some_and(|object| object.emitter.is_some())
                            });
                    saw_emitter_explosion |= world
                        .objects
                        .components
                        .explosions
                        .iter()
                        .any(Option::is_some);
                    let mut current = std::collections::BTreeSet::new();
                    for (id, _) in world.objects.components.projectiles.entries() {
                        current.insert(id.0);
                        let position = world.objects.world.colliders[id.0].pose.position;
                        let x = ((position[0] >> 3) - terrain.origin[0]) >> terrain.shifts[0];
                        let z = ((position[2] >> 3) - terrain.origin[1]) >> terrain.shifts[1];
                        if (0..8).contains(&x) && (0..8).contains(&z) {
                            visited.insert((x + z * 8) as usize);
                        }
                    }
                    saw_emitter_retirement |= previous_emitter_shots
                        .iter()
                        .any(|id| !current.contains(id));
                    previous_emitter_shots = current;
                }
                let mut expected = snapshot(&world, &stages);
                expected.push(world.shared.random as i32);
                let surface = if object.is_some() {
                    deform_assets::capture(world)
                } else {
                    Vec::new()
                };
                let mut resource_versions = Vec::new();
                if object.is_some() {
                    for id in deform_assets::ids(world) {
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
                    json!({"camera":world.camera_position,"camera_basis":world.camera_basis.0,"control":control,"world_event":world_event,"clock":if clock_sequence {player_scene_assets::clock(&race_clock)}else{Vec::new()},"player":if player_sequence {player_scene_assets::capture(world)}else{Vec::new()},"expected":expected,"surface":surface,"scenery":live,"resource_versions":resource_versions,
                        "complete":if setup.complete {complete_scene_assets::capture(world)}else{Vec::new()},
                        "pickup_flights": if world_pass {pickup_scene_assets::flights(world)} else {Vec::new()},"prefix": if setup.prepare {prefix_scene_assets::encode(world)}else{Vec::new()},
                        "pickup":if name.starts_with("pickup-"){pickup_scene_assets::capture(world)}else{Vec::new()},
                        "debris": if name.starts_with("debris-") {debris_scene_assets::capture(world)} else {Vec::new()},
                        "parents": if name.starts_with("auxiliary-") || name.starts_with("debris-") || name.starts_with("prefix-") || name.starts_with("pickup-") || name=="complete-wall" || name=="complete-prepare" || dynamic || player_sequence { parent_scene_assets::capture(world, &bindings) } else { Vec::new() }}),
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
        if ufo_sequence && !object_ufo {
            let base = player_initial[0] as usize;
            let hidden = frames
                .iter()
                .position(|f| f["player"][base - 1988 - 85].as_u64() == Some(1))
                .expect("UFO must hide the player");
            let shown = frames
                .iter()
                .enumerate()
                .skip(hidden + 1)
                .find(|(_, f)| f["player"][base - 1988 - 85].as_u64() == Some(2))
                .map(|(i, _)| i)
                .expect("UFO must show the player");
            assert!(hidden < shown);
            assert!(
                world.objects.components.ufos.iter().all(Option::is_none),
                "UFO must retire through the shared drain"
            );
            assert!(world.objects.components.ufo_occupied.is_empty());
            let car = world.vehicles.entries[0].as_ref().unwrap();
            assert_eq!(car.prefix.spin.disabled, 0);
            assert!(!car.physics.collision.disabled);
            assert!(car.race.driving);
            assert_eq!(
                car.race.elapsed, 4500,
                "race time continues while Player is hidden"
            );
            assert!(
                world
                    .vehicles
                    .visible
                    .contains(&tankracer_game::object_query::TankId(0))
            );
        }
        if late_surface {
            assert!(deform_hits > 0, "later-lap Crush contact required: {name}");
        }
        if object_ufo && case_steps >= 80 {
            assert!(
                object_ufo_coverage.iter().all(|v| *v),
                "complete object UFO transfer required: {name}/{object_ufo_coverage:?}"
            );
        }
        if flock_sequence {
            let c = flock_coverage[flock_id - 183];
            assert!(
                c[0] && (case_steps < 9 || c[4]),
                "flock takeoff and live wings required: {name}/{flock_coverage:?}"
            );
            if case_steps >= 64 {
                assert!(
                    c.iter().all(|v| *v),
                    "full flock lifecycle required: {name}/{c:?}"
                );
            }
        }
        if ambient_sequence {
            assert!(
                ambient_selected.contains(&0),
                "original sound region must be selected"
            );
            if name.ends_with("-cycle") {
                if case_steps >= 56 {
                    assert!(
                        ambient_selected.contains(&1)
                            && ambient_selected.contains(&-1)
                            && saw_ambient_reuse
                            && ambient_frozen > 0
                    );
                }
                assert!(saw_ambient_particle);
            } else if case_steps >= 16 {
                assert!(ambient_visible[..12].iter().all(|v| !*v));
                assert!(ambient_visible[12..].iter().all(|v| *v));
            }
        }
        if train_sequence {
            assert!(
                saw_train_running && saw_train_motion,
                "Train must activate and move: {name}"
            );
            if case_steps >= if name.contains("239") { 32 } else { 56 } {
                assert!(
                    saw_train_arrival,
                    "Train must reach its authored endpoint: {name}"
                );
                if name.ends_with("-reversal") {
                    assert!(saw_train_reversal);
                } else if !name.ends_with("125") {
                    assert!(saw_train_reset);
                }
            }
            if name.ends_with("-reversal") && case_steps >= 82 {
                assert!(saw_train_wait_end);
            }
        }
        if emitter_sequence {
            assert!(
                saw_emitter_shot,
                "authored emitter must publish a live projectile: {name}"
            );
            if case_steps >= if name.ends_with("-rain") { 38 } else { 29 } {
                assert!(
                    saw_emitter_explosion,
                    "authored emission must reach a real collision: {name}"
                );
            }
            if name.ends_with("-rain") && case_steps >= 48 {
                assert!(
                    saw_emitter_retirement,
                    "impacted rain projectile must retire through the shared drain"
                );
            }
        }
        let after_mesh = deform_assets::capture(world);
        let mut changed_vertices = 0;
        let mut cursor = 0;
        for id in deform_assets::ids(world) {
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
                || name.starts_with("complete-player-hit")
                || mine_sequence
                || ufo_sequence
                || emitter_sequence
                || train_sequence
                || ambient_sequence
                || flock_sequence
                || late_surface
                || world_pass
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
        if world_pickup {
            assert!(
                ordinary_hits > 0,
                "world pickup must collide through the real car update"
            );
            assert!(
                frames
                    .iter()
                    .any(|f| f["player"].as_array().unwrap()[62].as_i64() == Some(1)),
                "world pickup must publish its mine inventory before firing"
            );
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
        if mine_sequence {
            if mine_contact {
                assert!(
                    saw_mine_explosion,
                    "mine contact must reach its explosion: {name}; car={:?}; mines={:?}",
                    frames
                        .iter()
                        .map(|f| f["expected"].as_array().unwrap()[89..101].to_vec())
                        .collect::<Vec<_>>(),
                    world
                        .objects
                        .components
                        .mines
                        .entries()
                        .iter()
                        .map(|(id, s)| (
                            id,
                            s,
                            world.objects.world.colliders[id.0].pose,
                            world.objects.world.colliders[id.0].bounds
                        ))
                        .collect::<Vec<_>>()
                );
                assert!(
                    world.objects.components.mines.order.is_empty(),
                    "contact must retire its mine: {name}"
                );
                if name.ends_with("-electric") {
                    assert!(
                        world.vehicles.entries[0]
                            .as_ref()
                            .unwrap()
                            .physics
                            .contact
                            .sampling
                            .effect
                            .timer
                            > 0
                    );
                }
            } else {
                assert!(saw_third_mine, "mine must finish both repeat drops: {name}");
                assert_eq!(world.objects.components.mines.entries().len(), 2);
                assert_eq!(world.objects.components.mines.count, 2);
                assert!(
                    world
                        .objects
                        .components
                        .mines
                        .entries()
                        .iter()
                        .all(|(_, state)| !state.repeat)
                );
            }
        }
        if name == "complete-player-hit-reflect" {
            assert!(
                saw_reflection,
                "projectile must transfer to its reflecting owner"
            );
            assert_eq!(
                world.vehicles.entries[1].as_ref().unwrap().hit_counts,
                [[6; 6]; 2]
            );
        } else if name == "complete-player-hit-ufo" {
            assert!(
                world
                    .objects
                    .components
                    .projectiles
                    .entries()
                    .any(|(_, s)| s.attached_ufo.is_some() && s.flags & 4 != 0),
                "UFO projectile must attach"
            );
            assert!(
                world
                    .objects
                    .components
                    .ufos
                    .iter()
                    .flatten()
                    .any(|s| s.target == peer_entity),
                "UFO must resolve the live peer"
            );
        } else if name.starts_with("complete-player-hit") {
            let target = world.vehicles.entries[1].as_ref().unwrap();
            assert!(
                target.hit_counts.into_iter().flatten().any(|v| v > 6),
                "live projectile did not hit: {name}"
            );
            assert!(
                target.smoke.damage.into_iter().any(|v| v > 6),
                "hit did not publish damage smoke: {name}"
            );
            if name.ends_with("-player") {
                let part = if name.contains("-turret-") {
                    Part::Turret
                } else {
                    Part::Body
                };
                let flags = if part == Part::Turret {
                    target.damage.turret
                } else {
                    target.damage.body
                };
                assert!(
                    flags.into_iter().any(|v| v != 0),
                    "player geometry was not dented: {name}"
                );
                assert!(target.model.parts[part.slot()].as_ref().unwrap().revision() > 1);
            }
        }
        if name.starts_with("complete-player-ground") {
            assert!(saw_ground_impact, "ground impact missing: {name}");
            if name == "complete-player-ground-concussion" {
                assert!(
                    world
                        .objects
                        .components
                        .explosions
                        .iter()
                        .all(Option::is_none)
                );
                assert_eq!(world.objects.components.explosion_world.count, 0);
            } else {
                assert!(
                    world
                        .objects
                        .components
                        .explosions
                        .iter()
                        .any(Option::is_some),
                    "ground impact missing: {name}"
                );
            }
            assert!(
                world.objects.components.projectiles.marks.count > 0,
                "ground mark missing: {name}"
            );
        }
        cells.extend(visited.iter().copied());
        let attitude_orthogonal =
            setup.complete && world.numerics() == attitude::Numerics::OrthogonalV1;
        let mut noise_stage_seed = seed_before_noise;
        if cold_scene {
            ground::initialize_noise(&mut noise_stage_seed);
        } else {
            noise_stage_seed = initial_seed;
        }
        let mut exported_case = json!({"name":name,"constructed":constructed,"world_pass":world_pass,"attitude_orthogonal":attitude_orthogonal,"clock_initial":clock_initial,"player_initial":player_initial,"ordered":ordinary && (!player_sequence || flight_sequence),"complete_initial":complete_initial,"cars":actors,"car_bindings":car_bindings,"prefix_initial":prefix_initial,"pickup_initial":pickup_initial,"racer_slots":racer_slots,"entities":entities,"initial":initial,"seed_before_noise":seed_before_noise,"seed_after_noise":noise_stage_seed,"noise":noise,
            "frames":frames,"cells":visited,"fallback_contacts":0,"tank_contacts":hits,"deform_contacts":deform_hits,
            "deformed_vertices":changed_vertices,"surface_sample_points":sampled_points,"deform_spark_events":spark_events,"scene":object.is_some(),"scenery":ordinary,"ordinary_contacts":ordinary_hits,"scenery_ticks":scenery_ticks,
            "player_scene":if player_sequence {format!("player-scene-{name}.bin")} else {String::new()},"player_context":if player_sequence {format!("player-context-{name}.bin")} else {String::new()},"flock_coverage":flock_coverage,"ambient_coverage":{"selected":ambient_selected,"particle":saw_ambient_particle,"reused":saw_ambient_reuse,"frozen_updates":ambient_frozen,"visibility":ambient_visible},"train_coverage":{"running":saw_train_running,"motion":saw_train_motion,"arrival":saw_train_arrival,"reversal":saw_train_reversal,"reset":saw_train_reset,"wait_end":saw_train_wait_end},"emitter_coverage":{"shot":saw_emitter_shot,"explosion":saw_emitter_explosion,"retirement":saw_emitter_retirement},"retirement":retire || name=="complete-player-warp" || name=="complete-player-ground-concussion" || mine_sequence,"transient":transient,
            "retire_object":if retire {object.unwrap()+1} else if name=="complete-player-warp" {baseline.len()+1} else if name=="complete-player-ground-concussion" {baseline.len()+2} else {0},
            "retire_frame":if retire {1} else if name=="complete-player-warp" {2} else if name=="complete-player-ground-concussion" {6} else {65535},"scenery_initial":format!("scenery-initial-{name}.bin")});
        exported_case["cold_scene"] = json!(cold_scene);
        exported.push(exported_case);
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
    let mut whole_map = Vec::new();
    for cell in 0..64 {
        whole_map.extend(pages::cell_bytes(&terrain, cell));
    }
    fs::write(out.join("map-cells.bin"), whole_map).unwrap();
}
