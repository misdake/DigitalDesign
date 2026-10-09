//! Whole-sampler synthesizable top: the accepted preparation, demand-cache and
//! color leaves wired exactly as [`crate::texture::emu::sampler::SamplerEmu`].
//!
//! This module owns only the *new* wrapper logic; it never re-implements a
//! leaf. The emitted source is the concatenation of
//!
//! * [`crate::texture::sim::staged::bound::serial_rtl::source`] (the one-quad
//!   serial preparation controller and its D/LOD/coordinate/coefficient/
//!   membership/packet leaves);
//! * [`crate::texture::rtl::cache::verilog`] (the slot-specialized demand cache);
//! * [`crate::texture::rtl::color::verilog`] (the eight-edge color leaf);
//! * the hand-written `gpu_v2_texture_sampler` wrapper below.
//!
//! ## Wrapper contract (mirrors `SamplerEmu`)
//!
//! * One active quad at a time. A new raw quad is admitted only when no active
//!   quad is held, and only from the pre-edge state, so a same-edge result
//!   transfer can never fund a same-edge admission.
//! * The raw fields are the preparation fields: eight signed-40 helper UV, a
//!   signed-16 bias, `quad4`/`mask4`/`slot4`/`max_n4`, `has_mip` and `filter2`.
//! * Preparation head transfers exactly when the cache accepts; the cache
//!   transfers exactly when the color leaf accepts. Every branch ready/valid is
//!   a continuous pre-edge fanout of register state.
//! * Color outputs are always consumed (`out_ready` tied high) into the active
//!   result bank, validating the six-bit key, the mask coverage and the
//!   duplicate rule. A violation latches `wrapper_fault`.
//! * A freshly admitted quad starts with every unfilled lane white (255,255,255)
//!   and `done=0`; a zero mask therefore completes with no kernel, cache or
//!   color activity.
//! * The held result (`quad4`/`mask4`/`rgb96`) stays valid under backpressure.
//!   Any latched fault freezes compute and handshakes, asserts the cache abort,
//!   and preserves a presented request/terminal maintenance until it drains.
//!
//! Declared additive wrapper inventory (declarations, not a fitted Gowin
//! result): data 108 bits = `quad4` + `mask4` + `done4` + RGB96; control 2 bits
//! = active-valid and terminal wrapper-fault. The leaves keep their own
//! separately audited inventories.

use crate::texture::ports::Slot;
use crate::texture::rtl::{cache, color};
use crate::texture::sim::staged::bound::{serial, serial_rtl};

/// Verilog module name emitted by [`verilog`].
pub const MODULE: &str = "gpu_v2_texture_sampler";

/// Additive wrapper data bank: quad4 + mask4 + done4 + RGB96.
pub const WRAPPER_DATA_BITS: usize = 4 + 4 + 4 + 96;
/// Additive wrapper control bank: active-valid + terminal wrapper-fault.
pub const WRAPPER_CONTROL_BITS: usize = 2;
/// Result lane width: three UNORM8 channels.
pub const LANE_BITS: usize = 24;

/// Complete composed source plus the byte-exact wrapper text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub source: String,
    pub wrapper: String,
}

/// Compose the complete synthesizable whole-sampler source for a slot table.
pub fn build(slots: &[Slot]) -> Result<Plan, String> {
    build_with_config(slots, serial::Config::default())
}

/// Same wrapper/cache/color with the reviewed preparation configuration.
pub fn build_with_config(slots: &[Slot], config: serial::Config) -> Result<Plan, String> {
    if slots.is_empty() || slots.len() > 16 {
        return Err("whole sampler slot count".into());
    }
    for slot in slots {
        slot.validate()?;
    }
    let preparation = serial_rtl::source_with_config(config)?;
    let cache_source = cache::verilog(slots);
    let color_source = color::verilog();
    let wrapper = wrapper_source();
    let mut source = String::new();
    source.push_str(&preparation);
    source.push('\n');
    source.push_str(&cache_source);
    source.push('\n');
    source.push_str(color_source);
    source.push('\n');
    source.push_str(&wrapper);
    Ok(Plan { source, wrapper })
}

/// Complete composed Verilog source for a slot table.
pub fn verilog(slots: &[Slot]) -> Result<String, String> {
    Ok(build(slots)?.source)
}

pub fn verilog_with_config(slots: &[Slot], config: serial::Config) -> Result<String, String> {
    Ok(build_with_config(slots, config)?.source)
}

