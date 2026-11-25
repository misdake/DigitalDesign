//! Ordinary module encapsulation; Ptr values refer to these live resource bodies.
use crate::boot_resources::HELLO_TRIANGLE;
use crate::dsl_rt::*;
use crate::gpu_api::{Camera, Material, Mesh, Pass, RenderState, Transform};

struct Model {
    transform: Transform,
    mesh: Mesh,
    material: Material,
}

pub struct Scene {
    model: Model,
    camera: Camera,
    state: RenderState,
}
impl Scene {
    pub fn new() -> Self {
        Self {
            model: Model {
                transform: Transform::new(Pose::translation(Vec3::new(fix!(0), fix!(0), fix!(-4)))),
                mesh: Mesh::from_native(HELLO_TRIANGLE),
                material: Material::new(MaterialDesc::diffuse(Rgba8::white()).with_unlit(true)),
            },
            camera: Camera::new(CameraDesc::new(
                Vec3::zero(),
                Vec3::zero(),
                Projection::from_focal_pixels(fix!(220), fix!(80)),
            )),
            state: RenderState::new(RenderStateDesc::opaque(Lighting::ambient(fix!(1)))),
        }
    }
    pub fn draw(&self, pass: &mut Pass) {
        pass.set_camera(self.camera.ptr());
        pass.set_state(self.state.ptr());
        pass.draw_colored(
            self.model.transform.ptr(),
            self.model.mesh.ptr(),
            self.model.material.ptr(),
        );
    }
}
