//! Direct calls to the unmodified game library with frozen geometry-query results.
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use tankracer_assets::{
    archive::Archive,
    config::{ResolveContext, Surfaces, Tank},
    model::ShapeArchive,
    object_catalog::Catalog,
};
use tankracer_game::{
    contact, suspension,
    vehicle_model::{BuildContext, Bundles, ModelBank, VehicleModel},
};
fn flatten(s: &suspension::State, load: i32, pivot: [i32; 3]) -> Vec<i32> {
    let mut v = Vec::new();
    for p in s.points {
        v.extend([p.kind, p.gap, p.corner as i32, p.travel, p.rate, p.output]);
    }
    v.extend(s.angular);
    v.extend(s.tilt);
    v.extend([
        s.penetrating_input_sum,
        s.travel_sum,
        s.vertical_delta,
        s.world_vy,
        s.position_y,
        s.timer,
    ]);
    v.extend(s.auxiliary_type);
    v.extend(s.auxiliary_cached);
    v.extend([s.max_penetration, s.last_penetration, load]);
    v.extend(pivot);
    v
}
fn main() {
    let mut args = std::env::args_os().skip(1);
    let reference = PathBuf::from(args.next().unwrap());
    let output = PathBuf::from(args.next().unwrap());
    fs::create_dir_all(&output).unwrap();
    let data = fs::read(reference.join("assets/game/DATA/FASTFILE.DAT")).unwrap();
    let a = Archive::parse(&data).unwrap();
    let tank = Tank::decode(a.get("TANK1.DAT").unwrap()).unwrap();
    let surfaces = Surfaces::decode(a.get("SURFS.DAT").unwrap()).unwrap();
    let catalog = Catalog::decode_checked(a.get("OBPARAMS.DAT").unwrap()).unwrap();
    let bronze = fs::read(reference.join("assets/game/TANKS/BRONZE/FASTFILE.DAT")).unwrap();
    let ba = Archive::parse(&bronze).unwrap();
    let shared = fs::read(reference.join("assets/game/INGAME/FASTFILE.DAT")).unwrap();
    let sa = Archive::parse(&shared).unwrap();
    let bundles = Bundles {
        vehicle: vec![ShapeArchive::parse(ba.get("TANKB.SS").unwrap()).unwrap()],
        shared: vec![ShapeArchive::parse(sa.get("INGAME.SS").unwrap()).unwrap()],
        level: vec![],
    };
    let model = VehicleModel::build(
        &tank,
        &bundles,
        &mut ModelBank::default(),
        &surfaces,
        BuildContext {
            resolve: ResolveContext {
                player: true,
                ..ResolveContext::default()
            },
            ambient_wobble: false,
            session_active: false,
        },
    )
    .unwrap();
    let b = model.geometry_bounds;
    let layout = [b.min[0], b.min[1], b.min[2], b.max[0], b.max[2]];
    let c = contact::Config::try_from(&tank.suspension).unwrap();
    let s = &tank.suspension;
    let mut config = vec![
        c.lower,
        c.upper,
        c.force_positive.multiplier.into(),
        c.force_positive.shift.into(),
        c.force_negative.multiplier.into(),
        c.force_negative.shift.into(),
        c.output_gain.multiplier.into(),
        c.output_gain.shift.into(),
        c.tilt_gain.multiplier.into(),
        c.tilt_gain.shift.into(),
        c.shape_shift as i32,
        c.restore_positive.multiplier.into(),
        c.restore_positive.shift.into(),
        c.restore_negative.multiplier.into(),
        c.restore_negative.shift.into(),
        c.damping_shift.into(),
        c.travel_dead_step.into(),
        s.initial_weight,
        s.initial_pressure,
        s.pressure_fraction.multiplier.into(),
        (s.pressure_fraction.shift as u8).into(),
    ];
    config.extend(layout);
    let mut cases = Vec::<Value>::new();
    for (name, flag, session, mode, dt) in [
        ("mixed-contact", 0, 0, 0, 16),
        ("airborne-half", 1, 0, 0, 33),
        ("region-moon", 0, 1, 5, 16),
    ] {
        let mut state = suspension::State {
            points: std::array::from_fn(|i| suspension::Point {
                corner: i as u32,
                travel: [-12000, 3000, 19000, -7000][i],
                rate: [-5, 7, -11, 13][i],
                ..suspension::Point::default()
            }),
            angular: [-100, 75],
            world_vy: -5001,
            position_y: 1400,
            timer: 1,
            ..suspension::State::default()
        };
        let initial = flatten(&state, 0, [0; 3]);
        let mut frames = Vec::new();
        for frame in 1..=10 {
            let pattern = (frame - 1) % 5;
            let kinds = match pattern {
                0 => [0, 1, 2, 3],
                1 => [1, 1, 1, 1],
                2 => [2, 2, 2, 2],
                3 => [0, 2, 0, 2],
                _ => [3, 3, 3, 3],
            };
            let gaps: [i32; 4] = std::array::from_fn(|i| {
                if kinds[i] == 0 {
                    -275 - i as i32 * 150 - frame as i32
                } else if kinds[i] == 2 {
                    300
                } else {
                    0
                }
            });
            for i in 0..4 {
                state.points[i].kind = kinds[i];
                state.points[i].gap = gaps[i];
            }
            state.auxiliary_cached = [0; 2];
            let load = suspension::load(
                kinds,
                state.points.map(|p| p.travel),
                s.initial_weight,
                s.initial_pressure,
                s.pressure_fraction.multiplier,
                (s.pressure_fraction.shift as u8).into(),
            );
            let pivot = suspension::layout(kinds, layout);
            let env = suspension::Environment {
                dt,
                environment_flag: flag,
                session_active: session,
                mode,
                is_player: true,
                base: [0x4a77f, 0xfffb4856u32 as i32],
                region_min: [0x4a77f, 0xfffb4856u32 as i32],
                region_max: [0x813d2, 0xffff80ecu32 as i32],
                player_multiplier: catalog.moon.player.multiplier,
                player_shift: (catalog.moon.player.shift as u8).into(),
                other_multiplier: catalog.moon.other.multiplier,
                other_shift: (catalog.moon.other.shift as u8).into(),
                penetration_shift: (s.susp_to_shape.shift as u8).into(),
                auxiliary_query_type: [2, 1],
            };
            let flat_env = vec![
                dt,
                flag,
                session,
                mode,
                1,
                env.base[0],
                env.base[1],
                env.region_min[0],
                env.region_min[1],
                env.region_max[0],
                env.region_max[1],
                env.player_multiplier.into(),
                env.player_shift as i32,
                env.other_multiplier.into(),
                env.other_shift as i32,
                env.penetration_shift as i32,
                2,
                1,
            ];
            let had_timer = state.timer > 0;
            let mut queries = 0u16;
            state
                .step_resolved(&c, &env, |side, kind| {
                    queries |= 1 << side;
                    *kind = env.auxiliary_query_type[side];
                    Ok::<(), ()>(())
                })
                .unwrap();
            let mut kinds_event = 0u16;
            for i in 0..4 {
                if kinds[i] == 2 {
                    kinds_event |= 1 << i;
                }
                if kinds[i] == 0 {
                    kinds_event |= 16 << i;
                }
            }
            frames.push(json!({"input":{"environment":flat_env,"kinds":kinds,"gaps":gaps},"state":flatten(&state,load,pivot),"events":[queries,u16::from(state.max_penetration>0)|if had_timer {2}else{0},kinds_event],"rng_before":305441741u32,"rng_after":305441741u32}));
        }
        cases.push(json!({"name":name,"initial":initial,"config":config,"frames":frames}));
    }
    fs::write(output.join("suspension.json"),serde_json::to_vec_pretty(&json!({"scope":"real suspension/load/pivot; frozen terrain classifications, gaps and auxiliary query results; no drive, integration, rendering or RNG-consuming sampling", "tank":"TANK1.DAT", "configuration_debug":format!("{:?}",tank),"moon_debug":format!("{:?}",catalog.moon),"layout":layout,"cases":cases})).unwrap()).unwrap();
}
