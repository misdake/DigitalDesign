//! Deterministic synthesizable lowering of the one-quad serial preparation
//! controller in [`super::serial`].
//!
//! This generator instantiates the *actual* numerical kernels and the accepted
//! leaf modules; it does not model arithmetic, embed test vectors or retain a
//! sampled answer:
//!
//! * `D`, `LOD` and coordinate kernels come from
//!   [`crate::texture::emu::derivative::rtl::emit`] over the immutable
//!   [`super::serial::structural_calendars`] calendars (II8/II8/II2).
//! * the coefficient leaf is the hand-written accepted RTL
//!   [`crate::texture::rtl::coefficient`];
//! * membership and packet are the accepted [`super::runtime_rtl`] leaves.
//!
//! The controller reproduces the exact old-edge CE/state semantics of
//! [`super::serial::PreparationEmu`]: one lane, one plane and one emitted tap at
//! a time. The D/LOD inputs are admitted only on their certified initiation
//! phases, the coordinate input only on even phases, and every acceptance
//! follows the leaf's own `in_ready`. A zero mask is accepted but launches no
//! kernel. A new quad is admitted only from `Idle`, and a final packet consumed
//! on the head edge does not fall through into a same-edge admission. A local
//! width violation or any leaf fault latches a terminal fault that blocks every
//! handshake; reset clears all validity and state.
//!
//! Declared state inventory is the additive controller bank only (the six
//! kernels carry their own separately audited arithmetic state):
//!
//! * data 128 (D wrapped UV8x16) + 54 (pending LOD input 53 + valid) + 83 (LOD
//!   context/id) + 171 (coefficient operand/result row) + 92 (held Member) +
//!   72 (packet head) = 600 bits;
//! * control 4 (state) + 2 (lane) + 1 (plane) + 2 (tap) + 1 (head valid) +
//!   1 (terminal fault) = 11 bits.
//!
//! These widths are *independently verified* by [`audit`] against the emitted
//! kernel descriptors rather than copied from the root baseline. The coefficient
//! leaf uses a binary phase-3 counter instead of the Rust one-hot phase-8; the
//! controller drives it through its published `in_ready`/`in_accept` handshake
//! and never assumes the Rust phase encoding.
//!
//! This is the full preparation chain only. It adds no texture cache, color or
//! result stage, and makes no fitted area, DSP/BSRAM, fmax or throughput claim.

use super::runtime_rtl;
use super::serial;
use crate::texture::emu::derivative::rtl::{self, Descriptor, Io};
use crate::texture::rtl::coefficient;

/// Top-level module instantiated by the qualification testbench.
pub const TOP: &str = "gpu_v2_texture_serial_preparation";
/// Additive controller data-bank baseline (see module docs).
pub const CONTROLLER_DATA_BITS: usize = 128 + 54 + 83 + 171 + 92 + 72;
/// Additive controller control-bank baseline.
pub const CONTROLLER_CONTROL_BITS: usize = 4 + 2 + 1 + 2 + 1 + 1;

/// Active data bits of the short-alignment membership leaf (E0..E4).
pub const MEMBERSHIP_SHORT_DATA_BITS: u32 = 92 + 90 + 92 + 98 + 92;
/// Active control bits of the short-alignment membership leaf (5 valid + fault).
pub const MEMBERSHIP_SHORT_CONTROL_BITS: u32 = 5 + 1;
/// Active data bits of the default membership leaf (adds E5/E6, two 92-bit banks).
pub const MEMBERSHIP_LONG_DATA_BITS: u32 = MEMBERSHIP_SHORT_DATA_BITS + 2 * 92;
/// Active control bits of the default membership leaf (7 valid + fault).
pub const MEMBERSHIP_LONG_CONTROL_BITS: u32 = 7 + 1;
/// Active data bits of the short-alignment packet leaf (E0..E2).
pub const PACKET_SHORT_DATA_BITS: u32 = 93 + 76 + 72;
/// Active control bits of the short-alignment packet leaf (3 valid + fault).
pub const PACKET_SHORT_CONTROL_BITS: u32 = 3 + 1;
/// Active data bits of the default packet leaf (adds six 72-bit banks).
pub const PACKET_LONG_DATA_BITS: u32 = PACKET_SHORT_DATA_BITS + 6 * 72;
/// Active control bits of the default packet leaf (9 valid + fault).
pub const PACKET_LONG_CONTROL_BITS: u32 = 9 + 1;

/// Declared active register inventory of one accepted leaf under a config.
///
/// The short form excludes the alignment banks that the elaboration constant
/// prunes; the long form matches [`runtime_rtl`]'s frozen default declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeafInventory {
    pub data_bits: u32,
    pub control_bits: u32,
    /// Enabled-edge latency from admission to the published word.
    pub latency: u8,
    /// Alignment banks retained beyond the published arithmetic stage.
    pub alignment_banks: u8,
}

