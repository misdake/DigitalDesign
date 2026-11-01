//! Runtime target and remaining single-context command specializations.
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
        ports(s, TARGET_PORTS)?;
        link(s, "gpu_v2_vliw_backend_three_port back(", TARGET_PINS)?;
        once(s, "input draw_bank,draw_unlit,", "input draw_unlit,")?;
        *s = s
            .replace(".draw_bank(draw_bank),", "")
            .replace("draw_bank,draw_unlit,", "draw_unlit,");
        Ok(())
    })?;
    edit(&mut source, "gpu_v2_vliw_backend_three_port", |s| {
        ports(s, TARGET_PORTS)?;
        link(s, "gpu_v2_pixel_framebuffer_sink sink(", TARGET_PINS)?;
        once(
            s,
            "input wire draw_bank,draw_unlit,",
            "input wire draw_unlit,",
        )?;
        *s = s.replace(".draw_bank(draw_bank),", "");
        once(
            s,
            ".light_bank(),.retire_valid(draw_rebind&&pipeline_idle),.retire_bank(1'b0),",
            ".retire_valid(draw_rebind&&pipeline_idle),",
        )
    })?;
    edit(&mut source, "gpu_v2_vliw_sampling", |s| {
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
        once(s, "input draw_bank,draw_unlit,", "input draw_unlit,")?;
        once(
            s,
            "quad_rgb,draw_unlit,draw_bank}",
            "quad_rgb,draw_unlit,1'b0}",
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
        ports(s, TARGET_PORTS)?;
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
        link(s, "cache(\n", TARGET_PINS)
    })?;
    edit(&mut source, "gpu_v2_framebuffer_tdp", |s| {
        ports(s, TARGET_PORTS)?;
        once(s, " parameter[31:0]COLOR_BASE=0,DEPTH_BASE=16384,\n", "")?;
        once(s,
            "assign mem_req_addr=(mt_issue[1]?DEPTH_BASE:COLOR_BASE)+{14'd0,mt_linear_tile,mt_tile[5],mt_issue[0],7'b0};",
            "wire[6:0]target_page=(mt_issue[1]?depth_base[13:7]:color_base[13:7])+{5'd0,mt_linear_tile[8:7]};\n assign mem_req_addr={9'd0,target_page,mt_linear_tile[6:0],mt_tile[5],mt_issue[0],7'd0};")
    })?;
    direct_global(&mut source)?;
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
