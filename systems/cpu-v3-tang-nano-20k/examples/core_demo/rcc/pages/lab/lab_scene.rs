//! A persistent plane/cube scene shared by the four application tabs.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;

pub const LAB_FLOOR_Y: fix32 = fix!(-1.1);
pub const LAB_FLOOR_HALF_SIZE: fix32 = fix!(2);
pub const LAB_CUBE_HALF_SIZE: fix32 = fix!(0.8);

#[derive(Copy, Clone)]
pub struct LabView {
    pub yaw: fix32,
    pub pitch: fix32,
    pub distance: fix32,
    pub target_x: fix32,
    pub target_y: fix32,
    pub fov: u16,
}
impl LabView {
    pub const fn new() -> Self {
        Self {
            yaw: fix!(0),
            pitch: fix!(0.3),
            distance: fix!(6),
            target_x: fix!(1.5),
            target_y: fix!(0),
            fov: 60,
        }
    }
    pub fn direction(self) -> Vec3 {
        spherical_direction(self.yaw, self.pitch)
    }
}
#[derive(Copy, Clone)]
pub struct LabLight {
    pub yaw: fix32,
    pub pitch: fix32,
    pub ambient: fix32,
    pub directional: fix32,
}
impl LabLight {
    pub const fn new() -> Self {
        Self {
            yaw: fix!(-0.5),
            pitch: fix!(0.7),
            ambient: fix!(0.2),
            directional: fix!(0.8),
        }
    }
    pub const fn surface() -> Self {
        Self {
            yaw: fix!(-0.15),
            pitch: fix!(0.45),
            ambient: fix!(0.35),
            directional: fix!(0.75),
        }
    }
    pub fn direction(self) -> Vec3 {
        spherical_direction(self.yaw, self.pitch)
    }
}
#[derive(Copy, Clone)]
pub struct LabMaterial {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
    pub ambient: u16,
    pub diffuse: u16,
    pub specular: u16,
    pub gloss: u16,
}
impl LabMaterial {
    pub const fn new() -> Self {
        Self {
            red: 160,
            green: 220,
            blue: 255,
            ambient: 100,
            diffuse: 100,
            specular: 160,
            gloss: 2,
        }
    }
}
fn spherical_direction(yaw: fix32, pitch: fix32) -> Vec3 {
    let azimuth = fsincos(yaw);
    let elevation = fsincos(pitch);
    Vec3::new(
        azimuth.x() * elevation.y(),
        elevation.x(),
        azimuth.y() * elevation.y(),
    )
}
fn gloss_exponent(index: u16) -> Shininess {
    if index == 0 {
        return Shininess::S4;
    }
    if index == 1 {
        return Shininess::S8;
    }
    if index == 2 {
        return Shininess::S16;
    }
    if index == 3 {
        return Shininess::S32;
    }
    Shininess::S64
}

