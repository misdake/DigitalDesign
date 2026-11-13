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
        if index == 1 {
            return Self::Camera;
        }
        if index == 2 {
            return Self::Light;
        }
        if index == 3 {
            return Self::Material;
        }
        Self::Surface
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
        }
    }
    pub fn prepare(&mut self, grid: &mut UiGrid, input: GamepadFrame, now: u16) {
        let mut dt = now - self.previous;
        self.previous = now;
        if dt > 50 {
            dt = 50;
        }
        let seconds = fix32::from_int(dt as i16) * fix!(0.001);
        if input.pressed & BUTTON_R2 != 0 {
            self.tab = LabTab::from_index((self.tab as u16 + 1) % 4);
        }
        if input.pressed & BUTTON_L2 != 0 {
            self.tab = LabTab::from_index((self.tab as u16 + 3) % 4);
        }
        let mut view = LabView::new();
        let mut light = LabLight::new();
        let mut material = LabMaterial::new();
        page_start(grid, PageId::GpuLab, false, b"GPU LAB");
        text(grid, 2, 3, b"SURFACE");
        text(grid, 14, 3, b"CAMERA");
        text(grid, 25, 3, b"LIGHT");
        text(grid, 35, 3, b"MATERIAL");
        if self.tab == LabTab::Surface {
            light = LabLight::surface();
            self.surface.update(input.pressed);
            surface_labels(grid, self.surface);
            grid.write_glyph(1, 3, 62);
        } else if self.tab == LabTab::Camera {
            self.camera.update(input, seconds);
            camera_labels(grid, self.camera);
            view = self.camera.view;
            grid.write_glyph(13, 3, 62);
        } else if self.tab == LabTab::Light {
            self.light.update(input, seconds);
            light_labels(grid, self.light);
            light = self.light.light;
            grid.write_glyph(24, 3, 62);
        } else {
            self.material.update(input.pressed);
            material_labels(grid, self.material);
            material = self.material.material;
            grid.write_glyph(34, 3, 62);
        }
        self.scene.configure(&view, &light, &material);
        if self.tab == LabTab::Surface {
            self.scene.surface_mode(self.surface.mode);
        } else if self.tab == LabTab::Light {
            self.scene.show_light_direction();
        }
        // Tab controls live on their own icon row, separate from page navigation.
        hint(grid, 17, KEY_L2, ICON_PREVIOUS);
        hint(grid, 24, KEY_R2, ICON_NEXT);
        hint(grid, 35, KEY_TRIANGLE, ICON_RESET);
    }
    pub fn draw(&self, pass: &mut Pass) {
        self.scene.draw(pass);
    }
}
