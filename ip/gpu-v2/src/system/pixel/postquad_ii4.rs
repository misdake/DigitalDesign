//! Four-edge post-interpolator composition. Generic leaf interfaces are retained.
//! Exact weights have one authoritative bank; private transport carries masks.
use super::postquad;
use crate::{
    framebuffer::ports::MaterializedSurface,
    lighting::{dsp_sharing, rtl, LightingProfile},
    system::pixel::quad_ingress,
    texture::ports::Slot,
};

fn replace(source: &mut String, old: &str, new: &str) -> Result<(), String> {
    if source.matches(old).count() != 1 {
        return Err(format!("II4 lowering anchor must occur once: {old}"));
    }
    *source = source.replacen(old, new, 1);
    Ok(())
}

fn module(source: &str, name: &str) -> Result<String, String> {
    let start = source
        .find(&format!("module {name}("))
        .or_else(|| source.find(&format!("module {name} ")))
        .ok_or_else(|| format!("II4 missing module {name}"))?;
    let end = start + source[start..].find("endmodule").ok_or("II4 module end")? + 9;
    Ok(source[start..end].to_owned())
}

fn replace_modules(source: &mut String, fresh: &str) -> Result<(), String> {
    for chunk in fresh.split("\nmodule ").skip(1) {
        let name = chunk
            .split(['(', ' ', '\n'])
            .next()
            .ok_or("II4 module name")?;
        let new = format!(
            "module {}endmodule",
            chunk.split("endmodule").next().unwrap()
        );
        if let Ok(old) = module(source, name) {
            replace(source, &old, &new)?;
        } else {
            source.push('\n');
            source.push_str(&new);
            source.push('\n');
        }
    }
    Ok(())
}

fn global16(source: &mut String) -> Result<(), String> {
    let old = module(source, "gpu_v2_quad_ingress_closed_basic")?;
    let mut new = old.clone();
    for (a, b) in [
        ("descriptors[0:31]", "descriptors[0:15]"),
        ("reg [5:0] alloc_ptr, publish_ptr, retire_ptr;", "reg [4:0] alloc_ptr, publish_ptr, retire_ptr;"),
        ("assign global_used = alloc_ptr-retire_ptr;", "wire[4:0]global_distance=alloc_ptr-retire_ptr;assign global_used={1'b0,global_distance};"),
        ("global_used < 32", "global_used < 16"),
        ("wire [4:0] slot = row == 0 ? alloc_ptr[4:0] : alloc_ptr[4:0]-5'd1;", "wire[3:0]slot_low=row==0?alloc_ptr[3:0]:alloc_ptr[3:0]-4'd1;wire[4:0]slot={1'b0,slot_low};"),
        ("descriptors[descriptor_issue_slot]", "descriptors[descriptor_issue_slot[3:0]]"),
        ("descriptors[descriptor_head_slot]", "descriptors[descriptor_head_slot[3:0]]"),
    ] { replace(&mut new, a, b)?; }
    new = new
        .replace("alloc_ptr[4:0]", "{1'b0,alloc_ptr[3:0]}")
        .replace("retire_ptr[4:0]", "{1'b0,retire_ptr[3:0]}");
    replace(source, &old, &new)?;
    replace(
        source,
        "issue_slot<=issue_slot+1'b1",
        "issue_slot<={1'b0,issue_slot[3:0]+4'd1}",
    )?;
    replace(
        source,
        "raw_head_retire?retire_slot+5'd1:retire_slot",
        "raw_head_retire?{1'b0,retire_slot[3:0]+4'd1}:retire_slot",
    )
}