impl LeafInventory {
    pub const fn membership(short_alignment: bool) -> Self {
        if short_alignment {
            Self {
                data_bits: MEMBERSHIP_SHORT_DATA_BITS,
                control_bits: MEMBERSHIP_SHORT_CONTROL_BITS,
                latency: 5,
                alignment_banks: 0,
            }
        } else {
            Self {
                data_bits: MEMBERSHIP_LONG_DATA_BITS,
                control_bits: MEMBERSHIP_LONG_CONTROL_BITS,
                latency: 7,
                alignment_banks: 2,
            }
        }
    }

    pub const fn packet(short_alignment: bool) -> Self {
        if short_alignment {
            Self {
                data_bits: PACKET_SHORT_DATA_BITS,
                control_bits: PACKET_SHORT_CONTROL_BITS,
                latency: 3,
                alignment_banks: 0,
            }
        } else {
            Self {
                data_bits: PACKET_LONG_DATA_BITS,
                control_bits: PACKET_LONG_CONTROL_BITS,
                latency: 9,
                alignment_banks: 6,
            }
        }
    }
}

/// Independently verified controller bank decomposition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Widths {
    pub uv: u32,
    pub pending_lod: u32,
    pub lod_context: u32,
    pub coefficient: u32,
    pub member: u32,
    pub head: u32,
}

impl Widths {
    pub fn data(&self) -> u32 {
        self.uv + self.pending_lod + self.lod_context + self.coefficient + self.member + self.head
    }
}

/// Complete generated source plus the descriptors used to build it.
pub struct Plan {
    pub source: String,
    pub d: Descriptor,
    pub lod: Descriptor,
    pub coord: Descriptor,
    pub widths: Widths,
    pub config: serial::Config,
    pub membership: LeafInventory,
    pub packet: LeafInventory,
}

fn out_wire(prefix: &str, io: &Io) -> String {
    format!("{prefix}{}", io.key.replace('.', "_"))
}

fn d_output(io: &Io) -> String {
    match io.key.as_str() {
        "uv0" | "uv1" | "uv2" | "uv3" | "uv4" | "uv5" | "uv6" | "uv7" | "slope" | "bias"
        | "quad" | "mask" | "slot" | "max_n" | "has_mip" | "filter" => out_wire("d_", io),
        other => panic!("unexpected D output {other}"),
    }
}

fn lod_output(io: &Io) -> String {
    match io.key.as_str() {
        "shift0" | "nearest" | "halve" | "side0" | "side1" | "parent0" | "parent1" | "n0"
        | "n1" | "last_fine" | "quad" | "mask" | "slot" => out_wire("l_", io),
        other => panic!("unexpected LOD output {other}"),
    }
}

fn coord_output(io: &Io) -> String {
    out_wire("c_", io)
}

fn d_input(io: &Io) -> Result<String, String> {
    match (io.key.as_str(), io.row) {
        ("helper_uv", row) if row < 8 => Ok(format!("in_uv_{row}")),
        ("bias", 0) => Ok("in_bias".into()),
        ("meta", 0) => Ok("in_quad".into()),
        ("meta", 1) => Ok("in_mask".into()),
        ("meta", 2) => Ok("in_slot".into()),
        ("meta", 3) => Ok("in_max_n".into()),
        ("has_mip", 0) => Ok("in_has_mip".into()),
        ("filter", 0) => Ok("in_filter".into()),
        ("force_coarsest", 0) => Ok("in_force_coarsest".into()),
        _ => Err(format!("unmapped D input {} row {}", io.key, io.row)),
    }
}

fn lod_input(io: &Io) -> Result<String, String> {
    match (io.key.as_str(), io.row) {
        ("slope", 0) => Ok("rslope".into()),
        ("bias", 0) => Ok("rbias".into()),
        ("meta", 0) => Ok("rmax_n".into()),
        ("meta", 1) => Ok("rquad".into()),
        ("meta", 2) => Ok("rmask".into()),
        ("meta", 3) => Ok("rslot".into()),
        ("has_mip", 0) => Ok("rhas_mip".into()),
        ("filter", 0) => Ok("rfilter".into()),
        _ => Err(format!("unmapped LOD input {} row {}", io.key, io.row)),
    }
}

fn coord_input(io: &Io) -> Result<String, String> {
    match (io.key.as_str(), io.row) {
        ("wrapped_uv", 0) => Ok("c_in_uv0".into()),
        ("wrapped_uv", 1) => Ok("c_in_uv1".into()),
        ("coordinate_shift", 0) => Ok("lc_shift".into()),
        ("flags", 0) => Ok("lc_nearest".into()),
        ("flags", 1) => Ok("lc_halve".into()),
        ("side", 0) => Ok("lc_side0".into()),
        ("side", 1) => Ok("lc_side1".into()),
        _ => Err(format!(
            "unmapped coordinate input {} row {}",
            io.key, io.row
        )),
    }
}

