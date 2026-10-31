//! Current immutable-draw texture binding for the command profile.
//! Reuse the selected numerical pipeline, removing its factory Slot16 binding.
//! The generic standalone samplers retain their independently tested interface.

const PORTS: &str = "input[15:0]texture_base,input[3:0]texture_n,input texture_mip,input[1:0]texture_filter,input texture_invalidate,";
const CONNECT: &str = ".texture_base(texture_base),.texture_n(texture_n),.texture_mip(texture_mip),.texture_filter(texture_filter),.texture_invalidate(texture_invalidate),";

fn once(text: &mut String, old: &str, new: &str) -> Result<(), String> {
    if text.matches(old).count() != 1 {
        return Err(format!("Current texture anchor changed: {old}"));
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
        .ok_or_else(|| format!("Current texture module {name}"))?;
    let end = start + source[start..].find("endmodule").ok_or("Module end")? + 9;
    let mut body = source[start..end].to_owned();
    f(&mut body)?;
    source.replace_range(start..end, &body);
    Ok(())
}

pub fn lower(mut source: String, native: bool) -> Result<String, String> {
    source = source.replace("\r\n", "\n");
    edit(&mut source, "gpu_v2_vliw_three_port", |s| {
        once(s, "input[31:0]cfg_far_z,input[4:0]cfg_tex_w_log2,cfg_tex_h_log2,cfg_max_mip,input signed[7:0]cfg_lod_bias,",
            &format!("input[31:0]cfg_far_z,input signed[7:0]cfg_lod_bias,\n {PORTS}"))?;
        once(
            s,
            "input draw_bank,draw_unlit,input[26:0]draw_config,",
            "input draw_bank,draw_unlit,",
        )?;
        once(s, " wire[3:0]frontend_error,raster_error;",
            " // Both LOD axes and the sampler derive from the one installed descriptor.\n wire[4:0]cfg_tex_w_log2={1'b0,texture_n},cfg_tex_h_log2={1'b0,texture_n};\n wire[4:0]cfg_max_mip=texture_mip?{1'b0,texture_n}:5'd0;\n wire[3:0]frontend_error,raster_error;")?;
        once(s, ".draw_config(draw_config),", CONNECT)?;
        once(
            s,
            "draw_bank,draw_unlit,draw_config,specular",
            "draw_bank,draw_unlit,texture_base,texture_n,texture_mip,texture_filter,specular",
        )?;
        // This simulation-only stability checker observes installed fields once.
        once(s, "reg[257:0]held_config", "reg[253:0]held_config")?;
        once(s, "wire[257:0]live_config", "wire[253:0]live_config")
    })?;
    edit(&mut source, "gpu_v2_vliw_backend_three_port", |s| {
        once(s, "input wire[26:0]draw_config,", PORTS)?;
        once(s, ".draw_config(draw_config),", CONNECT)
    })?;
    edit(&mut source, "gpu_v2_vliw_sampling", |s| {
        once(s, "input wire[26:0] draw_config,", PORTS)?;
        once(
            s,
            "output wire[35:0] sample_data,raw_metadata,",
            "output wire[35:0] sample_data,output wire[31:0]raw_metadata,",
        )?;
        once(s, "assign raw_metadata={draw_config[26:25],draw_config[24],draw_config[23:20],\n   draw_config[19:16],sample_descriptor[8:5],sample_descriptor[3:0],",
            "assign raw_metadata={texture_filter,texture_mip,texture_n,\n   sample_descriptor[8:5],sample_descriptor[3:0],")?;
        once(s, ".draw_slot(draw_config[19:16]),", ".texture_base(texture_base),.texture_mip(texture_mip),.texture_invalidate(texture_invalidate),")?;
        once(s, ".in_slot(draw_config[19:16]),", "")?;
        once(s, ".in_max_n(draw_config[23:20]),.in_has_mip(draw_config[24]),.in_filter(draw_config[26:25])",
            ".in_max_n(texture_n),.in_has_mip(texture_mip),.in_filter(texture_filter)")?;
        once(
            s,
            ".next_key({packet_next_key,draw_config[19:16]})",
            ".next_key(packet_next_key)",
        )
    })?;
    edit(&mut source, "gpu_v2_texture_direct_lod", |s| {
        once(s, "in_quad,in_mask,in_slot,", "in_quad,in_mask,")?;
        once(
            s,
            "in_max_n>10||in_filter>2||!in_mask[in_lane]",
            "!in_mask[in_lane]",
        )?;
        once(s, "in_quad,in_lane,in_slot,", "in_quad,in_lane,4'd0,")
    })?;
    // Select the actual command cache owner; generic cache factories stay checked.
    edit(&mut source, "gpu_v2_texture_runtime_cache_unique", |s| {
        *s = include_str!("texture_cache16x8.v")
            .trim_end()
            .replace("@NATIVE@", if native { "1" } else { "0" });
        Ok(())
    })?;
    physical_groups(&mut source)?;
    for forbidden in [
        "slot_base_f",
        "slot_valid_f",
        "slot_max_f",
        "slot_full_f",
        "draw_config",
        "draw_slot",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "Legacy current-texture binding survived: {forbidden}"
            ));
        }
    }
    Ok(source)
}

