//! Hand-written synthesizable RTL for the captured-texel color leaf.
//!
//! This module mirrors [`crate::texture::emu::color::ColorEmu`] edge for edge.
//! The stream is one canonical 72-bit Group4 word per cycle: four already-tap-
//! ordered UNORM9 coefficients and four RAW565 texels. The fixed calendar is
//! decode (age 0), three registered product ages (ages 1..3), pairwise then full
//! four-tap tree (ages 4..5), one registered feedback add per group (age 6) and
//! per-channel high-byte + carry normalization (ages 7..8). Outputs are held in a
//! sixteen-entry FIFO; a `last` group reserves one result credit from the
//! pre-edge state and a credit returns only when a queued result commits, so no
//! same-edge returned credit can admit a blocked last group.
//!
//! The RTL contains ordinary logic and registers only; it does not embed a
//! golden trace. Integration tests drive it per edge against the live emulator.

use crate::texture::emu::color::{LATENCY as EMU_LATENCY, RESULT_CAPACITY as EMU_CAPACITY};

/// Verilog module name emitted by [`verilog`].
pub const MODULE: &str = "gpu_v2_texture_color";

/// Enabled-edge latency from acceptance to a queued result.
pub const LATENCY: usize = EMU_LATENCY as usize;
/// New result every enabled edge once the pipeline is full.
pub const INITIATION_INTERVAL: usize = 1;

/// Logical declaration inventory for this leaf. These are behavioral register
/// and multiplier counts, not fitted Gowin cells. Product registers are the
/// generic twelve `9x8` products repeated over three product ages; the physical
/// `MULT9X9` mapping (four half-slots per macro, `ceil(12/4)=3` macros) is a
/// separate placement claim and is not asserted here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    /// Logical 9x8 multiplier lanes per group.
    pub multiplier_lanes: usize,
    /// Registered product bits: twelve 17-bit results over three product ages.
    pub registered_product_bits: usize,
    /// Tree/feedback adder lanes (pair, partial, feedback).
    pub adder_lanes: usize,
    /// Pipeline register bits for decode, products, tree, feedback, normalize
    /// and the stream owner.
    pub pipeline_bits: usize,
    /// Held result FIFO payload bits, sixteen entries of key plus RGB8.
    pub fifo_payload_bits: usize,
    /// FIFO pointers, count, credits and the latched fault.
    pub control_bits: usize,
}
impl Default for Allocation {
    fn default() -> Self {
        Self {
            multiplier_lanes: 12,
            registered_product_bits: 3 * 12 * 17,
            adder_lanes: 6 + 3 + 3,
            pipeline_bits: 141 + 3 * 213 + 117 + 66 + 61 + 34 + 61 + 7,
            fifo_payload_bits: EMU_CAPACITY * (6 + 24),
            control_bits: 4 + 4 + 5 + 5 + 1,
        }
    }
}
impl Allocation {
    pub fn total_state_bits(self) -> usize {
        self.pipeline_bits + self.fifo_payload_bits + self.control_bits
    }
    /// Storage with no RAM port: the FIFO payload plus its pointers/count.
    pub fn portless_storage_bits(self) -> usize {
        self.fifo_payload_bits + 4 + 4 + 5
    }
}

/// Enabled-edge latency.
pub fn latency() -> usize {
    LATENCY
}

/// Initiation interval: one accepted group per enabled edge.
pub fn initiation_interval() -> usize {
    INITIATION_INTERVAL
}

/// The complete synthesizable module. Fixed widths; no parameters.
pub fn verilog() -> &'static str {
    r#"// gpu_v2_texture_color: captured-texel color leaf, eight enabled-edge latency.
