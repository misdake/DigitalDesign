//! Synthesizable scalar RTL leaves for the two independent live Runtime
//! pipelines: `runtime_membership.rs` and `runtime_packet.rs`.
//!
//! The Verilog files under `runtime_rtl/` are the synthesizable scalar leaf
//! modules. They re-implement the *typed* register pipelines of the private
//! actual models stage for stage, with the same CE/valid/fault contract and the
//! same frozen stage widths:
//!
//! * `membership_leaf` has seven stages E0..E6 and publishes the old E6 word at
//!   E7 ([`MEMBERSHIP_LATENCY`] = 7 enabled edges).
//! * `packet_leaf` has nine stages E0..E8 and publishes the old E8 bank at E9
//!   ([`PACKET_LATENCY`] = 9 enabled edges).
//!
//! Both are II=1 leaves with no FIFO and no admission arbiter: `in_ready` is
//! `ce` while unfaulted, so the caller reserves the Work/Pool credit externally.
//! The modules hold the exact declared widths: membership 648 data bits plus
//! seven valid bits and one fault bit; packet 673 data bits plus nine valid bits
//! and one fault bit.
//!
//! This unit never modifies the private pipelines or the transport types; those
//! remain the independent live reference. The co-simulation tests below compare
//! the RTL against that reference edge by edge, and a separate mathematical
//! golden (explicit bit layout, never the shared `Member` emitter) checks the
//! reference itself, so an RTL/emulator agreement cannot hide a shared bug.
//!
//! Limits: this leaf pair is not a full sampler. It adds no FIFO, no full Work
//! credit, no cache and no global scheduling; it does not claim fitted area,
//! fmax, placement or whole-sampler throughput.

pub const MEMBERSHIP_LATENCY: u8 = 7;
pub const PACKET_LATENCY: u8 = 9;
pub const MEMBERSHIP_DATA_BITS: u32 = 648;
pub const MEMBERSHIP_CONTROL_BITS: u32 = 8;
pub const PACKET_DATA_BITS: u32 = 673;
pub const PACKET_CONTROL_BITS: u32 = 10;

pub const MEMBERSHIP_RTL: &str = include_str!("runtime_rtl/membership_leaf.v");
pub const PACKET_RTL: &str = include_str!("runtime_rtl/packet_leaf.v");

