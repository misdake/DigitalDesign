//! Control-only lowering for the closed two-cycle Final + prefix ROP profile.
//! Call only after the complete actual postquad emitter has assembled its leaves.
//! Raw preflight and readiness must never depend on a committed advance. Compute
//! commits share the front/sink/Final bad union; maintenance and paid reads retain
//! their independent wall-clock ownership. No arithmetic or payload is changed.

fn replace_module(source: &mut String, module: &str, old: &str, new: &str) -> Result<(), String> {
    let prefix = format!("module {module}");
    let starts: Vec<_> = source
        .match_indices(&prefix)
        .filter_map(|(p, _)| {
            let next = source.as_bytes().get(p + prefix.len()).copied();
            matches!(next, Some(b'(' | b'#' | b' ' | b'\r' | b'\n')).then_some(p)
        })
        .collect();
    if starts.len() != 1 {
        return Err(format!(
            "Shared proposal module {module}: expected one declaration, got {}",
            starts.len()
        ));
    }
    let start = starts[0];
    let end = start
        + source[start..]
            .find("endmodule")
            .ok_or_else(|| format!("Missing endmodule for {module}"))?
        + "endmodule".len();
    let body = &source[start..end];
    if body.matches(old).count() != 1 {
        return Err(format!(
            "Shared proposal anchor in {module}: expected one match for {old:?}"
        ));
    }
    let rewritten = body.replace(old, new);
    source.replace_range(start..end, &rewritten);
    Ok(())
}

/// Independent raw proposal/preflight with one actual compute commit coalition.
/// Generic/default modules stay unchanged unless this explicit lowering is used.
pub fn lower_prefix_shared_proposal(source: String) -> Result<String, String> {
    // Freeze qualification/export bytes across Windows include_str line endings.
    let mut source = source.replace("\r\n", "\n");
    // shared raw preflight union
    replace_module(&mut source, "gpu_v2_postquad", "wire sink_fault,sink_idle,final_fault;", "wire sink_fault,sink_idle,final_fault,sink_bad,final_bad;\n wire coalition_allow=!(front_bad||sink_bad||final_bad);")?;
    // front common commit
    replace_module(&mut source, "gpu_v2_postquad", ".clk(clk),.reset(reset),.ce(compute_ce),.abort(abort||sink_fault||stopped||final_fault),", ".clk(clk),.reset(reset),.ce(compute_ce),.coalition_allow(coalition_allow),.abort(abort||sink_fault||stopped||final_fault),")?;
    // Final raw proposal and actual CE
    replace_module(&mut source, "gpu_v2_postquad", ".clk(clk),.reset(reset),.ce(advance),.abort(abort||stopped||sink_fault),", ".clk(clk),.reset(reset),.ce(advance),.proposal_ce(compute_ce),.commit_allow(coalition_allow),.preflight_bad(final_bad),.abort(abort||stopped||sink_fault),")?;
    // sink raw offer and actual allow
    replace_module(&mut source, "gpu_v2_postquad", ".clk(clk),.reset(reset),.ce(compute_ce),.lane_valid(lane_valid&&advance),.lane(out_lane),", ".clk(clk),.reset(reset),.ce(compute_ce),.commit_allow(coalition_allow),.preflight_bad(sink_bad),.lane_valid(lane_valid),.lane(out_lane),")?;
    // add coalition allow
    replace_module(
        &mut source,
        "gpu_v2_pixel_sampling_ingress",
        "input wire packet_enable,abort,result_ready,",
        "input wire packet_enable,abort,result_ready,coalition_allow,",
    )?;
    // shared commit without changing raw proposals
    replace_module(
        &mut source,
        "gpu_v2_pixel_sampling_ingress",
        "wire allow=!preflight_bad&&!fault;",
        "wire allow=coalition_allow&&!preflight_bad&&!fault;",
    )?;
    // raw CE and common allow
    replace_module(
        &mut source,
        "gpu_v2_postquad_final",
        "input wire clk,reset,ce,abort,in_valid,",
        "input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,",
    )?;
    // expose raw Final bad
    replace_module(
        &mut source,
        "gpu_v2_postquad_final",
        "output wire[23:0]out_rgb,output wire fault);",
        "output wire[23:0]out_rgb,output wire fault,preflight_bad);",
    )?;
    // leaf control mapping
    replace_module(&mut source, "gpu_v2_postquad_final", ".clk(clk),.reset(reset),.ce(ce),.commit_allow(1'b1),.abort(abort),", ".clk(clk),.reset(reset),.ce(ce),.proposal_ce(proposal_ce),.commit_allow(commit_allow),.preflight_bad(preflight_bad),.abort(abort),")?;
    // independent raw Final CE
    replace_module(
        &mut source,
        "gpu_v2_final_ordered_two_cycle",
        "input wire clk,reset,ce,commit_allow,abort,in_valid,",
        "input wire clk,reset,ce,proposal_ce,commit_allow,abort,in_valid,",
    )?;
    // raw h guard before committed ready
    replace_module(&mut source, "gpu_v2_final_ordered_two_cycle", "assign preflight_bad=in_ready&&in_valid&&(in_h>9'd256);", "assign preflight_bad=proposal_ce&&!reset&&!fault&&proposal_ready&&in_valid&&(in_h>9'd256);")?;
    // raw CE and common commit ports
    replace_module(
        &mut source,
        "gpu_v2_pixel_framebuffer_sink",
        "input wire clk,reset,ce,",
        "input wire clk,reset,ce,commit_allow,\n output wire preflight_bad,",
    )?;
    // cache raw/commit mapping
    replace_module(&mut source, "gpu_v2_pixel_framebuffer_sink", ".clk(clk),.reset(reset),.ce(ce&&!reset),", ".clk(clk),.reset(reset),.ce(ce&&!reset),.commit_allow(commit_allow),.preflight_bad(preflight_bad),")?;
    // control-only ports
    replace_module(&mut source, "framebuffer_cache", "    input             ce,", "    input             ce,\n    input             commit_allow,\n    output            preflight_bad,")?;
    // raw bad export and committed CE
    replace_module(&mut source, "framebuffer_cache", "    wire source_ready,source_bad,leaf_ce,return_bad;", "    wire source_ready,source_bad,leaf_ce,return_bad;\n    wire actual_ce=ce&&commit_allow;\n    assign preflight_bad=!reset&&source_bad;")?;
    // actual arithmetic edge
    replace_module(
        &mut source,
        "framebuffer_cache",
        "assign leaf_ce = leaf_proposal && !source_bad;",
        "assign leaf_ce = leaf_proposal && !source_bad && commit_allow;",
    )?;
    // actual depth write distinct from raw retirement
    replace_module(
        &mut source,
        "framebuffer_cache",
        "wire depth_fire = ce && !fault && !source_bad && depth_pending;",
        "wire depth_fire = actual_ce && !fault && !source_bad && depth_pending;",
    )?;
    // actual row accept
    replace_module(&mut source, "framebuffer_cache", "assign row_accept = ce && !flushing && !fault && row_credit && source_ready && row_valid && !source_bad;", "assign row_accept = actual_ce && !flushing && !fault && row_credit && source_ready && row_valid && !source_bad;")?;
    // actual compute state
    replace_module(
        &mut source,
        "framebuffer_cache",
        "if (ce && !fault && !source_bad) begin",
        "if (actual_ce && !fault && !source_bad) begin",
    )?;
    Ok(source)
}
