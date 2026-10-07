//! Hand-written synthesizable RTL for the final-color leaf.
//!
//! One Verilog module mirrors the emulator exactly and implements the fixed
//! [`super::sim::timed::StageCalendar`]: stage 1 forms `tint*texture` and
//! `specular*h`, stage 2 `+128`, stage 3 `+(t >> 8)`, stage 4 `base*g`,
//! stage 5 `sum`, stage 6 `+127`, stage 7 `+bit`, stage 8 the compare, stage 9
//! the saturating select. Nine one-operation stages advance on `ce`, the ninth
//! pushes a `{key,rgb}` word into an eight-deep FIFO, and `in_ready`/`out_valid`
//! are the pre-edge handshake. Result credits are reserved at acceptance from
//! the old state (no same-edge reuse). The ignored Icarus test drives this
//! module every edge and compares it with `emu::FinalEmu`; no expected RTL
//! output is precomputed.
//!
//! The `g/h` capture, `specular` and `tint` are the leaf's only inputs. There
//! are no memory ports, no context state and no dispatcher/ROP logic here.

use super::{PIPELINE_LATENCY, RESULT_CAPACITY};

pub const MODULE: &str = "gpu_v2_final_stage";

/// Behavioral declaration bits for this leaf. These are not fitted Gowin FF,
/// LUT or DSP numbers. `mul18_lanes` is the logical Dsp18 class: the module
/// instantiates combinational 8x8 / 8x9 products followed by output registers
/// (`s1_m1`, `s1_m2`, `s4_bg`), not placed or timing-proven Gowin DSP macros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub mul18_lanes: usize,
    pub add16_lanes: usize,
    pub add18_lanes: usize,
    pub compare_lanes: usize,
    pub select_lanes: usize,
    pub pipeline_bits: usize,
    pub fifo_payload_bits: usize,
    pub credit_bits: usize,
    pub control_bits: usize,
}
impl Allocation {
    pub fn total_state_bits(self) -> usize {
        self.pipeline_bits + self.fifo_payload_bits + self.credit_bits + self.control_bits
    }
    /// Storage with no RAM port: the FIFO payload plus its read/write pointers.
    pub fn portless_storage_bits(self) -> usize {
        self.fifo_payload_bits + self.control_bits
    }
}
impl Default for Allocation {
    fn default() -> Self {
        let per_stage = |bits: usize| 3 * bits;
        let pipeline_bits = per_stage(17 + 17 + 9)   // s1: m1, m2, g
            + per_stage(17 + 17 + 9)                 // s2: t, m2, g
            + per_stage(17 + 17 + 9)                 // s3: b, m2, g
            + per_stage(17 + 17)                     // s4: bg, m2
            + per_stage(18 + 1)                      // s5: sum, bit
            + per_stage(18 + 1)                      // s6: r1, bit
            + per_stage(18)                          // s7: r2
            + per_stage(10 + 1)                      // s8: rounded, over
            + per_stage(8)                           // s9: color
            + 9 * 6                                  // key pipe
            + 9; // valid pipe
        let fifo_payload_bits = RESULT_CAPACITY * (6 + 24);
        let credit_bits = 4;
        let control_bits = 3 + 3 + 4; // head, tail, count
        Self {
            mul18_lanes: 9,
            add16_lanes: 6,
            add18_lanes: 9,
            compare_lanes: 3,
            select_lanes: 3,
            pipeline_bits,
            fifo_payload_bits,
            credit_bits,
            control_bits,
        }
    }
}

pub fn latency() -> usize {
    PIPELINE_LATENCY
}

pub fn initiation_interval() -> usize {
    1
}

/// The complete synthesizable module. Fixed widths; no parameters.
pub fn source() -> &'static str {
    r#"// gpu_v2_final_stage: one-op-per-stage final color, nine stages, eight credits.
