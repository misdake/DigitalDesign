//! Native edge models, distinct from the abstract functional service in `sim`.
pub mod controller;
pub mod gear;
pub mod memory;
pub mod pins;
pub mod port;

/// Edges occurring at one simulation instant. All read the same pre-edge state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Edges {
    pub logic_rise: bool,
    pub native_rise: bool,
    pub native_fall: bool,
}
