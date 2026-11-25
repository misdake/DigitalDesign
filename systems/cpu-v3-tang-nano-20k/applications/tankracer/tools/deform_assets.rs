//! Original live Crush meshes, method configuration and expected mutable state.
use tankracer_game::vehicle_runtime::SinglePlayer;
fn append(out: &mut Vec<u16>, value: i32) {
    out.extend([value as u16, (value >> 16) as u16]);
}
pub fn encode(world: &SinglePlayer) -> Vec<u16> {
    let scene = &world.objects;
    let mut out = vec![0u16; scene.world.deformables.len() * 40];
    for (slot, id) in scene.world.deformables.iter().enumerate() {
        let component = scene.components.surfaces.entries[id.0].as_ref().unwrap();
        let mesh = scene
            .components
            .surfaces
            .meshes
            .get(component.mesh)
            .unwrap();
        assert!(
            component.sampling.state.records.is_empty(),
            "ENGLAND Crush records"
        );
        let vertex_offset = out.len();
        out.extend(mesh.vertices.iter().flatten().map(|v| *v as u16));
        let flag_offset = out.len();
        out.extend(component.vertex_flags.iter().map(|v| u16::from(*v)));
        let mut row = vec![
            id.0 as u16,
            mesh.vertices.len() as u16,
            component.vertex_flags.len() as u16,
            vertex_offset as u16,
            flag_offset as u16,
        ];
        append(&mut row, component.config.floor);
        append(&mut row, component.config.compression);
        row.extend([
            component.config.drift_shift as u16,
            component.config.material as u16,
            u16::from(component.config.has_sound),
        ]);
        for value in component.sampling.bounds.into_iter().flatten() {
            append(&mut row, value);
        }
        append(&mut row, component.sampling.edge_cap);
        let c = &scene.world.colliders[id.0];
        for value in [
            c.bounds.min[0],
            c.bounds.max[0],
            c.bounds.min[2],
            c.bounds.max[2],
        ] {
            append(&mut row, value);
        }
        append(&mut row, i32::from(component.state.dirty));
        append(
            &mut row,
            component.state.last_vehicle.map_or(-1, |v| v.0 as i32),
        );
        append(&mut row, component.state.sound_timer);
        row.extend([
            u16::from(component.sampling.state.dirty),
            u16::from(component.sampling.state.records_attached),
            0,
            0,
        ]);
        assert_eq!(row.len(), 40);
        out[slot * 40..(slot + 1) * 40].copy_from_slice(&row);
    }
    assert!(out.len() <= 32767);
    out
}
pub fn capture(world: &SinglePlayer) -> Vec<i32> {
    let scene = &world.objects;
    let mut out = Vec::new();
    for id in &scene.world.deformables {
        let component = scene.components.surfaces.entries[id.0].as_ref().unwrap();
        out.extend([
            i32::from(component.state.dirty),
            component.state.last_vehicle.map_or(-1, |v| v.0 as i32),
            component.state.sound_timer,
            i32::from(component.sampling.state.dirty),
            i32::from(component.sampling.state.records_attached),
        ]);
        out.extend(
            scene
                .components
                .surfaces
                .meshes
                .get(component.mesh)
                .unwrap()
                .vertices
                .iter()
                .flatten()
                .map(|v| i32::from(*v)),
        );
    }
    out
}
