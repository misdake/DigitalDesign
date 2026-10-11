//! Select write payload independently of the committed write-enable cone.
//! The ordered prefix forbids a color return while a paid depth is pending.
//! Every enabled bank write retains its original data, address and owner;
//! disabled-write payload is free to follow held maintenance/result state.
pub fn lower(mut source: String) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    let start = source
        .find("module framebuffer_cache #(")
        .ok_or("Missing framebuffer cache for write-data lowering")?;
    let end = start
        + source[start..]
            .find("endmodule")
            .ok_or("Missing framebuffer cache end")?
        + "endmodule".len();
    let mut body = source[start..end].to_owned();
    if !body.contains("wire return_proposed = leaf_proposal && leaf_out_valid;")
        || !body.contains("assign return_bad = return_proposed && (depth_pending")
        || !body.contains("wire return_fire = leaf_ce && leaf_out_valid;")
        || !body.contains("assign leaf_ce = leaf_proposal && !source_bad && commit_allow;")
        || !body.contains("wire depth_fire = actual_ce && !fault && !source_bad && depth_pending;")
    {
        return Err("Write-data lowering requires the ordered prefix fault fence".into());
    }
    for bank in 0..4 {
        let old = format!("wrd{bank}=0;");
        let new = format!(
            "wrd{bank}=mt_active?refill_bank{bank}:(depth_pending?depth_data:leaf_new_color);"
        );
        if body.matches(&old).count() != 1 {
            return Err(format!("Changed framebuffer write-data default {bank}"));
        }
        body = body.replace(&old, &new);
        for value in [
            format!("refill_bank{bank}"),
            "leaf_new_color".into(),
            "depth_data".into(),
        ] {
            let old = format!("wrd{bank}={value};");
            if body.matches(&old).count() != 1 {
                return Err(format!(
                    "Changed framebuffer write-data branch {bank}/{value}"
                ));
            }
            body = body.replace(&old, "");
        }
    }
    source.replace_range(start..end, &body);
    Ok(source)
}
