//! Runtime target and mesh color for the single-context command composition.
//! Generic leaf exporters retain their independently checked interfaces.

fn once(s: &mut String, old: &str, new: &str) -> Result<(), String> {
    if s.matches(old).count() != 1 {
        return Err(format!("Current state anchor changed: {old}"));
    }
    *s = s.replacen(old, new, 1);
    Ok(())
}
fn edit(
    source: &mut String,
    name: &str,
    f: impl FnOnce(&mut String) -> Result<(), String>,
) -> Result<(), String> {
    let a = source
        .find(&format!("module {name}("))
        .or_else(|| source.find(&format!("module {name} #")))
        .ok_or_else(|| format!("Current state module {name}"))?;
    let b = a + source[a..].find("endmodule").ok_or("State module end")? + 9;
    let mut s = source[a..b].to_owned();
    f(&mut s)?;
    source.replace_range(a..b, &s);
    Ok(())
}
fn ports(s: &mut String, text: &str) -> Result<(), String> {
    let p = s
        .find("\n input")
        .or_else(|| s.find("\ninput"))
        .ok_or("State port boundary")?;
    s.insert_str(p, &format!("\n {text}"));
    Ok(())
}
fn link(s: &mut String, instance: &str, pins: &str) -> Result<(), String> {
    once(s, instance, &format!("{instance}\n {pins}"))
}
const TARGET_PORTS: &str = "input[13:0]color_base,depth_base,";
const TARGET_PINS: &str = ".color_base(color_base),.depth_base(depth_base),";

