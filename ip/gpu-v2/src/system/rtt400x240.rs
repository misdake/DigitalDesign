//! Production fixed400x240 coordinate specialization over the shared column cache.
//! No runtime width/height, mip chain, repeat mode or CPU format conversion.

fn once(s: &mut String, old: &str, new: &str) -> Result<(), String> {
    if s.matches(old).count() != 1 {
        return Err(format!("RTT400 anchor: {old}"));
    }
    *s = s.replacen(old, new, 1);
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
        .ok_or_else(|| format!("RTT400 module {name}"))?;
    let end = start + source[start..].find("endmodule").ok_or("RTT400 end")? + 9;
    let mut body = source[start..end].to_owned();
    f(&mut body)?;
    source.replace_range(start..end, &body);
    Ok(())
}

pub fn lower(mut source: String) -> Result<String, String> {
    edit(&mut source, "gpu_command_processor", |s| {
        once(
            s,
            "output reg texture_mip,output reg[1:0]texture_filter,",
            "output reg texture_mip,texture_fb400240,output reg[1:0]texture_filter,",
        )?;
        once(
            s,
            "texture_mip<=0;texture_filter<=0;",
            "texture_mip<=0;texture_fb400240<=0;texture_filter<=0;",
        )?;
        once(
            s,
            "texture_mip<=cp_q[20];texture_filter<=cp_q[22:21];",
            "texture_mip<=cp_q[20];texture_fb400240<=cp_q[23];texture_filter<=cp_q[22:21];",
        )?;
        once(s,"0:object_bad=cp_q[63:56]!=0; // Texture fields are trusted driver data.",
             "0:object_bad=cp_q[63:56]!=0||(cp_q[23]&&(cp_q[19:16]!=9||cp_q[20]||cp_q[22:21]>1||cp_q[31:24]!=0||cp_q[8:0]!=0||cp_q[15:0]>16'd63488)); // Fixed view ABI; ordinary POT fields retain their contract.")
    })?;
    edit(&mut source, "gpu_v2_vliw_command", |s| {
        once(
            s,
            "wire texture_mip;wire[1:0]texture_filter;",
            "wire texture_mip,texture_fb400240;wire[1:0]texture_filter;",
        )?;
        if s.matches(".texture_filter(texture_filter),").count() != 2 {
            return Err("RTT400 top connections".into());
        }
        *s = s.replace(
            ".texture_filter(texture_filter),",
            ".texture_filter(texture_filter),.texture_fb400240(texture_fb400240),",
        );
        Ok(())
    })?;
    for name in [
        "gpu_v2_vliw_three_port",
        "gpu_v2_vliw_backend_three_port",
        "gpu_v2_vliw_sampling",
    ] {
        edit(&mut source, name, |s| {
            once(
                s,
                "input texture_mip,input[1:0]texture_filter,",
                "input texture_mip,input texture_fb400240,input[1:0]texture_filter,",
            )?;
            if name != "gpu_v2_vliw_sampling" {
                once(
                    s,
                    ".texture_filter(texture_filter),",
                    ".texture_filter(texture_filter),.texture_fb400240(texture_fb400240),",
                )?;
            } else {
                let uv = r#" function[15:0]view_clamp_uv;
  input[17:0]raw_uv;
  begin
   if(raw_uv[17])view_clamp_uv=16'd0;
   else if(raw_uv[16])view_clamp_uv=16'hffff;
   else view_clamp_uv=raw_uv[15:0];
  end
 endfunction
 wire[31:0]view_uv={view_clamp_uv(sample_data[35:18]),view_clamp_uv(sample_data[17:0])};
"#;
                once(
                    s,
                    " gpu_v2_texture_direct_lod sampling(",
                    &format!("{uv} gpu_v2_texture_direct_lod sampling("),
                )?;
                once(s,".in_uv({sample_data[33:18],sample_data[15:0]}),",
                     ".in_uv(texture_fb400240?view_uv:{sample_data[33:18],sample_data[15:0]}),.texture_fb400240(texture_fb400240),")?;
                once(s,".texture_base(texture_base),.texture_mip(texture_mip),",
                     ".texture_base(texture_base),.texture_mip(texture_mip),.texture_fb400240(texture_fb400240),")?;
            }
            if name == "gpu_v2_vliw_three_port" {
                once(s, "reg[253:0]held_config", "reg[254:0]held_config")?;
                once(s, "wire[253:0]live_config", "wire[254:0]live_config")?;
                once(s,"draw_unlit,texture_base,texture_n,texture_mip,texture_filter,specular",
                     "draw_unlit,texture_base,texture_n,texture_mip,texture_filter,texture_fb400240,specular")?;
            }
            Ok(())
        })?;
    }
    for name in [
        "gpu_v2_texture_runtime_backend_unique",
        "gpu_v2_runtime_packet_backend_unique",
        "packet_leaf_unique",
    ] {
        edit(&mut source, name, |s| {
            once(
                s,
                "input texture_mip,",
                "input texture_mip,input texture_fb400240,",
            )
            .or_else(|_| {
                once(
                    s,
                    "input wire texture_mip,",
                    "input wire texture_mip,input wire texture_fb400240,",
                )
            })?;
            if name != "packet_leaf_unique" {
                once(s,".texture_base(texture_base),.texture_mip(texture_mip),",
                     ".texture_base(texture_base),.texture_mip(texture_mip),.texture_fb400240(texture_fb400240),")?;
            } else {
                once(
                    s,
                    "wire[3:0]pitch_y_log2=(level<4)?4'd0:(level-4'd4);",
                    "wire[3:0]pitch_y_log2=texture_fb400240?4'd4:((level<4)?4'd0:(level-4'd4));",
                )?;
                let end = s.rfind("endmodule").ok_or("RTT packet end")?;
                s.insert_str(end,"`ifndef SYNTHESIS\n always@(posedge clk)if(!reset&&ce&&valid[0]&&texture_fb400240&&(tx>=25||ty>=30))$fatal(1,\"RTT400 Group bounds\");\n`endif\n");
            }
            Ok(())
        })?;
    }
    edit(&mut source, "gpu_v2_texture_direct_lod", |s| {
        once(
            s,
            "input in_valid,input[31:0]in_uv,",
            "input texture_fb400240,input in_valid,input[31:0]in_uv,",
        )?;
        once(s,".in_nearest(in_filter==0),.in_size_log2(physical),",
             ".texture_fb400240(texture_fb400240),.in_nearest(in_filter==0),.in_size_log2(physical),")
    })?;
    edit(&mut source, "gpu_v2_texture_serial_coord", |s| {
        once(
            s,
            "input in_nearest,input[3:0]in_size_log2,",
            "input texture_fb400240,input in_nearest,input[3:0]in_size_log2,",
        )?;
        once(
            s,
            " wire signed[18:0]fine=$signed({1'b0,scaled_uv})-(in_nearest?19'sd0:19'sd128);",
            "",
        )?;
        let arithmetic = r#" // U=25*q=q+8*q+16*q; V=15*q=16*q-q. Compress the
 // three operands before one carry-propagating add; no DSP or stage is added.
 wire view_subtract=!phase[0];
 wire[20:0]view_a={5'd0,axis_uv}^{21{view_subtract}};
 wire[20:0]view_b={2'd0,axis_uv,3'd0}&{21{phase[0]}};
 wire[20:0]view_c={1'b0,axis_uv,4'd0};
 wire[20:0]view_sum=view_a^view_b^view_c;
 wire[20:0]view_majority=(view_a&view_b)|(view_a&view_c)|(view_b&view_c);
 wire[20:0]view_carry={view_majority[19:0],view_subtract};
 wire[20:0]view_product=view_sum+view_carry;
 wire[16:0]view_scaled=view_product[20:4];
 // Select the scale before the one signed half-texel subtractor. Both modes
 // use the existing U/V phases and freeze all operands on a disabled CE.
 wire[17:0]shared_scaled=texture_fb400240?{1'b0,view_scaled}:scaled_uv;
 wire signed[18:0]fine=$signed({1'b0,shared_scaled})-(in_nearest?19'sd0:19'sd128);
 wire view_negative=fine[18];
 wire[16:0]view_fine=fine[16:0];
 wire[8:0]view_last=phase[0]?9'd399:9'd239;
 reg[17:0]view_result;
 always@*begin
  if(view_negative)view_result=18'd0;
  // A16-bit preclamped UV gives floor(400*q/65536)<=399 (V<=239).
  // Negative fine is handled above; only equality can reach the last texel.
  else if(view_fine[16:8]==view_last)view_result={1'b0,view_last,8'd0};
  else view_result={1'b0,view_fine};
 end
 wire[17:0]axis_result=texture_fb400240?view_result:{fine[17:8]&mask0,fine[7:0]};"#;
        once(
            s,
            " wire[17:0]axis_result={fine[17:8]&mask0,fine[7:0]};",
            arithmetic,
        )?;
        once(s, "reg[20:0]source_guard;", "reg[21:0]source_guard;")?;
        if s.matches("{in_wrapped_uv_1,in_size_log2,in_nearest}")
            .count()
            != 2
        {
            return Err("RTT400 coord source guard".into());
        }
        *s = s.replace(
            "{in_wrapped_uv_1,in_size_log2,in_nearest}",
            "{in_wrapped_uv_1,in_size_log2,in_nearest,texture_fb400240}",
        );
        Ok(())
    })?;
    Ok(source)
}
