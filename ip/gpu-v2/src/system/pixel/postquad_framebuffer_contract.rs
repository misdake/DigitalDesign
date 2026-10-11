//! Specialize the cache to its actual held-header, immutable-draw producer.
//! The parent changes the output header only after quad_accepted (row7 ACK).
//! The sink advances phase/lane only on the row ACK, so rows1..7 retain x/y/mask.
//! Draw pins survive all input rows; the ROP leaf captures their context before
//! pipeline_idle can release those pins. Private returns keep their own headers.
//! Standalone framebuffer_cache keeps its per-quad contexts and diagnostics.
pub fn lower(mut source: String) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    if !source
        .contains("// One actual post-quad chain. Draw pins remain immutable until pipeline_idle.")
        || !source.contains("wire[2:0]row_index={lane,phase};")
        || !source.contains("else if(row_accept&&lane_matches)begin")
        || !source.contains("assign quad_accepted=lane_accept&&(expected_lane==2'd3);")
        || !source.contains("if(quad_accepted)begin")
    {
        return Err("Framebuffer contract requires the actual held-header producer".into());
    }
    let start = source
        .find("module framebuffer_cache #(")
        .ok_or("Missing framebuffer cache for producer contract")?;
    let end = start
        + source[start..]
            .find("endmodule")
            .ok_or("Missing framebuffer cache end")?
        + "endmodule".len();
    let mut body = source[start..end].to_owned();
    let mut one = |old: &str, new: &str| -> Result<(), String> {
        if body.matches(old).count() != 1 {
            return Err(format!(
                "Changed framebuffer producer-contract anchor {old:?}"
            ));
        }
        body = body.replace(old, new);
        Ok(())
    };
    for old in [
        "        || (fill_row != 4'd0 && {depth_func,depth_write,blend} != {s_func[tail],s_dw[tail],s_blend[tail]})",
        "                        || (fill_row != 4'd0\n                            && {depth_func, depth_write, blend}\n                               != {s_func[tail], s_dw[tail], s_blend[tail]})",
        "        || (fill_row != 4'd0 && {row_x,row_y,row_mask} != {s_hx[tail],s_hy[tail],s_hm[tail]})",
        "                        || (fill_row != 4'd0\n                            && {row_x, row_y, row_mask}\n                               != {s_hx[tail], s_hy[tail], s_hm[tail]})",
    ] {
        one(old, "")?;
    }
    for (name, width, live) in [
        ("s_func", "[2:0]  ", "depth_func"),
        ("s_dw", "       ", "depth_write"),
        ("s_blend", "       ", "blend"),
    ] {
        one(&format!("    reg {width}{name} [0:1];\n"), "")?;
        one(
            &format!("                            {name}[tail] <= {live};\n"),
            "",
        )?;
        one(&format!("{name}[tail]"), live)?;
    }
    source.replace_range(start..end, &body);
    Ok(source)
}
