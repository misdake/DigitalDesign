//! Reusable shared SDRAM service. Board clocks/pads remain target-owned.
pub mod arbiter;
pub mod combination;
pub mod cycle;
pub mod ports;
pub mod profile;
pub mod sim;
pub mod streaming_port;

pub use arbiter::*;
pub use streaming_port::*;

/// One authoritative source bundle for board export and verification fixtures.
pub struct RtlSources;
impl RtlSources {
    pub const CONTROLLER: &'static str = include_str!("rtl/sdram_controller.v");
    pub const STREAM_SHARED_PORT: &'static str = include_str!("rtl/stream_sdram.v");
    pub const STREAM_GEARBOX: &'static str = include_str!("rtl/stream_bridge.v");
    pub const PIN_MODEL: &'static str = include_str!("rtl/sdram_pin_model.v");
    pub const INPUT_DELAY_MODEL: &'static str = include_str!("rtl/input_delay_model.v");

    pub fn controller(shared_bank_prepare: bool) -> String {
        let mut source = String::new();
        let mut selected = true;
        // Resolve the layout guard before export to keep the established
        // implementation's synthesis input identical.
        for line in Self::CONTROLLER.split_inclusive('\n') {
            match line.trim_end() {
                "`ifdef GOWIN_SDRAM_SHARED_PREPARE" => selected = shared_bank_prepare,
                "`else // GOWIN_SDRAM_SHARED_PREPARE" => selected = !shared_bank_prepare,
                "`endif // GOWIN_SDRAM_SHARED_PREPARE" => selected = true,
                _ if selected => source.push_str(line),
                _ => {}
            }
        }
        source
    }
}
