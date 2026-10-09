//! Hand-written synthesizable RTL for the conserving UNORM9 coefficient leaf.
//!
//! This module mirrors [`crate::texture::emu::coefficient::CoefficientEmu`] edge
//! for edge. The datapath is the real fixed calendar: a 315-bit numeric bank with
//! the exact `LAYOUT` slices, three 9x8 multiply sites each with three registered
//! 17-bit products, two select sites and three subtract sites, a period-8 phase
//! and an 11-bit valid shift, six metadata owners, two 171-bit ready rows and the
//! pre-edge `work_available` admission. It contains ordinary logic and registers
//! only; no golden trace is embedded and no counted/oracle helper is evaluated.
//!
//! The leaf is a numeric arithmetic frontier, not a complete sampler. It makes no
//! Gowin cell, DSP, PnR or full-pipeline claim.

/// Verilog module name emitted by [`verilog`].
pub const MODULE: &str = "gpu_v2_texture_coefficient";

/// Enabled-edge latency from acceptance to a visible result.
pub const LATENCY: usize = 12;
/// Initiation interval: one accepted cohort every two enabled edges.
pub const INITIATION_INTERVAL: usize = 2;
/// Logical numeric bank width (bits 0..314).
pub const NUMERIC_BITS: usize = 315;
/// Number of metadata owners.
pub const COHORT_CAPACITY: usize = 6;
/// Number of ready result rows.
pub const READY_CAPACITY: usize = 2;
/// Input bus width: explicit scalar fields plus the packed 99-bit metadata.
pub const IN_PAYLOAD_BITS: usize = 162;
/// Output bus width: eight 9-bit weights plus the packed 99-bit metadata.
pub const OUT_PAYLOAD_BITS: usize = 171;

/// Logical declaration inventory for this leaf. These are behavioral register and
/// multiplier counts, not fitted Gowin cells; the physical mapping is a separate
/// placement claim and is not asserted here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    /// 9x8 multiply sites.
    pub multiply_sites: usize,
    /// Registered 17-bit products per multiply site.
    pub products_per_site: usize,
    /// Product pipeline latency in enabled edges.
    pub multiply_latency: usize,
    /// Initiation interval in enabled edges.
    pub initiation_interval: usize,
    /// Shared select sites (nearest gate).
    pub select_sites: usize,
    /// Shared subtract sites.
    pub subtract_sites: usize,
    /// Numeric FF bank bits.
    pub numeric_ff_bits: usize,
    /// Metadata ring bits (six 99-bit owners).
    pub metadata_ff_bits: usize,
    /// Ready result bits (two 171-bit rows).
    pub ready_ff_bits: usize,
    /// Period-8 phase and 11-bit valid.
    pub phase_valid_bits: usize,
    /// Ring pointers, cohort count, ready pointers, queue count and fault.
    pub control_bits: usize,
}
impl Default for Allocation {
    fn default() -> Self {
        Self {
            multiply_sites: 3,
            products_per_site: 3,
            multiply_latency: 3,
            initiation_interval: 2,
            select_sites: 2,
            subtract_sites: 3,
            numeric_ff_bits: NUMERIC_BITS,
            metadata_ff_bits: COHORT_CAPACITY * 99,
            ready_ff_bits: READY_CAPACITY * OUT_PAYLOAD_BITS,
            phase_valid_bits: 3 + 11,
            control_bits: 3 + 3 + 3 + 1 + 1 + 2 + 1,
        }
    }
}
impl Allocation {
    /// Total declared state bits held by the leaf.
    pub fn total_state_bits(self) -> usize {
        let product_bits = self.multiply_sites * self.products_per_site * 17;
        self.numeric_ff_bits
            + product_bits
            + self.metadata_ff_bits
            + self.ready_ff_bits
            + self.phase_valid_bits
            + self.control_bits
    }
}

/// Enabled-edge latency.
pub fn latency() -> usize {
    LATENCY
}

/// Initiation interval.
pub fn initiation_interval() -> usize {
    INITIATION_INTERVAL
}

