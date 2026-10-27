//! H9-only structural split between raw proposals and global transaction commit.
//! All local validation and fault fences remain; this does not delay a fault.

pub fn lower(mut source: String) -> Result<String, String> {
    for (old, new) in [
        (
            "wire front_fault,front_bad,advance,public_credit,cache_drained;",
            "wire front_fault,front_bad,advance,public_credit,cache_drained;wire compute_ce;",
        ),
        (
            "wire compute_ce=ce&&!fault;",
            "assign compute_ce=ce&&!fault;",
        ),
        (
            "wire coalition_allow=!(front_bad||sink_bad||final_bad);",
            "wire coalition_allow=compute_ce&&!(front_bad||sink_bad||final_bad);",
        ),
        (
            ".ce(compute_ce),.coalition_allow(coalition_allow)",
            ".ce(ce),.coalition_allow(coalition_allow)",
        ),
        (
            ".ce(advance),.proposal_ce(compute_ce),.commit_allow(coalition_allow)",
            ".ce(advance),.proposal_ce(ce),.commit_allow(coalition_allow)",
        ),
        (
            ".ce(compute_ce),.commit_allow(coalition_allow),.preflight_bad(sink_bad)",
            ".ce(ce),.commit_allow(coalition_allow),.preflight_bad(sink_bad)",
        ),
        (
            "assign raw_retire_proposal=compute_ce&&lane_valid&&lane_ready&&out_lane==3;",
            "assign raw_retire_proposal=ce&&lane_valid&&lane_ready&&out_lane==3;",
        ),
    ] {
        if source.matches(old).count() != 1 {
            return Err(format!("H9 raw-control specialization anchor {old}"));
        }
        source = source.replacen(old, new, 1);
    }
    Ok(source)
}

/// Sibling faults stop the coalition centrally; local preflights see local state.
/// External abort and each component's own terminal fault retain their behavior.
/// Never acquire another transport owner after fault; already presented owners drain.
pub fn lower_local_fault(source: String) -> Result<String, String> {
    let mut source = lower(source)?;
    for (old, new) in [
        (
            ".abort(abort||sink_fault||stopped||final_fault)",
            ".abort(abort)",
        ),
        (".abort(abort||stopped||sink_fault)", ".abort(abort)"),
        (
            "0:if(fbreq||txreq)begin owner<=fbreq;arb_state<=1;end",
            "0:if(!fault&&(fbreq||txreq))begin owner<=fbreq;arb_state<=1;end",
        ),
    ] {
        if source.matches(old).count() != 1 {
            return Err(format!("H9 local-fault specialization anchor {old}"));
        }
        source = source.replacen(old, new, 1);
    }
    Ok(source)
}
