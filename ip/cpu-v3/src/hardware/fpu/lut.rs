//! Compatibility re-export of the FPU v2 special-function reference model.
//!
//! The single source of truth lives in the architecture layer
//! (`crate::architecture::fpu_lut`): the hidden-BSRAM LUT tables, the packed
//! interval rendering used to initialize the register-file Verilog, and the
//! RCP/RSQRT/SINCOS reference arithmetic. The hardware layer depends downward
//! on it so the RTL initialization and the architectural emulator can never
//! drift through a copied table or formula.
pub use crate::architecture::fpu_lut::*;
