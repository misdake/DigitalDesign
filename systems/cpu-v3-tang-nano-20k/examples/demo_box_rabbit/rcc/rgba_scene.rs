//! Persistent resources; recording selects opaque/transparent state without mutation.
use crate::boot_resources::{DEMO_ALPHA_SECTOR, DEMO_ALPHA_SECTORS};
use crate::dsl_rt::*;
use crate::gpu_api::{Camera, Material, Mesh, Pass, RenderState, Texture, TextureView, Transform};
use crate::gpu_demo_api::{demo_mesh, demo_texture};

const ROOM_FIRST: u16 = 0;
const ROOM_COUNT: u16 = 6;
const RGB_FIRST: u16 = 6;
const RABBIT_FIRST: u16 = 7;
const RABBIT_COUNT: u16 = 17;
const PANEL_FIRST: u16 = 24;

pub struct RgbaScene {
    mesh: Mesh,
    model: Transform,
    full_panel: Transform,
    half_panel: Transform,
    unlit: Material,
    lit: Material,
    half: Material,
    // Keep source owners alive alongside their persistent sampling views.
    brick: Texture,
    brick_view: TextureView,
    alpha: Texture,
    alpha_view: TextureView,
    opaque: RenderState,
    transparent: RenderState,
}

impl RgbaScene {
    pub fn new() -> Self {
        let brick = demo_texture();
        let brick_view = brick.view();
        let alpha = Texture::from_native(TextureDesc::from_memory(
            unsafe { GpuMemory::from_sectors(DEMO_ALPHA_SECTOR, DEMO_ALPHA_SECTORS) },
            TextureLayout::square(128, TextureFormat::Rgba4444, Mipmaps::Full),
        ));
        let mut alpha_view = alpha.view();
        alpha_view.set_address_mode(TextureAddressMode::ClampToEdge);
        let light = Lighting::directional(
            Vec3::new(fix!(-0.45), fix!(0.8), fix!(1.1)),
            fix!(1),
            fix!(1),
        );
        let opaque = RenderStateDesc::opaque(light);
        let transparent = RenderStateDesc::transparent(light);
        let unlit = MaterialDesc::lit(Rgba8::white(), Rgb8::black(), Shininess::S16)
            .with_response(fix!(0.5), fix!(0.5))
            .with_unlit(true);
        Self {
            mesh: demo_mesh(),
            model: Transform::new(Pose::identity()),
            full_panel: Transform::new(Pose::translation(Vec3::new(
                fix!(-1.05),
                fix!(-0.6),
                fix!(1),
            ))),
            half_panel: Transform::new(Pose::translation(Vec3::new(
                fix!(0.05),
                fix!(-0.6),
                fix!(1),
            ))),
            unlit: Material::new(unlit),
            lit: Material::new(
                MaterialDesc::lit(Rgba8::white(), Rgb8::new(160, 160, 160), Shininess::S32)
                    .with_response(fix!(0.25), fix!(0.75)),
            ),
            half: Material::new(
                MaterialDesc::lit(
                    Rgba8::new(255, 255, 255, 128),
                    Rgb8::black(),
                    Shininess::S16,
                )
                .with_response(fix!(0.5), fix!(0.5))
                .with_unlit(true),
            ),
            brick: brick,
            brick_view: brick_view,
            alpha: alpha,
            alpha_view: alpha_view,
            opaque: RenderState::new(opaque),
            transparent: RenderState::new(transparent),
        }
    }

    pub fn draw(&self, pass: &mut Pass, camera: &Camera) {
        let mesh = self.mesh.ptr();
        let room = mesh.slice(ROOM_FIRST, ROOM_COUNT);
        let rgb = mesh.slice(RGB_FIRST, 1);
        let rabbit = mesh.slice(RABBIT_FIRST, RABBIT_COUNT);
        pass.set_camera(camera.ptr());
        pass.set_state(self.opaque.ptr());
        pass.draw(self.model.ptr(), room, self.unlit.ptr(), &self.brick_view);
        pass.draw_colored(self.model.ptr(), rgb, self.unlit.ptr());
        pass.draw(self.model.ptr(), rabbit, self.lit.ptr(), &self.brick_view);
        // Transparent geometry follows all opaque draws and never writes depth.
        pass.set_state(self.transparent.ptr());
        let panel = mesh.slice(PANEL_FIRST, 1);
        pass.draw(
            self.full_panel.ptr(),
            panel,
            self.unlit.ptr(),
            &self.alpha_view,
        );
        pass.draw(
            self.half_panel.ptr(),
            panel,
            self.half.ptr(),
            &self.alpha_view,
        );
    }
}