pub struct LabScene {
    plane: Mesh,
    rgb_plane: Mesh,
    cube: Mesh,
    light_arrow: Mesh,
    surface: Model,
    floor: Model,
    object: Model,
    arrow: Model,
    material: Material,
    floor_material: Material,
    arrow_material: Material,
    camera: Camera,
    state: RenderState,
    view: TextureView,
    texture: Texture,
    composed: bool,
    textured: bool,
    show_light: bool,
}
impl LabScene {
    pub fn new() -> Self {
        let plane = Mesh::from_native(LAB_PLANE);
        let rgb_plane = Mesh::from_native(LAB_RGB_PLANE);
        let cube = Mesh::from_native(GEOMETRY_CUBE);
        let texture = Texture::from_native(GEOMETRY_CHECKER);
        let mut view = texture.view();
        view.set_address_mode(TextureAddressMode::ClampToEdge);
        Self {
            plane: plane,
            rgb_plane: rgb_plane,
            cube: cube,
            light_arrow: Mesh::from_native(LAB_LIGHT_ARROW),
            surface: Model::new(
                Transform::translation(Vec3::new(fix!(0), fix!(-0.25), fix!(0)))
                    .with_rotation(Vec3::new(fix!(0.15), fix!(-0.3), fix!(0)))
                    .with_scale(Vec3::splat(fix!(1.6))),
            ),
            floor: Model::new(
                Transform::translation(Vec3::new(fix!(0), LAB_FLOOR_Y, fix!(0)))
                    .with_rotation(Vec3::new(fix!(-90).to_radians(), fix!(0), fix!(0)))
                    .with_scale(Vec3::splat(LAB_FLOOR_HALF_SIZE)),
            ),
            object: Model::new(
                Transform::rotation(Vec3::new(fix!(0), fix!(0.4), fix!(0)))
                    .with_scale(Vec3::splat(LAB_CUBE_HALF_SIZE)),
            ),
            arrow: Model::new(Transform::identity()),
            material: Material::new(MaterialDesc::diffuse(Rgba8::white())),
            floor_material: Material::new(MaterialDesc::diffuse(Rgba8::gray(180))),
            arrow_material: Material::new(MaterialDesc::unlit(Rgba8::white())),
            camera: Camera::new(CameraDesc::new(
                Vec3::zero(),
                Vec3::zero(),
                Projection::from_focal_pixels(fix!(220), fix!(80)),
            )),
            state: RenderState::new(RenderStateDesc::opaque(Lighting::ambient(fix!(1)))),
            texture: texture,
            view: view,
            composed: false,
            textured: false,
            show_light: false,
        }
    }
    pub fn configure(&mut self, view: &LabView, light: &LabLight, material: &LabMaterial) {
        let direction = view.direction();
        let target = Vec3::new(view.target_x, view.target_y, fix!(0));
        let eye = Vec3::new(
            target.x + direction.x * view.distance,
            target.y + direction.y * view.distance,
            direction.z * view.distance,
        );
        self.camera.set_look_at_y_up(eye, target);
        self.camera.set_projection(Projection::from_vertical_fov(
            fix32::from_int(view.fov as i16).to_radians(),
            fix!(80),
        ));
        self.state.set_lighting(Lighting::directional(
            light.direction(),
            light.ambient,
            light.directional,
        ));
        self.material.set_uniform_color(Rgba8::rgb(
            material.red as u8,
            material.green as u8,
            material.blue as u8,
        ));
        self.material.set_unlit(false);
        self.material.set_response(
            fix32::from_int(material.ambient as i16) * fix!(0.01),
            fix32::from_int(material.diffuse as i16) * fix!(0.01),
        );
        self.material.set_specular(
            Rgb8::gray(material.specular as u8),
            gloss_exponent(material.gloss),
        );
        // The mesh points along +Z; this rotation yields the same vector as L.
        self.arrow.set_pose(
            Transform::translation(Vec3::new(fix!(-1.4), fix!(0.4), fix!(0)))
                .with_rotation(Vec3::new(-light.pitch, light.yaw, fix!(0))),
        );
        self.composed = true;
        self.textured = true;
        self.show_light = false;
    }
    pub fn show_light_direction(&mut self) {
        self.show_light = true;
    }
    pub fn surface_mode(&mut self, mode: u16) {
        self.composed = false;
        self.textured = mode == 1 || mode == 3;
        self.material.set_uniform_color(Rgba8::white());
        if mode >= 2 {
            self.material.set_specular(Rgb8::gray(96), Shininess::S16);
        } else {
            self.material.set_specular(Rgb8::black(), Shininess::S4);
        }
        self.material.set_unlit(mode < 2);
    }
    pub fn draw(&self, pass: &mut Pass) {
        pass.set_camera(self.camera.ptr());
        pass.set_state(self.state.ptr());
        if self.composed {
            pass.draw(
                self.plane.ptr(),
                self.floor.ptr(),
                self.floor_material.ptr(),
                self.view.ptr(),
            );
            pass.draw(
                self.cube.ptr(),
                self.object.ptr(),
                self.material.ptr(),
                self.view.ptr(),
            );
            if self.show_light {
                pass.draw_colored(
                    self.light_arrow.ptr(),
                    self.arrow.ptr(),
                    self.arrow_material.ptr(),
                );
            }
        } else if self.textured {
            pass.draw(
                self.plane.ptr(),
                self.surface.ptr(),
                self.material.ptr(),
                self.view.ptr(),
            );
        } else {
            pass.draw_colored(
                self.rgb_plane.ptr(),
                self.surface.ptr(),
                self.material.ptr(),
            );
        }
    }
}
