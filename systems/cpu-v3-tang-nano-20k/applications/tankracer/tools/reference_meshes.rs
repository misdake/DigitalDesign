//! Qualification-only readout of resource slots, including retired payloads.
use super::*;
pub fn inspect(meshes: &Meshes, id: MeshId) -> (Option<Geometry>, [Option<Vec<i16>>; 3], u16, u64) {
    let slot = &meshes.entries[id.0];
    (
        slot.geometry.clone(),
        slot.centers.clone(),
        slot.radius,
        slot.version,
    )
}
pub fn version(meshes: &mut Meshes, id: MeshId, value: u64) {
    meshes.entries[id.0].version = value;
}
pub fn id(index: usize) -> MeshId {
    MeshId(index)
}
pub fn metadata(meshes: &Meshes, id: MeshId) -> (bool, u16, Vec<SourceMaterial>) {
    let slot = &meshes.entries[id.0];
    (slot.shared, slot.hint, slot.materials.to_vec())
}
pub fn free(meshes: &Meshes) -> &[usize] {
    &meshes.free
}
pub fn bound(meshes: &Meshes, name: &str) -> Option<MeshId> {
    meshes.names.get(&name.to_ascii_lowercase()).copied()
}

pub fn bindings(meshes: &Meshes) -> Vec<(String, usize)> {
    let mut names = meshes
        .names
        .iter()
        .map(|(name, id)| (name.clone(), id.0))
        .collect::<Vec<_>>();
    names.sort();
    names
}
