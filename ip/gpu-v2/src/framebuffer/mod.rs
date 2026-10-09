//! ROP references, audited arithmetic, and independent finite-cache emulation/RTL.
//! The complete production GPU/system integration remains a separate boundary.
pub mod arithmetic;
pub mod emu;
pub mod ports;
pub mod rtl;
pub mod sim;
