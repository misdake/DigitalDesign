//! Authored parent bindings; model source and name keys are resolved by packing.
use serde_json::{json, Value};
use tankracer_game::vehicle_runtime::SinglePlayer;

pub fn configuration(world: &SinglePlayer) -> Vec<Value> {
    let c = &world.objects.components;
    let mut rows = Vec::new();
    for (id, object) in c.objects.iter().enumerate() {
        let Some(object) = object else { continue };
        let config = object.data.auxiliary.as_ref();
        let fall = c.falls.get(id).and_then(Option::as_ref);
        let closer = c.closers.get(id).and_then(Option::as_ref);
        if (fall.is_none() && closer.is_none())
            || (config.is_none()
                && object.data.debris.is_none()
                && fall.is_none_or(|f| f.attached.is_none()))
        {
            continue;
        }
        let mut gains = vec![0; 10];
        if let Some(config) = config {
            gains.clear();
            for gain in [
                config.bounce,
                config.friction,
                config.push,
                config.velocity,
                config.spin,
            ] {
                gains.extend([gain.multiplier, u16::from(gain.shift)]);
            }
        }
        rows.push(json!({"object":id,"kind":if fall.is_some(){1}else{2},
            "name":config.map_or("", |c| c.shape_name.as_str()),"gravity":config.map_or(0,|c|c.gravity),"gains":gains,
            "attached_count":fall.and_then(|f|f.attached.as_ref()).map_or(0,|a|a.len()),
            "current_mesh":object.data.mesh.map_or(-1, |m|m.index() as i32),
            "private_mesh":object.auxiliary_mesh.map_or(-1, |m|m.index() as i32),
            "active":object.auxiliary_active,
            "motion_count":object.auxiliary_motion.as_ref().map_or(0, |m|m.faces.len()),
            "reference_alias":closer.is_some_and(|c|c.reference_alias)}));
    }
    rows
}

fn wide(out: &mut Vec<u16>, value: i32) {
    out.extend([value as u16, (value >> 16) as u16]);
}
pub fn capture(world: &SinglePlayer, bindings: &[Value]) -> Vec<u16> {
    use tankracer_game::scene_meshes::rcc_probe as probe;
    let meshes = &world.objects.components.surfaces.meshes;
    let mut out = vec![0, meshes.len() as u16, probe::free(meshes).len() as u16];
    out.extend(probe::free(meshes).iter().map(|id| *id as u16));
    out.push(probe::bindings(meshes).len() as u16);
    for binding in bindings {
        out.push(
            probe::bound(meshes, binding["name"].as_str().unwrap())
                .map_or(65535, |id| id.index() as u16),
        );
    }
    for binding in bindings {
        let id = binding["object"].as_u64().unwrap() as usize;
        let object = world.objects.components.objects[id].as_ref();
        out.extend([id as u16, u16::from(object.is_some())]);
        let Some(object) = object else { continue };
        out.push(object.auxiliary_active as u16);
        for mesh in [object.data.mesh, object.auxiliary_mesh] {
            wide(&mut out, mesh.map_or(-1, |id| id.index() as i32));
        }
        out.push(
            object
                .auxiliary_motion
                .as_ref()
                .map_or(0, |m| m.faces.len()) as u16,
        );
        if let Some(motion) = &object.auxiliary_motion {
            for value in [
                motion.retired as i32,
                motion.ground_height,
                i32::from(motion.prepared),
                motion.lifetime,
            ] {
                wide(&mut out, value);
            }
            for face in &motion.faces {
                for value in face
                    .velocity
                    .into_iter()
                    .chain(face.angular)
                    .chain([face.life])
                {
                    wide(&mut out, value);
                }
                out.extend([face.extra_vertex as u16, face.mode]);
            }
        }
        if let Some(id) = object.auxiliary_mesh {
            let (geometry, centers, radius, version) = probe::inspect(meshes, id);
            let (shared, hint, _) = probe::metadata(meshes, id);
            out.extend([u16::from(shared), hint, radius]);
            for shift in [0, 16, 32, 48] {
                out.push((version >> shift) as u16);
            }
            let g = geometry.as_ref().unwrap();
            out.extend([g.vertices.len() as u16, g.triangles.len() as u16]);
            out.extend(g.vertices.iter().flatten().map(|v| *v as u16));
            out.extend(g.triangles.iter().map(|t| t.flags));
            for column in &centers {
                out.push(column.as_ref().map_or(65535, |c| c.len() as u16));
                if let Some(column) = column {
                    out.extend(column.iter().map(|v| *v as u16));
                }
            }
        }
    }
    out[0] = u16::try_from(out.len()).unwrap();
    out
}
