//! Independent closed-ROP arithmetic experiments. Calendar, depth and keyed
//! return flags are unchanged. The caller selects approximate alpha6 explicitly.
fn replace_once(source: &mut String, old: &str, new: &str) -> Result<(), String> {
    if source.matches(old).count() != 1 {
        return Err(format!("ROP precision anchor: {old}"));
    }
    *source = source.replace(old, new);
    Ok(())
}

/// REPLACE uses opaque arithmetic early; no later source-RGB bypass is needed.
/// Numeric equality holds for known legal inputs including alpha endpoints.
pub fn opaque_replace(mut source: String) -> Result<String, String> {
    replace_once(
        &mut source,
        "            a2 <= sa1;",
        "            a2 <= blend1 ? sa1 : 8'd255;",
    )?;
    replace_once(
        &mut source,
        "            ia2 <= 8'd255 - sa1;",
        "            ia2 <= blend1 ? 8'd255-sa1 : 8'd0;",
    )?;
    for stage in 2..=4 {
        replace_once(
            &mut source,
            &format!("gate{stage}, dw{stage}, blend{stage};"),
            &format!("gate{stage}, dw{stage};"),
        )?;
        replace_once(
            &mut source,
            &format!("            blend{stage} <= blend{};\n", stage - 1),
            "",
        )?;
    }
    for stage in 3..=4 {
        replace_once(
            &mut source,
            &format!("    reg [7:0]  sr{stage}, sg{stage}, sb{stage};\n"),
            "",
        )?;
        for channel in ['r', 'g', 'b'] {
            replace_once(
                &mut source,
                &format!(
                    "            s{channel}{stage} <= s{channel}{};\n",
                    stage - 1
                ),
                "",
            )?;
        }
    }
    for channel in ['r', 'g', 'b'] {
        replace_once(
            &mut source,
            &format!("b{channel}5 <= blend4 ? div_{channel}4[7:0] : s{channel}4;"),
            &format!("b{channel}5 <= div_{channel}4[7:0];"),
        )?;
    }
    Ok(source)
}

/// Approximate alpha has six fractional bits: W=round(A/4), denominator64.
/// W0..64 needs7 bits; colors remain8. Alpha0/255 and REPLACE stay exact.
/// Six native hard registered products remain; only their bounded low14 bits
/// feed the14-bit fabric sum. Existing quantizer and eight-edge timing remain.
pub fn alpha6(source: String) -> Result<String, String> {
    let mut source = opaque_replace(source)?;
    replace_once(
        &mut source,
        "    // ---- Stage 2: compare/expand ----",
        "    wire [8:0] alpha6_sum = {1'b0,sa1}+9'd2;\n    // ---- Stage 2: compare/expand ----",
    )?;
    replace_once(
        &mut source,
        "a2 <= blend1 ? sa1 : 8'd255;",
        "a2 <= blend1 ? {1'b0,alpha6_sum[8:2]} : 8'd64;",
    )?;
    replace_once(
        &mut source,
        "ia2 <= blend1 ? 8'd255-sa1 : 8'd0;",
        "ia2 <= blend1 ? 8'd64-{1'b0,alpha6_sum[8:2]} : 8'd0;",
    )?;
    replace_once(
        &mut source,
        "reg [16:0] nr4, ng4, nb4;",
        "reg [13:0] nr4, ng4, nb4;",
    )?;
    for channel in ['r', 'g', 'b'] {
        replace_once(
            &mut source,
            &format!("n{channel}4 <= ps{channel}3 + pd{channel}3 + 16'd127;"),
            &format!("n{channel}4 <= ps{channel}3[13:0] + pd{channel}3[13:0] + 14'd32;"),
        )?;
        replace_once(
            &mut source,
            &format!("div_{channel}4 = div255(n{channel}4);"),
            &format!("div_{channel}4 = {{1'b0,n{channel}4[13:6]}};"),
        )?;
    }
    Ok(source)
}
