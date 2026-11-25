//! Constructed ENGLAND mesh resources, compact indices and persistent operations.
use super::mesh_store_snapshot::{payload, resource};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::Path};
use tankracer_assets::model::Model;
use tankracer_game::{
    scene_meshes::{rcc_probe as probe, Meshes},
    vehicle_runtime::SinglePlayer,
};

fn binary(path: &Path, words: &[u16]) {
    let bytes: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    fs::write(path, bytes).unwrap();
}
fn checkpoint(
    meshes: &Meshes,
    keys: &[String],
    tables: &mut Vec<Value>,
    parts: &mut Vec<Vec<u16>>,
) {
    let bindings = probe::bindings(meshes);
    let mut head = vec![meshes.len() as u16, probe::free(meshes).len() as u16];
    head.extend(probe::free(meshes).iter().map(|n| *n as u16));
    head.push(bindings.len() as u16);
    for (name, id) in bindings {
        head.extend([keys.binary_search(&name).unwrap() as u16, id as u16]);
    }
    parts.push(head);
    for i in 0..meshes.len() {
        parts.push(resource(meshes, probe::id(i), tables));
    }
}
fn allocations(meshes: &Meshes, warmed: &[usize]) -> (usize, usize) {
    let mut total = 0;
    let mut allocated = 0;
    for i in 0..meshes.len() {
        let (g, centers, _, _) = probe::inspect(meshes, probe::id(i));
        let (shared, _, _) = probe::metadata(meshes, probe::id(i));
        let span = g.map_or(0, |g| g.vertices.len() * 3 + g.triangles.len())
            + centers.iter().flatten().map(Vec::len).max().unwrap_or(0) * 3;
        total += span;
        if !shared || warmed.contains(&i) {
            allocated += span;
        }
    }
    (total, allocated)
}
pub fn export(world: &mut SinglePlayer, models: &[Model], names: &[String], out: &Path) {
    let meshes = &mut world.objects.components.surfaces.meshes;
    assert!(probe::free(meshes).is_empty());
    let initial_count = meshes.len();
    let (initial_mutable, initial_payload) = allocations(meshes, &[]);
    let initial_bindings = probe::bindings(meshes);
    let mut keys: BTreeSet<String> = initial_bindings.iter().map(|(n, _)| n.clone()).collect();
    keys.extend(names.iter().cloned());
    let keys: Vec<_> = keys.into_iter().collect();
    assert!(initial_count <= 128 && keys.len() <= 128);
    let mut tables = vec![json!([])];
    let mut packets = Vec::new();
    let mut slots = Vec::new();
    for i in 0..initial_count {
        let id = probe::id(i);
        let (geometry, centers, radius, _) = probe::inspect(meshes, id);
        let (_, hint, materials) = probe::metadata(meshes, id);
        let record = resource(meshes, id, &mut tables);
        let model = Model {
            geometry: geometry.unwrap(),
            materials,
            face_centers: centers,
            radius,
            render_hint: hint,
            bounds: [[0; 3]; 2],
            vertex_normals: None,
        };
        packets.push(payload(&model, &mut tables));
        slots.push(i as u16);
        slots.extend(&record[..13]);
    }
    for model in models {
        packets.push(payload(model, &mut tables));
    }
    let source_count = packets.len();
    assert!(source_count <= 128);
    let mut indices: Vec<Vec<u16>> = Vec::new();
    let mut index_offsets = Vec::new();
    let mut index_words = Vec::new();
    let mut source_indices = Vec::new();
    let mut max_span = 0usize;
    for p in &packets {
        let vertices = usize::from(p[8]);
        let triangles = usize::from(p[9]);
        let start = 10 + vertices * 3;
        let column = &p[start..start + triangles * 3];
        let id = match indices.iter().position(|v| v == column) {
            Some(i) => i,
            None => {
                let i = indices.len();
                index_offsets.push(index_words.len());
                index_words.extend_from_slice(column);
                indices.push(column.to_vec());
                i
            }
        };
        source_indices.push(index_offsets[id]);
        max_span = max_span.max(vertices * 3 + triangles + usize::from(p[4]) * 3);
    }
    let source_offset = 16;
    let index_offset = source_offset + source_count * 8;
    let slot_offset = index_offset + index_words.len();
    let name_offset = slot_offset + slots.len();
    let payload_offset = name_offset + initial_bindings.len() * 2;
    let mut image = vec![0; payload_offset];
    let mut sources = Vec::new();
    for (i, p) in packets.iter().enumerate() {
        let offset = image.len();
        sources.extend([
            p[8],
            p[9],
            source_indices[i] as u16,
            p[0],
            offset as u16,
            (offset >> 16) as u16,
            p[4],
            0,
        ]);
        image.extend(p);
    }
    let total = image.len();
    image[..16].copy_from_slice(&[
        0x4d52,
        1,
        initial_count as u16,
        source_count as u16,
        initial_bindings.len() as u16,
        index_words.len() as u16,
        max_span as u16,
        source_offset as u16,
        index_offset as u16,
        slot_offset as u16,
        name_offset as u16,
        payload_offset as u16,
        (payload_offset >> 16) as u16,
        total as u16,
        (total >> 16) as u16,
        keys.len() as u16,
    ]);
    image[source_offset..index_offset].copy_from_slice(&sources);
    image[index_offset..slot_offset].copy_from_slice(&index_words);
    image[slot_offset..name_offset].copy_from_slice(&slots);
    for (i, (name, slot)) in initial_bindings.iter().enumerate() {
        image[name_offset + i * 2] = keys.binary_search(name).unwrap() as u16;
        image[name_offset + i * 2 + 1] = *slot as u16;
    }
    binary(&out.join("mesh-catalog.bin"), &image);
    let mut parts = Vec::new();
    checkpoint(meshes, &keys, &mut tables, &mut parts);
    let first_private = (0..initial_count)
        .find(|i| !probe::metadata(meshes, probe::id(*i)).0)
        .unwrap();
    let restart_ids = [0, first_private, initial_count - 1];
    let restart = vec![
        parts[0].clone(),
        parts[1].clone(),
        parts[1 + first_private].clone(),
        parts[initial_count].clone(),
    ];
    let mut controls = Vec::new();
    let mut copies = Vec::new();
    let mut motion_layout = Vec::new();
    let mut auxiliary_spans = Vec::new();
    let mut motion_words = 0;
    for (id, object) in world.objects.components.objects.iter().enumerate() {
        let Some(object) = object else { continue };
        let Some(auxiliary) = &object.data.auxiliary else {
            continue;
        };
        let name = auxiliary.shape_name.to_ascii_lowercase();
        let source = names.iter().position(|n| *n == name).unwrap();
        let faces = models[source].geometry.triangles.len();
        motion_layout.push([id, faces, motion_words]);
        motion_words += 8 + faces * 16;
        let shared = meshes.bind(&name, &models[source]);
        let private = meshes.private_copy(shared).unwrap();
        let (geometry, centers, _, _) = probe::inspect(meshes, private);
        let g = geometry.as_ref().unwrap();
        let stride = centers.iter().flatten().map(Vec::len).max().unwrap_or(0);
        auxiliary_spans.push(g.vertices.len() * 3 + g.triangles.len() + stride * 3);
        copies.push(private);
        controls.extend([
            0,
            (initial_count + source) as u16,
            keys.binary_search(&name).unwrap() as u16,
            shared.index() as u16,
            private.index() as u16,
            0,
        ]);
    }
    let first = copies[0];
    meshes.get_mut(first).unwrap().vertices[0][0] = -12345;
    let f = meshes.fragments(first).unwrap();
    *f.radius = 5120;
    f.geometry.triangles[0].flags ^= 257;
    f.centers[0].as_mut().unwrap()[0] = f.centers[0].as_ref().unwrap()[0].wrapping_sub(257);
    controls.extend([1, first.index() as u16, (-12345i16) as u16, 5120, 257, 0]);
    let detached = meshes.private_copy(first).unwrap();
    controls.extend([2, first.index() as u16, detached.index() as u16, 0, 0, 0]);
    meshes.release_private(first).unwrap();
    controls.extend([3, first.index() as u16, 0, 0, 0, 0]);
    let recycled = meshes.private_copy(detached).unwrap();
    assert_eq!(recycled, first);
    controls.extend([2, detached.index() as u16, recycled.index() as u16, 0, 0, 0]);
    meshes.get_mut(probe::id(0)).unwrap().vertices[0][0] = 23456;
    controls.extend([4, 0, 23456, 0, 0, 0]);
    let copied = meshes.private_copy(probe::id(0)).unwrap();
    controls.extend([2, 0, copied.index() as u16, 0, 0, 0]);
    checkpoint(meshes, &keys, &mut tables, &mut parts);
    let (mutable_words, final_payload) = allocations(meshes, &[0]);
    let mut external_blocks = 1;
    for i in initial_count..meshes.len() {
        if !probe::metadata(meshes, probe::id(i)).0 {
            external_blocks += 1;
        }
    }
    parts.extend(restart);
    let expected: Vec<u16> = parts
        .iter()
        .flat_map(|p| std::iter::once(p.len() as u16).chain(p.iter().copied()))
        .collect();
    binary(&out.join("mesh-catalog-controls.bin"), &controls);
    binary(&out.join("mesh-catalog-expected.bin"), &expected);
    fs::write(out.join("mesh-catalog.json"), serde_json::to_vec(&json!({
        "initial_slots":initial_count,"final_slots":meshes.len(),"sources":source_count,
        "names":keys,"material_tables":tables,
        "auxiliary_sources":names.iter().enumerate().map(|(i,n)|(n.clone(),initial_count+i)).collect::<std::collections::BTreeMap<_,_>>(),
        "operations":controls.len()/6,"auxiliary_copies":copies.len(),
        "motion_layout":motion_layout,"motion_words":motion_words,"auxiliary_spans":auxiliary_spans,
        "checkpoints":3,"restart_resources":restart_ids,"parts":parts.len(),"checked_words":parts.iter().map(Vec::len).sum::<usize>(),
        "maximum_part_words":parts.iter().map(Vec::len).max(),"maximum_span_words":max_span,
        "compact_index_bytes":index_words.len()*2,"source_metadata_bytes":source_count*12,
        "initial_mutable_bytes":initial_mutable*2,"initial_payload_bytes":initial_payload*2,
        "final_mutable_bytes":mutable_words*2,"final_payload_bytes":final_payload*2,"catalog_bytes":image.len()*2,
        "external_payload_bytes":(final_payload-initial_payload)*2,"external_blocks":external_blocks,
        "scope":"All constructed ENGLAND resource slots and name bindings, all authored auxiliary copies, detached current geometry and slot reuse"
    })).unwrap()).unwrap();
}