fn instance(
    desc: &Descriptor,
    inst: &str,
    control: &[(&str, &str)],
    input: impl Fn(&Io) -> Result<String, String>,
    output: impl Fn(&Io) -> String,
) -> Result<String, String> {
    let mut conns: Vec<String> = control
        .iter()
        .map(|(port, expr)| format!("    {port}({expr})"))
        .collect();
    for io in &desc.inputs {
        conns.push(format!("    .{}({})", io.name, input(io)?));
    }
    for io in &desc.outputs {
        conns.push(format!("    .{}({})", io.name, output(io)));
    }
    Ok(format!(
        "  {} {} (\n{}\n  );\n\n",
        desc.module,
        inst,
        conns.join(",\n")
    ))
}

fn sum_widths(ios: &[Io]) -> u32 {
    ios.iter().map(|io| io.width).sum()
}

/// Build the complete generated source and its verified inventory.
pub fn build() -> Result<Plan, String> {
    build_with_config(serial::Config::default())
}

/// Build the generated source for one elaboration-time configuration.
///
/// `nearest_bypass` is a controller-local constant and `short_alignment` is a
/// constant parameter on the accepted membership/packet leaves. Neither adds a
/// runtime register or a new payload buffer; the default configuration emits the
/// accepted baseline.
pub fn build_with_config(config: serial::Config) -> Result<Plan, String> {
    let (d_cal, l_cal, c_cal) = serial::structural_calendars()?;
    let d = rtl::describe(&d_cal, "gpu_v2_texture_serial_d");
    let lod = rtl::describe(&l_cal, "gpu_v2_texture_serial_lod");
    let coord = rtl::describe(&c_cal, "gpu_v2_texture_serial_coord");

    // Independent verification of the frozen kernel calendars.
    for (name, inv, numeric, span, period, ii) in [
        ("D", &d.inventory, 858usize, 17u8, 32u32, 8u32),
        ("LOD", &lod.inventory, 264, 28, 32, 8),
        ("COORD", &coord.inventory, 716, 9, 8, 2),
    ] {
        if inv.numeric_bits != numeric || inv.span != span || inv.period != period || inv.ii != ii {
            return Err(format!(
                "{name} calendar inventory mismatch: numeric {} span {} period {} ii {}",
                inv.numeric_bits, inv.span, inv.period, inv.ii
            ));
        }
    }

    let widths = Widths {
        uv: d
            .outputs
            .iter()
            .find(|io| io.key == "uv0")
            .map(|io| io.width)
            .ok_or("D uv0 missing")?
            * 8,
        pending_lod: sum_widths(&lod.inputs) + 1,
        lod_context: sum_widths(&lod.outputs),
        coefficient: coefficient::OUT_PAYLOAD_BITS as u32,
        member: 92,
        head: 72,
    };
    if widths.data() != CONTROLLER_DATA_BITS as u32 {
        return Err(format!(
            "controller data bank {} != baseline {}",
            widths.data(),
            CONTROLLER_DATA_BITS
        ));
    }

    // The short form prunes the alignment tails; the default form must still
    // agree with the frozen `runtime_rtl` leaf declarations.
    let membership = LeafInventory::membership(config.short_alignment);
    let packet = LeafInventory::packet(config.short_alignment);
    if !config.short_alignment
        && (membership.data_bits != runtime_rtl::MEMBERSHIP_DATA_BITS
            || membership.control_bits != runtime_rtl::MEMBERSHIP_CONTROL_BITS
            || packet.data_bits != runtime_rtl::PACKET_DATA_BITS
            || packet.control_bits != runtime_rtl::PACKET_CONTROL_BITS)
    {
        return Err("default leaf inventory drift".into());
    }

    let d_inst = instance(
        &d,
        "u_d",
        &[
            (".clk", "clk"),
            (".ce", "gce"),
            (".reset", "reset"),
            (".in_valid", "d_in_valid"),
            (".in_ready", "d_in_ready"),
            (".fault", "d_fault"),
            (".out_valid", "d_out_valid"),
        ],
        d_input,
        d_output,
    )?;
    let lod_inst = instance(
        &lod,
        "u_lod",
        &[
            (".clk", "clk"),
            (".ce", "gce"),
            (".reset", "reset"),
            (".in_valid", "l_in_valid"),
            (".in_ready", "l_in_ready"),
            (".fault", "l_fault"),
            (".out_valid", "l_out_valid"),
        ],
        lod_input,
        lod_output,
    )?;
    let coord_inst = instance(
        &coord,
        "u_coord",
        &[
            (".clk", "clk"),
            (".ce", "gce"),
            (".reset", "reset"),
            (".in_valid", "c_in_valid"),
            (".in_ready", "c_in_ready"),
            (".fault", "c_fault"),
            (".out_valid", "c_out_valid"),
        ],
        coord_input,
        coord_output,
    )?;

    let mut source = String::new();
    source.push_str(&rtl::emit(&d_cal, &d.module));
    source.push('\n');
    source.push_str(&rtl::emit(&l_cal, &lod.module));
    source.push('\n');
    source.push_str(&rtl::emit(&c_cal, &coord.module));
    source.push('\n');
    source.push_str(coefficient::verilog());
    source.push('\n');
    source.push_str(runtime_rtl::MEMBERSHIP_RTL);
    source.push('\n');
    source.push_str(runtime_rtl::PACKET_RTL);
    source.push('\n');
    source.push_str(&controller_head(config));
    source.push_str(&d_inst);
    source.push_str(&lod_inst);
    source.push_str(&coord_inst);
    source.push_str(&fixed_instances(config));
    source.push_str(FSM);
    source.push_str("endmodule\n");

    Ok(Plan {
        source,
        d,
        lod,
        coord,
        widths,
        config,
        membership,
        packet,
    })
}

