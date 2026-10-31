//! Command column packet/key specialization. Generic samplers stay unchanged.
//! External16x16 RGB565 tiles; the native command cache owns its16x8 lines.

fn once(text: &mut String, old: &str, new: &str) -> Result<(), String> {
    if text.matches(old).count() != 1 {
        return Err(format!("Texture16x8 anchor: {old}"));
    }
    *text = text.replacen(old, new, 1);
    Ok(())
}

fn edit(
    source: &mut String,
    name: &str,
    f: impl FnOnce(&mut String) -> Result<(), String>,
) -> Result<(), String> {
    let start = source
        .find(&format!("module {name}("))
        .or_else(|| source.find(&format!("module {name} #")))
        .ok_or_else(|| format!("Texture16x8 module {name}"))?;
    let end = start
        + source[start..]
            .find("endmodule")
            .ok_or("Texture16x8 module end")?
        + 9;
    let mut body = source[start..end].to_owned();
    f(&mut body)?;
    source.replace_range(start..end, &body);
    Ok(())
}

pub fn lower(mut source: String) -> Result<String, String> {
    edit(&mut source, "membership_leaf", |s| {
        once(
            s,
            "wire sx=(common0[63:60]<=3)||(common0[52:50]!=7);",
            "wire sx=(common0[63:60]<=4)||(common0[52:49]!=15);",
        )?;
        once(
            s,
            "in_c2[2:0],in_c0[2:0],in_c2[9:3],in_c0[9:3]",
            "in_c2[2:0],in_c0[3:0],in_c2[9:3],in_c0[9:4]",
        )
    })?;
    edit(&mut source, "packet_leaf_unique", |s| {
        *s = include_str!("texture_packet16x8.v").to_owned();
        Ok(())
    })?;
    // H is the canonical256B line address. Its15 bits plus7 localXY bits
    // retain Group35 and the qualified native FIFO/lookahead calendar.
    for name in [
        "gpu_v2_packet_store_unique",
        "gpu_v2_runtime_packet_backend_unique",
        "gpu_v2_texture_runtime_backend_unique",
        "gpu_v2_vliw_sampling",
    ] {
        edit(&mut source, name, |s| {
            *s = s
                .replace("[15:0]next_key", "[14:0]next_key")
                .replace("[15:0] next_key", "[14:0] next_key")
                .replace("[15:0]packet_next_key", "[14:0]packet_next_key");
            if name == "gpu_v2_packet_store_unique" {
                once(
                    s,
                    "assign next_key=data_do[15:0]",
                    "assign next_key=data_do[14:0]",
                )?;
            }
            Ok(())
        })?;
    }
    Ok(source)
}