//
// Decode a canonical Group4 word into four UNORM9 coefficients and four RAW565
// texels, expand each texel by bit replication, form twelve 9x8 products, shift
// them through three registered product ages, combine with a pairwise then full
// four-tap tree, add one registered feedback accumulate per group, and normalize
// each channel as exact nearest /511 over the conserved bound 255*511=130305:
// h=N>>9, low=N&255, increment=N[8] | carry8(h+low). A `last` group reserves one
// of sixteen result credits from the pre-edge state; the held result FIFO
// returns a credit only on later edges (no same-edge returned credit).
// Owner/order, sum-domain and result-credit faults latch `fault` and freeze.
module gpu_v2_texture_color(
  input  wire        clk,
  input  wire        reset,
  input  wire        ce,
  input  wire        in_valid,
  input  wire [71:0] in_payload,
  input  wire [15:0] in_tex0,
  input  wire [15:0] in_tex1,
  input  wire [15:0] in_tex2,
  input  wire [15:0] in_tex3,
  output wire        in_ready,
  input  wire        out_ready,
  output wire        out_valid,
  output wire [5:0]  out_key,
  output wire [7:0]  out_r,
  output wire [7:0]  out_g,
  output wire [7:0]  out_b,
  output reg         fault
);
  localparam [17:0] MAX_SUM = 18'd130305;

  wire [5:0]  key_in     = {in_payload[69:66], in_payload[71:70]};
  wire        first_in   = in_payload[64];
  wire        last_in    = in_payload[65];
  wire        last_offer = in_valid ? last_in : 1'b1;

  reg  [4:0] credits;
  assign in_ready = ce && !fault && (!last_offer || (credits < 5'd16));
  wire accepted = in_valid && in_ready;

  wire [15:0] tex [0:3];
  assign tex[0]=in_tex0; assign tex[1]=in_tex1;
  assign tex[2]=in_tex2; assign tex[3]=in_tex3;

  // RAW565 bit-replication expansion to RGB8.
  wire [8:0] w_in   [0:3];
  wire [7:0] col_in [0:3][0:2];
  genvar t, c, i;
  generate
    for (t=0;t<4;t=t+1) begin : dec_w
      assign w_in[t] = in_payload[28+9*t +: 9];
    end
    for (t=0;t<4;t=t+1) begin : dec_col
      assign col_in[t][0] = {tex[t][15:11], tex[t][15:13]};
      assign col_in[t][1] = {tex[t][10:5],  tex[t][10:9]};
      assign col_in[t][2] = {tex[t][4:0],   tex[t][4:2]};
    end
  endgenerate

  // Decode age (0).
  reg        d_valid, d_first, d_last;
  reg [5:0]  d_key;
  reg [8:0]  d_w   [0:3];
  reg [7:0]  d_col [0:3][0:2];

  // Three registered product ages (1..3).
  reg        p0_valid, p1_valid, p2_valid;
  reg [5:0]  p0_key, p1_key, p2_key;
  reg        p0_first, p1_first, p2_first;
  reg        p0_last, p1_last, p2_last;
  reg [16:0] p0_val [0:3][0:2];
  reg [16:0] p1_val [0:3][0:2];
  reg [16:0] p2_val [0:3][0:2];

  // Pair (4), partial (5), feedback sum (6), normalize (7).
  reg        q_valid, q_first, q_last;
  reg [5:0]  q_key;
  reg [17:0] q_val [0:1][0:2];
  reg        r_valid, r_first, r_last;
  reg [5:0]  r_key;
  reg [18:0] r_val [0:2];
  reg        s_valid;
  reg [5:0]  s_key;
  reg [17:0] s_val [0:2];
  reg        n_valid;
  reg [5:0]  n_key;
  reg [7:0]  n_h   [0:2];
  reg        n_inc [0:2];

  // Feedback accumulator and stream owner.
  reg        a_valid;
  reg [5:0]  a_key;
  reg [17:0] a_val [0:2];
  reg        own_valid;
  reg [5:0]  own_key;

  // Held result FIFO.
  reg [5:0]  f_key [0:15];
  reg [7:0]  f_rgb [0:15][0:2];
  reg [3:0]  f_head, f_tail;
  reg [4:0]  f_count;

  // Products, pairs, partials and feedback sum.
  wire [16:0] prod [0:3][0:2];
  wire [17:0] pair [0:1][0:2];
  wire [18:0] part [0:2];
  wire [17:0] prev [0:2];
  wire [19:0] sumv [0:2];
  generate
    for (t=0;t<4;t=t+1) for (c=0;c<3;c=c+1) begin : mult
      assign prod[t][c] = d_w[t] * d_col[t][c];
    end
    for (i=0;i<2;i=i+1) for (c=0;c<3;c=c+1) begin : tree
      assign pair[i][c] = {1'b0, p2_val[2*i][c]} + {1'b0, p2_val[2*i+1][c]};
    end
    for (c=0;c<3;c=c+1) begin : part_c
      assign part[c] = {1'b0, q_val[0][c]} + {1'b0, q_val[1][c]};
    end
    for (c=0;c<3;c=c+1) begin : fb
      assign prev[c] = r_first ? 18'd0 : a_val[c];
      assign sumv[c] = {1'b0, r_val[c]} + {2'b0, prev[c]};
    end
  endgenerate

  // Exact nearest /511 over N<=130305: h=N>>9, low=N&255,
  // increment=N[8] | carry8(h+low).
  wire [7:0] nh   [0:2];
  wire       ninc [0:2];
  generate
    for (c=0;c<3;c=c+1) begin : norm_c
      wire [8:0] hlow = {1'b0, s_val[c][17:9]} + {1'b0, s_val[c][7:0]};
      assign nh[c]   = s_val[c][17:9];
      assign ninc[c] = s_val[c][8] | hlow[8];
    end
  endgenerate

  // Fault detection. Sum-domain checks happen at the same stage boundaries as
  // the emulator; all latch `fault` and freeze the datapath.
  wire pair_bad = p2_valid && (
        (pair[0][0] > MAX_SUM) || (pair[0][1] > MAX_SUM) || (pair[0][2] > MAX_SUM) ||
        (pair[1][0] > MAX_SUM) || (pair[1][1] > MAX_SUM) || (pair[1][2] > MAX_SUM));
  wire part_bad = q_valid && (
        (part[0] > MAX_SUM) || (part[1] > MAX_SUM) || (part[2] > MAX_SUM));
  wire sum_bad = r_valid && (
        (sumv[0] > MAX_SUM) || (sumv[1] > MAX_SUM) || (sumv[2] > MAX_SUM));
  wire owner_bad =
      accepted && (first_in ? own_valid : (!own_valid || (own_key != key_in)));
  wire packet_bad = accepted && (in_payload[7:4] > 4'd10);
  wire feedback_bad =
      r_valid && (r_first ? a_valid : (!a_valid || (a_key != r_key)));

  // Result FIFO traffic and credit accounting (pop before push, as the emu).
  wire do_pop = out_ready && (f_count != 5'd0);
  wire do_push = n_valid;
  wire [4:0] count_pre_push = f_count - (do_pop ? 5'd1 : 5'd0);
  wire overflow_bad = do_push && (count_pre_push == 5'd16);
  wire [4:0] count_next = f_count + (do_push ? 5'd1 : 5'd0) - (do_pop ? 5'd1 : 5'd0);
  wire [4:0] credits_next =
      credits + ((accepted && last_in) ? 5'd1 : 5'd0) - (do_pop ? 5'd1 : 5'd0);

  // Post-edge credit ownership: one credit per in-flight `last` plus queued
  // results must equal `credits`.
  wire [4:0] inflight_next =
        ((accepted && last_in) ? 5'd1 : 5'd0)
      + ((d_valid && d_last)   ? 5'd1 : 5'd0)
      + ((p0_valid && p0_last) ? 5'd1 : 5'd0)
      + ((p1_valid && p1_last) ? 5'd1 : 5'd0)
      + ((p2_valid && p2_last) ? 5'd1 : 5'd0)
      + ((q_valid && q_last)   ? 5'd1 : 5'd0)
      + ((r_valid && r_last)   ? 5'd1 : 5'd0)
      + ((s_valid)             ? 5'd1 : 5'd0);
  wire [5:0] credit_total = {1'b0, count_next} + {1'b0, inflight_next};
  wire credit_bad = ({1'b0, credits_next} != credit_total);

  integer a, b, j, k;
  always @(posedge clk) begin
    if (reset) begin
      d_valid <= 1'b0;
      p0_valid <= 1'b0; p1_valid <= 1'b0; p2_valid <= 1'b0;
      q_valid <= 1'b0; r_valid <= 1'b0; s_valid <= 1'b0; n_valid <= 1'b0;
      a_valid <= 1'b0; own_valid <= 1'b0;
      f_head <= 4'd0; f_tail <= 4'd0; f_count <= 5'd0;
      credits <= 5'd0; fault <= 1'b0;
    end else if (ce && !fault) begin
      if (do_pop) f_head <= f_head + 4'd1;
      if (do_push) begin
        f_key[f_tail] <= n_key;
        for (j=0;j<3;j=j+1) f_rgb[f_tail][j] <= n_h[j] + n_inc[j];
        f_tail <= f_tail + 4'd1;
      end
      f_count <= count_next;
      credits <= credits_next;

      n_valid <= s_valid; n_key <= s_key;
      for (j=0;j<3;j=j+1) begin n_h[j] <= nh[j]; n_inc[j] <= ninc[j]; end

      s_valid <= r_valid && r_last; s_key <= r_key;
      for (j=0;j<3;j=j+1) s_val[j] <= sumv[j][17:0];

      if (r_valid) begin
        a_valid <= !r_last; a_key <= r_key;
        for (j=0;j<3;j=j+1) a_val[j] <= sumv[j][17:0];
      end

      r_valid <= q_valid; r_key <= q_key; r_first <= q_first; r_last <= q_last;
      for (j=0;j<3;j=j+1) r_val[j] <= part[j];

      q_valid <= p2_valid; q_key <= p2_key; q_first <= p2_first; q_last <= p2_last;
      for (a=0;a<2;a=a+1) for (b=0;b<3;b=b+1) q_val[a][b] <= pair[a][b];

      p2_valid <= p1_valid; p2_key <= p1_key; p2_first <= p1_first; p2_last <= p1_last;
      for (k=0;k<4;k=k+1) for (b=0;b<3;b=b+1) p2_val[k][b] <= p1_val[k][b];

      p1_valid <= p0_valid; p1_key <= p0_key; p1_first <= p0_first; p1_last <= p0_last;
      for (k=0;k<4;k=k+1) for (b=0;b<3;b=b+1) p1_val[k][b] <= p0_val[k][b];

      p0_valid <= d_valid; p0_key <= d_key; p0_first <= d_first; p0_last <= d_last;
      for (k=0;k<4;k=k+1) for (b=0;b<3;b=b+1) p0_val[k][b] <= prod[k][b];

      d_valid <= accepted; d_key <= key_in; d_first <= first_in; d_last <= last_in;
      for (k=0;k<4;k=k+1) d_w[k] <= w_in[k];
      for (k=0;k<4;k=k+1) for (b=0;b<3;b=b+1) d_col[k][b] <= col_in[k][b];

      if (accepted) begin
        own_valid <= !last_in;
        own_key <= key_in;
      end

      if (packet_bad || owner_bad || pair_bad || part_bad || sum_bad || feedback_bad ||
          overflow_bad || credit_bad)
        fault <= 1'b1;
    end
  end

  assign out_valid = !fault && (f_count != 5'd0);
  assign out_key   = f_key[f_head];
  assign out_r     = f_rgb[f_head][0];
  assign out_g     = f_rgb[f_head][1];
  assign out_b     = f_rgb[f_head][2];
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
    fn module_has_fixed_ports_and_calendar() {
        let v = verilog();
        assert!(v.contains(&format!("module {MODULE}(")));
        assert!(v.contains("assign in_ready = ce && !fault && (!last_offer || (credits < 5'd16));"));
        assert_eq!(latency(), 8);
        assert_eq!(initiation_interval(), 1);
        assert_eq!(Allocation::default().total_state_bits(), 1625);
    }
}
