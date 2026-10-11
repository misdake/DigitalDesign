//! Static paid descriptor-head alternative for the closed post-quad top.
//! The default emitter and checked async descriptor reference stay unchanged.
//! Only an idle join pays a head read; its hard DO is the sole active header.
use super::row_store;

pub const MODULE: &str = "gpu_v2_postquad_descriptor_bank";

/// One 32x23 synchronous 1R1W bank. Native keeps the header in SDPX9B DO.
pub fn source(native: bool) -> Result<String, String> {
    let bank = if native {
        row_store::verilog_sdp36(32, 23)?
    } else {
        row_store::verilog(32, 23)?
    };
    Ok(bank.replace("gpu_v2_pixel_row_store", MODULE))
}

fn replace_once(text: &mut String, old: &str, new: &str) -> Result<(), String> {
    let count = text.matches(old).count();
    if count != 1 {
        return Err(format!("paid descriptor anchor ({count} matches): {old}"));
    }
    *text = text.replacen(old, new, 1);
    Ok(())
}

/// Lower an already selected complete post-quad source, without a runtime mux.
/// Works with either checked done vectors or selected completion frontiers.
/// No read borrows a same-edge retirement or a same-edge first publication.
pub fn lower(source_text: &str, native: bool) -> Result<String, String> {
    let mut body = source_text.to_owned();
    replace_once(&mut body, "reg active;reg[22:0]header;", "")?;
    replace_once(&mut body, "reg[5:0]published_count;", "")?;
    replace_once(
        &mut body,
        "(* syn_ramstyle=\"distributed_ram\" *)reg[22:0]descriptors[0:31];",
        r#"// The paid hard DO remains the sole header until actual quad ACK.
 reg active;
 reg[5:0]published_count;
 wire[22:0]header;
 wire descriptor_return_valid,descriptor_collision;
 reg head_loaded;
 wire descriptor_read=advance&&!active&&!head_loaded&&!descriptor_return_valid&&published_count!=0;
 gpu_v2_postquad_descriptor_bank descriptor_bank(
 .clk(clk),.reset(reset),.read_en(descriptor_read),.read_address(retire_slot),
 .write_en(advance&&published),.write_address(published_slot),.write_data(published_begin),
 .return_valid(descriptor_return_valid),.return_data(header),.collision(descriptor_collision));
 always @(posedge clk)begin
  if(reset)head_loaded<=0;
  else begin
   // Already paid read returns capture ownership even on a CE0 wall edge.
   if(descriptor_return_valid)head_loaded<=1;
   if(quad_accepted)head_loaded<=0;
  end
 end"#,
    )?;
    replace_once(
        &mut body,
        "descriptors[published_slot]<=published_begin;",
        "/* Descriptor publication writes the single canonical bank. */",
    )?;
    replace_once(
        &mut body,
        "header<=descriptors[retire_slot];",
        "/* The already paid descriptor DO owns this active quad. */",
    )?;
    replace_once(
        &mut body,
        "if(!active&&published_count!=0&&",
        "if(!active&&published_count!=0&&(head_loaded||descriptor_return_valid)&&",
    )?;
    replace_once(
        &mut body,
        "if(light_collision||sample_collision||basic_collision||",
        "if(descriptor_collision||light_collision||sample_collision||basic_collision||",
    )?;
    let frontier = "wire[22:0]head_descriptor=descriptors[retire_slot];";
    if body.contains(frontier) {
        replace_once(&mut body, frontier, "wire[22:0]head_descriptor=header;")?;
    }
    if body.contains("descriptors[") {
        return Err("paid descriptor lowering retained an async descriptor access".into());
    }
    body.push('\n');
    body.push_str(&source(native)?);
    Ok(body)
}

pub const CONTROL_MODULE: &str = "gpu_v2_postquad_descriptor_control_bank";

/// The same paid bank, with only mask/unlit/untextured captured at its return.
/// Origin and the remaining immutable descriptor bits remain hard-DO loans.
/// Control ownership captures on wall clocks, independently of compute CE.
pub fn source_control(native: bool) -> Result<String, String> {
    let mut bank = source(native)?.replace(MODULE, CONTROL_MODULE);
    replace_once(
        &mut bank,
        " output wire collision);",
        " output reg [5:0] control,\n output wire collision);",
    )?;
    replace_once(
        &mut bank,
        "endmodule",
        r#" always @(posedge clk) begin
  if(reset) control<=6'd0;
  else if(return_valid) control<={return_data[6:5],return_data[3:0]};
 end
endmodule"#,
    )?;
    Ok(bank)
}

/// Static control-capture alternative. Frozen `lower`/`source` stay unchanged.
/// A paid return captures six bits before activation; it never borrows the
/// uncaptured return edge for control decisions. No new full-header copy.
pub fn lower_control(source_text: &str, native: bool) -> Result<String, String> {
    let mut body = lower(source_text, native)?;
    let bank = source(native)?;
    if !body.ends_with(&bank) {
        return Err("control capture missing frozen descriptor bank suffix".into());
    }
    body.truncate(body.len() - bank.len());
    replace_once(
        &mut body,
        " wire[22:0]header;",
        " wire[22:0]header_do;\n wire[5:0]descriptor_control;\n wire[22:0]header={header_do[22:7],descriptor_control[5:4],header_do[4],descriptor_control[3:0]};",
    )?;
    replace_once(
        &mut body,
        "gpu_v2_postquad_descriptor_bank descriptor_bank(",
        "gpu_v2_postquad_descriptor_control_bank descriptor_bank(",
    )?;
    replace_once(
        &mut body,
        ".return_data(header),.collision(descriptor_collision)",
        ".return_data(header_do),.control(descriptor_control),.collision(descriptor_collision)",
    )?;
    replace_once(
        &mut body,
        "(head_loaded||descriptor_return_valid)",
        "head_loaded",
    )?;
    body.push_str(&source_control(native)?);
    Ok(body)
}
