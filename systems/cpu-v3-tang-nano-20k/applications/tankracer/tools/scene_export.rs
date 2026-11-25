//! Canonical initial scene collision/class tables, shared by normal owners.
pub fn append_i32(out: &mut Vec<u16>, v: i32) {
    out.extend([v as u16, (v >> 16) as u16]);
}
pub fn record(world: &tankracer_game::obstacle_collision::World, index: usize) -> Vec<u16> {
    let c = &world.colliders[index];
    let o = &world.objects[index];
    let mut out = Vec::new();
    for v in [c.entity.0 as i32]
        .into_iter()
        .chain(c.pose.position)
        .chain(c.pose.matrix.0)
        .chain(
            c.external_basis
                .unwrap_or(tankracer_game::math::Matrix::IDENTITY)
                .0,
        )
        .chain(c.bounds.min)
        .chain(c.bounds.max)
        .chain([
            i32::from(c.radius),
            i32::from(c.external_basis.is_some()),
            o.class.0 as i32,
            o.weight,
            o.threshold,
            o.activity,
            o.flags as i32,
        ])
    {
        append_i32(&mut out, v);
    }
    out.extend(o.mesh_bounds.into_iter().flatten().map(|v| v as u16));
    assert_eq!(out.len(), 76);
    out
}
pub fn encode(world: &tankracer_game::obstacle_collision::World) -> (Vec<u16>, Vec<u16>) {
    let mut words = vec![0u16; 16 + 128 + world.classes.len() * 4];
    let cell_directory = 16;
    let class_directory = 144;
    for (i, c) in world.classes.iter().enumerate() {
        let base = words.len();
        let len = c.methods.len();
        assert!(len < 65535);
        words[class_directory + i * 4] = c.maximum_slot as u16;
        words[class_directory + i * 4 + 1] = (c.maximum_slot >> 16) as u16;
        words[class_directory + i * 4 + 2] = len as u16;
        words[class_directory + i * 4 + 3] = base as u16;
        words.extend(c.methods.iter().map(|m| m.map_or(65535, |m| m.0)));
    }
    for (i, ids) in world.grid.cells.iter().enumerate() {
        words[cell_directory + i * 2] = words.len() as u16;
        words[cell_directory + i * 2 + 1] = ids.len() as u16;
        words.extend(ids.iter().map(|id| id.0 as u16));
    }
    let deform_base = words.len();
    words.extend(world.deformables.iter().map(|id| id.0 as u16));
    while !words.len().is_multiple_of(16) {
        words.push(0);
    }
    let record_base = words.len();
    let mut records = Vec::new();
    for i in 0..world.objects.len() {
        let r = record(world, i);
        words.extend(&r);
        records.extend(r);
    }
    assert!(words.len() <= 65535);
    let len = words.len();
    words[..16].copy_from_slice(&[
        0x5343,
        1,
        len as u16,
        world.objects.len() as u16,
        world.classes.len() as u16,
        record_base as u16,
        class_directory as u16,
        cell_directory as u16,
        deform_base as u16,
        world.deformables.len() as u16,
        world.grid.origin[0] as u16,
        (world.grid.origin[0] >> 16) as u16,
        world.grid.origin[1] as u16,
        (world.grid.origin[1] >> 16) as u16,
        world.grid.shifts[0] as u16,
        world.grid.shifts[1] as u16,
    ]);
    (words, records)
}