// This file is emitted from the leaf's rtl module; edits belong there.
module gpu_v2_final_stage(
  input  wire        clk,
  input  wire        reset,
  input  wire        ce,
  input  wire        in_valid,
  input  wire [5:0]  in_key,
  input  wire [7:0]  in_tint_r,
  input  wire [7:0]  in_tint_g,
  input  wire [7:0]  in_tint_b,
  input  wire [7:0]  in_tex_r,
  input  wire [7:0]  in_tex_g,
  input  wire [7:0]  in_tex_b,
  input  wire [8:0]  in_g,
  input  wire [8:0]  in_h,
  input  wire [7:0]  in_spec_r,
  input  wire [7:0]  in_spec_g,
  input  wire [7:0]  in_spec_b,
  output wire        in_ready,
  input  wire        out_ready,
  output wire        out_valid,
  output wire [5:0]  out_key,
  output wire [7:0]  out_r,
  output wire [7:0]  out_g,
  output wire [7:0]  out_b
);
  reg [3:0] credits;
  reg [8:0] vpipe;
  reg [5:0] kpipe [0:8];

  reg [16:0] s1_m1 [0:2];
  reg [16:0] s1_m2 [0:2];
  reg [8:0]  s1_g  [0:2];
  reg [16:0] s2_t  [0:2];
  reg [16:0] s2_m2 [0:2];
  reg [8:0]  s2_g  [0:2];
  reg [16:0] s3_b  [0:2];
  reg [16:0] s3_m2 [0:2];
  reg [8:0]  s3_g  [0:2];
  reg [16:0] s4_bg [0:2];
  reg [16:0] s4_m2 [0:2];
  reg [17:0] s5_sum[0:2];
  reg        s5_bit[0:2];
  reg [17:0] s6_r1 [0:2];
  reg        s6_bit[0:2];
  reg [17:0] s7_r2 [0:2];
  reg [9:0]  s8_rd [0:2];
  reg        s8_ov [0:2];
  reg [7:0]  s9_col[0:2];

  reg [5:0]  f_key [0:7];
  reg [7:0]  f_col [0:7][0:2];
  reg [2:0]  f_head;
  reg [2:0]  f_tail;
  reg [3:0]  f_count;

  wire [7:0] tin [0:2];
  wire [7:0] tex [0:2];
  wire [7:0] spe [0:2];
  assign tin[0]=in_tint_r; assign tin[1]=in_tint_g; assign tin[2]=in_tint_b;
  assign tex[0]=in_tex_r;  assign tex[1]=in_tex_g;  assign tex[2]=in_tex_b;
  assign spe[0]=in_spec_r; assign spe[1]=in_spec_g; assign spe[2]=in_spec_b;

  wire [16:0] n_t [0:2];
  wire [16:0] n_b [0:2];
  wire [7:0]  n_base [0:2];
  wire [17:0] n_sum [0:2];
  wire [17:0] n_r1 [0:2];
  wire [17:0] n_r2 [0:2];
  wire [9:0]  n_rd [0:2];
  wire        n_ov [0:2];
  wire [7:0]  n_col [0:2];

  genvar c;
  generate
    for (c=0;c<3;c=c+1) begin : lanes
      assign n_t[c]    = s1_m1[c] + 17'd128;
      assign n_b[c]    = s2_t[c] + (s2_t[c] >> 8);
      assign n_base[c] = s3_b[c][15:8];
      assign n_sum[c]  = s4_bg[c] + s4_m2[c];
      assign n_r1[c]   = s5_sum[c] + 18'd127;
      assign n_r2[c]   = s6_r1[c] + {17'd0, s6_bit[c]};
      assign n_rd[c]   = s7_r2[c][17:8];
      assign n_ov[c]   = (n_rd[c] > 10'd255);
      assign n_col[c]  = s8_ov[c] ? 8'd255 : s8_rd[c][7:0];
    end
  endgenerate

  assign in_ready  = !reset && ce && (credits < 4'd8);
  assign out_valid = !reset && (f_count != 4'd0);
  assign out_key   = f_key[f_head];
  assign out_r     = f_col[f_head][0];
  assign out_g     = f_col[f_head][1];
  assign out_b     = f_col[f_head][2];

  integer i;
  always @(posedge clk) begin
    if (reset) begin
      vpipe   <= 9'd0;
      credits <= 4'd0;
      f_head  <= 3'd0;
      f_tail  <= 3'd0;
      f_count <= 4'd0;
    end else if (ce) begin
      if (out_ready && f_count != 4'd0) f_head <= f_head + 3'd1;
      if (vpipe[8]) begin
        f_key[f_tail]     <= kpipe[8];
        f_col[f_tail][0]  <= s9_col[0];
        f_col[f_tail][1]  <= s9_col[1];
        f_col[f_tail][2]  <= s9_col[2];
        f_tail            <= f_tail + 3'd1;
      end
      f_count <= f_count
                 + (vpipe[8] ? 4'd1 : 4'd0)
                 - ((out_ready && f_count != 4'd0) ? 4'd1 : 4'd0);
      credits <= credits
                 + ((in_valid && in_ready) ? 4'd1 : 4'd0)
                 - ((out_ready && f_count != 4'd0) ? 4'd1 : 4'd0);
      for (i=0;i<3;i=i+1) begin
        s9_col[i] <= n_col[i];
        s8_rd[i]  <= n_rd[i];
        s8_ov[i]  <= n_ov[i];
        s7_r2[i]  <= n_r2[i];
        s6_r1[i]  <= n_r1[i];
        s6_bit[i] <= s5_bit[i];
        s5_sum[i] <= n_sum[i];
        s5_bit[i] <= n_sum[i][8];
        s4_bg[i]  <= n_base[i] * s3_g[i];
        s4_m2[i]  <= s3_m2[i];
        s3_b[i]   <= n_b[i];
        s3_m2[i]  <= s2_m2[i];
        s3_g[i]   <= s2_g[i];
        s2_t[i]   <= n_t[i];
        s2_m2[i]  <= s1_m2[i];
        s2_g[i]   <= s1_g[i];
        if (in_valid && in_ready) begin
          s1_m1[i] <= tin[i] * tex[i];
          s1_m2[i] <= spe[i] * in_h;
          s1_g[i]  <= in_g;
        end
      end
      vpipe[8] <= vpipe[7]; kpipe[8] <= kpipe[7];
      vpipe[7] <= vpipe[6]; kpipe[7] <= kpipe[6];
      vpipe[6] <= vpipe[5]; kpipe[6] <= kpipe[5];
      vpipe[5] <= vpipe[4]; kpipe[5] <= kpipe[4];
      vpipe[4] <= vpipe[3]; kpipe[4] <= kpipe[3];
      vpipe[3] <= vpipe[2]; kpipe[3] <= kpipe[2];
      vpipe[2] <= vpipe[1]; kpipe[2] <= kpipe[1];
      vpipe[1] <= vpipe[0]; kpipe[1] <= kpipe[0];
      vpipe[0] <= (in_valid && in_ready) ? 1'b1 : 1'b0;
      if (in_valid && in_ready) kpipe[0] <= in_key;
    end
  end
endmodule
"#
}
