//! Ordered Context8 with immediate last-C release and no late-retire state.
pub const MODULE: &str = "gpu_v2_texture_runtime_frontend_early_shared";

/// Shared raw preflight/commit ports match the generic controller. The fixed
/// profile accepts Context8/EARLY1 and a zero late-retire proposal only.
pub fn verilog_shared() -> &'static str {
    include_str!("runtime_frontend_early.v")
}

#[cfg(test)]
#[path = "runtime_frontend_early_qualification.rs"]
mod qualification;
