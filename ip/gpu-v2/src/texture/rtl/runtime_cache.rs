//! Pool-borrowed demand cache with raw preflight and paid wall-clock returns.
use crate::texture::{ports::Slot, rtl::cache};

pub const MODULE: &str = "gpu_v2_texture_runtime_cache";

/// The producer must hold its old Group until `in_ready` actually consumes it.
/// One Group register belongs jointly to the paid read and captured Color row.
/// No cache demand-head payload or second Color-input payload is retained.
pub fn verilog(slots: &[Slot], native: bool) -> Result<String, String> {
    if slots.is_empty() || slots.len() > 16 {
        return Err("Runtime cache Slot16 table".into());
    }
    for slot in slots {
        if slot.valid {
            slot.validate()?;
        }
    }
    let checked = cache::verilog(slots);
    let start = checked
        .find("  // Immutable slot parameters")
        .ok_or("cache ROM anchor")?;
    let end = checked[start..]
        .find("  // Reset masks")
        .ok_or("cache ROM end")?
        + start;
    let hit = checked
        .find("  wire [3:0] ready_hits;")
        .ok_or("cache hit anchor")?;
    let hit_end = checked[hit..]
        .find("  always @(posedge clk) begin\n    if (reset)")
        .ok_or("cache bank-map end")?
        + hit;
    Ok(include_str!("runtime_cache.v")
        .replace("@NATIVE@", if native { "1" } else { "0" })
        .replace("@SLOT_FUNCTIONS@", &checked[start..end])
        .replace("@HIT_AND_BANK_MAP@", &checked[hit..hit_end]))
}

/// Closed composition experiment: the actual ordered Pool owns the old Group
/// until `read_issue` and cannot pop it while an MC descriptor is alive.
/// That producer contract discharges the redundant descriptor address check.
/// All TileKey, refill protocol and paid-return checks remain unchanged.
pub fn verilog_pool_closed(slots: &[Slot], native: bool) -> Result<String, String> {
    const CHECK: &str = " wire borrow_bad=running&&desc_valid&&(!head_valid||hk_addr!=desc_addr);";
    let checked = verilog(slots, native)?;
    if checked.matches(CHECK).count() != 1 {
        return Err("Runtime cache borrowed-address anchor".into());
    }
    Ok(checked.replace(
        CHECK,
        " wire borrow_bad=1'b0; // Actual ordered Pool holds Group until read_issue.",
    ))
}

/// Ordered Pool lookahead pays the next tag read on the current data issue.
/// One pending-read payload and one captured result allow Group II1. The
/// pending BSRAM output remains owned until a result slot is actually free;
/// wall-clock returns therefore remain safe under CE0 and foreign denial.
pub fn verilog_pipelined(slots: &[Slot], native: bool) -> Result<String, String> {
    let mut source = verilog(slots, native)?;
    for (old, new) in [
        (" input wire in_valid,input wire[71:0]in_payload,", " input wire in_valid,input wire[71:0]in_payload,\n input wire next_valid,input wire[21:0]next_key,"),
        ("reg[71:0]res_payload;", "reg[71:0]res_payload,paid_payload;"),
        (" wire return_bad=!reset&&rd_valid&&res_valid;", " // Two occupied stages are legal; the pending DO is held until capture.\n wire return_bad=1'b0;"),
        (" !rd_valid&&(!res_valid||out_ready)&&hit_ready;", " (!rd_valid||!res_valid||out_ready)&&hit_ready;"),
        (" wire[3:0]tag_set=head_set;", " wire[3:0]tag_set=(read_issue&&next_valid)?{next_key[16:15],next_key[9:8]}:head_set;\n wire result_pop=advance&&out_ready&&res_valid;\n wire return_capture=rd_valid&&(!res_valid||result_pop);"),
        ("if(!head_valid||read_issue||tag_write)tag_valid<=0;", "if(!head_valid||tag_write)tag_valid<=0;\n   else if(read_issue)tag_valid<=next_valid;"),
        ("if(rd_valid&&!res_valid)begin", "if(return_capture)begin"),
        ("res_valid<=1;rd_valid<=0;", "res_payload<=paid_payload;res_valid<=1;rd_valid<=0;"),
        ("if(out_ready&&res_valid)res_valid<=0;", "if(result_pop&&!return_capture)res_valid<=0;"),
        ("if(read_issue)begin res_payload<=head_payload;", "if(read_issue)begin paid_payload<=head_payload;"),
    ] {
        if source.matches(old).count() != 1 {
            return Err(format!("Pipelined cache anchor changed: {old}"));
        }
        source = source.replacen(old, new, 1);
    }
    Ok(source)
}

/// Same producer-owned miss descriptor contract as `verilog_pool_closed`.
pub fn verilog_pipelined_pool_closed(slots: &[Slot], native: bool) -> Result<String, String> {
    let source = verilog_pipelined(slots, native)?;
    let check = " wire borrow_bad=running&&desc_valid&&(!head_valid||hk_addr!=desc_addr);";
    if source.matches(check).count() != 1 {
        return Err("Pipeline borrowed-address anchor".into());
    }
    Ok(source.replacen(
        check,
        " wire borrow_bad=1'b0; // Actual ordered Pool holds Group until read_issue.",
        1,
    ))
}
