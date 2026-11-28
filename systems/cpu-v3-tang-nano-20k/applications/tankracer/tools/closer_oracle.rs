//! Original inverse basis, proximity gate and closing-barrier motion answers.
use serde_json::{json, Value};
use std::{collections::VecDeque, fs, path::Path};
use tankracer_game::{
    closer::{self, Phase},
    entity_factory,
    math::Matrix,
    object_query::EntityId,
    scene_entity::Handle,
    scene_objects::rcc_closer_probe,
};
fn phase(value: Phase) -> i32 {
    match value {
        Phase::Uninitialized => 0,
        Phase::Waiting => 1,
        Phase::Closing => 2,
        Phase::Fragments => 3,
    }
}
fn list(out: &mut Vec<i32>, ids: impl IntoIterator<Item = EntityId>) {
    let ids = ids.into_iter().map(|id| id.0 as i32).collect::<Vec<_>>();
    assert!(ids.len() <= 8);
    out.push(ids.len() as i32);
    out.extend(&ids);
    out.extend(std::iter::repeat_n(-1, 8 - ids.len()));
}
fn owner() -> entity_factory::State<Handle> {
    let mut owner = entity_factory::State::new(16).unwrap();
    owner.current_slot = 12;
    owner.ticks.flags = vec![0, 16, 48, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    owner.ticks.order = vec![EntityId(1), EntityId(2)];
    owner.ticks.next = Some(EntityId(2));
    owner.queues.notifications = VecDeque::from([EntityId(2), EntityId(0)]);
    owner.queues.destruction = VecDeque::from([EntityId(4)]);
    owner
}
fn movement_input(
    state: closer::State,
    matrix: Matrix,
    owner: &entity_factory::State<Handle>,
    dt: i32,
    limit: u32,
    entity: EntityId,
) -> Vec<i32> {
    let mut out = vec![
        2,
        dt,
        entity.0 as i32,
        limit as i32,
        state.speed,
        state.flags as i32,
        phase(state.phase),
        0,
        0,
    ];
    out.extend([0; 6]);
    out.extend(matrix.0);
    out.extend(owner.ticks.flags.iter().map(|v| *v as i32));
    list(&mut out, owner.queues.notifications.iter().copied());
    list(&mut out, owner.queues.destruction.iter().copied());
    assert_eq!(out.len(), 58);
    out
}
fn movement_output(
    status: i32,
    state: closer::State,
    matrix: Matrix,
    owner: &entity_factory::State<Handle>,
) -> Vec<i32> {
    let mut out = vec![status, state.speed, state.flags as i32, phase(state.phase)];
    out.extend(matrix.0);
    out.extend(owner.ticks.flags.iter().map(|v| *v as i32));
    list(&mut out, owner.queues.notifications.iter().copied());
    list(&mut out, owner.queues.destruction.iter().copied());
    out.extend([
        owner.current_slot as i32,
        owner.ticks.next.map_or(-1, |id| id.0 as i32),
        owner.ticks.order.len() as i32,
    ]);
    out.extend(owner.ticks.order.iter().map(|id| id.0 as i32));
    assert_eq!(out.len(), 52);
    out
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let out = Path::new(&args[0]);
    let mut cases = Vec::<Value>::new();
    let mut matrices = Vec::new();
    for v in -4096..=4096 {
        let mut m = Matrix::IDENTITY;
        m.0[7] = v;
        m.0[6] = -256;
        m.0[4] = -4000;
        m.0[1] = 11;
        matrices.push(m);
    }
    for a in [
        [0, 0, 0],
        [0, 0, 16384],
        [16384, 0, 0],
        [49152, 0, 0],
        [1300, 17000, 700],
        [7000, 5000, 3000],
    ] {
        matrices.push(Matrix::from_angles(a));
    }
    for component in [1, 4, 6, 7, 8] {
        for value in [i32::MIN, i32::MAX, -4097, 4097, 65536, -65536] {
            let mut m = Matrix::IDENTITY;
            m.0[component] = value;
            matrices.push(m);
        }
    }
    for m in matrices {
        let mut input = vec![0];
        input.extend(m.0);
        let expected = match m.angles() {
            Ok((angles, regular)) => vec![
                0,
                angles[0] as i32,
                angles[1] as i32,
                angles[2] as i32,
                i32::from(regular),
            ],
            Err(_) => vec![-2, 0x12345678, 0x12345678, 0x12345678, 0x12345678],
        };
        cases.push(json!({"input":input,"expected":expected}));
    }
    for id in [0u32, 1, 7, 16, u32::MAX] {
        for frame in 0..8 {
            for player in [
                None,
                Some([40959, 123, 0]),
                Some([40960, 0, 0]),
                Some([0, 0, -40960]),
                Some([i32::MIN, 0, i32::MAX]),
            ] {
                let mut state = closer::State {
                    phase: Phase::Waiting,
                    speed: -17,
                    flags: 0x81234567,
                };
                let mut input = vec![1, id as i32, frame, i32::from(player.is_some())];
                input.extend([0; 3]);
                input.extend(player.unwrap_or([0; 3]));
                input.extend([state.speed, state.flags as i32, phase(state.phase)]);
                closer::poll(&mut state, EntityId(id), frame as u32, [0; 3], player);
                cases.push(json!({"input":input,"expected":[0,state.speed,state.flags as i32,phase(state.phase)]}));
            }
        }
    }
    let matrices = [
        Matrix::IDENTITY,
        Matrix::from_angles([0, 0, 1000]),
        Matrix::from_angles([0, 0, (-1000i32) as u32]),
        Matrix::from_angles([0, 0, 32768]),
        Matrix::from_angles([16384, 0, 0]),
        Matrix([0; 9]),
        Matrix([4096, 0, 0, 0, 4096, 0, 0, 4097, 4096]),
    ];
    for matrix in matrices {
        for (speed, flags) in [
            (0, 0),
            (7284, 4),
            (3, 4),
            (-1, 2),
            (i32::MAX, 0x80000007),
            (i32::MIN, 0),
        ] {
            for dt in [0i32, 33, -33, i32::MAX] {
                for limit in [0, 1, 2, 100000, u32::MAX] {
                    for id in [1, 16] {
                        let mut state = closer::State {
                            speed,
                            flags,
                            phase: Phase::Closing,
                        };
                        let mut current = matrix;
                        let mut owner = owner();
                        let input = movement_input(state, current, &owner, dt, limit, EntityId(id));
                        let status = rcc_closer_probe::advance(
                            &mut state,
                            &mut current,
                            dt,
                            limit,
                            &mut owner,
                            EntityId(id),
                        )
                        .err()
                        .unwrap_or(0);
                        cases.push(json!({"input":input,"expected":movement_output(status,state,current,&owner)}));
                    }
                }
            }
        }
    }
    for angle in [1000, 32768, (-1000i32) as u32] {
        let mut state = closer::State {
            speed: 100,
            flags: 0,
            phase: Phase::Closing,
        };
        let mut matrix = Matrix::from_angles([0, 0, angle]);
        let mut owner = owner();
        for frame in 0..24 {
            let input = movement_input(state, matrix, &owner, 33, 100000, EntityId(1));
            let status = rcc_closer_probe::advance(
                &mut state,
                &mut matrix,
                33,
                100000,
                &mut owner,
                EntityId(1),
            )
            .err()
            .unwrap_or(0);
            cases.push(json!({"input":input,"expected":movement_output(status,state,matrix,&owner),"sequence":angle,"frame":frame}));
        }
    }
    fs::write(out.join("closer.json"), serde_json::to_vec(&json!({"cases":cases,"scope":"Original Matrix::angles, closer::poll and private closing movement, including queues and committed failure prefixes"})).unwrap()).unwrap();
    println!("{} closer and inverse-basis cases", cases.len());
}
