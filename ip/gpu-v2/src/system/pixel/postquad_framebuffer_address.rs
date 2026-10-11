//! Closed framebuffer-bank address selection, independent of global WRE.
//! Reset establishes the maintenance owner. Every enabled write keeps its exact
//! owner/address/data; only the address of a disabled write may change.
pub fn lower(mut source: String) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    let start = source
        .find("module framebuffer_cache #(")
        .ok_or("Missing framebuffer cache for write-address lowering")?;
    let end = start
        + source[start..]
            .find("endmodule")
            .ok_or("Missing framebuffer cache end")?
        + "endmodule".len();
    let mut body = source[start..end].to_owned();
    for bank in 0..4 {
        let old = format!("wre{bank}=0;wra{bank}=0;wrd{bank}=0;");
        let new = format!(
            "wre{bank}=0;wra{bank}=mt_active?maintenance_write_addr:result_addr{bank};wrd{bank}=0;"
        );
        let maintenance = format!("wra{bank}=maintenance_write_addr;");
        let result = format!("wra{bank}=result_addr{bank};");
        if body.matches(&old).count() != 1
            || body.matches(&maintenance).count() != 1
            || body.matches(&result).count() != 2
        {
            return Err(format!("Changed framebuffer write-address anchor {bank}"));
        }
        body = body
            .replace(&old, &new)
            .replace(&maintenance, "")
            .replace(&result, "");
    }
    source.replace_range(start..end, &body);
    Ok(source)
}
