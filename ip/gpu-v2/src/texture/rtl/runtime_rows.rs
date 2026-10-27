//! Actual row Runtime preparation through the ordered Coordinate result queue.
//! Coefficient/Member/cache consumption remains an explicit external boundary.
use super::{
    compact_coordinate, context_store, coordinate_store, lod, runtime_frontend,
    runtime_frontend_early, uv_rows,
};
use crate::texture::{emu::derivative::rtl, sim::staged::bound::serial};
use std::fmt::Write;

pub const MODULE: &str = "gpu_v2_texture_runtime_rows";

/// Static construction choice; each emitted top has one controller/store.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub early_ordered: bool,
    pub canonical_owner: bool,
    pub continuous_prepare: bool,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            early_ordered: true,
            canonical_owner: true,
            continuous_prepare: false,
        }
    }
}

/// Existing selected leaves, one Context8 controller and canonical Raw/Ready
/// stores. Debug D phase is the existing physical register, not another clock.
pub fn verilog(native: bool) -> Result<String, String> {
    verilog_profile(native, Profile::default())
}

pub fn verilog_profile(native: bool, profile: Profile) -> Result<String, String> {
    if profile.continuous_prepare && !profile.early_ordered {
        return Err("Continuous preparation requires the ordered Context8 profile".into());
    }
    let (d, _, _) = serial::structural_calendars()?;
    let descriptor = rtl::describe_debug(&d, "gpu_v2_texture_runtime_d");
    let mut source = rtl::emit_debug(&d, "gpu_v2_texture_runtime_d");
    let lod_source = lod::verilog();
    let reset = "if(reset) begin phase<=0;valid<=0;end";
    if lod_source.matches(reset).count() != 1 {
        return Err("Runtime LOD issue-offset template changed".into());
    }
    source.push_str(&lod_source.replace(reset, "if(reset) begin phase<=4;valid<=0;end"));
    source.push_str(compact_coordinate::verilog());
    source.push_str(&uv_rows::source(native));
    let frontend = if profile.continuous_prepare {
        include_str!("runtime_frontend_ordered.v").to_owned()
    } else if profile.early_ordered {
        runtime_frontend_early::verilog_shared().to_owned()
    } else {
        runtime_frontend::verilog_shared()
    };
    source.push_str(&frontend);
    source.push_str(&context_store::runtime_source_shared(8, native)?);
    let queue = if profile.canonical_owner {
        coordinate_store::canonical_source_trusted(6, native)?
    } else {
        coordinate_store::compact_source(6, native)?
    };
    // Split the existing queue's validation and commit without altering its
    // counters, pointers or RAMs. This avoids a feedback path through CE.
    if !queue.contains("assign bad=!reset && ce &&") {
        return Err("Coordinate queue preflight template changed".into());
    }
    source.push_str(
        &queue
            .replace(
                "module gpu_v2_coordinate_store ",
                "module gpu_v2_runtime_coordinate_store ",
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
            ),
    );
    let mut declarations = String::new();
    let mut ports = String::from(".clk(clk),.reset(reset),.ce(advance),.in_valid(raw_issue),.in_ready(d_ready),.fault(d_fault),.out_valid(d_return),.dbg_bank(),.dbg_phase(d_phase)");
    for p in &descriptor.outputs {
        let name = p.name.strip_prefix("out_").unwrap();
        let name = if name == "slot" { "slot_meta" } else { name };
        writeln!(declarations, "wire [{}:0] d_{};", p.width - 1, name).unwrap();
        write!(ports, ",.{}(d_{})", p.name, name).unwrap();
    }
    for p in &descriptor.inputs {
        let input = match p.key.as_str() {
            "helper_uv" => format!("d_uv[{}+:18]", p.row * 18),
            "bias" => "raw_q[15:0]".into(),
            "force_coarsest" => "raw_q[16]".into(),
            "has_mip" => "raw_q[33]".into(),
            "filter" => "raw_q[35:34]".into(),
            "meta" => format!("raw_q[{}+:4]", 17 + p.row * 4),
            other => return Err(format!("unexpected D input {other}")),
        };
        write!(ports, ",.{}({input})", p.name).unwrap();
    }
    let top = include_str!("runtime_rows.v")
        .replace(
            "gpu_v2_texture_runtime_frontend_shared",
            if profile.continuous_prepare {
                "gpu_v2_texture_runtime_frontend_prepare_shared"
            } else if profile.early_ordered {
                runtime_frontend_early::MODULE
            } else {
                "gpu_v2_texture_runtime_frontend_shared"
            },
        )
        .replace("// D_DECLARATIONS", &declarations)
        .replace(
            "// D_INSTANCE",
            &format!("{} derivative({ports});", descriptor.module),
        );
    Ok(top + &source)
}
