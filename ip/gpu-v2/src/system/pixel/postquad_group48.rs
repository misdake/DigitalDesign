//! Producer16/Group48 on the existing ring64; all state widths remain unchanged.
pub fn lower(mut source: String) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    let begin = source
        .find("module gpu_v2_packet_store_unique ")
        .ok_or("Group48 module")?;
    let end = begin + source[begin..].find("endmodule").ok_or("Group48 end")? + 9;
    let old_body = source[begin..end].to_owned();
    let mut body = old_body.clone();
    for (old, new) in [("rows>=48", "rows>=63"), ("gc<32", "gc<48")] {
        if body.matches(old).count() != 1 {
            return Err(format!("Group48 anchor {old}"));
        }
        body = body.replace(old, new);
    }
    Ok(source.replace(&old_body, &body))
}
