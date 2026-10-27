//! Single published Quad feeding actual Lighting and row Runtime D/LOD/C.
//! Coefficient consumption, public-ID credit, draw configuration, branch-result
//! storage and final global retirement remain explicit parent boundaries.
use super::{lighting_consumer, quad_ingress};
use crate::texture::rtl::runtime_rows;

pub fn verilog(backend: quad_ingress::rtl::Backend) -> Result<String, String> {
    verilog_profile(backend, runtime_rows::Profile::default())
}

pub fn verilog_profile(
    backend: quad_ingress::rtl::Backend,
    profile: runtime_rows::Profile,
) -> Result<String, String> {
    let native = matches!(backend, quad_ingress::rtl::Backend::Native);
    Ok(format!(
        "{}\n{}\n{}\n{}",
        include_str!("pixel_runtime_ingress.v"),
        quad_ingress::rtl::verilog_shared(backend)?,
        lighting_consumer::verilog()?,
        runtime_rows::verilog_profile(native, profile)?
    ))
}
