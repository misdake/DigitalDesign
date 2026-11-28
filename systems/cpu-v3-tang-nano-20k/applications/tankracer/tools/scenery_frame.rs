//! Semantic live-world snapshots with immutable geometry supplied by the scene.
use super::{identity_assets, scene_export, scenery_assets};
use tankracer_game::{scene_objects::rcc_lifecycle_probe, vehicle_runtime::SinglePlayer};
fn append(out: &mut Vec<u16>, value: i32) {
    out.extend([value as u16, (value >> 16) as u16]);
}
pub fn lifetime_flags(world: &SinglePlayer, index: usize) -> u16 {
    let c = &world.objects.components;
    let mut flags = 0u16;
    if c.falls
        .get(index)
        .and_then(Option::as_ref)
        .is_some_and(|s| s.attached.is_some())
    {
        flags |= 64;
    }
    if let Some(object) = c.objects.get(index).and_then(Option::as_ref) {
        flags |= 1
            | (u16::from(object.transient) << 1)
            | (u16::from(object.data.auxiliary.is_some()) << 2)
            | (u16::from(object.data.debris.is_some()) << 3)
            | (u16::from(object.auxiliary_mesh.is_some()) << 4)
            | (u16::from(object.auxiliary_motion.is_some()) << 5);
        if object.auxiliary_active != 0 {
            flags |= 128;
        }
    }
    // Resource owners outside the ordinary Fall/Bounce family retain their
    // own component retirement methods.
    if c.actors
        .get(tankracer_game::object_query::ObstacleId(index))
        .is_some()
        || c.spins.get(index).is_some_and(Option::is_some)
        || c.movers.get(index).is_some_and(Option::is_some)
        || c.explosions.get(index).is_some_and(Option::is_some)
        || c.surfaces.entries.get(index).is_some_and(Option::is_some)
    {
        flags |= 256;
    }
    let class = world.objects.world.objects[index].class;
    if flags & 1 != 0
        && class.0 > 3
        && class != world.registry.fall
        && class != world.registry.bounce
        && class != world.registry.closer
    {
        flags |= 256;
    }
    let id = tankracer_game::object_query::ObstacleId(index);
    if c.projectiles.entries().any(|(object, _)| object == id)
        || c.mines.entries().iter().any(|(object, _)| *object == id)
        || c.explosions.get(index).is_some_and(Option::is_some)
        || c.ufos.get(index).is_some_and(Option::is_some)
    {
        flags &= !256;
    }
    flags
}
pub fn initial(world: &SinglePlayer) -> Vec<u16> {
    let mut image = scenery_assets::encode(world);
    let start = image[4] as usize;
    for index in 0..world.objects.world.objects.len() {
        image[start + index * 8 + 7] = lifetime_flags(world, index);
    }
    image
}
pub fn initial_lists(world: &SinglePlayer) -> Vec<u16> {
    let mut out = vec![
        world.registry.generic.0 as u16,
        world.registry.obstacle.0 as u16,
    ];
    for ids in rcc_lifecycle_probe::lists(&world.objects.components) {
        out.push(ids.len() as u16);
        for id in ids {
            append(&mut out, id);
        }
    }
    out
}
pub fn capture(world: &SinglePlayer, baseline: &[Vec<u16>]) -> Vec<u16> {
    let image = scenery_assets::encode(world);
    let count = world.objects.world.objects.len();
    assert!(baseline.len() <= count);
    let mut out = vec![0; 24 + count * 8];
    out[..5].copy_from_slice(&[0x5346, 1, 0, count as u16, 24]);
    for index in 0..count {
        let input = &image[image[4] as usize + index * 8..][..8];
        let mut row = <[u16; 8]>::try_from(input).unwrap();
        row[7] = lifetime_flags(world, index);
        let (states, configs) = match row[0] {
            1 | 3 => (10, 8),
            2 => (22, 22),
            4 => (6, 0),
            _ => (0, 0),
        };
        if states != 0 {
            let start = image[6] as usize + row[1] as usize;
            row[1] = u16::try_from(out.len()).unwrap();
            out.extend(&image[start..start + states]);
            let start = image[8] as usize + row[2] as usize;
            row[2] = u16::try_from(out.len()).unwrap();
            out.extend(&image[start..start + configs]);
        } else {
            row[1] = 0;
            row[2] = 0;
        }
        out[24 + index * 8..32 + index * 8].copy_from_slice(&row);
    }
    out[5] = u16::try_from(out.len()).unwrap();
    let ids = identity_assets::encode(&world.identity.entities);
    out[6] = ids.len() as u16;
    out.extend(ids);
    out[7] = u16::try_from(out.len()).unwrap();
    out[8] = world.identity.ticks.flags.len() as u16;
    for flags in &world.identity.ticks.flags {
        append(&mut out, *flags as i32);
    }
    out[9] = u16::try_from(out.len()).unwrap();
    let lists = [
        world
            .identity
            .ticks
            .order
            .iter()
            .map(|e| e.0 as i32)
            .collect::<Vec<_>>(),
        world
            .identity
            .queues
            .notifications
            .iter()
            .map(|e| e.0 as i32)
            .collect(),
        world
            .identity
            .queues
            .destruction
            .iter()
            .map(|e| e.0 as i32)
            .collect(),
        world
            .objects
            .components
            .ordinary
            .iter()
            .map(|e| e.0 as i32)
            .collect(),
        world
            .objects
            .components
            .special
            .iter()
            .map(|e| e.0 as i32)
            .collect(),
    ];
    for ids in lists
        .into_iter()
        .chain(rcc_lifecycle_probe::lists(&world.objects.components))
    {
        out.push(ids.len() as u16);
        for id in ids {
            append(&mut out, id);
        }
    }
    out[10] = u16::try_from(out.len()).unwrap();
    for cell in &world.objects.world.grid.cells {
        out.push(cell.len() as u16);
    }
    for id in world.objects.world.grid.cells.iter().flatten() {
        append(&mut out, id.0 as i32);
    }
    out[11] = u16::try_from(out.len()).unwrap();
    let mut changes = 0;
    for index in 0..count {
        let mut current = scene_export::record(&world.objects.world, index);
        // An appended collider with no external basis owns no matrix there.
        // Match the allocation snapshot's canonical absent-Option encoding.
        if index >= baseline.len() && current[58] == 0 && current[59] == 0 {
            current[26..44].fill(0);
        }
        if baseline.get(index) != Some(&current) {
            out.push(index as u16);
            out.extend(current);
            changes += 1;
        }
    }
    out[12] = changes;
    for (i, value) in [
        world.identity.current_slot as i32,
        world.shared.random as i32,
        world.identity.ticks.next.map_or(-1, |e| e.0 as i32),
    ]
    .into_iter()
    .enumerate()
    {
        out[13 + i * 2] = value as u16;
        out[14 + i * 2] = (value >> 16) as u16;
    }
    out[19] = world.registry.generic.0 as u16;
    out[20] = world.registry.obstacle.0 as u16;
    out[2] = u16::try_from(out.len()).unwrap();
    out
}
