//! Original resource preparation and two persistent physical vehicle owners.
use std::{fs, path::Path, sync::Arc};
use tankracer_assets::{archive::Archive, config::ResolveContext, terrain::TerrainData};
use tankracer_game::{
    object_query::TankId,
    session::loading::{Files, Prepared, Selection},
    vehicle_loading::Request,
    vehicle_model::BuildContext,
    vehicle_resources,
    vehicle_runtime::SinglePlayer,
    vehicle_startup::Environment,
};
pub fn two_cars(root: &Path, vehicle: u32) -> SinglePlayer {
    let read =
        |name: &str| fs::read(root.join("assets/game").join(name).join("FASTFILE.DAT")).unwrap();
    let data = read("DATA");
    let bronze = read("TANKS/BRONZE");
    let shared = read("INGAME");
    let level = read("LEVELS/ENGLAND");
    let archive = Archive::parse(&level).unwrap();
    let terrain = Arc::new(TerrainData::decode_bsm(archive.get("ENGLAND.BSM").unwrap()).unwrap());
    let mut world = Prepared::load_race(
        terrain,
        Files {
            data: &data,
            vehicles: &[&bronze],
            shared: &shared,
            level: &level,
        },
        Selection::default_cup(0, 1).unwrap(),
        false,
    )
    .unwrap()
    .finish()
    .unwrap()
    .world;
    let model = vehicle_resources::prepare(
        &data,
        &[&bronze],
        &shared,
        vehicle,
        BuildContext {
            resolve: ResolveContext {
                player: true,
                ..Default::default()
            },
            ambient_wobble: false,
            session_active: false,
        },
    )
    .unwrap()
    .model;
    let spawn = world.vehicles.entries[0]
        .as_ref()
        .unwrap()
        .physics
        .motion
        .poses
        .body;
    let id = world
        .add_vehicle(Request {
            model,
            spawn,
            static_allocation: true,
            environment: Environment {
                ambient_wobble: false,
                session_active: false,
                ambient_phase: 0,
                gun_pivot: [0; 3],
            },
        })
        .unwrap();
    assert_eq!(id, TankId(1));
    world
}
pub fn complete(root: &Path) -> tankracer_game::session::Session {
    constructed(root).into_session().unwrap().0
}
pub fn race(root: &Path) -> SinglePlayer {
    let read = |name: &str| {
        let path = root.join("assets/game").join(name).join("FASTFILE.DAT");
        assert!(
            path.is_file(),
            "missing original archive: {}",
            path.display()
        );
        fs::read(path).unwrap()
    };
    let data = read("DATA");
    let vehicles = [
        read("TANKS/BRONZE"),
        read("TANKS/SILVER"),
        read("TANKS/GOLD"),
    ];
    let shared = read("INGAME");
    let level = read("LEVELS/ENGLAND");
    let archive = Archive::parse(&level).unwrap();
    let terrain = Arc::new(TerrainData::decode_bsm(archive.get("ENGLAND.BSM").unwrap()).unwrap());
    Prepared::load_race(
        terrain,
        Files {
            data: &data,
            vehicles: &vehicles.each_ref().map(Vec::as_slice),
            shared: &shared,
            level: &level,
        },
        Selection::default_cup(0, 1).unwrap(),
        true,
    )
    .unwrap()
    .finish()
    .unwrap()
    .world
}
pub fn full_race(root: &Path) -> SinglePlayer {
    let read = |name: &str| {
        let path = root.join("assets/game").join(name).join("FASTFILE.DAT");
        assert!(
            path.is_file(),
            "missing original archive: {}",
            path.display()
        );
        fs::read(path).unwrap()
    };
    let data = read("DATA");
    let vehicles = [
        read("TANKS/BRONZE"),
        read("TANKS/SILVER"),
        read("TANKS/GOLD"),
    ];
    let shared = read("INGAME");
    let level = read("LEVELS/ENGLAND");
    let archive = Archive::parse(&level).unwrap();
    let terrain = Arc::new(TerrainData::decode_bsm(archive.get("ENGLAND.BSM").unwrap()).unwrap());
    Prepared::rcc_load_full_race(
        terrain,
        Files {
            data: &data,
            vehicles: &vehicles.each_ref().map(Vec::as_slice),
            shared: &shared,
            level: &level,
        },
        Selection::default_cup(0, 1).unwrap(),
    )
    .unwrap()
    .finish()
    .unwrap()
    .world
}
pub fn constructed(root: &Path) -> tankracer_game::world_startup::Constructed {
    construct(root, None)
}
pub fn constructed_pair(
    root: &Path,
    peer: tankracer_game::vehicle_model::VehicleModel,
) -> tankracer_game::world_startup::Constructed {
    construct(root, Some(peer))
}
fn construct(
    root: &Path,
    peer: Option<tankracer_game::vehicle_model::VehicleModel>,
) -> tankracer_game::world_startup::Constructed {
    let read = |name: &str| {
        let path = root.join("assets/game").join(name).join("FASTFILE.DAT");
        assert!(
            path.is_file(),
            "missing original archive: {}",
            path.display()
        );
        fs::read(path).unwrap()
    };
    let data = read("DATA");
    let vehicles = [
        read("TANKS/BRONZE"),
        read("TANKS/SILVER"),
        read("TANKS/GOLD"),
    ];
    let shared = read("INGAME");
    let level = read("LEVELS/ENGLAND");
    let files = Files {
        data: &data,
        vehicles: &vehicles.each_ref().map(Vec::as_slice),
        shared: &shared,
        level: &level,
    };
    let selection = Selection::default_cup(0, 1).unwrap();
    if let Some(peer) = peer {
        let archive = Archive::parse(&level).unwrap();
        let terrain =
            Arc::new(TerrainData::decode_bsm(archive.get("ENGLAND.BSM").unwrap()).unwrap());
        let mut prepared = Prepared::load_complete(terrain, files, selection).unwrap();
        prepared.rcc_add_peer(peer).unwrap();
        tankracer_game::world_startup::Constructed::from_prepared(prepared, selection).unwrap()
    } else {
        tankracer_game::world_startup::Constructed::load(files, selection).unwrap()
    }
}
