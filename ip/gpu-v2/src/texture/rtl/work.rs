//! Work SPSC transport with asynchronous-read SSRAM and two registered heads.
//! Rows publish only after a full write. A read edge captures payload and valid
//! together; the consumer observes old head registers. No RAM read bypass,
//! same-edge returned credit, dynamic allocator or extra publication age.
//! This isolated block does not make the overlapped Runtime a complete RTL top.

pub const MODULE: &str = "gpu_v2_texture_work";

pub fn verilog() -> &'static str {
    include_str!("work.v")
}

/// Selected Member76 plus immutable cursor2; identical old-state transport control.
pub fn compact_verilog() -> String {
    verilog()
        .replace("[91:0]", "[75:0]")
        .replace("[93:0]", "[77:0]")
        .replace("[93:92]", "[77:76]")
}

/// Same Work16 transport with only emit/cursor in its two logical heads.
/// The payload bank has one asynchronous read at reclaim_ptr; control has
/// one asynchronous read at fetch_ptr. A published row remains immutable
/// until final ACK. Old count forbids financing put with same-edge ACK.
/// Packet E0 therefore captures the same old member without a wide head mux.
/// The closed pixel sampling top selects this binding; the generic and compact
/// reference emitters remain unchanged.
pub fn compact_direct_verilog() -> String {
    include_str!("work_direct.v").to_owned()
}
