//! Full current V/T/R/I and selected postquad backend, with one result pool.
use crate::{
    framebuffer::ports::MaterializedSurface, system::pixel::quad_ingress, texture::ports::Slot,
};

pub const MODULE: &str = "gpu_v2_vliw";

pub fn verilog(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    if surface.width != 400 || surface.height != 240 {
        return Err("VLIW viewport and NDC ROM require 400x240".into());
    }
    let portable = matches!(backend, quad_ingress::rtl::Backend::Portable);
    let mut result = include_str!("vliw.v").to_owned();
    result.push('\n');
    result.push_str(&crate::frontend::vliw::verilog(portable));
    result.push('\n');
    result.push_str(&super::pixel::vliw_backend::verilog_selected(
        slots, surface, backend,
    )?);
    Ok(result)
}