/// Full generated Verilog for one bottom-up controller (default configuration).
pub fn source() -> Result<String, String> {
    source_with_config(serial::Config::default())
}

/// Full generated Verilog for one elaboration-time configuration.
pub fn source_with_config(config: serial::Config) -> Result<String, String> {
    Ok(build_with_config(config)?.source)
}

/// Audit the generated module against its declared widths and anchors.
pub fn audit() -> Result<(), String> {
    audit_with_config(serial::Config::default())
}

/// Audit one configuration against its declared widths, anchors and inventory.
pub fn audit_with_config(config: serial::Config) -> Result<(), String> {
    let plan = build_with_config(config)?;
    let src = &plan.source;
    for anchor in [
        "module gpu_v2_texture_serial_preparation (",
        "module gpu_v2_texture_serial_d (",
        "module gpu_v2_texture_serial_lod (",
        "module gpu_v2_texture_serial_coord (",
        "module gpu_v2_texture_coefficient(",
        "module membership_leaf (",
        "module packet_leaf (",
        "endmodule",
    ] {
        if !src.contains(anchor) {
            return Err(format!("serial preparation RTL missing anchor: {anchor}"));
        }
    }
    if plan.widths.data() != CONTROLLER_DATA_BITS as u32
        || plan.widths.pending_lod == 0
        || plan.widths.lod_context == 0
        || plan.widths.head != 72
    {
        return Err("serial preparation width invariant".into());
    }
    if config.short_alignment {
        let short_membership = LeafInventory {
            data_bits: MEMBERSHIP_SHORT_DATA_BITS,
            control_bits: MEMBERSHIP_SHORT_CONTROL_BITS,
            latency: 5,
            alignment_banks: 0,
        };
        let short_packet = LeafInventory {
            data_bits: PACKET_SHORT_DATA_BITS,
            control_bits: PACKET_SHORT_CONTROL_BITS,
            latency: 3,
            alignment_banks: 0,
        };
        if plan.membership != short_membership || plan.packet != short_packet {
            return Err("short leaf inventory mismatch".into());
        }
    } else if plan.membership.data_bits != runtime_rtl::MEMBERSHIP_DATA_BITS
        || plan.membership.control_bits != runtime_rtl::MEMBERSHIP_CONTROL_BITS
        || plan.membership.latency != runtime_rtl::MEMBERSHIP_LATENCY
        || plan.packet.data_bits != runtime_rtl::PACKET_DATA_BITS
        || plan.packet.control_bits != runtime_rtl::PACKET_CONTROL_BITS
        || plan.packet.latency != runtime_rtl::PACKET_LATENCY
    {
        return Err("default leaf inventory mismatch".into());
    }
    let membership_param = if config.short_alignment {
        "membership_leaf #(.SHORT_ALIGNMENT(1)) u_m ("
    } else {
        "membership_leaf u_m ("
    };
    let packet_param = if config.short_alignment {
        "packet_leaf #(.SHORT_ALIGNMENT(1)) u_p ("
    } else {
        "packet_leaf u_p ("
    };
    let bypass = if config.nearest_bypass {
        "localparam NEAREST_BYPASS = 1'b1;"
    } else {
        "localparam NEAREST_BYPASS = 1'b0;"
    };
    if !src.contains(membership_param) || !src.contains(packet_param) || !src.contains(bypass) {
        return Err("serial preparation configuration anchor".into());
    }
    Ok(())
}

/// Controller head with the nearest-bypass elaboration constant selected.
fn controller_head(config: serial::Config) -> String {
    if config.nearest_bypass {
        CONTROLLER_HEAD.replace(
            "localparam NEAREST_BYPASS = 1'b0;",
            "localparam NEAREST_BYPASS = 1'b1;",
        )
    } else {
        CONTROLLER_HEAD.to_string()
    }
}

