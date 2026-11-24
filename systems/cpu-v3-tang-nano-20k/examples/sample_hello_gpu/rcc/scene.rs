//! Ordinary module encapsulation; Ptr values refer to these live resource bodies.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;

pub struct Scene {
    mesh: Mesh,
    model: Model,
    camera: Camera,
    material: Material,
    state: RenderState,
}
impl Scene {
    pub fn new() -> Self {
        Self {
            mesh: Mesh::from_native(HELLO_TRIANGLE),
            model: Model::new(Transform::translation(Vec3::new(
                fix!(0),
                fix!(0),
                fix!(-4),
            ))),
            camera: Camera::new(CameraDesc::new(
                Vec3::zero(),
                Vec3::zero(),
                Projection::from_focal_pixels(fix!(220), fix!(80)),
            )),
            material: Material::new(MaterialDesc::diffuse(Rgba8::white()).with_unlit(true)),
            state: RenderState::new(RenderStateDesc::opaque(Lighting::ambient(fix!(1)))),
        }
    }
    pub fn draw(&self, pass: &mut Pass) {
        pass.set_camera(self.camera.ptr());
        pass.set_state(self.state.ptr());
        pass.draw_colored(self.mesh.ptr(), self.model.ptr(), self.material.ptr());
    }
}