pub fn lower(mut source: String) -> Result<String, String> {
    edit(&mut source, "gpu_v2_vliw_three_port", |s| {
        ports(s, &format!("{TARGET_PORTS}input[31:0]mesh_color,"))?;
        once(
            s,
            " wire[3:0]frontend_error,raster_error;",
            " wire[31:0]quad_color;\n wire[3:0]frontend_error,raster_error;",
        )?;
        link(
            s,
            "gpu_v2_vliw_backend_three_port back(",
            &format!("{TARGET_PINS}.quad_color(quad_color),"),
        )?;
        link(
            s,
            "gpu_v2_vliw_frontend front(",
            ".mesh_color(mesh_color),.quad_color(quad_color),",
        )?;
        once(s, "input draw_bank,draw_unlit,", "input draw_unlit,")?;
        *s = s
            .replace(".draw_bank(draw_bank),", "")
            .replace("draw_bank,draw_unlit,", "draw_unlit,")
            .replace("input[7:0]alpha,", "")
            .replace(".alpha(alpha),", "")
            .replace("specular,alpha,", "specular,");
        Ok(())
    })?;
    edit(&mut source, "gpu_v2_vliw_frontend", |s| {
        ports(s, "input[31:0]mesh_color,output[31:0]quad_color,")?;
        link(s, "shared_meshlet meshlet(", ".mesh_color(mesh_color),")?;
        link(
            s,
            "gpu_v2_vliw_interpolator interpolator(",
            ".pool_color(quad_color),",
        )
    })?;
    edit(&mut source, "shared_meshlet", |s| {
        ports(s, "input[31:0]mesh_color,")?;
        link(s, " core(\n", ".mesh_color(mesh_color),")
    })?;
    edit(&mut source, "shared_frontend", |s| {
        ports(s, "input[31:0]mesh_color,")?;
        once(
            s,
            "reg vertex_epoch;wire export_epoch;",
            "reg vertex_epoch;wire export_epoch;\n reg[31:0]epoch_color[0:1];\n wire[31:0]export_color=epoch_color[export_epoch];",
        )?;
        once(
            s,
            "if(meshlet_reset)begin published_vertices<=0;",
            "if(meshlet_reset)begin epoch_color[!vertex_epoch]<=mesh_color;published_vertices<=0;",
        )?;
        once(
            s,
            "word_data<=response_index<10 ? {export_hi,export_q} : {18'd0,export_q};",
            "word_data<=response_index<10 ? {export_hi,export_q} :\n response_index==10?{export_color[17:0],export_q}:\n response_index==11?{4'd0,export_color[31:18],export_q}:{18'd0,export_q};",
        )
    })?;
    edit(&mut source, "gpu_v2_vliw_interpolator", |s| {
        ports(s, "output reg[31:0]pool_color,")?;
        once(
            s,
            "wire cfg_apply,context_ready,sc_rgb;",
            "reg[31:0]prepared_color;\n wire cfg_apply,context_ready,sc_rgb;",
        )?;
        once(
            s,
            "if(reset)begin rgb<=0;",
            "if(reset)begin pool_color<=32'hffffffff;prepared_color<=32'hffffffff;rgb<=0;",
        )?;
        once(
            s,
            "if(cfg_apply)begin rgb<=sc_rgb;",
            "if(raw_valid&&raw_ready)begin\n    if(raw_index==10)prepared_color[17:0]<=raw_data[53:36];\n    if(raw_index==11)prepared_color[31:18]<=raw_data[49:36];\n   end\n   if(cfg_apply)begin pool_color<=prepared_color;rgb<=sc_rgb;",
        )
    })?;
    edit(&mut source, "gpu_v2_vliw_backend_three_port", |s| {
        ports(s, &format!("{TARGET_PORTS}input[31:0]quad_color,"))?;
        link(s, "gpu_v2_pixel_framebuffer_sink sink(", TARGET_PINS)?;
        link(
            s,
            "gpu_v2_vliw_sampling front(",
            ".quad_color(quad_color),.issue_color(issue_color),",
        )?;
        once(
            s,
            "wire[6:0]basic_address;",
            "wire[23:0]issue_color;\n wire[6:0]basic_address;",
        )?;
        once(s, "wire[15:0]depth_data;", "wire[23:0]depth_data;")?;
        once(
            s,
            ".depth(depth_data),",
            ".depth(depth_data[15:0]),.depth_alpha(depth_data[23:16]),",
        )?;
        once(
            s,
            "wire[23:0]final_texture=input_bypass[1]?24'hffffff:sample_data;",
            "wire[23:0]final_texture=input_bypass[1]?basic_data:sample_data;",
        )?;
        once(s, ".in_tint(basic_data),", ".in_tint(issue_color),")?;
        once(
            s,
            ".rgba({alpha,out_covered?final_RGB:24'd0})",
            ".rgba({8'd0,out_covered?final_RGB:24'd0})",
        )?;
        once(
            s,
            "input wire draw_bank,draw_unlit,",
            "input wire draw_unlit,",
        )?;
        *s = s
            .replace(".draw_bank(draw_bank),", "")
            .replace("input wire[7:0]alpha,", "");
        once(
            s,
            ".light_bank(),.retire_valid(draw_rebind&&pipeline_idle),.retire_bank(1'b0),",
            ".retire_valid(draw_rebind&&pipeline_idle),",
        )
    })?;
    edit(&mut source, "gpu_v2_vliw_sampling", |s| {
        ports(s, "input[31:0]quad_color,output[23:0]issue_color,")?;
        link(
            s,
            "gpu_v2_vliw_quad_pool quad(",
            ".quad_color(quad_color),.issue_color(issue_color),",
        )?;
        once(
            s,
            "output wire[15:0]depth_data,",
            "output wire[23:0]depth_data,",
        )?;
        once(
            s,
            "input wire draw_bank,draw_unlit,",
            "input wire draw_unlit,",
        )?;
        *s = s
            .replace(".draw_bank(draw_bank),", "")
            .replace(
                "output wire light_bank,input wire retire_valid,retire_bank,",
                "input wire retire_valid,",
            )
            .replace(" assign light_bank=light_descriptor[9];", "")
            .replace(".source_bank(light_bank),", "")
            .replace(
                ".retire_valid(retire_valid),.retire_bank(retire_bank),",
                ".retire_valid(retire_valid),",
            );
        Ok(())
    })?;
    edit(&mut source, "gpu_v2_vliw_quad_pool", |s| {
        ports(s, "input[31:0]quad_color,output[23:0]issue_color,")?;
        once(s, "input draw_bank,draw_unlit,", "input draw_unlit,")?;
        once(
            s,
            "quad_rgb,draw_unlit,draw_bank}",
            "quad_rgb,draw_unlit,1'b0}",
        )?;
        once(
            s,
            "wire[18:0]issue_meta,light_meta,publish_meta;",
            "wire[18:0]issue_meta,light_meta,publish_meta;\n gpu_v2_vliw_pool_meta #(.WIDTH(24)) ci(clk,reset,allocate,allocated[4:0],quad_color[23:0],descriptor_issue_slot,issue_color);",
        )?;
        once(s, "output[15:0]depth_data,", "output[23:0]depth_data,")?;
        once(
            s,
            "attr_pixel[6:0],depth_event_z16,",
            "attr_pixel[6:0],{quad_color[31:24],depth_event_z16},",
        )
    })?;
    edit(&mut source, "gpu_v2_vliw_pool_depth_ram", |s| {
        *s = s.replace("[15:0]", "[23:0]");
        once(
            s,
            ".BIT_WIDTH_0(18),.BIT_WIDTH_1(18)",
            ".BIT_WIDTH_0(36),.BIT_WIDTH_1(36)",
        )?;
        once(
            s,
            ".DI({20'd0,wd}),.ADA({3'd0,wa,4'b0011}),.ADB({3'd0,ra,4'b0000})",
            ".DI({12'd0,wd}),.ADA({2'd0,wa,1'b0,4'b1111}),.ADB({2'd0,ra,5'd0})",
        )
    })?;
    edit(&mut source, "gpu_v2_pixel_lighting_consumer", |s| {
        once(
            s,
            "input wire[3:0]source_mask,input wire source_bank,",
            "input wire[3:0]source_mask,",
        )?;
        once(
            s,
            "input wire retire_valid,retire_bank,",
            "input wire retire_valid,",
        )?;
        once(s, "reg loaded_valid,loaded_bank;", "reg loaded_valid;")?;
        once(
            s,
            "(!loaded_valid||loaded_bank!=source_bank)",
            "!loaded_valid",
        )?;
        *s = s
            .replace("loaded_bank<=0;", "")
            .replace(
                "retire_valid&&loaded_valid&&retire_bank==loaded_bank",
                "retire_valid",
            )
            .replace("loaded_bank<=source_bank;", "");
        Ok(())
    })?;
    edit(&mut source, "gpu_v2_pixel_framebuffer_sink", |s| {
        ports(s, &format!("{TARGET_PORTS}input[7:0]depth_alpha,"))?;
        let a = s
            .find(" parameter [31:0] COLOR_BASE=")
            .ok_or("Fixed target defaults")?;
        let b = a + s[a..].find('\n').ok_or("Target defaults end")?;
        s.replace_range(a..b, "");
        once(
            s,
            "gpu_v2_framebuffer_tdp #(.COLOR_BASE(COLOR_BASE),.DEPTH_BASE(DEPTH_BASE),",
            "gpu_v2_framebuffer_tdp #(",
        )?;
        link(
            s,
            "cache(\n",
            &format!("{TARGET_PINS}.row_alpha(depth_alpha),"),
        )
    })?;
    edit(&mut source, "gpu_v2_framebuffer_tdp", |s| {
        ports(s, &format!("{TARGET_PORTS}input[7:0]row_alpha,"))?;
        once(s, "reg[31:0]source_rgba;", "reg[23:0]source_rgb;")?;
        *s = s
            .replace("source_rgba[7:0]", "source_rgb[7:0]")
            .replace("source_rgba[15:8]", "source_rgb[15:8]")
            .replace("source_rgba[23:16]", "source_rgb[23:16]");
        once(s, ".src_a(source_rgba[31:24])", ".src_a(row_alpha)")?;
        once(s, "source_rgba<=row_data;", "source_rgb<=row_data[23:0];")?;
        once(s, " parameter[31:0]COLOR_BASE=0,DEPTH_BASE=16384,\n", "")?;
        once(
            s,
            "assign mem_req_addr=(mt_issue[1]?DEPTH_BASE:COLOR_BASE)+{14'd0,mt_linear_tile,mt_tile[5],mt_issue[0],7'b0};",
            "wire[6:0]target_page=(mt_issue[1]?depth_base[13:7]:color_base[13:7])+{5'd0,mt_linear_tile[8:7]};\n assign mem_req_addr={9'd0,target_page,mt_linear_tile[6:0],mt_tile[5],mt_issue[0],7'd0};",
        )
    })?;
    direct_global(&mut source)?;
    registered_texture_return(&mut source)?;
    for old in [
        "draw_bank",
        "loaded_bank",
        "source_bank",
        "retire_bank",
        "COLOR_BASE",
        "DEPTH_BASE",
    ] {
        if source.contains(old) {
            return Err(format!("Obsolete current state survived: {old}"));
        }
    }
    Ok(source)
}

