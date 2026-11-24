//! One persistent object/floor scene and a camera-oriented corner axis gizmo.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod boot_resources;
use crate::pages::lab::surface_tab::*;
use crate::pages::lab::surface_tab::{LabColor, LabGeometry};
use boot_resources::*;

pub const LAB_FLOOR_Y: fix32 = fix!(-1.1);
pub const LAB_FLOOR_HALF_SIZE: fix32 = fix!(2);
pub const LAB_OBJECT_HALF_SIZE: fix32 = fix!(0.8);

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
            target_x: fix!(0),
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
    pub gloss: GlossPreset,
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
            gloss: GlossPreset::S16,
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
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GlossPreset {
    S4 = 0,
    S8 = 1,
    S16 = 2,
    S32 = 3,
    S64 = 4,
}
impl GlossPreset {
    pub fn shininess(self) -> Shininess {
        if self == Self::S4 {
            return Shininess::S4;
        }
        if self == Self::S8 {
            return Shininess::S8;
        }
        if self == Self::S16 {
            return Shininess::S16;
        }
        if self == Self::S32 {
            return Shininess::S32;
        }
        Shininess::S64
    }
}

pub struct LabScene {
    plane: Mesh,
    rgb_plane: Mesh,
    cube: Mesh,
    rgb_cube: Mesh,
    sphere: Mesh,
    rgb_sphere: Mesh,
    torus: Mesh,
    rgb_torus: Mesh,
    axes: Mesh,
    axes_model: Model,
    axes_camera: Camera,
    axes_state: RenderState,
    light_arrow: Mesh,
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
    white_texture: Texture,
    white_view: TextureView,
    geometry: LabGeometry,
    color: LabColor,
}
impl LabScene {
    pub fn new() -> Self {
        let plane = Mesh::from_native(LAB_PLANE);
        let rgb_plane = Mesh::from_native(LAB_RGB_PLANE);
        let cube = Mesh::from_native(GEOMETRY_CUBE);
        let texture = Texture::from_native(GEOMETRY_CHECKER);
        let mut view = texture.view();
        view.set_address_mode(TextureAddressMode::ClampToEdge);
        let white_texture = Texture::from_native(LAB_WHITE);
        let white_view = white_texture.view();
        Self {
            plane: plane,
            rgb_plane: rgb_plane,
            cube: cube,
            rgb_cube: Mesh::from_native(LAB_RGB_CUBE),
            sphere: Mesh::from_native(LAB_SPHERE),
            rgb_sphere: Mesh::from_native(LAB_RGB_SPHERE),
            torus: Mesh::from_native(LAB_TORUS),
            rgb_torus: Mesh::from_native(LAB_RGB_TORUS),
            axes: Mesh::from_native(LAB_AXES),
            axes_model: Model::new(Transform::identity()),
            axes_camera: Camera::new(CameraDesc::new(
                Vec3::zero(),
                Vec3::zero(),
                Projection::from_focal_pixels(fix!(200), fix!(80)),
            )),
            axes_state: RenderState::new(
                RenderStateDesc::opaque(Lighting::ambient(fix!(1)))
                    .with_depth(DepthFunc::Always, false),
            ),
            light_arrow: Mesh::from_native(LAB_LIGHT_ARROW),
            floor: Model::new(
                Transform::translation(Vec3::new(fix!(0), LAB_FLOOR_Y, fix!(0)))
                    .with_rotation(Vec3::new(fix!(-90).to_radians(), fix!(0), fix!(0)))
                    .with_scale(Vec3::splat(LAB_FLOOR_HALF_SIZE)),
            ),
            object: Model::new(
                Transform::rotation(Vec3::new(fix!(0), fix!(0.4), fix!(0)))
                    .with_scale(Vec3::splat(LAB_OBJECT_HALF_SIZE)),
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
            white_texture: white_texture,
            white_view: white_view,
            geometry: LabGeometry::Sphere,
            color: LabColor::Uniform,
        }
    }
    pub fn configure(&mut self, view: &LabView, light: &LabLight, material: &LabMaterial) {
        let direction = view.direction();
        let yaw = fsincos(view.yaw);
        let pitch = fsincos(view.pitch);
        // Frame the object beside the menu in camera-right coordinates. A fixed
        // world-X offset would move it behind the menu after half an orbit.
        let target = Vec3::new(
            view.target_x + yaw.y() * fix!(1.5),
            view.target_y,
            -yaw.x() * fix!(1.5),
        );
        let eye = Vec3::new(
            target.x + direction.x * view.distance,
            target.y + direction.y * view.distance,
            target.z + direction.z * view.distance,
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
        self.material.set_response(
            fix32::from_int(material.ambient as i16) * fix!(0.01),
            fix32::from_int(material.diffuse as i16) * fix!(0.01),
        );
        self.material.set_specular(
            Rgb8::gray(material.specular as u8),
            material.gloss.shininess(),
        );
        // The mesh points along +Z; this rotation yields the same vector as L.
        self.arrow.set_pose(
            Transform::translation(Vec3::new(fix!(-1.4), fix!(0.4), fix!(0)))
                .with_rotation(Vec3::new(-light.pitch, light.yaw, fix!(0))),
        );
        // Rotation is the same world-to-view basis used by the orbit camera.
        // Translation and projection are fixed, so pan/FOV cannot lose the gizmo.
        let right = Vec4::new(yaw.y(), fix!(0), -yaw.x(), fix!(-3.4));
        let up = Vec4::new(
            -yaw.x() * pitch.x(),
            pitch.y(),
            -yaw.y() * pitch.x(),
            fix!(-1.2),
        );
        let back = Vec4::new(
            yaw.x() * pitch.y(),
            pitch.x(),
            yaw.y() * pitch.y(),
            fix!(-5),
        );
        let world = Mat3x4::from_rows(right, up, back);
        let normal = Mat3x3::identity();
        self.axes_model.set_matrices(world, normal);
    }
    pub fn surface_mode(&mut self, surface: SurfaceTab) {
        self.geometry = surface.geometry;
        self.color = surface.color;
        self.material.set_unlit(!surface.lit);
    }
    fn object_mesh(&self) -> MeshPtr {
        if self.color == LabColor::VertexRgb {
            if self.geometry == LabGeometry::Quad {
                return self.rgb_plane.ptr();
            }
            if self.geometry == LabGeometry::Cube {
                return self.rgb_cube.ptr();
            }
            if self.geometry == LabGeometry::Sphere {
                return self.rgb_sphere.ptr();
            }
            return self.rgb_torus.ptr();
        }
        if self.geometry == LabGeometry::Quad {
            return self.plane.ptr();
        }
        if self.geometry == LabGeometry::Cube {
            return self.cube.ptr();
        }
        if self.geometry == LabGeometry::Sphere {
            return self.sphere.ptr();
        }
        self.torus.ptr()
    }
    pub fn draw(&self, pass: &mut Pass) {
        pass.set_camera(self.camera.ptr());
        pass.set_state(self.state.ptr());
        pass.draw(
            self.plane.ptr(),
            self.floor.ptr(),
            self.floor_material.ptr(),
            self.view.ptr(),
        );
        let mesh = self.object_mesh();
        if self.color == LabColor::VertexRgb {
            pass.draw_colored(mesh, self.object.ptr(), self.material.ptr());
        } else {
            let texture: TextureViewPtr = if self.color == LabColor::Texture {
                self.view.ptr()
            } else {
                self.white_view.ptr()
            };
            pass.draw(mesh, self.object.ptr(), self.material.ptr(), texture);
        }
        // The direction marker remains part of the same scene on every tab.
        pass.draw_colored(
            self.light_arrow.ptr(),
            self.arrow.ptr(),
            self.arrow_material.ptr(),
        );
        pass.set_camera(self.axes_camera.ptr());
        pass.set_state(self.axes_state.ptr());
        pass.draw_colored(
            self.axes.ptr(),
            self.axes_model.ptr(),
            self.arrow_material.ptr(),
        );
    }
}