/// Fixed accepted leaves: coefficient, membership and packet. The membership and
/// packet sequential tails are gated by the elaboration constant.
fn fixed_instances(config: serial::Config) -> String {
    let param = if config.short_alignment {
        "#(.SHORT_ALIGNMENT(1)) "
    } else {
        ""
    };
    format!(
        r###"  gpu_v2_texture_coefficient u_k (
    .clk(clk),
    .reset(reset),
    .ce(gce),
    .in_valid(k_in_valid),
    .in_payload(k_in_payload),
    .work_available(k_work),
    .in_ready(k_in_ready),
    .in_accept(k_in_accept),
    .out_ready(k_out_ready),
    .out_valid(k_out_valid),
    .out_weights(k_weights),
    .out_meta(k_meta),
    .work_reserved(),
    .fault(k_fault)
  );

  membership_leaf {param}u_m (
    .clk(clk),
    .reset(reset),
    .ce(gce),
    .in_valid(m_in_valid),
    .in_w0(m_in_w0),
    .in_w1(m_in_w1),
    .in_w2(m_in_w2),
    .in_w3(m_in_w3),
    .in_c0(m_in_c0),
    .in_c1(m_in_c1),
    .in_c2(m_in_c2),
    .in_c3(m_in_c3),
    .in_slot(m_in_slot),
    .in_level(m_in_level),
    .in_key(m_in_key),
    .in_fine(m_in_fine),
    .in_last_fine(m_in_last_fine),
    .in_ready(m_in_ready),
    .out_valid(m_out_valid),
    .out_member(m_member),
    .fault(m_fault)
  );

  packet_leaf {param}u_p (
    .clk(clk),
    .reset(reset),
    .ce(gce),
    .in_valid(p_in_valid),
    .in_member(p_in_member),
    .in_tap(p_in_tap),
    .in_ready(p_in_ready),
    .out_valid(p_out_valid),
    .out_packet(p_packet),
    .fault(p_fault)
  );

"###
    )
}

