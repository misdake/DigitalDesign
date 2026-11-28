//! Original entity allocation, binding, release and record accounting.
use super::*;
use tankracer_game::{
    dispatch::{Class, ClassId, MethodId},
    entities,
    entity_factory::{self as factory, BaseMethod},
    object_query::{EntityId, ObstacleId, TankId},
    scene_entity::{Handle, SessionObject},
};
fn error(error: entities::Error) -> i32 {
    match error {
        entities::Error::Capacity => 4,
        entities::Error::MissingEntity(_) => 6,
        entities::Error::InvalidLimit => 2,
        entities::Error::Limit => 3,
    }
}
fn table(state: &entities::Entities<Handle>) -> Vec<i32> {
    let mut out = vec![
        state.capacity() as i32,
        state.free_head().0 as i32,
        state.high_water().0 as i32,
        state.record_count() as i32,
        i32::from(state.sequential),
        state.next_requested.0 as i32,
    ];
    for index in 0..state.capacity() {
        let record = state.record(EntityId(index));
        let (kind, object) = match record.and_then(|r| r.object) {
            None => (0, 0),
            Some(Handle::Obstacle(id)) => (1, id.0 as i32),
            Some(Handle::Vehicle(id)) => (2, id.0 as i32),
            Some(Handle::Session(SessionObject::Camera)) => (3, 0),
            Some(Handle::Session(SessionObject::FinishOwner)) => (4, 0),
        };
        out.extend([
            i32::from(record.is_some()),
            record.and_then(|r| r.class).map_or(-1, |c| c.0 as i32),
            kind,
            object,
            state.next_free(EntityId(index)).unwrap().0 as i32,
        ]);
    }
    out
}
struct Release;
impl factory::Methods<Handle> for Release {
    fn invoke(
        &mut self,
        method: MethodId,
        call: factory::Invocation<'_, Handle>,
        env: factory::Environment<'_>,
    ) -> Result<(), i32> {
        assert_eq!(method, MethodId(1));
        BaseMethod::ReleaseId.invoke(call, env)
    }
}
fn apply(state: &mut factory::State<Handle>, op: [i32; 4]) -> (i32, i32) {
    let [op, a, b, c] = op;
    let id = EntityId(a as u32);
    let result = match op {
        0 => state.entities.reset(a as u32).map(|_| 0),
        1 => state.entities.grow().map(|_| 0),
        2 => state
            .entities
            .unlink_free(id, b as u32)
            .map(|id| id.0 as i32),
        3 => state
            .entities
            .new_record(id, b as u32)
            .map(|id| id.0 as i32),
        4 => state.entities.create_record(b as u32).map(|id| id.0 as i32),
        5 => state.entities.release_id(id).map(|_| 0),
        6 => state.entities.detach(id).map(|_| 0),
        7 | 8 => match state.entities.record_mut(id) {
            None => Err(entities::Error::MissingEntity(id)),
            Some(record) => {
                if op == 7 {
                    record.class = Some(ClassId(b as u16 as usize));
                } else {
                    record.object = match b {
                        0 => None,
                        1 => Some(Handle::Obstacle(ObstacleId(c as u32 as usize))),
                        2 => Some(Handle::Vehicle(TankId(c as u32 as usize))),
                        3 => Some(Handle::Session(SessionObject::Camera)),
                        4 => Some(Handle::Session(SessionObject::FinishOwner)),
                        _ => unreachable!(),
                    };
                }
                Ok(0)
            }
        },
        9 => {
            state.entities.sequential = a != 0;
            state.entities.next_requested = EntityId(b as u32);
            Ok(0)
        }
        10 => {
            let classes = [std::sync::Arc::new(Class {
                maximum_slot: 3,
                methods: vec![None, None, None, Some(MethodId(1))],
            })];
            let env = factory::Environment {
                classes: &classes,
                slots: factory::Slots {
                    allocate: 0,
                    initialize: 1,
                    destroy: 2,
                    release: 3,
                },
                work_limit: 1000,
            };
            return match state.context().destroy(id, env, &mut Release) {
                Ok(()) => (0, 0),
                Err(_) => (6, 0),
            };
        }
        _ => unreachable!(),
    };
    result.map_or_else(|e| (error(e), 0), |id| (0, id))
}
fn emit(cases: &mut Vec<Value>, state: &mut factory::State<Handle>, op: [i32; 4]) {
    let before = table(&state.entities);
    assert!(state.entities.capacity() <= 400);
    let mut input = before[..6].to_vec();
    input.extend(op);
    input.extend(&before[6..]);
    let (status, id) = apply(state, op);
    assert!(state.entities.capacity() <= 400);
    let mut expected = vec![status, id];
    expected.extend(table(&state.entities));
    cases.push(json!({"kind":32,"input":input,"expected":expected,"status":0}));
}
pub(super) fn append(cases: &mut Vec<Value>) {
    for limit in [0, 1, 2, 64, -1] {
        let mut state = factory::State::new(8).unwrap();
        for op in [
            [3, 3, 64, 0],
            [7, 3, 0, 0],
            [8, 3, 1, -1],
            [3, 3, limit, 0],
            [8, 3, 2, i32::MIN],
            [8, 3, 3, 99],
            [8, 3, 4, 99],
            [8, 3, 0, 99],
            [2, 7, limit, 0],
            [2, 0, limit, 0],
            [2, 99, limit, 0],
            [5, 3, 0, 0],
            [5, 3, 0, 0],
            // This deliberately cyclic free chain needs a short work budget.
            [2, 6, if limit == -1 { 64 } else { limit }, 0],
            [4, 0, limit, 0],
            [6, 3, 0, 0],
            [6, 3, 0, 0],
            [6, -1, 0, 0],
            [0, 8, 0, 0],
            [3, 1, 64, 0],
            [7, 1, 0, 0],
            [9, 1, 1, 0],
            [4, 0, limit, 0],
            [9, 0, 0, 0],
            [7, 1, 0, 0],
            [10, 1, 0, 0],
            [10, 1, 0, 0],
            [3, 1, 64, 0],
            [7, 1, 0, 0],
            [0, 8, 0, 0],
            [10, 1, 0, 0],
            [5, 0, 0, 0],
            [2, 0, limit, 0],
            [0, 2, 0, 0],
            [4, 0, 64, 0],
            [4, 0, 64, 0],
            [1, 0, 0, 0],
            [3, 260, 2, 0],
            [0, 1, 0, 0],
        ] {
            emit(cases, &mut state, op);
        }
    }
    for capacity in [2, 8, 128] {
        let mut state = factory::State::new(capacity).unwrap();
        for _ in 0..capacity + 2 {
            emit(cases, &mut state, [4, 0, 1000, 0]);
        }
        for op in [
            [7, 1, 0, 0],
            [8, 1, 1, 17],
            [10, 1, 0, 0],
            [4, 0, 1000, 0],
            [0, capacity as i32, 0, 0],
            [3, 1, 1000, 0],
        ] {
            emit(cases, &mut state, op);
        }
    }
    for requested in [
        0,
        1,
        2,
        7,
        8,
        127,
        128,
        135,
        136,
        260,
        i32::MAX,
        i32::MIN,
        -1,
    ] {
        for limit in [0, 1, 2, 64, -1] {
            if requested == i32::MAX && (limit == 64 || limit == -1) {
                continue;
            }
            let mut state = factory::State::new(8).unwrap();
            emit(cases, &mut state, [3, requested, limit, 0]);
        }
    }
    for next in [0, 3, i32::MAX, i32::MIN, -1] {
        let mut state = factory::State::new(8).unwrap();
        emit(cases, &mut state, [9, 1, next, 0]);
        emit(cases, &mut state, [4, 0, 0, 0]);
        emit(cases, &mut state, [4, 0, 1, 0]);
    }
    for id in [-1, 0, 1, 3, 8] {
        let mut state = factory::State::new(8).unwrap();
        emit(cases, &mut state, [3, 3, 64, 0]);
        for class in [0, 65535] {
            emit(cases, &mut state, [7, id, class, 0]);
        }
        for kind in 0..5 {
            emit(
                cases,
                &mut state,
                [8, id, kind, if kind == 2 { i32::MIN } else { -1 }],
            );
        }
        emit(cases, &mut state, [10, id, 0, 0]);
    }
    let mut state = factory::State::new(8).unwrap();
    for op in [
        [9, 1, 3, 0],
        [0, 8, 0, 0],
        [4, 0, 64, 0],
        [0, 2, 0, 0],
        [4, 0, 1, 0],
        [0, 0, 0, 0],
        [0, -1, 0, 0],
        [0, 400, 0, 0],
    ] {
        emit(cases, &mut state, op);
    }
}
