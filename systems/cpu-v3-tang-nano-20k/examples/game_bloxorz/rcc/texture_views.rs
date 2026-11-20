//! Bounded game atlases and carrier images use nonperiodic sampling.
use crate::dsl_rt::*;
mod gpu_api;
use gpu_api::*;

pub fn clamped_texture_view(texture: &Texture) -> TextureView {
    let mut view = texture.view();
    view.set_address_mode(TextureAddressMode::ClampToEdge);
    view
}
