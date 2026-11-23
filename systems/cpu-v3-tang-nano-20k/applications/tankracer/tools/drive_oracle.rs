//! Real drive calls and archive-derived descriptors; boost inputs use the actual prefix mapping.
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use tankracer_assets::{
    archive::Archive,
    config::{Damping, Surfaces, Tank},
    object_catalog::Catalog,
};
use tankracer_game::{drive, vehicle_prefix::boost};
fn damping(v: &mut Vec<i32>, d: Damping) {
    v.extend([d.step as i32, d.fraction as i32, d.shift as i32]);
}
pub fn config(tank: &Tank, surfaces: &Surfaces, t: &drive::Tuning) -> Vec<i32> {
    let d = &tank.drive;
    let mut v = vec![
        d.max_rpm,
        d.max_power,
        d.max_turn,
        d.initial_power,
        d.power_mul,
        d.max_brakes,
        d.turn_fraction.multiplier as i32,
        d.turn_fraction.shift as u8 as i32,
        d.hill_roll.multiplier as i32,
        d.hill_roll.shift as u8 as i32,
        d.understeer_fraction.multiplier as i32,
        d.understeer_fraction.shift as u8 as i32,
        d.rearweight_fraction.multiplier as i32,
        d.rearweight_fraction.shift as u8 as i32,
        d.rearweight_accel_fraction.multiplier as i32,
        d.rearweight_accel_fraction.shift as u8 as i32,
        d.traction_limit,
        d.catchup_traction_limit,
        d.sidegrip_fraction.multiplier as i32,
        d.sidegrip_fraction.shift as u8 as i32,
    ];
    for a in [
        d.forward_drag,
        d.track_drag,
        d.track_air_drag,
        d.track_spin_drag,
        d.gravity_drag,
        d.rotational_drag,
        d.air_rotational_drag,
        d.sideways_drag,
        d.force_drag,
    ] {
        damping(&mut v, a);
    }
    v.extend([d.turbo_amount, d.turbo_power_mul]);
    v.extend(t.gear_limits);
    v.extend(t.gear_offsets);
    for g in t.gear_gain {
        v.extend([g.multiplier as i32, g.shift as u8 as i32]);
    }
    for a in [t.engine_damping, t.boost_approach, t.boost_decay] {
        damping(&mut v, a);
    }
    assert_eq!(v.len(), 78);
    for s in surfaces.entries {
        for g in [s.drag, s.grip].into_iter().chain(s.slide) {
            v.extend([g.multiplier as i32, g.shift as u8 as i32]);
        }
    }
    assert_eq!(v.len(), 258);
    v
}
pub fn flatten(s: &drive::State) -> Vec<i32> {
    s.velocity
        .into_iter()
        .chain(s.angular)
        .chain([
            s.track_speed,
            s.boost_target,
            s.engine_display,
            s.slip,
            s.was_active as i32,
            s.gear as i32,
            s.lateral_velocity,
        ])
        .chain(s.acceleration)
        .chain([s.load])
        .collect()
}
pub fn frame(input: &drive::Input, e: &drive::Environment) -> Vec<i32> {
    vec![
        e.dt,
        e.mode,
        i32::from(e.player),
        i32::from(e.active),
        i32::from(e.launch_eligible),
        e.materials[0] as i32,
        e.materials[1] as i32,
        e.kinds[0],
        e.kinds[1],
        e.kinds[2],
        e.kinds[3],
        e.contact_y[0],
        e.contact_y[1],
        e.contact_y[2],
        e.contact_y[3],
        e.airborne as i32,
        e.boost_charge as i32,
        e.boost_power,
        e.extra_drive,
        input.throttle,
        input.brake,
        input.steering,
        input.action_flags as i32,
        i32::from(input.alternate),
        input.alternate_lateral as i32,
        input.alternate_rotation as i32,
    ]
}
fn main() {
    let mut args = std::env::args_os().skip(1);
    let reference = PathBuf::from(args.next().unwrap());
    let output = PathBuf::from(args.next().unwrap());
    fs::create_dir_all(&output).unwrap();
    let bytes = fs::read(reference.join("assets/game/DATA/FASTFILE.DAT")).unwrap();
    let archive = Archive::parse(&bytes).unwrap();
    let tank = Tank::decode(archive.get("TANK1.DAT").unwrap()).unwrap();
    let surfaces = Surfaces::decode(archive.get("SURFS.DAT").unwrap()).unwrap();
    let catalog = Catalog::decode_checked(archive.get("OBPARAMS.DAT").unwrap()).unwrap();
    let tuning = drive::Tuning::default();
    let config = config(&tank, &surfaces, &tuning);
    let mut cases = Vec::<Value>::new();
    for (name, initial_v, launch, boosted, alternate) in [
        (
            "throttle-brake-reverse",
            [5001, -2000, -45000],
            false,
            false,
            false,
        ),
        ("launch", [-25000, 1200, 16000], true, false, false),
        ("authored-boost", [7000, 0, 130000], false, true, false),
        ("alternate-weather", [-9001, 0, -17000], false, false, true),
    ] {
        let mut state = drive::State {
            velocity: initial_v,
            angular: [-777, 13003, 999],
            track_speed: initial_v[2],
            engine_display: 17,
            load: 22000,
            ..drive::State::default()
        };
        let initial = flatten(&state);
        let mut frames = Vec::new();
        let mut boost = boost::State::default();
        if boosted {
            boost.apply(
                tank.drive.turbo_amount,
                catalog.turbo.player_power,
                catalog.turbo.player_loss,
            );
        }
        for index in 0..10 {
            let dt = if index & 1 == 0 { 16 } else { 33 };
            boost.decay(dt, true);
            let kinds = match index % 4 {
                0 => [1, 1, 1, 1],
                1 => [0, 1, 2, 2],
                2 => [2, 2, 2, 2],
                _ => [4, 4, 1, 1],
            };
            let airborne = kinds.into_iter().filter(|&k| k == 2).count() as i8;
            let input = drive::Input {
                throttle: if index < 4 {
                    65535
                } else if index < 7 {
                    0
                } else {
                    -65535
                },
                brake: if (4..7).contains(&index) { 65535 } else { 0 },
                steering: if index & 1 == 0 { -18000 } else { 21000 },
                action_flags: if alternate { 17 } else { 0 },
                alternate: alternate && index < 5,
                alternate_lateral: 1000,
                alternate_rotation: -37,
            };
            let env = drive::Environment {
                dt,
                mode: if index == 9 { 5 } else { 0 },
                player: true,
                active: true,
                launch_eligible: launch,
                materials: if index & 1 == 0 { [1, 2] } else { [7, 5] },
                kinds,
                contact_y: [-648, -649, -651, -645],
                airborne,
                boost_charge: boost.timer,
                boost_power: boost.extra_speed,
                extra_drive: boost.power,
            };
            state.load = 22000 + index * 111;
            let load = state.load;
            let result = state.step(&tank.drive, &surfaces, &tuning, &input, &env);
            let status = match result {
                Ok(()) => 0,
                Err(drive::Error::MissingSurface(_)) => 1,
                Err(drive::Error::InvalidWeather) => 2,
                Err(drive::Error::GearLimit) => 3,
            };
            assert_eq!(status, 0);
            frames.push(json!({"input":{"frame":frame(&input,&env),"load":load},"state":flatten(&state),"status":status,"boost_prefix":[boost.extra_speed,boost.power,boost.power_loss,i32::from(boost.timer)]}));
        }
        cases.push(json!({"name":name,"initial":initial,"config":config,"frames":frames}));
    }
    fs::write(output.join("drive.json"),serde_json::to_vec_pretty(&json!({"scope":"real drive::State::step; real tank/surface/tuning descriptors and prefix-generated boost inputs; excludes suspension, world/body transforms, integration and RNG-consuming sampling","tank":"TANK1.DAT","turbo":format!("{:?}",catalog.turbo),"boost_mapping":{"boost_charge":"prefix.timer","boost_power":"prefix.extra_speed","extra_drive":"prefix.power"},"cases":cases})).unwrap()).unwrap();
}
