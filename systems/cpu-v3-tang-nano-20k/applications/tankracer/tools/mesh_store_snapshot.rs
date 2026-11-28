//! Complete immutable and mutable resource snapshots for source qualification.
use serde_json::{json, Value};
use tankracer_assets::{
    mesh::{Geometry, SourceMaterial},
    model::Model,
};
use tankracer_game::scene_meshes::{rcc_probe as probe, Meshes};
fn wide(out: &mut Vec<u16>, value: u32) {
    out.extend([value as u16, (value >> 16) as u16]);
}
fn geometry(g: &Geometry) -> Vec<u16> {
    let mut out = vec![g.vertices.len() as u16, g.triangles.len() as u16];
    out.extend(g.vertices.iter().flatten().map(|v| *v as u16));
    out.extend(g.triangles.iter().flat_map(|t| t.indices.map(u16::from)));
    out.extend(g.triangles.iter().map(|t| t.flags));
    for t in &g.triangles {
        wide(&mut out, t.source_offset);
        out.extend(t.normal.map(|v| v as u16));
        out.extend([t.color, u16::from(t.sides)]);
        out.extend(t.uv.map(|uv| u16::from_le_bytes(uv)));
        wide(&mut out, t.material.map_or(u32::MAX, |m| m.index() as u32));
        wide(&mut out, t.material_source_offset);
        if let Some(b) = &t.billboard {
            out.extend([
                1,
                b.center[0] as u16,
                b.center[1] as u16,
                b.half_width,
                u16::from(b.positive_vertices),
            ]);
        } else {
            out.extend([0; 5]);
        }
    }
    assert_eq!(out.len(), 2 + g.vertices.len() * 3 + g.triangles.len() * 23);
    out
}
fn material_value(materials: &[SourceMaterial]) -> Value {
    json!(materials
        .iter()
        .map(|m| (&m.name, &m.spellings))
        .collect::<Vec<_>>())
}
fn material_key(materials: &[SourceMaterial], tables: &mut Vec<Value>) -> u16 {
    let value = material_value(materials);
    if let Some(i) = tables.iter().position(|v| *v == value) {
        return i as u16;
    }
    tables.push(value);
    (tables.len() - 1) as u16
}
pub fn payload(model: &Model, tables: &mut Vec<Value>) -> Vec<u16> {
    let g = geometry(&model.geometry);
    let stride = model
        .face_centers
        .iter()
        .flatten()
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    let lengths = model
        .face_centers
        .each_ref()
        .map(|c| c.as_ref().map_or(65535, |c| c.len() as u16));
    let mut out = vec![
        g.len() as u16,
        model.render_hint,
        model.radius,
        material_key(&model.materials, tables),
        stride as u16,
    ];
    out.extend(lengths);
    out.extend(g);
    for column in &model.face_centers {
        for i in 0..stride {
            out.push(column.as_ref().and_then(|c| c.get(i)).copied().unwrap_or(0) as u16);
        }
    }
    out
}
pub fn snapshot(
    meshes: &Meshes,
    result: i32,
    tables: &mut Vec<Value>,
    names: [&str; 2],
) -> Vec<u16> {
    let mut out = Vec::new();
    wide(&mut out, result as u32);
    out.extend([meshes.len() as u16, probe::free(meshes).len() as u16]);
    out.extend(probe::free(meshes).iter().map(|v| *v as u16));
    let bindings = [
        probe::bound(meshes, names[0]),
        probe::bound(meshes, names[1]),
    ];
    out.push(bindings.iter().filter(|b| b.is_some()).count() as u16);
    out.extend(bindings.map(|b| b.map_or(65535, |id| id.index() as u16)));
    for i in 0..meshes.len() {
        out.extend(resource(meshes, probe::id(i), tables));
    }
    out
}

pub fn resource(
    meshes: &Meshes,
    id: tankracer_game::scene_meshes::MeshId,
    tables: &mut Vec<Value>,
) -> Vec<u16> {
    let mut out = Vec::new();
    let (g, centers, radius, revision) = probe::inspect(meshes, id);
    let (shared, hint, materials) = probe::metadata(meshes, id);
    let g = g.as_ref().map(geometry).unwrap_or_default();
    let lengths = centers
        .each_ref()
        .map(|c| c.as_ref().map_or(65535, |c| c.len() as u16));
    let stride = centers.iter().flatten().map(Vec::len).max().unwrap_or(0) as u16;
    out.extend([
        u16::from(!g.is_empty()) | (u16::from(shared) << 1),
        hint,
        radius,
        material_key(&materials, tables),
    ]);
    wide(&mut out, revision as u32);
    wide(&mut out, (revision >> 32) as u32);
    out.push(g.len() as u16);
    out.extend(lengths);
    out.push(stride);
    out.extend(g);
    for column in centers.into_iter().flatten() {
        out.extend(column.into_iter().map(|v| v as u16));
    }
    out
}
