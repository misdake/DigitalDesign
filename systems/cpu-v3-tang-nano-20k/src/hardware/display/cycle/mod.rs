//! Edge-driven production Display components. These components own no system clock or MC.

mod line_buffer;

pub use line_buffer::{LineBuffer, LineBufferInput, RamWord};

/// Rising edges sampled at one root-scheduler instant.
/// Coincident edges must see the same pre-edge state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub logic_rise: bool,
    pub pixel_rise: bool,
}
