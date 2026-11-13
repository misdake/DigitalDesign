//! Actual GPU geometry: fixed nonuniform matrices beside a live rotating TRS model.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod state;
use state::*;
mod ui;
use ui::*;

const FIXED_POSE: Transform = Transform::translation(Vec3::new(fix!(1.4), fix!(0), fix!(-4.5)))
    .with_rotation(Vec3::new(
        fix!(-20).to_radians(),
        fix!(30).to_radians(),
        fix!(0),
    ))
    .with_scale(Vec3::new(fix!(0.7), fix!(1), fix!(0.7)));
pub const FIXED_WORLD: Mat3x4 = FIXED_POSE.world_matrix();
pub const FIXED_NORMAL: Mat3x3 = FIXED_POSE.normal_matrix();
pub const GEOMETRY_PROJECTION: Projection = Projection::from_focal_pixels(fix!(200), fix!(199));

fn geometry_update(model: &mut Model, camera: &mut Camera, state: DemoState) {
    let angle = animation_phase(state);
    model.set_pose(
        Transform::translation(Vec3::new(fix!(-1.4), fix!(0), fix!(-4.5)))
            .with_rotation(Vec3::new(fix!(-20).to_radians(), angle, fix!(0)))
            .with_scale(Vec3::splat(fix!(0.7))),
    );
    camera.set_projection(Projection::from_focal_pixels(
        fix32::from_int(160 + state.parameter as i16 * 5),
        fix!(199),
    ));
}
fn geometry_labels(grid: &mut UiGrid, state: DemoState) {
    page_start(grid, state, b"GEOMETRY");
    text(grid, 10, 12, b"STATIC");
    text(grid, 30, 12, b"ROTATING");
}

pub fn geometry_draw(
    pass: &mut Pass,
    mesh: &Mesh,
    fixed: &Model,
    rotating: &Model,
    material: &Material,
    view: &TextureView,
) {
    pass.draw(mesh, fixed, material, view);
    pass.draw(mesh, rotating, material, view);
}

pub fn geometry_fixed_model() -> Model {
    Model::from_matrices(FIXED_WORLD, FIXED_NORMAL)
}
pub fn geometry_rotating_model() -> Model {
    Model::new(Transform::identity())
}
pub fn geometry_camera() -> Camera {
    Camera::new(CameraDesc::new(
        Vec3::zero(),
        Vec3::zero(),
        GEOMETRY_PROJECTION,
    ))
}
pub fn geometry_render_state() -> RenderState {
    RenderState::new(RenderStateDesc::opaque(Lighting::directional(
        Vec3::new(fix!(-0.4), fix!(0.5), fix!(1)),
        fix!(0.35),
        fix!(0.65),
    )))
}

pub fn geometry_prepare(
    rotating: &mut Model,
    camera: &mut Camera,
    grid: &mut UiGrid,
    state: DemoState,
) {
    geometry_update(rotating, camera, state);
    geometry_labels(grid, state);
}
pub fn geometry_pass_state(pass: &mut Pass, camera: &Camera, state: &RenderState) {
    pass.set_camera(camera);
    pass.set_state(state);
}