/// Declared register calendar and RTL-anchor audit for both leaves.
///
/// These are declarations, not fitted results: the register widths are the
/// typed widths of the private pipelines, and each anchor is checked against the
/// exact bytes that are simulated.
pub mod timing {
    /// One registered stage of a leaf.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StageDecl {
        pub name: &'static str,
        pub data_bits: u32,
        /// Exact substring that must appear in the simulated RTL for this stage.
        pub rtl_anchor: &'static str,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Declaration {
        pub name: &'static str,
        pub module: &'static str,
        pub latency: u8,
        pub initiation_interval: u8,
        pub data_bits: u32,
        pub control_bits: u32,
        pub stages: &'static [StageDecl],
        /// Exact width/type substrings that must appear in the simulated RTL.
        pub checks: &'static [&'static str],
    }

    const fn stage(name: &'static str, data_bits: u32, rtl_anchor: &'static str) -> StageDecl {
        StageDecl {
            name,
            data_bits,
            rtl_anchor,
        }
    }

    /// E0 input 92, E1 slice 90, E2 equal 92, E3 pairs 98, E4 emit 92,
    /// E5/E6 align 92 each.
    pub const MEMBERSHIP_STAGES: &[StageDecl] = &[
        stage("E0 input", 92, "<= in_valid;"),
        stage("E1 slice", 90, "<= c0_0[9:3];"),
        stage("E2 equal", 92, "<= (tx0_1 == tx1_1);"),
        stage("E3 pairs", 98, "<= same_x2;"),
        stage("E4 emit", 92, "<= member_next;"),
        stage("E5 align", 92, "<= member4;"),
        stage("E6 output", 92, "<= member5;"),
    ];

    /// E0 capture 93, E1 selection 76, E2..E8 banks 72 each.
    pub const PACKET_STAGES: &[StageDecl] = &[
        stage("E0 input", 93, "<= cap_next;"),
        stage("E1 selected", 76, "<= sel_next;"),
        stage("E2 pack", 72, "<= pack_next;"),
        stage("E3 align", 72, "<= w0;"),
        stage("E4 align", 72, "<= w1;"),
        stage("E5 align", 72, "<= w2;"),
        stage("E6 align", 72, "<= w3;"),
        stage("E7 align", 72, "<= w4;"),
        stage("E8 output", 72, "<= w5;"),
    ];

    /// Typed widths of the membership boundary and its retained banks.
    pub const MEMBERSHIP_CHECKS: &[&str] = &[
        "input      [8:0]  in_w0,",
        "input      [9:0]  in_c0,",
        "input      [3:0]  in_slot,",
        "input      [5:0]  in_key,",
        "output reg [91:0] out_member,",
        "reg [8:0]  w0_0, w1_0, w2_0, w3_0;",
        "reg [9:0]  c0_0, c1_0, c2_0, c3_0;",
        "reg [91:0] member4;",
    ];

    /// Typed widths of the packet capture, selection and 72-bit banks.
    pub const PACKET_CHECKS: &[&str] = &[
        "input      [91:0] in_member,",
        "input      [1:0]  in_tap,",
        "reg  [92:0] cap_e0;",
        "reg [75:0] sel_e1;",
        "reg [71:0] w0, w1, w2, w3, w4, w5;",
        "output reg [71:0] out_packet,",
    ];

    pub const MEMBERSHIP: Declaration = Declaration {
        name: "membership_leaf",
        module: "module membership_leaf (",
        latency: super::MEMBERSHIP_LATENCY,
        initiation_interval: 1,
        data_bits: super::MEMBERSHIP_DATA_BITS,
        control_bits: super::MEMBERSHIP_CONTROL_BITS,
        stages: MEMBERSHIP_STAGES,
        checks: MEMBERSHIP_CHECKS,
    };

    pub const PACKET: Declaration = Declaration {
        name: "packet_leaf",
        module: "module packet_leaf (",
        latency: super::PACKET_LATENCY,
        initiation_interval: 1,
        data_bits: super::PACKET_DATA_BITS,
        control_bits: super::PACKET_CONTROL_BITS,
        stages: PACKET_STAGES,
        checks: PACKET_CHECKS,
    };

    const COMMON_ANCHORS: [&str; 4] = [
        "if (reset)",
        "if (do_fault)",
        "if (advance)",
        "assign in_ready  = ce & ~fault;",
    ];

    /// Check one declaration against the exact RTL source it describes.
    pub fn audit(rtl: &str, decl: &Declaration) -> Result<(), String> {
        if !rtl.contains(decl.module) {
            return Err(format!("{}: module anchor missing", decl.name));
        }
        for anchor in COMMON_ANCHORS {
            if !rtl.contains(anchor) {
                return Err(format!("{}: common anchor missing: {anchor}", decl.name));
            }
        }
        let mut data = 0u32;
        for stage in decl.stages {
            if !rtl.contains(stage.rtl_anchor) {
                return Err(format!(
                    "{}: stage {} anchor missing: {}",
                    decl.name, stage.name, stage.rtl_anchor
                ));
            }
            data += stage.data_bits;
        }
        if data != decl.data_bits {
            return Err(format!(
                "{}: declared data bits {} != summed {}",
                decl.name, decl.data_bits, data
            ));
        }
        if decl.stages.len() as u32 + 1 != decl.control_bits {
            return Err(format!("{}: valid+fault control bits", decl.name));
        }
        if usize::from(decl.latency) != decl.stages.len() {
            return Err(format!("{}: latency != stage count", decl.name));
        }
        for check in decl.checks {
            if !rtl.contains(check) {
                return Err(format!("{}: width check missing: {check}", decl.name));
            }
        }
        Ok(())
    }

    pub fn membership_audit(rtl: &str) -> Result<(), String> {
        audit(rtl, &MEMBERSHIP)
    }

    pub fn packet_audit(rtl: &str) -> Result<(), String> {
        audit(rtl, &PACKET)
    }
}

#[cfg(test)]
#[path = "runtime_rtl_tests.rs"]
mod tests;
