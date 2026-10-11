//! Borrow one held Final lane across the real framebuffer's two row writes.
use crate::framebuffer::{ports::MaterializedSurface, rtl};

#[path = "framebuffer_closed_bank.rs"]
mod closed_bank;
#[path = "framebuffer_commit.rs"]
mod commit;
#[path = "framebuffer_overlap.rs"]
mod overlap;
#[path = "framebuffer_prefix.rs"]
mod prefix;

pub const MODULE: &str = "gpu_v2_pixel_framebuffer_sink";

/// The producer holds the entire lane until `lane_accept`, including uncovered
/// lanes. Header and immutable draw context remain identical across all four
/// lanes. `quad_accepted` is row7 admission; `commit_valid` is later ROP commit.
/// Existing cache maintenance and accepted transport keep advancing at CE0.
pub fn verilog(surface: MaterializedSurface) -> Result<String, String> {
    surface.validate()?;
    Ok(format!(
        "{}\n{}\n{}",
        include_str!("framebuffer_sink.v")
            .replace("@COLOR@", &surface.color_base_bytes.to_string())
            .replace("@DEPTH@", &surface.depth_base_bytes.to_string())
            .replace("@WIDTH@", &surface.width.to_string())
            .replace("@HEIGHT@", &surface.height.to_string()),
        rtl::CACHE,
        rtl::ROP_LEAF,
    ))
}

/// Closed cache/ROP experiment: the active cache owns rowbuf until RS_DONE.
/// Only a true leaf depth-written flag can enable a bank depth write, and its
/// value is exactly that lane's immutable source depth. The generic standalone
/// leaf and default factory retain their full visible output-depth contract.
pub fn verilog_closed_depth(surface: MaterializedSurface) -> Result<String, String> {
    let mut source = verilog(surface)?;
    replace_once(
        &mut source,
        "resd[leaf_out_key] <= leaf_new_depth;",
        "resd[leaf_out_key] <= rowbuf[{leaf_out_key, 1'b1}][15:0];",
    )?;
    for stage in 2..=7 {
        replace_once(
            &mut source,
            &format!("reg [15:0] oc{stage}, od{stage}, sd{stage};"),
            &format!("reg [15:0] oc{stage};"),
        )?;
        let previous = stage - 1;
        replace_once(
            &mut source,
            &format!("od{stage} <= od{previous};"),
            "// Depth payload remains in the active cache row owner.",
        )?;
        replace_once(
            &mut source,
            &format!("sd{stage} <= sd{previous};"),
            "// Source depth is borrowed only at the actual keyed return.",
        )?;
    }
    replace_once(
        &mut source,
        "new_depth <= (gate7 && dw7) ? sd7 : od7;",
        "new_depth <= 16'd0; // Closed cache borrows depth; this port is unused.",
    )?;
    Ok(source)
}

/// Closed even-quad cache layout: physical-bank temporaries and direct bank
/// ports replace arbitrary lane permutations. Accepted ingress already checks
/// alignment, and each maintenance beat owns four consecutive pixels.
pub fn verilog_closed_bank_depth(surface: MaterializedSurface) -> Result<String, String> {
    closed_bank::lower(verilog_closed_depth(surface)?)
}

/// After all four stage-1 inputs are accepted, the old bank rows are dead.
/// Reuse them for keyed returns, retaining the all-results-before-write fence.
pub fn verilog_closed_bank_reuse_depth(surface: MaterializedSurface) -> Result<String, String> {
    closed_bank::reuse_dead(verilog_closed_bank_depth(surface)?)
}

/// Keep the existing synchronous Output2 read calendar in one block RAM.
pub fn verilog_closed_bank_block(surface: MaterializedSurface) -> Result<String, String> {
    let mut source = verilog_closed_bank_depth(surface)?;
    replace_once(
        &mut source,
        "    reg [31:0] oram [0:15];",
        "    (* syn_ramstyle = \"block_ram\" *) reg [31:0] oram [0:15];",
    )?;
    Ok(source)
}

/// A false color-written flag prevents the cache from using the old-color
/// fallback. Its late transport is unnecessary inside this closed composition.
pub fn verilog_closed_bank_color(surface: MaterializedSurface) -> Result<String, String> {
    closed_bank::loan_color(verilog_closed_bank_depth(surface)?)
}

/// Borrow immutable Output2 pairs rather than snapshotting a whole quad. The
/// source depth travels through the real leaf; all result writes stay fenced.
pub fn verilog_closed_bank_stream(surface: MaterializedSurface) -> Result<String, String> {
    closed_bank::stream(verilog_closed_bank_color(surface)?)
}

/// Borrow the existing second Output2 slot while the first quad retires. Only
/// different aligned quads in the same resident tile overlap; dependencies and
/// misses retain the checked serial calendar. There is one result owner and
/// one old-operand row, with no added payload queue or arithmetic unit.
pub fn verilog_closed_bank_overlap(surface: MaterializedSurface) -> Result<String, String> {
    overlap::lower(verilog_closed_bank_stream(surface)?)
}

/// Commit on the actual synchronous depth-bank write edge. Both plane writes
/// still follow the complete result fence, and Output2 keeps old-state credit.
pub fn verilog_closed_bank_overlap_commit(surface: MaterializedSurface) -> Result<String, String> {
    commit::lower(verilog_closed_bank_overlap(surface)?)
}

/// Two original descriptor slots own accepted prefixes and ordered returns.
/// Accepted pairs feed the real leaf; final paid depth write publishes a quad.
pub fn verilog_closed_bank_prefix(surface: MaterializedSurface) -> Result<String, String> {
    prefix::lower(verilog_closed_bank_stream(surface)?)
}

fn replace_once(source: &mut String, anchor: &str, replacement: &str) -> Result<(), String> {
    if source.matches(anchor).count() != 1 {
        return Err(format!("Closed framebuffer depth anchor: {anchor}"));
    }
    *source = source.replace(anchor, replacement);
    Ok(())
}