/// The hand-written wrapper module text (no leaf bodies).
pub fn wrapper_source() -> String {
    format!(
        r###"// {MODULE}: whole-sampler wrapper. One active quad, held result,
// pre-edge ready/valid fanout, color validation and terminal wrapper fault.
// Leaves are instantiated by name only; this module owns the 108+2 wrapper bank.
module {MODULE} (
  input  wire        clk,
  input  wire        reset,
  input  wire        ce,
  input  wire        in_valid,
  input  wire signed [17:0] in_uv_0,
  input  wire signed [17:0] in_uv_1,
  input  wire signed [17:0] in_uv_2,
  input  wire signed [17:0] in_uv_3,
  input  wire signed [17:0] in_uv_4,
  input  wire signed [17:0] in_uv_5,
  input  wire signed [17:0] in_uv_6,
  input  wire signed [17:0] in_uv_7,
  input  wire signed [15:0] in_bias,
  input  wire [3:0]  in_quad,
  input  wire [3:0]  in_mask,
  input  wire [3:0]  in_slot,
  input  wire [3:0]  in_max_n,
  input  wire        in_has_mip,
  input  wire [1:0]  in_filter,
  input wire in_force_coarsest,
  output wire        in_ready,
  output wire        in_accept,
  input  wire        out_ready,
  output wire        out_valid,
  output wire [3:0]  out_quad,
  output wire [3:0]  out_mask,
  output wire [95:0] out_rgb,
  output wire        out_transfer,
  output wire        cache_accept,
  output wire        color_accept,
  output wire        fault,
  output wire        mem_req_valid,
  output wire [31:0] mem_req_addr,
  input  wire        mem_req_ready,
  input  wire        mem_resp_valid,
  input  wire [3:0]  mem_resp_index,
  input  wire [63:0] mem_resp_data,
  input  wire        mem_resp_complete,
  input  wire        mem_resp_ok
);
  wire        prep_in_ready, prep_in_accept, prep_out_valid;
  wire [71:0] prep_out_packet;
  wire [3:0]  prep_out_state;
  wire        prep_fault;
  wire        cache_in_ready, cache_out_valid;
  wire [71:0] cache_out_payload;
  wire [15:0] cache_tex0, cache_tex1, cache_tex2, cache_tex3;
  wire        cache_fault;
  wire        color_in_ready, color_out_valid;
  wire [5:0]  color_key;
  wire [7:0]  color_r, color_g, color_b;
  wire        color_fault;

  reg         active_valid;
  reg  [3:0]  rquad, rmask, rdone;
  reg  [95:0] rrgb;
  reg         wrapper_fault;

  wire fault_eff = wrapper_fault | prep_fault | cache_fault | color_fault;
  wire gce = ce & ~fault_eff;

  // Wrapper admission and held-result handshake, all from pre-edge registers.
  wire admit = in_valid & ~active_valid & ~fault_eff;
  assign in_ready  = ~active_valid & ~fault_eff & prep_in_ready;
  assign in_accept = prep_in_accept;
  wire done_all = (rdone == rmask);
  assign out_valid    = active_valid & done_all & ~fault_eff;
  assign out_quad     = rquad;
  assign out_mask     = rmask;
  assign out_rgb      = rrgb;
  assign out_transfer = ce & out_ready & out_valid;
  assign fault        = fault_eff;

  // Exact branch chain identities: prep transfer == cache admit, cache
  // transfer == color accept.
  assign cache_accept = prep_out_valid & cache_in_ready;
  assign color_accept = cache_out_valid & color_in_ready;

  wire [1:0] c_lane = color_key[1:0];
  wire [3:0] c_quad = color_key[5:2];
  wire color_bad = color_out_valid & ~fault_eff &
      ((c_quad != rquad) | ~rmask[c_lane] | rdone[c_lane]);
  wire color_owner_bad = color_out_valid & ~fault_eff & ~active_valid;

  always @(posedge clk) begin
    if (reset) begin
      active_valid <= 1'b0;
      rquad <= 4'd0;
      rmask <= 4'd0;
      rdone <= 4'd0;
      rrgb <= 96'd0;
      wrapper_fault <= 1'b0;
    end else if (ce & ~fault_eff) begin
      if (color_out_valid) begin
        if (color_owner_bad | color_bad) begin
          wrapper_fault <= 1'b1;
        end else begin
          rrgb[c_lane*24 +: 24] <= {{color_r, color_g, color_b}};
          rdone[c_lane] <= 1'b1;
        end
      end
      if (in_accept) begin
        active_valid <= 1'b1;
        rquad <= in_quad;
        rmask <= in_mask;
        rdone <= 4'd0;
        rrgb <= {{96{{1'b1}}}};
      end
      if (out_valid & out_ready) begin
        active_valid <= 1'b0;
      end
    end
  end

  {prep_module} u_prep (
    .clk(clk), .reset(reset), .ce(gce), .in_valid(admit),
    .in_force_coarsest(in_force_coarsest),
    .in_uv_0(in_uv_0), .in_uv_1(in_uv_1), .in_uv_2(in_uv_2), .in_uv_3(in_uv_3),
    .in_uv_4(in_uv_4), .in_uv_5(in_uv_5), .in_uv_6(in_uv_6), .in_uv_7(in_uv_7),
    .in_bias(in_bias), .in_quad(in_quad), .in_mask(in_mask), .in_slot(in_slot),
    .in_max_n(in_max_n), .in_has_mip(in_has_mip), .in_filter(in_filter),
    .in_ready(prep_in_ready), .in_accept(prep_in_accept),
    .out_ready(cache_in_ready), .out_valid(prep_out_valid),
    .out_packet(prep_out_packet), .out_state(prep_out_state), .fault(prep_fault)
  );

  {cache_module} u_cache (
    .clk(clk), .reset(reset), .ce(gce), .abort(fault_eff),
    .in_valid(prep_out_valid), .in_payload(prep_out_packet),
    .in_ready(cache_in_ready), .out_ready(color_in_ready),
    .out_valid(cache_out_valid), .out_payload(cache_out_payload),
    .out_tex0(cache_tex0), .out_tex1(cache_tex1),
    .out_tex2(cache_tex2), .out_tex3(cache_tex3),
    .mem_req_valid(mem_req_valid), .mem_req_addr(mem_req_addr),
    .mem_req_ready(mem_req_ready),
    .mem_resp_valid(mem_resp_valid), .mem_resp_index(mem_resp_index),
    .mem_resp_data(mem_resp_data),
    .mem_resp_complete(mem_resp_complete), .mem_resp_ok(mem_resp_ok),
    .fault(cache_fault)
  );

  {color_module} u_color (
    .clk(clk), .reset(reset), .ce(gce),
    .in_valid(cache_out_valid), .in_payload(cache_out_payload),
    .in_tex0(cache_tex0), .in_tex1(cache_tex1),
    .in_tex2(cache_tex2), .in_tex3(cache_tex3),
    .in_ready(color_in_ready), .out_ready(1'b1), .out_valid(color_out_valid),
    .out_key(color_key), .out_r(color_r), .out_g(color_g), .out_b(color_b),
    .fault(color_fault)
  );
endmodule
"###,
        prep_module = serial_rtl::TOP,
        cache_module = cache::MODULE,
        color_module = color::MODULE,
    )
}

/// Audit the composed module against the expected anchors and inventory.
pub fn audit() -> Result<(), String> {
    let slots = vec![Slot {
        base_address: 0,
        has_full_mip: false,
        max_size_log2: 0,
        valid: true,
    }];
    let plan = build(&slots)?;
    for anchor in [
        format!("module {MODULE} ("),
        format!("module {} (", serial_rtl::TOP),
        format!("module {}(", cache::MODULE),
        format!("module {}(", color::MODULE),
        "endmodule".into(),
    ] {
        if !plan.source.contains(&anchor) {
            return Err(format!("whole sampler RTL missing anchor: {anchor}"));
        }
    }
    for anchor in [
        "wire fault_eff = wrapper_fault | prep_fault | cache_fault | color_fault;",
        "assign in_ready  = ~active_valid & ~fault_eff & prep_in_ready;",
        "assign cache_accept = prep_out_valid & cache_in_ready;",
        "assign color_accept = cache_out_valid & color_in_ready;",
        ".abort(fault_eff)",
    ] {
        if !plan.wrapper.contains(anchor) {
            return Err(format!("whole sampler wrapper missing anchor: {anchor}"));
        }
    }
    if WRAPPER_DATA_BITS != 108 || WRAPPER_CONTROL_BITS != 2 {
        return Err("whole sampler wrapper inventory".into());
    }
    // Seven preparation modules (D/LOD/coordinate/coefficient/membership/
    // packet/controller), the cache, the color leaf and this wrapper.
    if plan.source.matches("endmodule").count() != 10 {
        return Err("whole sampler unexpected module count".into());
    }
    Ok(())
}
