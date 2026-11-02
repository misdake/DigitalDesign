//! Edge-driven production Display components. These components own no system clock or MC.

mod control;
mod fill;
mod timing;

pub use control::{ControlInput, ControlOutput, DisplayControl};
pub use timing::{Timing, TimingInput, TimingOutput};
mod line_buffer;
mod pixels;

pub use fill::{Fill, FillInput, FillOutput};

pub use line_buffer::{LineBuffer, LineBufferInput, RamWord};
pub use pixels::{ColorPrecode, PixelRead, PixelReadInput, PixelReadOutput, Precode};

/// Rising edges sampled at one root-scheduler instant.
/// Coincident edges must see the same pre-edge state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub logic_rise: bool,
    pub pixel_rise: bool,
}
