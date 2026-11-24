//! Four persistent control tabs sharing one ordinary GPU scene.
use crate::dsl_rt::*;
mod gpu_api;
use crate::common::state::*;
use crate::common::ui::*;
use gpu_api::*;
mod boot_resources;
use boot_resources::*;
mod gamepad;
use crate::pages::lab::camera_tab::*;
use crate::pages::lab::lab_scene::*;
use crate::pages::lab::light_tab::*;
use crate::pages::lab::material_tab::*;
use crate::pages::lab::surface_tab::*;
use gamepad::*;

#[repr(u16)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum LabTab {
    Surface,
    Camera,
    Light,
    Material,
}
impl LabTab {
    pub fn from_index(index: u16) -> Self {
        if index >= 4 {
            return Self::Surface;
        }
        // All values in the guarded range are declared variants.
        unsafe { enum_from_u16_unchecked::<Self>(index) }
    }
}
pub struct GpuLab {
    scene: LabScene,
    surface: SurfaceTab,
    camera: CameraTab,
    light: LightTab,
    material: MaterialTab,
    tab: LabTab,
    previous: u16,
    show_camera: bool,
}
impl GpuLab {
    pub fn new(now: u16) -> Self {
        Self {
            scene: LabScene::new(),
            surface: SurfaceTab::new(),
            camera: CameraTab::new(),
            light: LightTab::new(),
            material: MaterialTab::new(),
            tab: LabTab::Surface,
            previous: now,
            show_camera: false,
        }
    }
    pub fn prepare(&mut self, grid: &mut UiGrid, input: GamepadFrame, now: u16) {
        let mut dt = now - self.previous;
        self.previous = now;
        if dt > 50 {
            dt = 50;
        }
        let seconds = fix32::from_int(dt as i16) * fix!(0.001);
        if input.pressed & BUTTON_SQUARE != 0 {
            self.show_camera = !self.show_camera;
        }
        if input.pressed & BUTTON_R2 != 0 {
            self.tab = LabTab::from_index((self.tab as u16 + 1) % 4);
        }
        if input.pressed & BUTTON_L2 != 0 {
            self.tab = LabTab::from_index((self.tab as u16 + 3) % 4);
        }
        let reset = input.pressed & BUTTON_TRIANGLE != 0;
        if reset {
            self.surface = SurfaceTab::new();
            self.camera = CameraTab::new();
            self.light = LightTab::new();
            self.material = MaterialTab::new();
        }
        page_start(grid, PageId::GpuLab, false, b"GPU LAB");
        text(grid, 2, 3, b"SURFACE");
        text(grid, 14, 3, b"CAMERA");
        text(grid, 25, 3, b"LIGHT");
        text(grid, 35, 3, b"MATERIAL");
        text(grid, 36, 5, b"TO LIGHT");
        if self.tab == LabTab::Surface {
            if !reset {
                self.surface.update(input.pressed);
            }
            if !self.show_camera {
                surface_labels(grid, self.surface);
            }
            grid.write_glyph(1, 3, 62);
        } else if self.tab == LabTab::Camera {
            if !reset {
                self.camera.update(input, seconds);
            }
            if !self.show_camera {
                camera_labels(grid, self.camera);
            }
            grid.write_glyph(13, 3, 62);
        } else if self.tab == LabTab::Light {
            if !reset {
                self.light.update(input, seconds);
            }
            if !self.show_camera {
                light_labels(grid, self.light);
            }
            grid.write_glyph(24, 3, 62);
        } else {
            if !reset {
                self.material.update(input.pressed);
            }
            if !self.show_camera {
                material_labels(grid, self.material);
            }
            grid.write_glyph(34, 3, 62);
        }
        let view = self.camera.view;
        let light = self.light.light;
        let material = self.material.material;
        if self.show_camera {
            camera_parameters(grid, view);
        }
        self.scene.configure(&view, &light, &material);
        self.scene.surface_mode(self.surface);
        grid.write_glyph(41, 12, ICON_AXIS_X);
        grid.write_glyph(44, 12, ICON_AXIS_Y);
        grid.write_glyph(47, 12, ICON_AXIS_Z);
        // Tab controls live on their own icon row, separate from page navigation.
        hint(grid, 17, KEY_L2, ICON_PREVIOUS);
        hint(grid, 24, KEY_R2, ICON_NEXT);
        hint(grid, 29, KEY_SQUARE, ICON_SELECT);
        hint(grid, 35, KEY_TRIANGLE, ICON_RESET);
    }
    pub fn draw(&self, pass: &mut Pass) {
        self.scene.draw(pass);
    }
}