/// Retire logical addressing at the producer, before the ordered Group RAM.
/// All consumers share the stored physical sector; no address work feeds ready.
fn physical_groups(source: &mut String) -> Result<(), String> {
    edit(source, "packet_leaf_unique", |s| {
        *s = include_str!("texture_packet.v").to_owned();
        Ok(())
    })?;
    for name in [
        "gpu_v2_texture_runtime_backend_unique",
        "gpu_v2_runtime_packet_backend_unique",
        "gpu_v2_packet_store_unique",
    ] {
        edit(source, name, |s| {
            *s = s
                .replace("[33:0]", "[34:0]")
                .replace("[17:0] next_key", "[15:0] next_key")
                .replace("[17:0]next_key", "[15:0]next_key");
            if name == "gpu_v2_packet_store_unique" {
                once(s, "head0[17:0]:head1[17:0]", "head0[15:0]:head1[15:0]")?;
                once(s, "data_do[17:0]", "data_do[15:0]")?;
                once(s, ".DI({2'd0,write_payload})", ".DI({1'b0,write_payload})")?;
            } else {
                once(
                    s,
                    " input wire clk,",
                    " input wire[15:0]texture_base,input wire texture_mip,\n input wire clk,",
                )?;
                if name == "gpu_v2_texture_runtime_backend_unique" {
                    once(s," .issue_enable(packet_enable),", " .texture_base(texture_base),.texture_mip(texture_mip),.issue_enable(packet_enable),")?;
                } else {
                    once(s,".ce(advance),.in_valid(issue),", ".ce(advance),.texture_base(texture_base),.texture_mip(texture_mip),.in_valid(issue),")?;
                }
            }
            Ok(())
        })?;
    }
    edit(source, "gpu_v2_vliw_sampling", |s| {
        once(s, "[33:0]packet_out_payload", "[34:0]packet_out_payload")?;
        once(s, "[17:0]packet_next_key", "[15:0]packet_next_key")?;
        once(s,".ce(ce),.commit_allow(allow),.abort(1'b0),", ".ce(ce),.commit_allow(allow),.abort(1'b0),.texture_base(texture_base),.texture_mip(texture_mip),")?;
        once(s,".texture_base(texture_base),.texture_mip(texture_mip),.texture_invalidate(texture_invalidate),", ".texture_invalidate(texture_invalidate),")
    })?;
    edit(source, "gpu_v2_texture_runtime_color_checked", |s| {
        once(
            s,
            "wire packet_bad=accepted&&(in_payload[7:4]>4'd10);",
            "wire packet_bad=1'b0; // Physical groups have no redundant level field.",
        )
    })
}