fn metadata(source: &mut String) -> Result<(), String> {
    let old = module(source, "gpu_v2_context_store_shared")?;
    let mut new = old.clone();
    replace(&mut new, "assign raw_ra=d_return?d_owner:d_head;", "reg[3:0]identity_loan;\n always@(posedge clk)begin\n  if(reset)identity_loan<=0;\n  else if(advance&&d_issue)identity_loan<=raw_q[20:17];\n end\n assign raw_ra=d_return?d_owner:d_head;")?;
    replace(
        &mut new,
        "(d_return && raw_q[20:17]!=derived_data[37:34])",
        "(d_return && identity_loan!=derived_data[37:34])",
    )?;
    // Keep range, arithmetic, phase, owner, collision and public-input checks.
    new = new
        .replace(".RAD(raw_ra)", ".RAD(d_head)")
        .replace("raw_q=raw_ram[raw_ra]", "raw_q=raw_ram[d_head]");
    replace(source, &old, &new)
}

fn direct_weights(source: &mut String) -> Result<(), String> {
    let old = module(source, "gpu_v2_runtime_coefficient")?;
    let mut new = old.replace(
        "module gpu_v2_runtime_coefficient(",
        "module gpu_v2_runtime_coefficient_directweight(",
    );
    replace(&mut new, "  output reg fault,output wire proposal_ready,preflight_bad", "  output reg fault,output wire proposal_ready,preflight_bad,\n  output wire direct_weight_write,output wire[6:0] direct_weight_address,output wire[35:0] direct_weight_value")?;
    replace(&mut new, "  reg [35:0] fine_corners;", "  wire [35:0] fine_corners=0;\n  reg [3:0] fine_mask;\n  reg [7:0] weight_key0,weight_key1;")?;
    replace(
        &mut new,
        "  reg [71:0] ready_ff0,ready_ff1;",
        r#"  reg [7:0] ready_ff0,ready_ff1;
  wire [3:0] current_mask;
  genvar weight_lane;
  generate for(weight_lane=0;weight_lane<4;weight_lane=weight_lane+1)begin:nonzero_weights
    assign current_mask[weight_lane]=|returning_corners[weight_lane*9+:9];
    assign ready_value[weight_lane*9+:9]={8'b0,(rrd?ready_ff1[weight_lane]:ready_ff0[weight_lane])};
    assign ready_value[36+weight_lane*9+:9]={8'b0,(rrd?ready_ff1[4+weight_lane]:ready_ff0[4+weight_lane])};
  end endgenerate
  wire[7:0] mask_out={current_mask,fine_mask};
  assign direct_weight_write=!reset && advance && (v3 || (v4 && !weight_key1[7])) && !(accepted && (bad_input || work_available>16));
  assign direct_weight_address={weight_key1[5:0],weight_key1[6]^(v4&&!weight_key1[7])};
  assign direct_weight_value=returning_corners;
  always@(posedge clk)begin
    if(!reset && advance)begin
      if(accepted)weight_key0<={(lv0==0),lv0[0],key_in[5:0]};
      if(v2)weight_key1<=weight_key0;
    end
  end
"#,
    )?;
    for (a, b) in [
        ("ready_ff0<=out_w;", "ready_ff0<=mask_out;"),
        ("ready_ff1<=out_w;", "ready_ff1<=mask_out;"),
        ("  assign ready_value=rrd ? ready_ff1 : ready_ff0;", ""),
        (
            "row_totals<=0; fine_corners<=0;",
            "row_totals<=0; fine_mask<=0;",
        ),
        (
            "if(v3) fine_corners<=returning_corners;",
            "if(v3) fine_mask<=current_mask;",
        ),
    ] {
        replace(&mut new, a, b)?;
    }
    let backend = module(source, "gpu_v2_texture_runtime_backend_unique")?;
    let mut fresh = backend.clone();
    replace(&mut fresh, " assign weight_write=advance&&member_return;\n assign weight_address={member_payload[71:68],member_payload[73:72],member_payload[64]};\n assign weight_value=member_payload[35:0];", " // Exact weights are already in their sole bank; member transports masks.")?;
    replace(
        &mut fresh,
        " gpu_v2_runtime_coefficient coefficient(",
        " gpu_v2_runtime_coefficient_directweight coefficient(",
    )?;
    replace(&mut fresh, ".fault(coefficient_fault),.proposal_ready(coefficient_ready),.preflight_bad(coefficient_bad));", ".fault(coefficient_fault),.proposal_ready(coefficient_ready),.preflight_bad(coefficient_bad),\n .direct_weight_write(weight_write),.direct_weight_address(weight_address),.direct_weight_value(weight_value));")?;
    replace(source, &backend, &fresh)?;
    source.push('\n');
    source.push_str(&new);
    source.push('\n');
    Ok(())
}

fn mac_register(source: &mut String) -> Result<(), String> {
    let mut body = module(source, "gpu_v2_lighting")?;
    let old = body.clone();
    let mut units = Vec::new();
    for chunk in body.split("MULTADDALU18X18 #(").skip(1) {
        let (parameters, rest) = chunk.split_once(") dsp").ok_or("II4 MAC instance")?;
        if !parameters.contains(".OUT_REG(1'b0)") {
            return Err("II4 MAC output already registered".into());
        }
        units.push(rest.split('(').next().ok_or("II4 MAC number")?.to_owned());
    }
    if units.len() != 1 {
        return Err("II4 expects one shared dot MAC".into());
    }
    for unit in units {
        replace(
            &mut body,
            &format!("reg [53:0] unit{unit}_tail;"),
            &format!("wire [53:0] unit{unit}_tail;"),
        )?;
        replace(
            &mut body,
            &format!("wire [53:0] unit{unit}_dsp_out;"),
            &format!("wire [53:0] unit{unit}_dsp_out;\nassign unit{unit}_tail=unit{unit}_dsp_out;"),
        )?;
        replace(
            &mut body,
            &format!("unit{unit}_tail<=unit{unit}_dsp_out;\n"),
            "",
        )?;
    }
    let start = body.find("MULTADDALU18X18 #(").unwrap();
    let tail = body[start..].replacen(".OUT_REG(1'b0)", ".OUT_REG(1'b1)", 1);
    body.replace_range(start.., &tail);
    replace(source, &old, &body)
}

fn private_lighting(source: &mut String) -> Result<(), String> {
    // Only this closed parent guarantees compute_tick => core.out_ready.
    // Public acknowledgments and result writes still use the common commit CE.
    for (name, edits) in [
        ("gpu_v2_postquad", vec![(
            ".clk(clk),.reset(reset),.ce(ce),.coalition_allow(coalition_allow),.abort(abort),",
            ".clk(clk),.reset(reset),.ce(ce),.light_compute_tick(compute_ce),.coalition_allow(coalition_allow),.abort(abort),",
        )]),
        ("gpu_v2_pixel_sampling_ingress", vec![
            ("input wire clk,reset,ce,", "input wire clk,reset,ce,light_compute_tick,"),
            (".clk(clk),.reset(reset),.ce(advance),\n .source_valid(light_valid)", ".clk(clk),.reset(reset),.ce(advance),.compute_tick(light_compute_tick),\n .source_valid(light_valid)"),
        ]),
        ("gpu_v2_pixel_lighting_consumer", vec![
            ("input wire clk,reset,ce,", "input wire clk,reset,ce,compute_tick,"),
            ("assign context_accepted=switching && core_context_ready;", "assign context_accepted=ce && switching && core_context_ready;"),
            ("assign pixel_accepted=core_in_valid && core_in_ready;", "assign pixel_accepted=ce && core_in_valid && core_in_ready;"),
            (".clk(clk),.reset(reset),.ce(ce),", ".clk(clk),.reset(reset),.ce(compute_tick),"),
        ]),
        ("gpu_v2_lighting", vec![(
            "wire advance = ce && configured && !(out_valid && !out_ready);",
            "// Actual closed parent proves ce implies out_ready.\nwire advance = ce && configured;",
        )]),
    ] {
        let old = module(source, name)?;
        let mut new = old.clone();
        for (a, b) in edits {
            replace(&mut new, a, b)?;
        }
        replace(source, &old, &new)?;
    }
    Ok(())
}

fn tile_address(source: &mut String, width: u16) -> Result<(), String> {
    // Predecode alongside the held header, before cache readiness is observed.
    // This removes stride arithmetic from the cache -> global-CE return path.
    // The tile register has exactly the header's owner and capture edges.
    if (width / 16).is_power_of_two() {
        return Ok(());
    }
    let terms: Vec<_> = (0..6)
        .filter(|bit| (width / 16) & (1 << bit) != 0)
        .map(|bit| format!("(next_tile_y << {bit})"))
        .collect();
    replace(source," reg active;reg[22:0]header;",
        &format!(" reg active;reg[22:0]header;reg[8:0]header_tile;\n wire[8:0]next_tile_y={{5'd0,head_descriptor[22:19]}};\n wire[8:0]next_tile={}+{{4'd0,head_descriptor[15:11]}};",terms.join("+")))?;
    replace(
        source,
        "stopped<=0;active<=0;state<=J_READ;",
        "stopped<=0;active<=0;header_tile<=0;state<=J_READ;",
    )?;
    if source.matches("header<=head_descriptor;").count() != 2 {
        return Err("II4 tile must share both canonical header capture edges".into());
    }
    *source = source.replace(
        "header<=head_descriptor;",
        "header<=head_descriptor;header_tile<=next_tile;",
    );
    replace(source,".x({header[15:8],1'b0}),.y({header[22:16],1'b0}),.mask(header[3:0]),", ".sink_tile(header_tile),.x({header[15:8],1'b0}),.y({header[22:16],1'b0}),.mask(header[3:0]),")?;
    replace(
        source,
        " input wire[8:0]x,input wire[7:0]y,input wire[3:0]mask,",
        " input wire[8:0]sink_tile,x,input wire[7:0]y,input wire[3:0]mask,",
    )?;
    replace(
        source,
        ".row_x(x),.row_y(y),.row_mask(mask),",
        ".row_tile(sink_tile),.row_x(x),.row_y(y),.row_mask(mask),",
    )?;
    replace(
        source,
        "    input      [8:0]  row_x,",
        "    input      [8:0]  row_tile,row_x,",
    )?;
    replace(
        source,
        "    wire [8:0] head_tile = ({1'b0,row_y} >> 4) * (WIDTH >> 4) + ({5'd0,row_x} >> 4);",
        "    wire[8:0]head_tile=row_tile;",
    )
}

/// Complete II4 chain with the existing eight-row interpolator interface.
/// The wide pool is explicit so DSP/Logic comparisons use the same boundary.
pub fn verilog(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    wide_pool: usize,
) -> Result<String, String> {
    verilog_profile(slots, surface, backend, wide_pool, true)
}

/// Optional modulo-four sharing of the normalization ROM ports.
pub fn verilog_profile(
    slots: &[Slot],
    surface: MaterializedSurface,
    backend: quad_ingress::rtl::Backend,
    wide_pool: usize,
    shared_roms: bool,
) -> Result<String, String> {
    let mut source = postquad::verilog_combined_group48_pool64_candidate(slots, surface, backend)?;
    let (options, plans) = if shared_roms {
        dsp_sharing::four_cycle_shared_roms(wide_pool)?
    } else {
        dsp_sharing::four_cycle(wide_pool)?
    };
    let light = rtl::generate_with_schedule_plans(LightingProfile::Fast, options, &plans)?;
    replace_modules(&mut source, &format!("\n{}", light.source))?;
    replace_modules(
        &mut source,
        &format!("\n{}", include_str!("postquad_ii4_color.v")),
    )?;
    global16(&mut source)?;
    metadata(&mut source)?;
    direct_weights(&mut source)?;
    mac_register(&mut source)?;
    tile_address(&mut source, surface.width)?;
    private_lighting(&mut source)?;
    Ok(source)
}
