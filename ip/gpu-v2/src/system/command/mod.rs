//! Production command GPU: explicit parents and shared component factories.
pub mod abi;
pub use abi::*;
pub const MODULE: &str = "gpu_v2_vliw_command";

pub(super) mod backend;

use crate::{framebuffer::ports::MaterializedSurface, system::pixel::quad_ingress};

pub fn verilog(
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
) -> Result<String, String> {
    if surface.width != 400 || surface.height != 240 {
        return Err("VLIW viewport and NDC ROM require 400x240".into());
    }
    let mut source = "`define GPU_V2_COMMAND_PORTS\n`define GPU_V2_TRUSTED_FRONTEND\n".to_owned();
    source.push_str(include_str!("../vliw_command.v"));
    let mut numerical = format!(
        "`define GPU_V2_LIVE_GLOBALS\n{}\n`undef GPU_V2_LIVE_GLOBALS\n",
        include_str!("compute.v")
    );
    numerical.push('\n');
    numerical.push_str(&crate::frontend::vliw::verilog_command(matches!(
        backend,
        quad_ingress::rtl::Backend::Portable
    )));
    numerical.push('\n');
    numerical.push_str(&self::backend::verilog(surface, backend)?);
    source.push_str(&format!(
        "`define GPU_V2_COMMAND_MAINTENANCE\n{numerical}\n`undef GPU_V2_COMMAND_MAINTENANCE\n"
    ));
    source.push_str(include_str!("../command_processor.v"));
    source.push_str("`define GPU_V2_COMMAND_TRANSPORT\n");
    source.push_str(include_str!("../command_dma.v"));
    source.push_str("\n`undef GPU_V2_COMMAND_TRANSPORT\n");
    source.push_str(include_str!("../ui_grid.v"));
    source.push_str(&super::vliw_memory::command_verilog());
    Ok(source.replace("\r\n", "\n"))
}

#[cfg(test)]
mod tests;