// The command profile changes globals only after a complete drain. Raw values
// therefore remain owned by the CP FFs through every downstream consumer.
fn direct_global(source: &mut String) -> Result<(), String> {
    edit(source, "rp_prepare", |s| {
        once(s, "reg[31:0]far_z;", "wire[31:0]far_z=cfg_far_z;")?;
        once(s, "far_z<=0;", "")?;
        once(s, "far_z<=cfg_far_z;", "")?;
        once(
            s,
            "reg[4:0]tex_u,tex_v,max_mip;reg signed[7:0]bias;",
            "wire[4:0]tex_u=cfg_tex_u,tex_v=cfg_tex_v,max_mip=cfg_max_mip;wire signed[7:0]bias=cfg_bias;",
        )?;
        once(s, "tex_u<=0;tex_v<=0;max_mip<=0;bias<=0;", "")?;
        once(
            s,
            "tex_u<=cfg_tex_u;tex_v<=cfg_tex_v;max_mip<=cfg_max_mip;bias<=cfg_bias;",
            "",
        )
    })?;
    edit(source, "rc_lod", |s| {
        // Keep the calculated bias register: its adder is outside the pixel path.
        once(
            s,
            "reg[4:0]tex_u,tex_v,max_mip;",
            "wire[4:0]tex_u=cfg_tex_u,tex_v=cfg_tex_v,max_mip=cfg_max_mip;",
        )?;
        once(s, "tex_u<=0;tex_v<=0;max_mip<=0;", "")?;
        once(
            s,
            "tex_u<=cfg_tex_u;tex_v<=cfg_tex_v;max_mip<=cfg_max_mip;",
            "",
        )
    })?;
    edit(source, "rc_compute", |s| {
        once(s, "reg[31:0]far_z;", "wire[31:0]far_z=cfg_far;")?;
        once(s, "far_z<=32'd13041664;", "")?;
        once(s, "far_z<=cfg_far;", "")
    })?;
    edit(source, "gpu_v2_lighting", |s| {
        once(
            s,
            " input wire context_valid, output wire context_ready,\n",
            "",
        )?;
        once(s, "reg configured;reg [1:0] phase;", "reg [1:0] phase;")?;
        once(
            s,
            "reg [1:0] ctx_mode; reg [4:0] ctx_code; reg [15:0] ctx_epoch;",
            "wire[1:0]ctx_mode=context_mode;wire[4:0]ctx_code=context_code;wire[15:0]ctx_epoch=context_epoch;",
        )?;
        once(
            s,
            "reg signed [15:0] ctx_l0,ctx_l1,ctx_l2,ctx_p0,ctx_p1,ctx_p2;",
            "wire signed[15:0]ctx_l0=light_x,ctx_l1=light_y,ctx_l2=light_z,ctx_p0=ray_x,ctx_p1=ray_y,ctx_p2=ray_k;",
        )?;
        once(
            s,
            "reg [8:0] ctx_i0,ctx_i1; reg [3:0] ctx_power;",
            "wire[8:0]ctx_i0=ambient,ctx_i1=directional;wire[3:0]ctx_power;",
        )?;
        once(
            s,
            "reg [3:0] context_rom [0:31];",
            "reg [3:0] context_rom [0:31];\nassign ctx_power=context_rom[context_code];",
        )?;
        once(s, "wire advance = ce && configured;", "wire advance = ce;")?;
        once(
            s,
            "wire datapath_ce = advance && !reset && !(context_valid && context_ready);",
            "wire datapath_ce = advance && !reset;",
        )?;
        once(
            s,
            "assign context_ready = ce && !reset && !(|valid_pipe);\n",
            "",
        )?;
        once(
            s,
            "assign in_ready = advance && !reset && phase==0 && !context_valid;",
            "assign in_ready = advance && !reset && phase==0;",
        )?;
        once(
            s,
            "configured<=0; phase<=0; calendar<=0;",
            "phase<=0; calendar<=4'b0001;",
        )?;
        let start = s
            .find("  if(context_valid && context_ready) begin")
            .ok_or("Lighting context install")?;
        let end = start
            + s[start..]
                .find("  end else if(advance) begin")
                .ok_or("Lighting context end")?;
        s.replace_range(
            start..end + "  end else if(advance) begin".len(),
            "  if(advance) begin",
        );
        Ok(())
    })?;
    edit(source, "gpu_v2_pixel_lighting_consumer", |s| {
        once(
            s,
            "input wire source_context_valid,source_valid,",
            "input wire source_valid,",
        )?;
        once(s, " input wire retire_valid,\n", "")?;
        once(
            s,
            " output wire context_wait,context_accepted,pixel_accepted,",
            " output wire pixel_accepted,",
        )?;
        once(s, " reg loaded_valid;\n", "")?;
        once(
            s,
            " wire switching=source_context_valid&&covered&&!loaded_valid;\n",
            "",
        )?;
        once(
            s,
            " wire core_context_ready,core_in_ready;",
            " wire core_in_ready;",
        )?;
        once(
            s,
            " wire core_in_valid=source_valid&&covered&&!switching;",
            " wire core_in_valid=source_valid&&covered;",
        )?;
        once(
            s,
            " assign context_wait=switching;\n assign context_accepted=ce&&switching&&core_context_ready;\n",
            "",
        )?;
        once(
            s,
            "(!covered||(!switching&&core_in_ready))",
            "(!covered||core_in_ready)",
        )?;
        let a = s
            .find(" always @(posedge clk)begin")
            .ok_or("Lighting loaded flag")?;
        let b = a + s[a..]
            .find(" gpu_v2_lighting core(")
            .ok_or("Lighting instance")?;
        s.replace_range(a..b, "");
        once(
            s,
            " .context_valid(switching),.context_ready(core_context_ready),\n",
            "",
        )
    })?;
    edit(source, "gpu_v2_vliw_quad_pool", |s| {
        once(s, "light_valid,light_context_valid,", "light_valid,")?;
        once(
            s,
            "localparam L_READ=0,L_X=1,L_CONTEXT=2,L_PIXEL=3;",
            "localparam L_READ=0,L_X=1,L_PIXEL=2;",
        )?;
        once(
            s,
            " assign light_context_valid=!reset&&!fault&&(light_state==L_CONTEXT||light_state==L_PIXEL);\n",
            "",
        )?;
        once(
            s,
            "light_state<=L_CONTEXT;end\n     L_CONTEXT:light_state<=L_PIXEL;",
            "light_state<=L_PIXEL;end",
        )
    })?;
    edit(source, "gpu_v2_vliw_sampling", |s| {
        once(s, " input wire retire_valid,\n", "")?;
        once(
            s,
            " output wire context_wait,context_accepted,pixel_accepted,",
            " output wire pixel_accepted,",
        )?;
        once(s, "light_valid,light_context_valid,", "light_valid,")?;
        once(s, ".light_context_valid(light_context_valid),", "")?;
        once(s, ".source_context_valid(light_context_valid),", "")?;
        once(s, " .retire_valid(retire_valid),\n", "")?;
        once(
            s,
            ".context_wait(context_wait),.context_accepted(context_accepted),",
            "",
        )
    })?;
    edit(source, "gpu_v2_vliw_backend_three_port", |s| {
        once(s, "blend,draw_rebind,", "blend,")?;
        once(s, " .retire_valid(draw_rebind&&pipeline_idle),\n", "")?;
        once(s, "||(draw_rebind&&!pipeline_idle)", "")
    })?;
    edit(source, "gpu_v2_vliw_three_port", |s| {
        once(s, "blend,draw_rebind,", "blend,")?;
        once(s, ".draw_rebind(draw_rebind&&pipeline_idle),", "")?;
        once(
            s,
            "   if(draw_rebind&&!pipeline_idle)$fatal(1,\"draw rebind before whole GPU drain\");\n",
            "",
        )
    })
}

