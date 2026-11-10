//! Direct final-LOD -> selected C -> canonical coordinate queue.
use super::{compact_coordinate, coordinate_store};

pub const MODULE: &str = "gpu_v2_texture_direct_lod";
pub const INITIATION_INTERVAL: usize = 2;

fn checked_queue(native: bool) -> Result<String, String> {
    let queue = coordinate_store::canonical_source_trusted(6, native)?
        .replace(
            "module gpu_v2_coordinate_store ",
            "module gpu_v2_direct_coordinate_store ",
        )
        .replace(
            "input wire clk,reset,ce,",
            "input wire clk,reset,proposal_ce,commit_allow,",
        )
        .replace(
            "localparam A=",
            "wire ce=proposal_ce&&commit_allow;\n localparam A=",
        )
        .replace(
            "assign bad=!reset && ce &&",
            "assign bad=!reset && proposal_ce &&",
        );
    Ok(queue)
}

pub fn verilog(native: bool) -> Result<String, String> {
    Ok(include_str!("direct_lod.v").to_owned()
        + "\n"
        + compact_coordinate::verilog()
        + "\n"
        + &checked_queue(native)?)
}

/// Explicit checked source-Q variant; no outer arithmetic or coordinate-port rewriting.
pub fn verilog_source_q(native: bool) -> Result<String, String> {
    Ok(source_q_body(false)
        + "\n"
        + compact_coordinate::verilog_source_q()
        + "\n"
        + &checked_queue(native)?)
}
/// Installed command descriptor, including fixed framebuffer-view mode.
pub fn command_body() -> String {
    source_q_body(true)
}
fn source_q_body(command: bool) -> String {
    let (view_port, slot_port, input_contract, preflight, coord_view, owner_slot, stop) = if command
    {
        (
            "input texture_fb400240,texture_clamp,",
            "",
            "!in_mask[in_lane]",
            "1'b0",
            ".texture_fb400240(texture_fb400240),.texture_clamp(texture_clamp),",
            "4'd0",
            COMMAND_STOP,
        )
    } else {
        (
            "",
            ",in_slot",
            "in_max_n>10||in_filter>2||!in_mask[in_lane]",
            "input_bad||queue_bad",
            "",
            "in_slot",
            CHECKED_STOP,
        )
    };
    include_str!("direct_lod_source_q.v")
        .replace("@VIEW_PORT@", view_port)
        .replace("@SLOT_PORT@", slot_port)
        .replace("@INPUT_CONTRACT@", input_contract)
        .replace("@PREFLIGHT@", preflight)
        .replace("@COORD_VIEW@", coord_view)
        .replace("@OWNER_SLOT@", owner_slot)
        .replace(
            "@PHYSICAL@",
            if command {
                "texture_clamp?fine:(fine==0?4'd1:fine)"
            } else {
                "fine==0?4'd1:fine"
            },
        )
        .replace("@STOP_PROCESS@", stop)
}

const CHECKED_STOP: &str = r#" always @(posedge clk)begin
  if(reset)begin stopped<=0;end
  else begin
   if(running&&preflight_bad)stopped<=1;
   if(advance&&c_fault)stopped<=1;
  end
 end"#;

const COMMAND_STOP: &str = r#" always @(posedge clk)begin
  if(reset)begin begin end end
  else begin
   if(running&&preflight_bad)begin
`ifndef SYNTHESIS
 if(!reset&&((1)!=0))$fatal(1,"GPU contract: stopped");
`endif
 end
   if(advance&&c_fault)begin
`ifndef SYNTHESIS
 if(!reset&&((1)!=0))$fatal(1,"GPU contract: stopped");
`endif
 end
  end
 end

 always @(posedge clk)stopped<=0;

`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&((running)&&(input_bad||queue_bad)))$fatal(1,"GPU contract: preflight_bad");
`endif"#;
