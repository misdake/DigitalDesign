//! Direct final-LOD -> selected C -> canonical coordinate queue.
use super::{compact_coordinate, coordinate_store};

pub const MODULE: &str = "gpu_v2_texture_direct_lod";
pub const INITIATION_INTERVAL: usize = 4;

pub fn verilog(native: bool) -> Result<String, String> {
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
    Ok(include_str!("direct_lod.v").to_owned()
        + "\n"
        + compact_coordinate::verilog()
        + "\n"
        + &queue)
}