/// The paid payload and line register drive the texel read on the next edge.
/// Its synchronous DO is the result slot; no second 64-bit texel copy exists.
/// The paid slot may prefetch while Color holds the previous native DO. The
/// next texel read replaces DO only on its previous consumer's final edge.
/// One pin protects the newest paid line; a held DO no longer reads that line.
fn registered_texture_return(source: &mut String) -> Result<(), String> {
    edit(source, "gpu_v2_texture_runtime_cache_unique", |s| {
        once(s, "reg[15:0]res_tex[0:3];", "")?;
        once(
            s,
            " (!rd_valid||!res_valid||out_ready)&&hit_ready;",
            " !rd_valid&&hit_ready;",
        )?;
        once(s, "wire[15:0]data_di[0:3],data_do[0:3];", "")?;
        once(
            s,
            "wire head_valid=in_valid;",
            "wire[15:0]data_di[0:3],data_do[0:3];\n wire head_valid=in_valid;",
        )?;
        *s = s.replace("res_tex[", "data_do[");
        once(s, "for(b=0;b<4;b=b+1)data_do[b]<=data_do[b];", "")?;
        let begin = s
            .find("  // Direct four address bits;")
            .ok_or("Texture registered row begin")?;
        let end = s[begin..]
            .find(" wire data_write=")
            .ok_or("Texture registered row end")?
            + begin;
        let rows = s[begin..end]
            .replace("head_tlx", "data_tlx")
            .replace("head_tly", "data_tly");
        s.replace_range(
            begin..end,
            &format!(
                " wire[2:0]data_tlx=paid_payload[18:16],data_tly=paid_payload[21:19];\n{rows}"
            ),
        );
        for bank in 0..4 {
            once(
                s,
                &format!("data_ra[{bank}]={{hit_line,bank_row{bank}}}"),
                &format!("data_ra[{bank}]={{pin_line,bank_row{bank}}}"),
            )?;
        }
        let begin = s
            .find("genvar db;generate")
            .ok_or("Texture data bank begin")?;
        let end = s[begin..]
            .find("end endgenerate")
            .ok_or("Texture data bank end")?
            + begin
            + "end endgenerate".len();
        let banks = s[begin..end]
            .replace(".CEB(read_issue)", ".CEB(return_capture)")
            .replace(
                "else if(read_issue)q<=mem[data_ra[db]]",
                "else if(return_capture)q<=mem[data_ra[db]]",
            );
        s.replace_range(begin..end, &banks);
        Ok(())
    })
}
