//! Actual GPU geometry: fixed nonuniform matrices beside a live rotating TRS model.
use crate::dsl_rt::*;
mod gpu_api;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;

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

fn geometry_update(model: &mut Model, camera: &mut Camera, state: AnimationState) {
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
fn geometry_labels(grid: &mut UiGrid, state: AnimationState) {
    page_start(grid, PageId::Geometry, state.paused, b"GEOMETRY");
    text(grid, 10, 12, b"STATIC");
    text(grid, 30, 12, b"ROTATING");
}

pub struct GeometryPage {
    view: TextureView,
    texture: Texture,
    mesh: Mesh,
    fixed: Model,
    rotating: Model,
    camera: Camera,
    material: Material,
    render_state: RenderState,
}
impl GeometryPage {
    pub fn new() -> Self {
        let mesh = Mesh::from_native(GEOMETRY_CUBE);
        let texture = Texture::from_native(GEOMETRY_CHECKER);
        let mut view = texture.view();
        view.set_address_mode(TextureAddressMode::ClampToEdge);
        let fixed = Model::from_matrices(FIXED_WORLD, FIXED_NORMAL);
        let rotating = Model::new(Transform::identity());
        let camera = Camera::new(CameraDesc::new(
            Vec3::zero(),
            Vec3::zero(),
            GEOMETRY_PROJECTION,
        ));
        let material = Material::new(MaterialDesc::diffuse(Rgba8::white()));
        let render_state = RenderState::new(RenderStateDesc::opaque(Lighting::directional(
            Vec3::new(fix!(-0.4), fix!(0.5), fix!(1)),
            fix!(0.35),
            fix!(0.65),
        )));
        Self {
            view: view,
            texture: texture,
            mesh: mesh,
            fixed: fixed,
            rotating: rotating,
            camera: camera,
            material: material,
            render_state: render_state,
        }
    }
    pub fn prepare(&mut self, grid: &mut UiGrid, state: AnimationState) {
        geometry_update(&mut self.rotating, &mut self.camera, state);
        geometry_labels(grid, state);
    }
    pub fn draw(&self, pass: &mut Pass) {
        pass.set_camera(self.camera.ptr());
        pass.set_state(self.render_state.ptr());
        pass.draw(
            self.mesh.ptr(),
            self.fixed.ptr(),
            self.material.ptr(),
            self.view.ptr(),
        );
        pass.draw(
            self.mesh.ptr(),
            self.rotating.ptr(),
            self.material.ptr(),
            self.view.ptr(),
        );
    }
}
