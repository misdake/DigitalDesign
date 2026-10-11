//! Fixed Context8 queue controller. Metadata/UV/arithmetic payload stays on
//! live leaf ports. Does not emit a complete overlapped Runtime top.
pub const MODULE: &str = "gpu_v2_texture_runtime_frontend";
pub fn verilog() -> &'static str {
    include_str!("runtime_frontend.v")
}

/// Same controller/register body, with raw preflight separated from actual
/// shared commit. No reservation bit, descriptor or counter is added.
/// `proposal_cmds[6:0]` is {reclaim,empty,C-take,UV-request,Ready,LOD,D}.
/// Parent validation must use these ungated proposals, not the committed action
/// outputs, when joining preflight errors from the other live leaves.
pub fn verilog_shared() -> String {
    let source = verilog();
    assert!(source.contains("wire running=ce&&!reset&&!fault;"));
    assert!(source.contains("if(admit&&(!input_ready||valid[allocate]))bad=1;"));
    assert!(source.contains("if(bad)fault<=1;\n   else begin"));
    let mut source = source
        .replace(MODULE, "gpu_v2_texture_runtime_frontend_shared")
        .replace(
            "input wire clk,reset,ce,",
            "input wire clk,reset,proposal_ce,commit_allow,reserve_check,",
        )
        .replace(
            "output wire input_ready,raw_issue,derived_issue,ready_read,",
            "output wire preflight_bad,advance,reserve_ready,\n    output wire[6:0]proposal_cmds,\n    output wire input_ready,raw_issue,derived_issue,ready_read,",
        )
        .replace(
            "wire running=ce&&!reset&&!fault;",
            "wire running=proposal_ce&&!reset&&!fault;",
        )
        .replace("function[2:0]advance;", "function[2:0]next_slot;")
        .replace("begin advance=(a==", "begin next_slot=(a==")
        .replace("advance(", "next_slot(")
        .replace(
            "if(admit&&(!input_ready||valid[allocate]))bad=1;",
            "if(reserve_check&&(!reserve_ready))bad=1;\n   if(admit&&(!input_ready||valid[allocate]))bad=1;",
        )
        .replace(
            "assign input_ready=!reset&&!fault&&occupied<CONTEXTS;",
            "assign input_ready=!reset&&!fault&&occupied<CONTEXTS;\n assign reserve_ready=input_ready&&allocate<CONTEXTS&&!valid[allocate];\n assign preflight_bad=bad;\n assign advance=running&&!bad&&commit_allow;\n assign proposal_cmds={rf,empty,ct,(cr&&c_mask!=0&&uv_phase_odd&&uv_c_empty),cr,li,di};",
        )
        .replace("assign raw_issue=di&&!bad;assign derived_issue=li&&!bad;assign ready_read=cr&&!bad;", "assign raw_issue=di&&advance;assign derived_issue=li&&advance;assign ready_read=cr&&advance;")
        .replace("assign uv_request=cr&&c_mask!=0&&uv_phase_odd&&uv_c_empty&&!bad;", "assign uv_request=cr&&c_mask!=0&&uv_phase_odd&&uv_c_empty&&advance;")
        .replace("assign c_take=ct&&!bad;assign c_empty=empty&&!bad;", "assign c_take=ct&&advance;assign c_empty=empty&&advance;")
        .replace("assign reclaim_fire=rf&&!bad;", "assign reclaim_fire=rf&&advance;")
        .replace("if(bad)fault<=1;\n   else begin", "if(bad)fault<=1;\n   else if(commit_allow) begin");
    // Keep the diagnostic layout and all original sequential declarations exact.
    assert!(!source.contains("wire running=ce"));
    source.push('\n');
    source
}
