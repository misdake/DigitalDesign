//! Real ingress controller composed with existing registered SPSC/Basic leaves.

use super::super::{row_store, spsc};

#[derive(Clone, Copy, Debug)]
pub enum Backend {
    Portable,
    Native,
}

pub fn verilog(backend: Backend) -> Result<String, String> {
    let config = spsc::Config {
        entries: 2,
        rows: 8,
        width: 36,
        read_timing: spsc::ReadTiming::Registered,
        max_wall: 1,
    };
    let queue = match backend {
        Backend::Portable => spsc::rtl::source(config)?,
        Backend::Native => spsc::rtl::source_sdp36(config)?,
    };
    // The paid hard DO remains reserved if both heads are full. If a head is
    // free, capture occurs on the return wall edge even with compute CE low.
    // All issue, push and pop enables already include CE in the reused leaf.
    if !queue.contains("(!reset && ce && pending_valid")
        || !queue.contains("end else if (ce) begin")
    {
        return Err("registered SPSC wall-return template changed".into());
    }
    let queue = queue
        .replace(spsc::rtl::MODULE, "gpu_v2_quad_source_rows")
        .replace("(!reset && ce && pending_valid", "(!reset && pending_valid")
        .replace("end else if (ce) begin", "end else begin");
    let basic = match backend {
        Backend::Portable => row_store::verilog(256, 24)?,
        Backend::Native => row_store::verilog_sdp36(256, 24)?,
    };
    Ok(include_str!("ingress.v").to_owned() + "\n" + &queue + "\n" + &basic)
}

/// Same ingress/queue/bank body with raw preflight independent of joined commit.
/// Queue output heads and paid wall returns remain live during a denied edge.
pub fn verilog_shared(backend: Backend) -> Result<String, String> {
    let source = verilog(backend)?;
    if !source.contains("wire proposed_ready = !reset && !fault && ce")
        || !source.contains("assign advance = !reset && ce && !fault && !bad_edge;")
    {
        return Err("quad shared-preflight template changed".into());
    }
    Ok(source
        .replace("module gpu_v2_quad_ingress(", "module gpu_v2_quad_ingress_shared(")
        .replacen("input wire clk, reset, ce,", "input wire clk, reset, proposal_ce, commit_allow,", 1)
        .replace("output wire advance,", "output wire advance, preflight_bad,")
        .replace("wire proposed_ready = !reset && !fault && ce", "wire proposed_ready = !reset && !fault && proposal_ce")
        .replace("(!ce || retire_ptr", "(!proposal_ce || retire_ptr")
        .replace("wire bad_collision = ce &&", "wire bad_collision = proposal_ce &&")
        .replace("assign advance = !reset && ce && !fault && !bad_edge;", "assign preflight_bad = !reset && !fault && bad_edge;\n assign advance = !reset && proposal_ce && !fault && !bad_edge && commit_allow;"))
}
