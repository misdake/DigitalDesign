//! Fixed-grid UI specialization of the command composition only.
//! Generic component interfaces and their independent oracles stay unchanged.

const PORTS: &str = "input ui_active,ui_cmd_valid,ui_cmd_write,ui_lower_ready,input[8:0]ui_tile,output ui_cmd_ready,ui_cmd_done,input ui_pixel_valid,ui_pixel_half,input[4:0]ui_pixel_word,input[63:0]ui_pixels,";
const PINS: &str = ".ui_active(ui_active),.ui_cmd_valid(ui_cmd_valid),.ui_cmd_write(ui_cmd_write),.ui_lower_ready(ui_lower_ready),.ui_tile(ui_tile),.ui_cmd_ready(ui_cmd_ready),.ui_cmd_done(ui_cmd_done),.ui_pixel_valid(ui_pixel_valid),.ui_pixel_half(ui_pixel_half),.ui_pixel_word(ui_pixel_word),.ui_pixels(ui_pixels),";

fn once(s: &mut String, old: &str, new: &str) -> Result<(), String> {
    if s.matches(old).count() != 1 {
        return Err(format!("UI anchor changed: {old}"));
    }
    *s = s.replacen(old, new, 1);
    Ok(())
}

pub fn lower(mut source: String) -> Result<String, String> {
    for (module, instance) in [
        (
            "gpu_v2_vliw_three_port",
            "gpu_v2_vliw_backend_three_port back(",
        ),
        (
            "gpu_v2_vliw_backend_three_port",
            "gpu_v2_pixel_framebuffer_sink sink(",
        ),
        ("gpu_v2_pixel_framebuffer_sink", " cache("),
        ("gpu_v2_framebuffer_tdp", ""),
    ] {
        let begin = source
            .find(&format!("module {module}("))
            .or_else(|| source.find(&format!("module {module} #")))
            .ok_or_else(|| format!("UI module {module}"))?;
        let end = begin + source[begin..].find("endmodule").ok_or("UI end")? + 9;
        let mut s = source[begin..end].to_owned();
        let p = s.find("\n input").ok_or("UI ports")?;
        s.insert_str(p, &format!("\n {PORTS}"));
        if !instance.is_empty() {
            once(&mut s, instance, &format!("{instance}\n {PINS}"))?;
        } else {
            once(
                &mut s,
                "wire[2:0]mt_end=mt_write&&!mt_remaining[1]?",
                "wire[2:0]mt_end=(mt_write&&!mt_remaining[1]&&!ui_active)?",
            )?;
            once(
                &mut s,
                "wire only_color=mt_write&&!mt_remaining[1];",
                "wire only_color=mt_write&&!mt_remaining[1]&&!ui_active;",
            )?;
            once(&mut s, "wire[8:0]mt_linear_tile=mt_linear_sum[6];", "wire[8:0]mt_linear_tile=ui_active?ui_tile:mt_linear_sum[6];\n wire mt_half=ui_active?mt_issue[1]:mt_tile[5];\n assign ui_cmd_ready=ui_active&&!mt_active&&!flushing;\n assign ui_cmd_done=ui_active&&mt_active&&mem_complete_valid&&(mt_write?retire_last:mt_retire[0]);")?;
            once(
                &mut s,
                "(mt_issue[1]?depth_base[13:7]:color_base[13:7])",
                "((mt_issue[1]&&!ui_active)?depth_base[13:7]:color_base[13:7])",
            )?;
            once(&mut s, "wire[9:0]maintenance_write_addr={mt_data[5],1'b0,mt_line,mt_data[4:0]};", "wire[9:0]maintenance_write_addr=ui_active?{4'd0,mt_data[5:0]}:{mt_data[5],1'b0,mt_line,mt_data[4:0]};")?;
            once(&mut s, "wire[9:0]maintenance_read_addr={maintenance_read_word[5],1'b0,mt_line,maintenance_read_word[4:0]};", "wire[9:0]maintenance_read_addr=ui_active?{4'd0,maintenance_read_word[5:0]}:{maintenance_read_word[5],1'b0,mt_line,maintenance_read_word[4:0]};")?;
            once(&mut s, "wire m_rd_issue=mt_active&&mt_write&&data_left&&(!wr_beat_valid||(wbeat&&!data_last));\n wire[6:0]maintenance_read_word=mt_data+{6'd0,wbeat};", "wire[6:0]maintenance_read_word=mt_data+{6'd0,wbeat};\n wire m_rd_issue=mt_active&&mt_write&&data_left&&(!wr_beat_valid||(wbeat&&!data_last))&&(!ui_active||!maintenance_read_word[5]||ui_lower_ready);")?;
            once(
                &mut s,
                "if(data_last)wr_beat_valid<=0;",
                "if(data_last||(mt_write&&!m_rd_issue))wr_beat_valid<=0;",
            )?;
            once(&mut s, "assign mem_req_valid=mt_request&&!reset;", "assign mem_req_valid=mt_request&&!reset&&(!ui_active||!mt_write||!mt_issue[1]||ui_lower_ready);")?;
            once(
                &mut s,
                "mt_linear_tile[6:0],mt_tile[5],mt_issue[0]",
                "mt_linear_tile[6:0],mt_half,mt_issue[0]",
            )?;
            once(
                &mut s,
                "if(!mt_write&&!stop_publication&&!mt_cancel)begin",
                "if(!ui_active&&!mt_write&&!stop_publication&&!mt_cancel)begin",
            )?;
            once(&mut s, "if(flushing)begin\n     if(dirty_found)", "if(ui_cmd_valid&&ui_cmd_ready)begin\n     mt_active<=1;mt_cancel<=0;mt_request<=0;mt_prefetch<=0;mt_line<=0;mt_tvalid<=0;mt_write<=ui_cmd_write;\n     mt_remaining<=1;mt_issue<=0;mt_retire<=0;mt_data<=0;wr_beat_valid<=0;\n    end else if(flushing)begin\n     if(dirty_found)")?;
            once(&mut s, "wire a_en=a_read||a_write_d||a_write_c;", "wire[15:0]up=ui_pixels[n*16+:16];\n  wire uw=ui_pixel_valid&&up[0];\n  wire a_en=ui_active?uw:(a_read||a_write_d||a_write_c);")?;
            once(
                &mut s,
                "wire a_we=a_write_d||a_write_c;",
                "wire a_we=ui_active?uw:(a_write_d||a_write_c);",
            )?;
            once(&mut s, "wire[9:0]a_addr=a_we?{a_write_d,result_addr}:incoming_addr;", "wire[9:0]a_addr=ui_active?{4'd0,ui_pixel_half,ui_pixel_word}:(a_we?{a_write_d,result_addr}:incoming_addr);")?;
            once(&mut s, "wire[15:0]a_data=depth_pending?depth_data:color_data;", "wire[15:0]a_data=ui_active?{up[15:11],up[10:6],up[10],up[5:1]}:(depth_pending?depth_data:color_data);")?;
            once(&mut s, "if(a_read&&(a_write_d||a_write_c))$fatal(1,\"TDP A static calendar collision\");", "if(ui_active&&a_we&&b_en&&a_addr==b_addr)$fatal(1,\"UI half-buffer collision\");\n   if(ui_active&&mt_active&&mt_write&&mt_issue>=3&&!ui_lower_ready)$fatal(1,\"UI lower readiness withdrew after publication\");\n   if(a_read&&(a_write_d||a_write_c))$fatal(1,\"TDP A static calendar collision\");")?;
        }
        source.replace_range(begin..end, &s);
    }
    Ok(source)
}
