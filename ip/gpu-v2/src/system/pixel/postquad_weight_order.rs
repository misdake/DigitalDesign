//! Physical-bank Color input: inverse coefficient permutation, unchanged leases.
pub fn lower(mut source: String) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    let start = source
        .find("module gpu_v2_texture_runtime_cache ")
        .ok_or("cache module")?;
    let end = start + source[start..].find("endmodule").ok_or("cache end")?;
    let mut body = source[start..end].to_owned();
    let old = r#" assign out_payload=res_payload;
 // Canonical tap order: retain the existing two conditional bank swaps.
 wire[15:0]x0=res_payload[22]?res_tex[1]:res_tex[0];
 wire[15:0]x1=res_payload[22]?res_tex[0]:res_tex[1];
 wire[15:0]x2=res_payload[22]?res_tex[3]:res_tex[2];
 wire[15:0]x3=res_payload[22]?res_tex[2]:res_tex[3];
 wire y0=res_payload[25]^res_payload[23],y1=y0^res_payload[22];
 assign out_tex0=y0?x2:x0;assign out_tex1=y1?x3:x1;
 assign out_tex2=y0?x0:x2;assign out_tex3=y1?x1:x3;"#;
    let new = r#" // Inverse mapping on36 coefficient bits replaces64-bit texel mapping.
 wire y0=res_payload[25]^res_payload[23],y1=y0^res_payload[22];
 wire[8:0]v0=y0?res_payload[46+:9]:res_payload[28+:9];
 wire[8:0]v1=y1?res_payload[55+:9]:res_payload[37+:9];
 wire[8:0]v2=y0?res_payload[28+:9]:res_payload[46+:9];
 wire[8:0]v3=y1?res_payload[37+:9]:res_payload[55+:9];
 wire[35:0]physical_weights=res_payload[22]?{v2,v3,v0,v1}:{v3,v2,v1,v0};
 assign out_payload={res_payload[71:64],physical_weights,res_payload[27:0]};
 assign out_tex0=res_tex[0];assign out_tex1=res_tex[1];
 assign out_tex2=res_tex[2];assign out_tex3=res_tex[3];"#;
    if body.matches(old).count() != 1 {
        return Err("weight order permutation anchor".into());
    }
    body = body.replace(old, new);
    source.replace_range(start..end, &body);
    Ok(source)
}
