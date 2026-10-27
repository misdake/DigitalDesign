//! Meshpack-v6 vertex decoding and transform; this component does not form triangles.
pub mod do_loan;
pub mod emu;
pub mod format;
pub mod ports;
pub mod rtl;
pub mod sim;

pub mod single_edge;
/// Selected independent engine; the reference engines remain explicit.
pub use single_edge::VertexEmu;
pub const ENABLED_EDGES: usize = 90;

pub mod do_loan_reference;
