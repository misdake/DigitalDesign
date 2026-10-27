//! Dirty planes are captured only when the maintenance owner is claimed.
//! Original registered request/accepted/presented flags and R/W calendar remain.
pub fn lower(mut source: String) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    let start = source
        .find("module framebuffer_cache #(")
        .ok_or("planes module")?;
    let end = start + source[start..].find("endmodule").ok_or("planes end")?;
    let mut body = source[start..end].to_owned();
    for (old, new) in REPLACEMENTS {
        if body.matches(old).count() != 1 {
            return Err(format!("planes anchor {old}"));
        }
        body = body.replace(old, new);
    }
    source.replace_range(start..end, &body);
    Ok(source)
}
const REPLACEMENTS: &[(&str, &str)] = &[
    (
        r###"                        if (mt_plane == 1'b0 && dird[mt_line]) begin
                            mt_plane <= 1'b1;
                            mt_sector <= 2'd0; mt_beat <= 5'd0;
                            mt_accepted <= 1'b0; mt_presented <= 1'b0;
                            req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                            mst <= M_WPRE;
                        end else if (mt_plane == 1'b1 && dirc[mt_line]) begin
                            mt_plane <= 1'b0;
                            mt_sector <= 2'd0; mt_beat <= 5'd0;
                            mt_accepted <= 1'b0; mt_presented <= 1'b0;
                            req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                            mst <= M_WPRE;
"###,
        r###"                        mt_remaining[mt_plane] <= 1'b0;
                        if (mt_plane ? mt_remaining[0] : mt_remaining[1]) begin
                            mt_plane <= !mt_plane;
                            mt_sector <= 2'd0; mt_beat <= 5'd0;
                            mt_accepted <= 1'b0; mt_presented <= 1'b0;
                            req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                            mst <= M_WPRE;
"###,
    ),
    (
        r###"    reg        mt_plane;"###,
        r###"    reg        mt_plane;
    reg [1:0] mt_remaining;"###,
    ),
    (
        r###"            fault_m <= 1'b0;"###,
        r###"            mt_remaining <= 2'b0;
            fault_m <= 1'b0;"###,
    ),
    (
        r###"                            mt_target <= head_tile;"###,
        r###"                            mt_target <= head_tile;
                            mt_remaining <= {sel_dird,sel_dirc};"###,
    ),
    (
        r###"                                mt_write <= 1'b1;"###,
        r###"                                mt_write <= 1'b1;
                                mt_remaining <= {dird[f],dirc[f]};"###,
    ),
];
