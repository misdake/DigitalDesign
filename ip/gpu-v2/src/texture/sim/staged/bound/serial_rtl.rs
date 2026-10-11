//! Deterministic synthesizable lowering of the one-quad serial preparation
//! controller in [`super::serial`].
//!
//! This generator instantiates the *actual* numerical kernels and the accepted
//! leaf modules; it does not model arithmetic, embed test vectors or retain a
//! sampled answer:
//!
//! * `D` comes from
//!   [`crate::texture::emu::derivative::rtl::emit`] over the immutable
//!   [`super::serial::structural_calendars`] calendars (II8/II8/II2).
//! * LOD uses the grouped checked-domain kernel in [`crate::texture::rtl::lod`],
//!   with the generic calendar and emulator retained independently;
//! * coordinate uses the checked physical-mip kernel in
//!   [`crate::texture::rtl::compact_coordinate`]; the generic calendar and independent
//!   emulator remain its numerical/phase reference;
//! * the coefficient leaf is the hand-written accepted RTL
//!   [`crate::texture::rtl::compact_coefficient`];
//! * membership and packet use [`crate::texture::rtl::compact_leaves`].
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
//! * data54 (pending LOD input53 + valid) +26 (LOD decision/id) +124
//!   (fine fraction16 + weights72 + metadata36) =204 bits. Coordinate borrows
//!   D's held UV8x16; Member and Packet borrow stable leaf result banks.
//! * control 4 (state) + 2 (lane) + 1 (plane) + 2 (tap) + 1 (head valid) +
//!   1 (terminal fault) = 11 bits.
//!
//! These widths are *independently verified* by [`audit`] against the emitted
//! kernel descriptors rather than copied from the root baseline. The coefficient
//! leaf uses local parity plus five valid bits; the
//! controller drives it through its published `in_ready`/`in_accept` handshake
//! and never assumes the Rust phase encoding.
//!
//! This is the full preparation chain only. It adds no texture cache, color or
//! result stage, and makes no fitted area, DSP/BSRAM, fmax or throughput claim.

use super::runtime_rtl;
use super::serial;
use crate::texture::emu::derivative::rtl::{self, Descriptor, Io};
use crate::texture::rtl::{
    compact_coefficient as coefficient, compact_coordinate as coordinate, compact_leaves,
    lod as lod_rtl,
};

/// Top-level module instantiated by the qualification testbench.
pub const TOP: &str = "gpu_v2_texture_serial_preparation";
/// Additive controller data-bank baseline (see module docs).
pub const CONTROLLER_DATA_BITS: usize = 54 + 26 + 124;
/// Additive controller control-bank baseline.
pub const CONTROLLER_CONTROL_BITS: usize = 4 + 2 + 1 + 2 + 1 + 1;

