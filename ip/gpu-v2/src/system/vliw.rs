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
    render_globals(slots, surface, backend, false)
}

pub(crate) fn verilog_command(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    render_globals(slots, surface, backend, true)
}

fn render_globals(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    live_globals: bool,
) -> Result<String, String> {
    if surface.width != 400 || surface.height != 240 {
        return Err("VLIW viewport and NDC ROM require 400x240".into());
    }
    let portable = matches!(backend, quad_ingress::rtl::Backend::Portable);
    let mut result = if live_globals {
        format!(
            "`define GPU_V2_LIVE_GLOBALS\n{}\n`undef GPU_V2_LIVE_GLOBALS\n",
            include_str!("vliw.v")
        )
    } else {
        include_str!("vliw.v").to_owned()
    };
    result.push('\n');
    result.push_str(&if live_globals {
        crate::frontend::vliw::verilog_command(portable)
    } else {
        crate::frontend::vliw::verilog(portable)
    });
    result.push('\n');
    result.push_str(&if live_globals {
        super::pixel::vliw_backend::verilog_selected_command(slots, surface, backend)?
    } else {
        super::pixel::vliw_backend::verilog_selected(slots, surface, backend)?
    });
    Ok(result)
}
