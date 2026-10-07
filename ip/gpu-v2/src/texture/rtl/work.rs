//! Work SPSC transport with asynchronous-read SSRAM and two registered heads.
//! Rows publish only after a full write. A read edge captures payload and valid
//! together; the consumer observes old head registers. No RAM read bypass,
//! same-edge returned credit, dynamic allocator or extra publication age.
//! This isolated block does not make the overlapped Runtime a complete RTL top.

pub const MODULE: &str = "gpu_v2_texture_work";

pub fn verilog() -> &'static str {
    include_str!("work.v")
}