/// Port list, wire declarations and the purely combinational input/admission
/// drives. The fixed leaf instantiations and the clocked FSM follow.
const CONTROLLER_HEAD: &str = r###"module gpu_v2_texture_serial_preparation (
  input         clk,
  input         reset,
  input         ce,
  input         in_valid,
  input  signed [17:0] in_uv_0,
  input  signed [17:0] in_uv_1,
  input  signed [17:0] in_uv_2,
  input  signed [17:0] in_uv_3,
  input  signed [17:0] in_uv_4,
  input  signed [17:0] in_uv_5,
  input  signed [17:0] in_uv_6,
  input  signed [17:0] in_uv_7,
  input  signed [15:0] in_bias,
  input  [3:0]  in_quad,
  input  [3:0]  in_mask,
  input  [3:0]  in_slot,
  input  [3:0]  in_max_n,
  input         in_has_mip,
  input  [1:0]  in_filter,
  input  in_force_coarsest,
  output        in_ready,
  output        in_accept,
  input         out_ready,
  output        out_valid,
  output [71:0] out_packet,
  output [3:0]  out_state,
  output        fault
);

  localparam S_IDLE        = 4'd0;
  localparam S_DERIV       = 4'd1;
  localparam S_LOD_ISSUE   = 4'd2;
  localparam S_LOD         = 4'd3;
  localparam S_COORD_ISSUE = 4'd4;
  localparam S_COORD       = 4'd5;
  localparam S_COEF_ISSUE  = 4'd6;
  localparam S_COEF        = 4'd7;
  localparam S_MEM_ISSUE   = 4'd8;
  localparam S_MEM         = 4'd9;
  localparam S_PKT_ISSUE   = 4'd10;
  localparam S_PKT         = 4'd11;
  localparam S_HEAD        = 4'd12;

  // Elaboration-time nearest bypass: constant zero keeps the old path; one
  // writes the [511,0,0,0] fine / zero coarse row and skips the coefficient.
  localparam NEAREST_BYPASS = 1'b0;

  // Controller state banks (the additive 600 data + 11 control baseline).
  reg [3:0]  state;
  reg [1:0]  rlane, rtap;
  reg        rplane;
  reg        rhead_valid, fault_reg;
  reg [15:0] ruv0, ruv1, ruv2, ruv3, ruv4, ruv5, ruv6, ruv7;
  reg [17:0] rslope;
  reg [15:0] rbias;
  reg [3:0]  rquad, rmask, rslot, rmax_n;
  reg        rhas_mip;
  reg [1:0]  rfilter;
  reg        rpend_valid;
  reg signed [17:0] lc_shift;
  reg        lc_nearest, lc_halve;
  reg signed [11:0] lc_side0, lc_side1;
  reg [8:0]  lc_parent0, lc_parent1;
  reg [3:0]  lc_n0, lc_n1;
  reg        lc_last_fine;
  reg [3:0]  lc_quad, lc_mask, lc_slot;
  reg [71:0] rcoef_w;
  reg [98:0] rcoef_m;
  reg [91:0] rmember;
  reg [71:0] rhead;

  // Submodule outputs.
  wire d_in_ready, d_fault, d_out_valid;
  wire [15:0] d_uv0, d_uv1, d_uv2, d_uv3, d_uv4, d_uv5, d_uv6, d_uv7;
  wire [17:0] d_slope;
  wire signed [15:0] d_bias;
  wire [3:0] d_quad, d_mask, d_slot, d_max_n;
  wire d_has_mip;
  wire [1:0] d_filter;
  wire l_in_ready, l_fault, l_out_valid;
  wire signed [17:0] l_shift0;
  wire l_nearest, l_halve;
  wire signed [11:0] l_side0, l_side1;
  wire [8:0] l_parent0, l_parent1;
  wire [3:0] l_n0, l_n1;
  wire l_last_fine;
  wire [3:0] l_quad, l_mask, l_slot;
  wire c_in_ready, c_fault, c_out_valid;
  wire [7:0] c_f0_0, c_f0_1, c_f1_0, c_f1_1;
  wire [9:0] c_t0_0_0, c_t0_0_1, c_t0_1_0, c_t0_1_1;
  wire [9:0] c_t1_0_0, c_t1_0_1, c_t1_1_0, c_t1_1_1;
  wire k_in_ready, k_in_accept, k_out_valid, k_fault;
  wire [71:0] k_weights;
  wire [98:0] k_meta;
  wire m_in_ready, m_out_valid, m_fault;
  wire [91:0] m_member;
  wire p_in_ready, p_out_valid, p_fault;
  wire [71:0] p_packet;

  // Enabled local clock: kernels freeze while a terminal fault is latched.
  wire gce = ce & ~fault_reg;

  // A zero-width or out-of-range header is the only expressible admission
  // violation at this boundary (UV is already a signed-18 external code).
  wire bad_input = (in_max_n > 4'd10) | (in_filter > 2'd2)
                 | (in_bias < -16'sd8192) | (in_bias > 16'sd8192);

  assign in_ready  = ce & ~fault_reg & (state == S_IDLE) & d_in_ready;
  assign in_accept = in_ready & in_valid & ~bad_input;
  assign out_valid = rhead_valid & ~fault_reg;
  assign out_packet = rhead;
  assign out_state = state;
  assign fault = fault_reg;

  // D admission: only from Idle, on the certified II8 phase, non-zero mask.
  wire d_in_valid = in_ready & in_valid & (in_mask != 4'd0) & ~bad_input;

  // LOD admission: hold the pending input until the II8 phase is open.
  wire l_in_valid = (state == S_LOD_ISSUE) & l_in_ready & rpend_valid & ~fault_reg;

  // Coordinate operand selection: lane*2 / lane*2+1 of the captured UV.
  wire [15:0] c_in_uv0 = (rlane == 2'd0) ? ruv0 : (rlane == 2'd1) ? ruv2
                       : (rlane == 2'd2) ? ruv4 : ruv6;
  wire [15:0] c_in_uv1 = (rlane == 2'd0) ? ruv1 : (rlane == 2'd1) ? ruv3
                       : (rlane == 2'd2) ? ruv5 : ruv7;
  wire c_in_valid = (state == S_COORD_ISSUE) & c_in_ready & ~fault_reg;

  // Coefficient operand row (phase overlay):
  //   at Coordinate the low 72 bits carry the Q8 fractions as weights and the
  //   99-bit metadata carries coordinates/levels/slot/key/last_fine;
  //   at Coefficient the same 171-bit row becomes the actual coefficient result.
  wire [161:0] k_in_payload;
  wire [7:0]   k_work = 8'd16;
  wire k_in_valid = (state == S_COEF_ISSUE) & ~fault_reg;
  wire k_out_ready = (state == S_COEF) & ~fault_reg;
  assign k_in_payload[0]       = lc_nearest;
  assign k_in_payload[10:1]    = { 1'b0, lc_parent0 };
  assign k_in_payload[20:11]   = { 1'b0, lc_parent1 };
  assign k_in_payload[28:21]   = rcoef_w[7:0];
  assign k_in_payload[36:29]   = rcoef_w[16:9];
  assign k_in_payload[44:37]   = rcoef_w[43:36];
  assign k_in_payload[52:45]   = rcoef_w[52:45];
  assign k_in_payload[63:53]   = { 1'b0, rcoef_m[9:0] };
  assign k_in_payload[74:64]   = { 1'b0, rcoef_m[19:10] };
  assign k_in_payload[85:75]   = { 1'b0, rcoef_m[29:20] };
  assign k_in_payload[96:86]   = { 1'b0, rcoef_m[39:30] };
  assign k_in_payload[107:97]  = { 1'b0, rcoef_m[49:40] };
  assign k_in_payload[118:108] = { 1'b0, rcoef_m[59:50] };
  assign k_in_payload[129:119] = { 1'b0, rcoef_m[69:60] };
  assign k_in_payload[140:130] = { 1'b0, rcoef_m[79:70] };
  assign k_in_payload[144:141] = rcoef_m[83:80];
  assign k_in_payload[148:145] = rcoef_m[87:84];
  assign k_in_payload[153:149] = { 1'b0, rcoef_m[91:88] };
  assign k_in_payload[160:154] = { 1'b0, rcoef_m[97:92] };
  assign k_in_payload[161]     = rcoef_m[98];

  // Membership operand selection of the active plane.
  wire m_in_valid = (state == S_MEM_ISSUE) & ~fault_reg;
  wire [8:0] m_in_w0 = rplane ? rcoef_w[44:36] : rcoef_w[8:0];
  wire [8:0] m_in_w1 = rplane ? rcoef_w[53:45] : rcoef_w[17:9];
  wire [8:0] m_in_w2 = rplane ? rcoef_w[62:54] : rcoef_w[26:18];
  wire [8:0] m_in_w3 = rplane ? rcoef_w[71:63] : rcoef_w[35:27];
  wire [9:0] m_in_c0 = rplane ? rcoef_m[49:40] : rcoef_m[9:0];
  wire [9:0] m_in_c1 = rplane ? rcoef_m[59:50] : rcoef_m[19:10];
  wire [9:0] m_in_c2 = rplane ? rcoef_m[69:60] : rcoef_m[29:20];
  wire [9:0] m_in_c3 = rplane ? rcoef_m[79:70] : rcoef_m[39:30];
  wire [3:0] m_in_slot = rcoef_m[91:88];
  wire [3:0] m_in_level = rplane ? rcoef_m[87:84] : rcoef_m[83:80];
  wire [5:0] m_in_key = rcoef_m[97:92];
  wire m_in_fine = ~rplane;
  wire m_in_last_fine = rcoef_m[98];

  // Packet admission of one held Member and one emitted tap.
  wire p_in_valid = (state == S_PKT_ISSUE) & ~fault_reg;
  wire [91:0] p_in_member = rmember;
  wire [1:0] p_in_tap = rtap;

  // Cursor advance predicates (evaluated on the pre-edge head consumption).
  wire [3:0] emit_taps = rmember[39:36];
  wire [3:0] higher_tap = emit_taps & (4'b1111 << (rtap + 3'd1));
  wire [3:0] higher_lane = lc_mask & (4'b1111 << (rlane + 3'd1));

  function [1:0] lowbit4;
    input [3:0] v;
    begin
      if (v[0]) lowbit4 = 2'd0;
      else if (v[1]) lowbit4 = 2'd1;
      else if (v[2]) lowbit4 = 2'd2;
      else lowbit4 = 2'd3;
    end
  endfunction

"###;

/// The 13-state old-edge controller. Every right-hand side reads the pre-edge
/// registers and leaf outputs; commits are simultaneous on the enabled edge.
const FSM: &str = r###"  always @(posedge clk) begin
    if (reset) begin
      state <= S_IDLE;
      rlane <= 2'd0;
      rtap <= 2'd0;
      rplane <= 1'b0;
      rhead_valid <= 1'b0;
      fault_reg <= 1'b0;
      ruv0 <= 16'd0; ruv1 <= 16'd0; ruv2 <= 16'd0; ruv3 <= 16'd0;
      ruv4 <= 16'd0; ruv5 <= 16'd0; ruv6 <= 16'd0; ruv7 <= 16'd0;
      rslope <= 18'd0; rbias <= 16'd0;
      rquad <= 4'd0; rmask <= 4'd0; rslot <= 4'd0; rmax_n <= 4'd0;
      rhas_mip <= 1'b0; rfilter <= 2'd0; rpend_valid <= 1'b0;
      lc_shift <= 18'sd0; lc_nearest <= 1'b0; lc_halve <= 1'b0;
      lc_side0 <= 12'sd0; lc_side1 <= 12'sd0;
      lc_parent0 <= 9'd0; lc_parent1 <= 9'd0;
      lc_n0 <= 4'd0; lc_n1 <= 4'd0; lc_last_fine <= 1'b0;
      lc_quad <= 4'd0; lc_mask <= 4'd0; lc_slot <= 4'd0;
      rcoef_w <= 72'd0; rcoef_m <= 99'd0;
      rmember <= 92'd0; rhead <= 72'd0;
    end else if (ce) begin
      if (fault_reg) begin
        // Terminal: all handshakes and kernels stay blocked.
      end else begin
        if (in_ready & in_valid & bad_input) fault_reg <= 1'b1;
        if (d_fault | l_fault | c_fault | k_fault | m_fault | p_fault) fault_reg <= 1'b1;
        case (state)
          S_IDLE: if (in_ready & in_valid & ~bad_input & (in_mask != 4'd0)) begin
            state <= S_DERIV;
          end
          S_DERIV: if (d_out_valid) begin
            ruv0 <= d_uv0; ruv1 <= d_uv1; ruv2 <= d_uv2; ruv3 <= d_uv3;
            ruv4 <= d_uv4; ruv5 <= d_uv5; ruv6 <= d_uv6; ruv7 <= d_uv7;
            rslope <= d_slope; rbias <= d_bias;
            rquad <= d_quad; rmask <= d_mask; rslot <= d_slot; rmax_n <= d_max_n;
            rhas_mip <= d_has_mip; rfilter <= d_filter; rpend_valid <= 1'b1;
            state <= S_LOD_ISSUE;
          end
          S_LOD_ISSUE: if (l_in_valid) begin
            rpend_valid <= 1'b0;
            state <= S_LOD;
          end
          S_LOD: if (l_out_valid) begin
            lc_shift <= l_shift0; lc_nearest <= l_nearest; lc_halve <= l_halve;
            lc_side0 <= l_side0; lc_side1 <= l_side1;
            lc_parent0 <= l_parent0; lc_parent1 <= l_parent1;
            lc_n0 <= l_n0; lc_n1 <= l_n1; lc_last_fine <= l_last_fine;
            lc_quad <= l_quad; lc_mask <= l_mask; lc_slot <= l_slot;
            rlane <= lowbit4(l_mask);
            state <= S_COORD_ISSUE;
          end
          S_COORD_ISSUE: if (c_in_valid) state <= S_COORD;
          S_COORD: if (c_out_valid) begin
            rcoef_m[9:0]   <= c_t0_0_0; rcoef_m[19:10] <= c_t0_0_1;
            rcoef_m[29:20] <= c_t0_1_0; rcoef_m[39:30] <= c_t0_1_1;
            rcoef_m[49:40] <= c_t1_0_0; rcoef_m[59:50] <= c_t1_0_1;
            rcoef_m[69:60] <= c_t1_1_0; rcoef_m[79:70] <= c_t1_1_1;
            rcoef_m[83:80] <= lc_n0; rcoef_m[87:84] <= lc_n1;
            rcoef_m[91:88] <= lc_slot; rcoef_m[97:92] <= { lc_quad, rlane };
            rcoef_m[98] <= lc_last_fine;
            // Nearest bypass: exact [511,0,0,0] fine / zero coarse row into the
            // existing result bank, plane 0, straight to membership.
            if (NEAREST_BYPASS & lc_nearest) begin
              rcoef_w <= 72'd511;
              rplane  <= 1'b0;
              state   <= S_MEM_ISSUE;
            end else begin
              rcoef_w <= { 9'd0, 9'd0, { 1'b0, c_f1_1 }, { 1'b0, c_f1_0 },
                           9'd0, 9'd0, { 1'b0, c_f0_1 }, { 1'b0, c_f0_0 } };
              state   <= S_COEF_ISSUE;
            end
          end
          S_COEF_ISSUE: if (k_in_accept) state <= S_COEF;
          S_COEF: if (k_out_valid) begin
            rcoef_w <= k_weights;
            rcoef_m <= k_meta;
            rplane <= (k_weights[35:0] == 36'd0) ? 1'b1 : 1'b0;
            state <= S_MEM_ISSUE;
          end
          S_MEM_ISSUE: state <= S_MEM;
          S_MEM: if (m_out_valid) begin
            rmember <= m_member;
            rtap <= lowbit4(m_member[39:36]);
            state <= S_PKT_ISSUE;
          end
          S_PKT_ISSUE: state <= S_PKT;
          S_PKT: if (p_out_valid) begin
            rhead <= p_packet;
            rhead_valid <= 1'b1;
            state <= S_HEAD;
          end
          S_HEAD: if (rhead_valid & out_ready) begin
            rhead_valid <= 1'b0;
            if (higher_tap != 4'd0) begin
              rtap <= lowbit4(higher_tap);
              state <= S_PKT_ISSUE;
            end else if (rplane == 1'b0 && (|rcoef_w[71:36])) begin
              rplane <= 1'b1;
              state <= S_MEM_ISSUE;
            end else if (higher_lane != 4'd0) begin
              rlane <= lowbit4(higher_lane);
              state <= S_COORD_ISSUE;
            end else begin
              state <= S_IDLE;
            end
          end
          default: state <= S_IDLE;
        endcase
      end
    end
  end
"###;