/// The complete synthesizable module. Fixed widths; no parameters.
pub fn verilog() -> &'static str {
    r#"// gpu_v2_texture_coefficient: conserving UNORM9 coefficient leaf.
//
// A cohort is accepted on an even phase and then walks a fixed two-edge calendar
// through a 315-bit numeric bank, three 9x8 multiply sites (three registered
// products each), two select sites and three subtract sites. Corner weights are
// packed into a two-row ready store at age ten and published at age eleven.
// Admission uses the pre-edge work_available count; the leaf retains no work
// credit. Invalid input or an out-of-range work count latches `fault` and freezes.
module gpu_v2_texture_coefficient(
  input  wire         clk,
  input  wire         reset,
  input  wire         ce,
  input  wire         in_valid,
  input  wire [161:0] in_payload,
  input  wire [7:0]   work_available,
  output wire         in_ready,
  output wire         in_accept,
  input  wire         out_ready,
  output wire         out_valid,
  output wire [71:0]  out_weights,
  output wire [98:0]  out_meta,
  output wire [1:0]   work_reserved,
  output reg          fault
);

  // Explicit scalar input fields.
  wire        nearest = in_payload[0];
  wire [9:0]  p0      = in_payload[10:1];
  wire [9:0]  p1      = in_payload[20:11];
  wire [7:0]  f0u     = in_payload[28:21];
  wire [7:0]  f0v     = in_payload[36:29];
  wire [7:0]  f1u     = in_payload[44:37];
  wire [7:0]  f1v     = in_payload[52:45];
  wire [10:0] c00 = in_payload[63:53];
  wire [10:0] c01 = in_payload[74:64];
  wire [10:0] c02 = in_payload[85:75];
  wire [10:0] c03 = in_payload[96:86];
  wire [10:0] c10 = in_payload[107:97];
  wire [10:0] c11 = in_payload[118:108];
  wire [10:0] c12 = in_payload[129:119];
  wire [10:0] c13 = in_payload[140:130];
  wire [3:0]  lv0     = in_payload[144:141];
  wire [3:0]  lv1     = in_payload[148:145];
  wire [4:0]  slot_in = in_payload[153:149];
  wire [6:0]  key_in  = in_payload[160:154];
  wire        last_fine = in_payload[161];

  // Packed 99-bit metadata, same layout as the Rust `pack_metadata`.
  wire [98:0] meta_in = { last_fine, key_in[5:0], slot_in[3:0], lv1, lv0,
                          c13[9:0], c12[9:0], c11[9:0], c10[9:0],
                          c03[9:0], c02[9:0], c01[9:0], c00[9:0] };

  // Input validation (mirrors the Rust limits; nearest forces fu=fv=0 on capture).
  wire [10:0] psum = {1'b0, p0} + {1'b0, p1};
  wire bad_input =
       (p0 > 10'd511) || (p1 > 10'd511) || (psum != 11'd511)
    || (last_fine != (p1 == 10'd0))
    || (nearest && !(p0 == 10'd511 && p1 == 10'd0))
    || c00[10] || c01[10] || c02[10] || c03[10]
    || c10[10] || c11[10] || c12[10] || c13[10]
    || (lv0 > 4'd10) || (lv1 > 4'd10)
    || (slot_in > 5'd15) || (key_in > 7'd63);

  // Numeric bank and product sites.
  reg [314:0] bits;
  reg [16:0]  prod0 [0:2];
  reg [16:0]  prod1 [0:2];
  reg [16:0]  prod2 [0:2];
  // Period-8 phase counter (one-hot phase & 0x55 != 0 is `ph` even).
  reg [2:0]   ph;
  reg [10:0]  valid;
  // Six metadata owners with mod-6 read/write pointers.
  reg [98:0]  meta [0:5];
  reg [2:0]   crow, cwr, cohorts;
  // Two ready rows and their pointers/count.
  reg [71:0]  rw [0:1];
  reg [98:0]  rm [0:1];
  reg         rrd, rwr;
  reg [1:0]   queued;

  // Exact LAYOUT slices, indexed by the cohort iteration 0..3.
  wire [8:0] L_Ne [0:3];
  assign L_Ne[0]=9'd314; assign L_Ne[1]=9'd314; assign L_Ne[2]=9'd314; assign L_Ne[3]=9'd314;
  wire [8:0] L_FP [0:3];
  assign L_FP[0]=9'd36; assign L_FP[1]=9'd45; assign L_FP[2]=9'd54; assign L_FP[3]=9'd63;
  wire [8:0] L_CP [0:3];
  assign L_CP[0]=9'd0; assign L_CP[1]=9'd9; assign L_CP[2]=9'd18; assign L_CP[3]=9'd27;
  wire [8:0] L_CUR [0:3];
  assign L_CUR[0]=9'd242; assign L_CUR[1]=9'd250; assign L_CUR[2]=9'd258; assign L_CUR[3]=9'd234;
  wire [8:0] L_CVR [0:3];
  assign L_CVR[0]=9'd266; assign L_CVR[1]=9'd274; assign L_CVR[2]=9'd282; assign L_CVR[3]=9'd290;
  wire [8:0] L_FU [0:3];
  assign L_FU[0]=9'd234; assign L_FU[1]=9'd242; assign L_FU[2]=9'd250; assign L_FU[3]=9'd258;
  wire [8:0] L_FV [0:3];
  assign L_FV[0]=9'd9; assign L_FV[1]=9'd18; assign L_FV[2]=9'd27; assign L_FV[3]=9'd0;
  wire [8:0] L_CU [0:3];
  assign L_CU[0]=9'd266; assign L_CU[1]=9'd274; assign L_CU[2]=9'd282; assign L_CU[3]=9'd290;
  wire [8:0] L_CV [0:3];
  assign L_CV[0]=9'd306; assign L_CV[1]=9'd306; assign L_CV[2]=9'd306; assign L_CV[3]=9'd306;
  wire [8:0] L_FB [0:3];
  assign L_FB[0]=9'd72; assign L_FB[1]=9'd81; assign L_FB[2]=9'd72; assign L_FB[3]=9'd81;
  wire [8:0] L_FT [0:3];
  assign L_FT[0]=9'd90; assign L_FT[1]=9'd99; assign L_FT[2]=9'd90; assign L_FT[3]=9'd99;
  wire [8:0] L_FTR [0:3];
  assign L_FTR[0]=9'd162; assign L_FTR[1]=9'd171; assign L_FTR[2]=9'd162; assign L_FTR[3]=9'd171;
  wire [8:0] L_FTL [0:3];
  assign L_FTL[0]=9'd54; assign L_FTL[1]=9'd63; assign L_FTL[2]=9'd36; assign L_FTL[3]=9'd45;
  wire [8:0] L_FBR [0:3];
  assign L_FBR[0]=9'd108; assign L_FBR[1]=9'd117; assign L_FBR[2]=9'd108; assign L_FBR[3]=9'd117;
  wire [8:0] L_FBL [0:3];
  assign L_FBL[0]=9'd180; assign L_FBL[1]=9'd189; assign L_FBL[2]=9'd180; assign L_FBL[3]=9'd189;
  wire [8:0] L_CB [0:3];
  assign L_CB[0]=9'd126; assign L_CB[1]=9'd135; assign L_CB[2]=9'd126; assign L_CB[3]=9'd135;
  wire [8:0] L_CT [0:3];
  assign L_CT[0]=9'd144; assign L_CT[1]=9'd153; assign L_CT[2]=9'd144; assign L_CT[3]=9'd153;
  wire [8:0] L_CTR [0:3];
  assign L_CTR[0]=9'd216; assign L_CTR[1]=9'd216; assign L_CTR[2]=9'd216; assign L_CTR[3]=9'd216;
  wire [8:0] L_CTL [0:3];
  assign L_CTL[0]=9'd18; assign L_CTL[1]=9'd27; assign L_CTL[2]=9'd0; assign L_CTL[3]=9'd9;
  wire [8:0] L_CBR [0:3];
  assign L_CBR[0]=9'd198; assign L_CBR[1]=9'd207; assign L_CBR[2]=9'd198; assign L_CBR[3]=9'd207;
  wire [8:0] L_CBL [0:3];
  assign L_CBL[0]=9'd225; assign L_CBL[1]=9'd225; assign L_CBL[2]=9'd225; assign L_CBL[3]=9'd225;

  // Per-age iteration = ((ph - age) mod 8) / 2, invariant per cohort.
  wire [2:0] d1 = ph - 3'd1;
  wire [2:0] d2 = ph - 3'd2;
  wire [2:0] d4 = ph - 3'd4;
  wire [2:0] d5 = ph - 3'd5;
  wire [2:0] d6 = ph - 3'd6;
  wire [2:0] d7 = ph - 3'd7;
  wire [2:0] d9 = ph - 3'd1;
  wire [2:0] d10 = ph - 3'd2;
  wire [1:0] it0 = ph[2:1];
  wire [1:0] it1 = d1[2:1];
  wire [1:0] it2 = d2[2:1];
  wire [1:0] it4 = d4[2:1];
  wire [1:0] it5 = d5[2:1];
  wire [1:0] it6 = d6[2:1];
  wire [1:0] it7 = d7[2:1];
  wire [1:0] it8 = ph[2:1];
  wire [1:0] it9 = d9[2:1];
  wire [1:0] it10 = d10[2:1];

  wire v1=valid[0], v2=valid[1], v3=valid[2], v4=valid[3], v5=valid[4];
  wire v6=valid[5], v7=valid[6], v8=valid[7], v9=valid[8], v10=valid[9], v11=valid[10];

  // Reads (pre-edge bits).
  wire [8:0] rd_FP1  = bits[L_FP[it1] +: 9];
  wire [7:0] rd_FV1  = bits[L_FV[it1] +: 8];
  wire [0:0] rd_Ne1  = bits[L_Ne[it1] +: 1];
  wire [7:0] rd_CUR1 = bits[L_CUR[it1] +: 8];
  wire [7:0] rd_CVR1 = bits[L_CVR[it1] +: 8];
  wire [8:0] rd_CP2  = bits[L_CP[it2] +: 9];
  wire [7:0] rd_CV2  = bits[L_CV[it2] +: 8];
  wire [8:0] rd_FP4  = bits[L_FP[it4] +: 9];
  wire [7:0] rd_FU4  = bits[L_FU[it4] +: 8];
  wire [8:0] rd_CP5  = bits[L_CP[it5] +: 9];
  wire [8:0] rd_FT5  = bits[L_FT[it5] +: 9];
  wire [7:0] rd_FU5  = bits[L_FU[it5] +: 8];
  wire [7:0] rd_CU5  = bits[L_CU[it5] +: 8];
  wire [8:0] rd_CT6  = bits[L_CT[it6] +: 9];
  wire [7:0] rd_CU6  = bits[L_CU[it6] +: 8];
  wire [8:0] rd_FB7  = bits[L_FB[it7] +: 9];
  wire [8:0] rd_FT8  = bits[L_FT[it8] +: 9];
  wire [8:0] rd_CB8  = bits[L_CB[it8] +: 9];
  wire [8:0] rd_CT9  = bits[L_CT[it9] +: 9];
  wire [8:0] rd_FTL10 = bits[L_FTL[it10] +: 9];
  wire [8:0] rd_FTR10 = bits[L_FTR[it10] +: 9];
  wire [8:0] rd_FBL10 = bits[L_FBL[it10] +: 9];
  wire [8:0] rd_FBR10 = bits[L_FBR[it10] +: 9];
  wire [8:0] rd_CTL10 = bits[L_CTL[it10] +: 9];
  wire [8:0] rd_CTR10 = bits[L_CTR[it10] +: 9];
  wire [8:0] rd_CBL10 = bits[L_CBL[it10] +: 9];
  wire [8:0] rd_CBR10 = bits[L_CBR[it10] +: 9];

  // 9x8 products, extended to 17 bits before the multiply.
  wire [16:0] pm0 = {8'b0, rd_FP1} * {9'b0, rd_FV1};
  wire [16:0] pm1 = {8'b0, rd_CP2} * {9'b0, rd_CV2};
  wire [16:0] pm2 = {8'b0, prod0[2][16:8]} * {9'b0, rd_FU4};
  wire [16:0] pm3 = {8'b0, rd_FT5} * {9'b0, rd_FU5};
  wire [16:0] pm4 = {8'b0, prod0[2][16:8]} * {9'b0, rd_CU5};
  wire [16:0] pm5 = {8'b0, rd_CT6} * {9'b0, rd_CU6};

  wire [16:0] new0 = v1 ? pm0 : (v2 ? pm1 : 17'd0);
  wire [16:0] new1 = v4 ? pm2 : (v5 ? pm3 : 17'd0);
  wire [16:0] new2 = v5 ? pm4 : (v6 ? pm5 : 17'd0);

  // Subtract destinations (nonnegative).
  wire [8:0] sub_FT  = rd_FP4 - prod0[2][16:8];
  wire [8:0] sub_CT  = rd_CP5 - prod0[2][16:8];
  wire [8:0] sub_FBL = rd_FB7 - prod1[2][16:8];
  wire [8:0] sub_FTL = rd_FT8 - prod1[2][16:8];
  wire [8:0] sub_CBL = rd_CB8 - prod2[2][16:8];
  wire [8:0] sub_CTL = rd_CT9 - prod2[2][16:8];

  // Output row at age ten.
  wire [71:0] out_w = { rd_CBR10, rd_CBL10, rd_CTR10, rd_CTL10,
                        rd_FBR10, rd_FBL10, rd_FTR10, rd_FTL10 };

  // Metadata owner of the age-ten cohort.
  function [2:0] m6;
    input [2:0] c;
    input [3:0] o;
    reg [3:0] s;
    begin
      s = {1'b0, c} + o;
      m6 = (s >= 4'd6) ? (s - 4'd6) : s[2:0];
    end
  endfunction
  wire [2:0] idx10 = m6(crow, {3'b0, valid[10]});

  // Admission uses the pre-edge work count; absent input costs nothing.
  wire [1:0] cost_in = {1'b0, (p0 != 10'd0)} + {1'b0, (p1 != 10'd0)};
  wire [1:0] cost = in_valid ? cost_in : 2'd0;
  wire advance = ce && !fault && (queued < 2'd2);
  wire phase_even = ~ph[0];
  assign in_ready = advance && (cohorts < 3'd6) && phase_even
                    && (work_available >= {6'b0, cost});
  wire accepted = in_valid && in_ready;
  wire do_consume = ce && !fault && out_ready && (queued != 2'd0);
  wire do_capture = advance && v11;
  wire [1:0] queued_next = queued + {1'b0, do_capture} - {1'b0, do_consume};
  wire [2:0] cohorts_next = cohorts + {2'b0, accepted} - {2'b0, do_capture};

  assign in_accept = accepted;
  assign work_reserved = accepted ? cost_in : 2'd0;
  assign out_valid = !fault && (queued != 2'd0);
  assign out_weights = rw[rrd];
  assign out_meta = rm[rrd];

  always @(posedge clk) begin
    if (reset) begin
      bits <= 315'd0;
      prod0[0]<=17'd0; prod0[1]<=17'd0; prod0[2]<=17'd0;
      prod1[0]<=17'd0; prod1[1]<=17'd0; prod1[2]<=17'd0;
      prod2[0]<=17'd0; prod2[1]<=17'd0; prod2[2]<=17'd0;
      ph <= 3'd0;
      valid <= 11'd0;
      crow <= 3'd0; cwr <= 3'd0; cohorts <= 3'd0;
      rrd <= 1'b0; rwr <= 1'b0; queued <= 2'd0;
      fault <= 1'b0;
    end else if (!fault) begin
      if (accepted && (bad_input || (work_available > 8'd16))) begin
        fault <= 1'b1;
      end else begin
        queued <= queued_next;
        cohorts <= cohorts_next;
        if (do_consume) rrd <= ~rrd;
        if (advance) begin
          // Product shifts happen on every enabled edge.
          prod0[0] <= new0; prod0[1] <= prod0[0]; prod0[2] <= prod0[1];
          prod1[0] <= new1; prod1[1] <= prod1[0]; prod1[2] <= prod1[1];
          prod2[0] <= new2; prod2[1] <= prod2[0]; prod2[2] <= prod2[1];

          if (accepted) begin
            bits[L_Ne[it0] +: 1] <= nearest;
            bits[L_FP[it0] +: 9] <= p0[8:0];
            bits[L_CP[it0] +: 9] <= p1[8:0];
            bits[L_CUR[it0] +: 8] <= f1u;
            bits[L_CVR[it0] +: 8] <= f1v;
            bits[L_FU[it0] +: 8] <= nearest ? 8'd0 : f0u;
            bits[L_FV[it0] +: 8] <= nearest ? 8'd0 : f0v;
            meta[cwr] <= meta_in;
            cwr <= (cwr == 3'd5) ? 3'd0 : cwr + 3'd1;
          end
          if (v1) begin
            bits[L_CU[it1] +: 8] <= rd_Ne1 ? 8'd0 : rd_CUR1;
            bits[L_CV[it1] +: 8] <= rd_Ne1 ? 8'd0 : rd_CVR1;
          end
          if (v4) begin
            bits[L_FB[it4] +: 9] <= prod0[2][16:8];
            bits[L_FT[it4] +: 9] <= sub_FT;
          end
          if (v5) begin
            bits[L_CB[it5] +: 9] <= prod0[2][16:8];
            bits[L_CT[it5] +: 9] <= sub_CT;
          end
          if (v7) begin
            bits[L_FBR[it7] +: 9] <= prod1[2][16:8];
            bits[L_FBL[it7] +: 9] <= sub_FBL;
          end
          if (v8) begin
            bits[L_FTR[it8] +: 9] <= prod1[2][16:8];
            bits[L_FTL[it8] +: 9] <= sub_FTL;
            bits[L_CBR[it8] +: 9] <= prod2[2][16:8];
            bits[L_CBL[it8] +: 9] <= sub_CBL;
          end
          if (v9) begin
            bits[L_CTR[it9] +: 9] <= prod2[2][16:8];
            bits[L_CTL[it9] +: 9] <= sub_CTL;
          end
          if (v10) begin
            rw[rwr] <= out_w;
            rm[rwr] <= meta[idx10];
          end
          if (do_capture) begin
            rwr <= ~rwr;
            crow <= (crow == 3'd5) ? 3'd0 : crow + 3'd1;
          end
          valid <= {valid[9:0], accepted};
          ph <= ph + 3'd1;
        end
      end
    end
  end
endmodule
"#
}

/// Alias for [`verilog`], matching the `rtl::source()` convention of the other
/// hand-written leaves.
pub fn source() -> &'static str {
    verilog()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_declares_the_fixed_calendar_and_ports() {
        let v = verilog();
        assert!(v.contains(&format!("module {MODULE}(")));
        assert!(v.contains("input  wire [161:0] in_payload,"));
        assert!(v.contains("output wire [71:0]  out_weights,"));
        assert!(v.contains("output wire [98:0]  out_meta,"));
        assert!(v.contains("wire advance = ce && !fault && (queued < 2'd2);"));
        assert!(v.contains("assign in_ready = advance && (cohorts < 3'd6) && phase_even"));
        assert_eq!(latency(), 12);
        assert_eq!(initiation_interval(), 2);
        let a = Allocation::default();
        assert_eq!(a.multiply_sites, 3);
        assert_eq!(a.products_per_site, 3);
        assert_eq!(a.multiply_latency, 3);
        assert_eq!(a.initiation_interval, 2);
        assert_eq!(a.numeric_ff_bits, 315);
        assert_eq!(a.metadata_ff_bits, 6 * 99);
        assert_eq!(a.ready_ff_bits, 2 * 171);
        assert_eq!(a.total_state_bits(), 315 + 153 + 594 + 342 + 14 + 14);
    }
}
