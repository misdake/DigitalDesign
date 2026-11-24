//! Application style and producer-owned native descriptions, not a second driver.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;
mod gpu_board;
use gpu_board::*;
mod boot_resources;
use boot_resources::*;

pub fn game_mesh_part(first: u16, count: u16) -> Mesh {
    Mesh::from_native(BLOX_MESH_DESC.subset(first, count))
}
pub fn game_texture() -> Texture {
    Texture::from_native(BLOX_TEXTURE_DESC)
}
pub fn game_screen_camera() -> Camera {
    Camera::new(CameraDesc::new(
        Vec3::zero(),
        Vec3::zero(),
        Projection::from_focal_pixels(fix!(200), fix!(199)),
    ))
}
pub fn game_material(color: Rgba8, unlit: bool) -> Material {
    Material::new(
        MaterialDesc::lit(color, Rgb8::black(), Shininess::S16)
            .with_response(fix!(0.5), fix!(0.5))
            .with_unlit(unlit),
    )
}
pub const fn game_lighting(direction: Vec3) -> Lighting {
    Lighting::directional(direction, fix!(1), fix!(1))
}
pub fn game_ui_grid() -> UiGrid {
    let atlas = UiAtlas::from_desc(BLOX_UI_DESC);
    let mut grid = UiGrid::from_memory(board_ui_memory(false), &atlas);
    grid.clear();
    grid
}
