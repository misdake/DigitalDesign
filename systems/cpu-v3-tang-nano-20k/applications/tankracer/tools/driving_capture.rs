//! Canonical diagnostic fields shared by real-map driving owners.
use super::reference;
use tankracer_assets::terrain::TerrainData;
use tankracer_game::{contact_pipeline as ground, motion, pose, powertrain, vehicle_prefix::boost};
#[path = "sampling_snapshot.rs"]
mod sampling_snapshot;

pub fn state(
    m: &motion::State,
    p: &powertrain::State,
    b: &boost::State,
    g: &ground::Pipeline,
    data: &TerrainData,
) -> Vec<i32> {
    let mut out = reference::attitude_state(m);
    out.extend(reference::spring(
        &g.response,
        g.sampling.primary.load,
        g.pivot,
    ));
    out.extend(reference::drive_fixture::flatten(&p.drive_state(m)));
    out.extend(reference::motion_state(m));
    out.extend(reference::boost_state(b));
    for t in m.poses.tracks {
        out.extend(t.matrix.0);
        out.extend(t.position);
    }
    out.extend(sampling_snapshot::values(&g.sampling, data));
    assert_eq!(out.len(), 236);
    out
}
pub fn body_state(m: &motion::State, pose: &pose::Config, gun: &[[i16; 3]]) -> Vec<i32> {
    let a = &m.articulation;
    let l = &m.lean;
    let h = &m.hull;
    let mut out = vec![
        a.recoil,
        a.recoil_rate,
        a.pitch,
        a.pitch_rate,
        l.lateral.position,
        l.lateral.rate,
        l.longitudinal.position,
        l.longitudinal.rate,
    ];
    out.extend(h.position);
    out.extend(h.matrix.0);
    out.extend(h.vertices.into_iter().flatten().map(i32::from));
    out.extend([
        i32::from(h.radius),
        i32::from(m.ready),
        pose.turret_offset[2],
    ]);
    out.extend(gun.iter().flatten().copied().map(i32::from));
    out
}
