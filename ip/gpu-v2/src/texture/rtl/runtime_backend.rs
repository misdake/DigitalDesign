//! Actual selected C6 -> Coefficient ready2/cohorts3 -> short Member ->
//! Work16/short Packet/ordered Pool P16/G32. Cache consumption is external.
use super::{compact_coefficient, compact_leaves, runtime_packet_backend};

pub const MODULE: &str = "gpu_v2_texture_runtime_backend";

/// Scoped shared-preflight lowering of existing selected leaves. Arithmetic,
/// RAMs and queue registers remain those of the authoritative leaf emitters.
pub fn verilog(native: bool) -> Result<String, String> {
    let coefficient = compact_coefficient::verilog();
    let coefficient_ports = "output reg          fault\n);";
    let coefficient_ready =
        "assign in_ready=advance && cohorts<3 && phase_even && work_available>={6'b0,cost};";
    if !coefficient.contains(coefficient_ports) || !coefficient.contains(coefficient_ready) {
        return Err("Coefficient shared preflight anchors changed".into());
    }
    let coefficient = coefficient
        .replace("module gpu_v2_texture_coefficient(", "module gpu_v2_runtime_coefficient(")
        .replace("input  wire         ce,", "input wire proposal_ce,commit_allow,")
        .replace(coefficient_ports, "output reg fault,output wire proposal_ready,preflight_bad\n);\n wire ce=proposal_ce&&commit_allow;")
        .replace(coefficient_ready, &format!("assign proposal_ready=!reset&&!fault&&queued<2&&cohorts<3&&phase_even&&work_available>={{6'b0,cost}};\n assign preflight_bad=!reset&&proposal_ce&&!fault&&in_valid&&proposal_ready&&(bad_input||work_available>16);\n {coefficient_ready}"));
    let packet = runtime_packet_backend::source(native);
    for anchor in [
        "input wire clk,reset,ce,abort,issue_enable,in_valid,",
        "wire attempted=ce && !reset && !fault_eff;",
        "wire preflight_bad=attempted && (input_bad || capture_bad || write_bad);",
        "wire advance=attempted && !preflight_bad;",
        "assign fault=fault_eff || preflight_bad;",
    ] {
        if !packet.contains(anchor) {
            return Err(format!("Packet shared preflight anchor changed: {anchor}"));
        }
    }
    let packet = packet
        .replace(runtime_packet_backend::MODULE, "gpu_v2_runtime_packet_backend_shared")
        .replace("input wire clk,reset,ce,abort,issue_enable,in_valid,", "input wire clk,reset,proposal_ce,commit_allow,abort,issue_enable,in_valid,")
        .replace("output wire fault,", "output wire fault,preflight_bad,advance,proposal_ready,proposal_ack,")
        .replace("wire attempted=ce && !reset && !fault_eff;", "wire attempted=proposal_ce && !reset && !fault_eff;")
        .replace("wire preflight_bad=attempted && (input_bad || capture_bad || write_bad);", "assign preflight_bad=attempted && (input_bad || capture_bad || write_bad);")
        .replace("wire advance=attempted && !preflight_bad;", "assign advance=attempted && !preflight_bad && commit_allow;")
        .replace("assign fault=fault_eff || preflight_bad;", "assign fault=fault_eff;\n assign proposal_ready=!reset&&!fault_eff&&work_ready;\n assign proposal_ack=attempted&&issue&&((member[39:36]>>({1'b0,tap}+3'd1))==0);");
    Ok(include_str!("runtime_backend.v").to_owned()
        + "\n"
        + &coefficient
        + "\n"
        + compact_leaves::membership()
        + "\n"
        + &packet)
}

/// Static closed-producer profile. C6 must supply conserving parent weights:
/// `p0 + p1 == 511`, each in `0..=511`. This shortens raw cost readiness;
/// all scalar and transport checks remain enabled. No extra payload/state.
pub const CANONICAL_MODULE: &str = "gpu_v2_texture_runtime_backend_canonical";

pub fn verilog_canonical(native: bool) -> Result<String, String> {
    let source = verilog(native)?;
    let cost = "wire [1:0] cost_in={1'b0,(p0!=0)}+{1'b0,(p1!=0)};";
    if source.matches(cost).count() != 1 {
        return Err("Canonical coefficient cost anchor changed".into());
    }
    Ok(source
        .replace(
            &format!("module {MODULE}("),
            &format!("module {CANONICAL_MODULE}("),
        )
        .replace(
            cost,
            "wire [1:0] cost_in=(p1==10'd0||p1==10'd511)?2'd1:2'd2;",
        ))
}
