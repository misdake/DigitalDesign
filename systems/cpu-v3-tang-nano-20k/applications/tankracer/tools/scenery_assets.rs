//! Live ordinary scenery components and scheduling state from original owners.
//! Word header: magic/version, object/flag counts, eleven offset/count pairs,
//! then i32 slot/next/seed/Fall class/minimum turn/maximum turn and total words.
//! Lists reserve entity/object capacity; state and configuration offsets are
//! relative to their sections. Extended Fall retains a distinct kind (3).
use tankracer_game::{object_query::EntityId, vehicle_runtime::SinglePlayer};

const HEADER: usize = 48;
pub const RECORDS: usize = 0;
pub const STATE: usize = 1;
pub const CONFIG: usize = 2;
pub const FLAGS: usize = 3;
pub const CLASSES: usize = 4;
pub const OBJECTS: usize = 5;
pub const ORDER: usize = 6;
pub const NOTIFICATIONS: usize = 7;
pub const DESTRUCTION: usize = 8;
pub const ORDINARY: usize = 9;
pub const SPECIAL: usize = 10;

fn append(out: &mut Vec<u16>, value: i32) {
    out.extend([value as u16, (value >> 16) as u16]);
}
fn section(image: &mut Vec<u16>, index: usize, words: &[u16], count: usize, capacity: usize) {
    assert!(words.len() <= capacity);
    image[4 + index * 2] = u16::try_from(image.len()).unwrap();
    image[5 + index * 2] = u16::try_from(count).unwrap();
    image.extend(words);
    image.resize(image.len() + capacity - words.len(), 0);
}
fn list(image: &mut Vec<u16>, index: usize, ids: impl Iterator<Item = u32>, capacity: usize) {
    let mut words = Vec::new();
    for id in ids {
        append(&mut words, id as i32);
    }
    section(image, index, &words, words.len() / 2, capacity * 2);
}
fn config(configs: &mut Vec<u16>, entries: &mut Vec<(Vec<i32>, u16)>, values: Vec<i32>) -> u16 {
    for (entry, offset) in entries.iter() {
        if entry == &values {
            return *offset;
        }
    }
    let offset = u16::try_from(configs.len()).unwrap();
    for value in &values {
        append(configs, *value);
    }
    entries.push((values, offset));
    offset
}
pub fn encode(world: &SinglePlayer) -> Vec<u16> {
    let scene = &world.objects;
    let count = scene.world.objects.len();
    let entities = world.identity.entities.capacity() as usize;
    let flag_count = world.identity.ticks.flags.len();
    let mut records = vec![0; count * 8];
    let mut states = Vec::new();
    let mut configs = Vec::new();
    let mut entries = Vec::new();
    let t = scene.components.fall_tuning;
    for index in 0..count {
        let row = &mut records[index * 8..index * 8 + 8];
        if let Some(object) = scene.components.objects.get(index).and_then(Option::as_ref) {
            row[3] = u16::from(object.data.mesh.is_some()) * 2;
            if let Some(cell) = object.data.cell {
                row[4..7].copy_from_slice(&[cell.x as u16, cell.z as u16, 1]);
            }
            if let Some(fall) = scene.components.falls.get(index).and_then(Option::as_ref) {
                // Extended Fall keeps its state; its class methods remain an
                // explicit separate binding until attached effects are ported.
                row[0] = if fall.attached.is_none()
                    && object.data.auxiliary.is_none()
                    && object.data.debris.is_none()
                {
                    1
                } else {
                    3
                };
                row[1] = u16::try_from(states.len()).unwrap();
                for value in [
                    fall.angular_velocity,
                    fall.rotation,
                    fall.turn_rate,
                    fall.slope_limit,
                    fall.last_vehicle.map_or(-1, |e| e.0 as i32),
                ] {
                    append(&mut states, value);
                }
                row[2] = config(
                    &mut configs,
                    &mut entries,
                    vec![
                        t.squash_angle,
                        t.gravity,
                        t.elasticity_multiplier,
                        t.elasticity_shift as i32,
                    ],
                );
            } else if let Some(bounce) =
                scene.components.bounces.get(index).and_then(Option::as_ref)
            {
                row[0] = 2;
                row[1] = u16::try_from(states.len()).unwrap();
                row[3] |= u16::from(bounce.reference_alias);
                let b = &bounce.state;
                for value in b.velocity.into_iter().chain([
                    b.spin,
                    b.ground,
                    b.timer as i32,
                    b.age as i32,
                    i32::from(b.airborne),
                    i32::from(b.reverse_spin),
                    i32::from(b.special),
                    i32::from(b.raised),
                ]) {
                    append(&mut states, value);
                }
                let c = bounce.config;
                row[2] = config(
                    &mut configs,
                    &mut entries,
                    vec![
                        c.elasticity as i32,
                        c.friction as i32,
                        c.height_factor as i32,
                        c.hit_spin.multiplier as i16 as i32,
                        c.hit_spin.shift as i32,
                        c.slow_spin.multiplier as i16 as i32,
                        c.slow_spin.shift as i32,
                        c.deflection as i32,
                        c.gravity,
                        c.height_period as i32,
                        object.data.impact_threshold,
                    ],
                );
            } else if let Some(closer) =
                scene.components.closers.get(index).and_then(Option::as_ref)
            {
                row[0] = 4;
                row[1] = u16::try_from(states.len()).unwrap();
                row[3] |= u16::from(closer.reference_alias);
                for value in [
                    closer.state.speed,
                    closer.state.flags as i32,
                    match closer.state.phase {
                        tankracer_game::closer::Phase::Uninitialized => 0,
                        tankracer_game::closer::Phase::Waiting => 1,
                        tankracer_game::closer::Phase::Closing => 2,
                        tankracer_game::closer::Phase::Fragments => 3,
                    },
                ] {
                    append(&mut states, value);
                }
            }
        }
    }
    let mut image = vec![0; HEADER];
    image[..4].copy_from_slice(&[0x534d, 1, count as u16, flag_count as u16]);
    section(&mut image, RECORDS, &records, records.len(), records.len());
    section(&mut image, STATE, &states, states.len(), states.len());
    section(&mut image, CONFIG, &configs, configs.len(), configs.len());
    let flags = world
        .identity
        .ticks
        .flags
        .iter()
        .flat_map(|f| [*f as u16, (*f >> 16) as u16])
        .collect::<Vec<_>>();
    section(&mut image, FLAGS, &flags, flag_count, flags.len());
    let classes = (0..entities)
        .map(|i| {
            world
                .identity
                .entities
                .record(EntityId(i as u32))
                .and_then(|e| e.class)
                .map_or(65535, |c| c.0 as u16)
        })
        .collect::<Vec<_>>();
    section(&mut image, CLASSES, &classes, entities, entities);
    let objects = (0..entities)
        .map(|index| {
            world
                .identity
                .entities
                .record(EntityId(index as u32))
                .and_then(|e| e.object)
                .and_then(|handle| handle.obstacle())
                .map_or(65535, |id| u16::try_from(id.0).unwrap())
        })
        .collect::<Vec<_>>();
    section(&mut image, OBJECTS, &objects, entities, entities);
    list(
        &mut image,
        ORDER,
        world.identity.ticks.order.iter().map(|e| e.0),
        entities,
    );
    list(
        &mut image,
        NOTIFICATIONS,
        world.identity.queues.notifications.iter().map(|e| e.0),
        entities,
    );
    list(
        &mut image,
        DESTRUCTION,
        world.identity.queues.destruction.iter().map(|e| e.0),
        entities,
    );
    list(
        &mut image,
        ORDINARY,
        scene.components.ordinary.iter().map(|e| e.0 as u32),
        count,
    );
    list(
        &mut image,
        SPECIAL,
        scene.components.special.iter().map(|e| e.0 as u32),
        count,
    );
    let environment = world.registry.hit_environment(t, false, 65536);
    for (index, value) in [
        world.identity.current_slot as i32,
        world.identity.ticks.next.map_or(-1, |e| e.0 as i32),
        world.shared.random as i32,
        world.registry.fall.0 as i32,
        environment.minimum_turn_rate,
        environment.maximum_turn_rate,
    ]
    .into_iter()
    .enumerate()
    {
        image[26 + index * 2] = value as u16;
        image[27 + index * 2] = (value >> 16) as u16;
    }
    image[38] = u16::try_from(image.len()).unwrap();
    image
}
