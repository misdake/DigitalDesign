//! Composition of command processor, DMA scratchpad and vertex owned ports.
pub mod ports;
pub mod sim;
pub mod source_capture;
pub mod source_record;
pub mod vliw;
pub mod vliw_dma;

mod vliw_numeric;
pub(crate) use vliw_numeric::words as interpolation_constants;
