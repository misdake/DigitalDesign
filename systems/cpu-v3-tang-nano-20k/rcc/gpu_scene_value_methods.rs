//! Public enum conveniences; their RCC language gate is tested explicitly.
mod gpu_scene_objects;
use gpu_scene_objects::*;

impl Shininess {
    pub fn nearest(exponent: u16) -> Self {
        gpu_scene_shininess_nearest(exponent)
    }
    pub fn exponent(&self) -> u16 {
        gpu_scene_shininess_exponent(*self)
    }
}
