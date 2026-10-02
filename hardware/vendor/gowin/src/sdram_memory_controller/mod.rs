//! Reusable shared SDRAM service. Board clocks/pads remain target-owned.
//! The legacy module identities are deliberately retained during migration.
pub mod arbiter;
pub mod combination;
pub mod emu;
pub mod ports;
pub mod probe;
pub mod shared_port;
pub mod sim;

pub use arbiter::*;
pub use shared_port::*;

/// One authoritative source bundle for board export and verification fixtures.
pub struct RtlSources;
impl RtlSources {
    pub const CONTROLLER: &'static str = include_str!("rtl/sdram_controller.v");
    pub const GEARBOX: &'static str = include_str!("rtl/native_bridge_108m_54m.v");
    pub const SHARED_PORT: &'static str = include_str!("rtl/display_sdram.v");
    pub const PIN_MODEL: &'static str = include_str!("rtl/sdram_pin_model.v");
    pub const BRIDGE_TESTBENCH: &'static str = include_str!("rtl/sdram_native_bridge_tb.v");
}