/// Active data bits of the compact membership leaf: Common72 + Member76.
pub const MEMBERSHIP_SHORT_DATA_BITS: u32 = 72 + 76;
/// Two valid bits and one terminal fault bit.
pub const MEMBERSHIP_SHORT_CONTROL_BITS: u32 = 2 + 1;
/// Frozen default membership calendar, independent of the compact lowering.
pub const MEMBERSHIP_LONG_DATA_BITS: u32 = 72 + 6 * 76;
/// Active control bits of the default membership leaf (7 valid + fault).
pub const MEMBERSHIP_LONG_CONTROL_BITS: u32 = 7 + 1;
/// Compact packet capture77 and packed72; selection is combinational at E1.
pub const PACKET_SHORT_DATA_BITS: u32 = 77 + 72;
/// Two valid bits and one fault bit.
pub const PACKET_SHORT_CONTROL_BITS: u32 = 2 + 1;
/// Frozen default packet calendar, independent of the compact lowering.
pub const PACKET_LONG_DATA_BITS: u32 = 77 + 8 * 72;
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
                latency: 2,
                alignment_banks: 0,
            }
        } else {
            Self {
                data_bits: MEMBERSHIP_LONG_DATA_BITS,
                control_bits: MEMBERSHIP_LONG_CONTROL_BITS,
                latency: 7,
                alignment_banks: 5,
            }
        }
    }

    pub const fn packet(short_alignment: bool) -> Self {
        if short_alignment {
            Self {
                data_bits: PACKET_SHORT_DATA_BITS,
                control_bits: PACKET_SHORT_CONTROL_BITS,
                latency: 2,
                alignment_banks: 0,
            }
        } else {
            Self {
                data_bits: PACKET_LONG_DATA_BITS,
                control_bits: PACKET_LONG_CONTROL_BITS,
                latency: 9,
                alignment_banks: 7,
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
    /// Generic independent numerical/calendar reference.
    pub lod: Descriptor,
    pub lod_data_bits: usize,
    pub lod_control_bits: usize,
    /// Generic numerical reference descriptor; selected physical storage is
    /// reported separately, not substituted into the reference calendar.
    pub coord: Descriptor,
    /// Actual selected coordinate declarations (V/config32 + fine result36).
    pub coordinate_data_bits: usize,
    /// Interface bits read from D's paid bank; not controller UV storage.
    pub borrowed_uv_bits: u32,
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
        ("D", &d.inventory, 219usize, 3u8, 8u32, 8u32),
        ("LOD", &lod.inventory, 83, 4, 8, 8),
        ("COORD", &coord.inventory, 188, 2, 2, 2),
    ] {
        if inv.numeric_bits != numeric || inv.span != span || inv.period != period || inv.ii != ii {
            return Err(format!(
                "{name} calendar inventory mismatch: numeric {} span {} period {} ii {}",
                inv.numeric_bits, inv.span, inv.period, inv.ii
            ));
        }
    }

    let borrowed_uv_bits = (0..8).try_fold(0, |bits, i| {
        let io = d
            .outputs
            .iter()
            .find(|io| io.key == format!("uv{i}"))
            .ok_or("D borrowed UV output missing")?;
        if io.width != 16 {
            return Err("D borrowed UV output width");
        }
        Ok(bits + io.width)
    })?;
    let widths = Widths {
        uv: 0,
        pending_lod: sum_widths(&lod.inputs) + 1,
        lod_context: 26,
        coefficient: (coefficient::OUT_PAYLOAD_BITS + 16) as u32,
        member: 0,
        head: 0,
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
        && (membership.data_bits != MEMBERSHIP_LONG_DATA_BITS
            || membership.control_bits != runtime_rtl::MEMBERSHIP_CONTROL_BITS
            || packet.data_bits != PACKET_LONG_DATA_BITS
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
    source.push_str(&lod_rtl::verilog());
    source.push('\n');
    if coord.module != coordinate::MODULE {
        return Err("selected coordinate module identity".into());
    }
    source.push_str(coordinate::verilog());
    source.push('\n');
    source.push_str(&coefficient::serial_verilog());
    source.push('\n');
    source.push_str(compact_leaves::membership());
    source.push('\n');
    source.push_str(compact_leaves::packet());
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
        lod_data_bits: lod_rtl::NUMERIC_BITS,
        lod_control_bits: lod_rtl::CONTROL_BITS,
        coord,
        coordinate_data_bits: coordinate::NUMERIC_BITS,
        borrowed_uv_bits,
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
        "reg [71:0] ready_ff0;",
        "assign ready_value=ready_ff0;",
        "cohorts<1 && queued==0",
        "module membership_leaf #(",
        "module packet_leaf #(",
        "endmodule",
    ] {
        if !src.contains(anchor) {
            return Err(format!("serial preparation RTL missing anchor: {anchor}"));
        }
    }
    if src.contains("ready_ff1") {
        return Err("closed Serial coefficient has a second ready row".into());
    }
    if plan.widths.data() != CONTROLLER_DATA_BITS as u32
        || plan.widths.uv != 0
        || plan.borrowed_uv_bits != 128
        || plan.widths.pending_lod == 0
        || plan.widths.lod_context == 0
        || plan.widths.head != 0
    {
        return Err("serial preparation width invariant".into());
    }
    if src.contains("ruv0") || !src.contains("wire [15:0] c_in_uv0 = (rlane == 2'd0) ? d_uv0") {
        return Err("Serial Coordinate must borrow D UV directly".into());
    }
    if config.short_alignment {
        let short_membership = LeafInventory {
            data_bits: MEMBERSHIP_SHORT_DATA_BITS,
            control_bits: MEMBERSHIP_SHORT_CONTROL_BITS,
            latency: 2,
            alignment_banks: 0,
        };
        let short_packet = LeafInventory {
            data_bits: PACKET_SHORT_DATA_BITS,
            control_bits: PACKET_SHORT_CONTROL_BITS,
            latency: 2,
            alignment_banks: 0,
        };
        if plan.membership != short_membership || plan.packet != short_packet {
            return Err("short leaf inventory mismatch".into());
        }
    } else if plan.membership.data_bits != MEMBERSHIP_LONG_DATA_BITS
        || plan.membership.control_bits != runtime_rtl::MEMBERSHIP_CONTROL_BITS
        || plan.membership.latency != runtime_rtl::MEMBERSHIP_LATENCY
        || plan.packet.data_bits != PACKET_LONG_DATA_BITS
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
    let head = CONTROLLER_HEAD.replace(
        "// READY_DECODE",
        &crate::texture::rtl::context_store::ready_decode("lc", "lc_decision"),
    );
    if config.nearest_bypass {
        head.replace(
            "localparam NEAREST_BYPASS = 1'b0;",
            "localparam NEAREST_BYPASS = 1'b1;",
        )
    } else {
        head
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

  // Controller state banks (204 data +11 control; borrowed leaf results).
  reg [3:0]  state;
  reg [1:0]  rlane, rtap;
  reg        rplane;
  reg        rhead_valid, fault_reg;
  // Closed Serial borrows D UV until the last Coordinate acceptance.
  reg [17:0] rslope;
  reg [15:0] rbias;
  reg [3:0]  rquad, rmask, rslot, rmax_n;
  reg        rhas_mip;
  reg [1:0]  rfilter;
  reg        rpend_valid;
  reg [25:0] lc_decision;
  // READY_DECODE
  wire lc_nearest=lc_decision[4],lc_halve=(lc_n0>1),lc_last_fine=(lc_parent1==0);
  wire[3:0] lc_quad=lc_decision[17:14],lc_mask=lc_decision[21:18],lc_slot=lc_decision[25:22];
  reg [15:0] rfrac;
  reg [71:0] rcoef_w;
  reg [35:0] rcoef_m;
  wire [75:0] rmember;
  wire [71:0] rhead;

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
  wire [35:0] k_meta;
  wire m_in_ready, m_out_valid, m_fault;
  wire [75:0] m_member;
  wire p_in_ready, p_out_valid, p_fault;
  wire [71:0] p_packet;
  assign rmember=m_member;
  assign rhead=p_packet;

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
  wire [15:0] c_in_uv0 = (rlane == 2'd0) ? d_uv0 : (rlane == 2'd1) ? d_uv2
                       : (rlane == 2'd2) ? d_uv4 : d_uv6;
  wire [15:0] c_in_uv1 = (rlane == 2'd0) ? d_uv1 : (rlane == 2'd1) ? d_uv3
                       : (rlane == 2'd2) ? d_uv5 : d_uv7;
  wire c_in_valid = (state == S_COORD_ISSUE) & c_in_ready & ~fault_reg;

  // Fractions have their own 32-bit bank, avoiding operand/result overlay.
  // Metadata stays with this lane until its last group; coefficient echoes
  // cannot change it. Member and Packet result banks remain stable until the
  // controller explicitly issues their next job, so no payload copy is needed.
  wire [161:0] k_in_payload;
  wire [7:0]   k_work = 8'd16;
  wire k_in_valid = (state == S_COEF_ISSUE) & ~fault_reg;
  wire k_out_ready = (state == S_COEF) & ~fault_reg;
  assign k_in_payload[0]       = lc_nearest;
  assign k_in_payload[10:1]    = { 1'b0, lc_parent0 };
  assign k_in_payload[20:11]   = { 1'b0, lc_parent1 };
  assign k_in_payload[28:21]   = rfrac[7:0];
  assign k_in_payload[36:29]   = rfrac[15:8];
  assign k_in_payload[44:37]   = 8'd0;
  assign k_in_payload[52:45]   = 8'd0;
  assign k_in_payload[63:53]={1'b0,rcoef_m[9:0]};
  assign k_in_payload[74:64]=11'd0;
  assign k_in_payload[85:75]={1'b0,rcoef_m[19:10]};
  assign k_in_payload[140:86]=55'd0;
  assign k_in_payload[144:141]=rcoef_m[25:22];
  assign k_in_payload[148:145]=lc_n1;
  assign k_in_payload[153:149]={1'b0,rcoef_m[29:26]};
  assign k_in_payload[160:154]={1'b0,rcoef_m[35:30]};
  assign k_in_payload[161]=lc_last_fine;
  wire [8:0] cx=(rcoef_m[9:1]-{8'd0,rcoef_m[20]});
  wire [8:0] cy=(rcoef_m[19:11]-{8'd0,rcoef_m[21]});
  wire [9:0] cmask=10'd1023>>(4'd11-rcoef_m[25:22]);
  wire [9:0] coarse_x=(rcoef_m[25:22]<=1)?rcoef_m[9:0]:({1'b0,cx}&cmask);
  wire [9:0] coarse_y=(rcoef_m[25:22]<=1)?rcoef_m[19:10]:({1'b0,cy}&cmask);
  wire [8:0] m_in_w0 = rplane ? rcoef_w[44:36] : rcoef_w[8:0];
  wire [8:0] m_in_w1 = rplane ? rcoef_w[53:45] : rcoef_w[17:9];
  wire [8:0] m_in_w2 = rplane ? rcoef_w[62:54] : rcoef_w[26:18];
  wire [8:0] m_in_w3 = rplane ? rcoef_w[71:63] : rcoef_w[35:27];
  wire [9:0] m_in_c0=rplane?coarse_x:rcoef_m[9:0];
  wire [9:0] m_in_c2=rplane?coarse_y:rcoef_m[19:10];
  wire [9:0] m_in_c1=0,m_in_c3=0; // Selected leaf consumes base only.
  wire [3:0] m_in_slot=rcoef_m[29:26];
  wire [3:0] m_in_level=rplane?lc_n1:rcoef_m[25:22];
  wire [5:0] m_in_key=rcoef_m[35:30];
  wire m_in_fine=~rplane;
  wire m_in_last_fine=!(|rcoef_w[71:36]);
  wire m_in_valid=(state==S_MEM_ISSUE)&~fault_reg;
  wire p_in_valid=(state==S_PKT_ISSUE)&~fault_reg;
  wire [75:0] p_in_member = rmember;
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
      rpend_valid <= 1'b0;
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
            lc_decision <= {l_slot,l_mask,l_quad,l_parent1,l_nearest,l_n0};
            rlane <= lowbit4(l_mask);
            state <= S_COORD_ISSUE;
          end
          S_COORD_ISSUE: if (c_in_valid) state <= S_COORD;
          S_COORD: if (c_out_valid) begin
            rcoef_m<={{lc_quad,rlane},lc_slot,lc_n0,
              (!c_t0_1_0[0]&&!c_f0_1[7]),(!c_t0_0_0[0]&&!c_f0_0[7]),c_t0_1_0,c_t0_0_0};
            if (NEAREST_BYPASS & lc_nearest) begin
              rcoef_w <= 72'd511;
              rplane  <= 1'b0;
              state   <= S_MEM_ISSUE;
            end else begin
              rfrac <= {c_f0_1,c_f0_0};
              state   <= S_COEF_ISSUE;
            end
          end
          S_COEF_ISSUE: if (k_in_accept) state <= S_COEF;
          S_COEF: if (k_out_valid) begin
            rcoef_w <= k_weights;
            // Keep the current lane metadata; no redundant coefficient echo.
            rplane <= (k_weights[35:0] == 36'd0) ? 1'b1 : 1'b0;
            state <= S_MEM_ISSUE;
          end
          S_MEM_ISSUE: state <= S_MEM;
          S_MEM: if (m_out_valid) begin
            // Borrow the membership result until the next plane is issued.
            rtap <= lowbit4(m_member[39:36]);
            state <= S_PKT_ISSUE;
          end
          S_PKT_ISSUE: state <= S_PKT;
          S_PKT: if (p_out_valid) begin
            // Packet result remains stable throughout the held-head state.
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
