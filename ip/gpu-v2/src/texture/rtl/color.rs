//! Captured-texel Color RTL, matching the live two-edge emulator.
//! Twelve independent 9x9 multipliers feed balanced fabric four-tap sums.
//! Feedback, exact nearest /511 and result write retain the following edge.
//! GPU_V2_GOWIN_DSP binds actual MULT9X9 cells; the portable
//! implementation and independent product-based emulator have the same public
//! CE, fault boundary and latency.
//! Standalone fit evidence does not qualify a complete sampler/controller.
use crate::texture::emu::color::LATENCY as EMU_LATENCY;

/// Verilog module name emitted by [`verilog`].
pub const MODULE: &str = "gpu_v2_texture_color";

/// Enabled-edge latency from acceptance to a queued result.
pub const LATENCY: usize = EMU_LATENCY as usize;
/// New result every enabled edge once the pipeline is full.
pub const INITIATION_INTERVAL: usize = 1;

/// Shared logical weight decode in the portable/product-based reference.
pub const DECODE_WEIGHT_BITS: usize = 4 * 9;
/// The private native Serial profile has no soft weight decode bank.
pub const SERIAL_NATIVE_SOFT_WEIGHT_BITS: usize = 0;
/// Live weight bits in the twelve existing MULT9X9 A input registers. Each of
/// four weights is repeated across three channels; tied-zero upper bits are
/// excluded. This is hard input state, not additional DSP macro allocation.
pub const SERIAL_NATIVE_HARD_WEIGHT_BITS: usize = 3 * 4 * 9;
/// E1 products in the twelve existing MULT9X9 output registers. The native
/// Serial profile sums them combinationally at E2 and retains no soft partial.
pub const SERIAL_NATIVE_HARD_PRODUCT_BITS: usize = 3 * 4 * 17;
pub const SERIAL_NATIVE_SOFT_PARTIAL_BITS: usize = 0;

/// Logical declaration inventory for this leaf. These are behavioral register
/// and arithmetic counts, not fitted Gowin cells. Twelve logical `9x8`
/// products feed three balanced fabric trees; only the complete unsigned sums
/// are retained. This physical binding is separate from the product-based
/// reference emulator's inventory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    /// Logical 9x8 multiplier lanes per group.
    pub multiplier_lanes: usize,
    /// Individual products are combinational in the selected RTL.
    pub registered_product_bits: usize,
    /// Three 19-bit full tap sums captured at age1.
    pub registered_partial_bits: usize,
    /// Native fused multiply-add macros (none in the selected binding).
    pub fused_macros: usize,
    /// Fabric pair, full-sum and feedback adder lanes.
    pub adder_lanes: usize,
    /// Pipeline register bits for decode, products, feedback and stream owner.
    pub pipeline_bits: usize,
    /// Soft result payload bits (asynchronous SRAM directly supplies the head).
    pub fifo_payload_bits: usize,
    /// Result storage pointers/occupancy, excluding arithmetic-stage credits.
    pub fifo_control_bits: usize,
    pub bsram: usize,
    pub fifo_output_hard_bits: usize,
    pub sdp4_cells: usize,
    /// FIFO pointers, count, credits and the latched fault.
    pub control_bits: usize,
}
impl Default for Allocation {
    fn default() -> Self {
        Self {
            multiplier_lanes: 12,
            registered_product_bits: 0,
            registered_partial_bits: 3 * 19,
            fused_macros: 0,
            adder_lanes: 6 + 3 + 3,
            pipeline_bits: 141 + 66 + 58 + 7,
            fifo_payload_bits: 0,
            fifo_control_bits: 4 + 4 + 5,
            bsram: 0,
            fifo_output_hard_bits: 0,
            sdp4_cells: 8,
            control_bits: 4 + 4 + 5 + 5 + 1,
        }
    }
}
impl Allocation {
    /// One registered result row for the Serial wrapper's always-ready sink.
    pub fn serial_closed() -> Self {
        Self {
            fifo_payload_bits: 32,
            fifo_control_bits: 1,
            sdp4_cells: 0,
            control_bits: 2 + 1 + 1,
            ..Self::default()
        }
    }
    /// Logical portable/product-reference inventory with seventeen-bit sums.
    /// Native Serial removes the shared36-bit soft weight decode and51-bit
    /// soft partial bank, using existing hard DSP input/output registers.
    /// These logical pipeline totals describe the portable reference and do
    /// not represent the native mapped soft FF count.
    pub fn serial_trusted() -> Self {
        Self {
            registered_partial_bits: 3 * 17,
            pipeline_bits: 141 + 60 + 58 + 7,
            ..Self::serial_closed()
        }
    }
    pub fn total_state_bits(self) -> usize {
        self.pipeline_bits + self.fifo_payload_bits + self.fifo_output_hard_bits + self.control_bits
    }
    /// Soft FIFO pointers/count; SRAM payload uses real ports.
    pub fn portless_storage_bits(self) -> usize {
        self.fifo_payload_bits + self.fifo_control_bits
    }
}

/// Dedicated Serial composition: the consumer is structurally tied ready.
/// A result written on this edge is consumed on the following enabled edge;
/// simultaneous old-row pop and new-row write use a register, not a RAM port.
/// Standalone and overlapping consumers continue to use [`verilog`].
pub fn verilog_serial_closed() -> String {
    let mut source = verilog().to_owned();
    for (old, new) in [
        ("reg  [4:0] credits;", "reg [1:0] credits;"),
        (
            "wire [31:0] result_do;",
            "wire [31:0] result_do;\n  reg [31:0] closed_result;\n  assign result_do=closed_result;",
        ),
        (
            "reg [3:0]  f_head, f_tail;",
            "// Closed Serial consumes a result on its first offered edge.",
        ),
        ("reg [4:0]  f_count;", "reg f_count;"),
        ("wire [3:0] result_read_address=f_head;", ""),
        (
            "f_head <= 4'd0; f_tail <= 4'd0; f_count <= 5'd0;",
            "f_count <= 1'b0;",
        ),
        ("if (do_pop) f_head <= f_head + 4'd1;", ""),
        (
            "if (do_push) begin\n        f_tail <= f_tail + 4'd1;\n      end",
            "",
        ),
    ] {
        assert_eq!(source.matches(old).count(), 1, "closed Color anchor {old}");
        source = source.replace(old, new);
    }
    let data = source.find("wire [31:0] result_write_data").unwrap();
    let start = data + source[data..].find("`ifdef GPU_V2_GOWIN_DSP").unwrap();
    let end = start + source[start..].find("`endif").unwrap() + "`endif".len();
    source.replace_range(
        start..end,
        "always @(posedge clk) if(result_write) closed_result<=result_write_data;",
    );
    source
}

/// Private production profile selected only by the closed Serial builder.
/// Coefficient conserves511; Membership/Packet partition nonzero taps exactly
/// once and preserve first/last. The private cache reorders weights with texels
/// by the same bijection, preserving every weighted contribution.
/// Every prefix therefore has nonnegative weight<=511, so each channel partial
/// and accumulated sum is <=255*511<2^17. Generic/Runtime retain full detection.
/// Native E0 captures weights in DSP A registers, while RGB and all metadata
/// retain their soft decode. E1 captures products in existing DSP output
/// registers; the fabric sum feeds E2 feedback/normalization directly. Result
/// age and global CE remain unchanged. Portable retains its independent soft
/// weight decode and one-edge registered sum.
pub(crate) fn verilog_serial_trusted() -> String {
    let mut source = verilog_serial_closed();
    for (old, new) in [
        ("wire [18:0] part [0:2];", "wire [16:0] part [0:2];"),
        ("wire [19:0] sumv [0:2];", "wire [16:0] sumv [0:2];"),
        ("assign sumv[c] = {1'b0,part[c]} + {3'b0,prev[c]};", "assign sumv[c] = part[c] + prev[c];"),
        ("wire sum_bad = p_valid && (\n    sumv[0]>MAX_SUM || sumv[1]>MAX_SUM || sumv[2]>MAX_SUM);", "wire sum_bad = 1'b0;"),
        ("output wire [18:0] sum);", "output wire [16:0] sum);"),
        ("reg [18:0] q;", "reg [16:0] q;"),
    ] {
        assert_eq!(source.matches(old).count(), 1, "trusted Color anchor {old}");
        source = source.replace(old, new);
    }
    serial_native_product_outputs(serial_native_weight_inputs(source))
}

fn serial_native_product_outputs(mut source: String) -> String {
    for (old, new) in [
        (
            ".OUT_REG(1'b0)",
            ".OUT_REG(1'b1)",
        ),
        (
            " reg [16:0] q;\n always @(posedge clk) if(ce) q<=full_sum;\n assign sum=q;",
            "`ifdef GPU_V2_GOWIN_DSP\n assign sum=full_sum[16:0];\n`else\n reg [16:0] q;\n always @(posedge clk) if(ce) q<=full_sum;\n assign sum=q;\n`endif",
        ),
        (
            "// Native E0 captures DSP weights and soft RGB; E1 captures the full sum. Portable retains soft E0 decode.",
            "// Native E0 captures DSP weights and soft RGB; E1 captures hard products. Portable retains its registered full sum.",
        ),
    ] {
        assert_eq!(source.matches(old).count(), 1, "native Serial product anchor {old}");
        source = source.replace(old, new);
    }
    source
}

fn serial_native_weight_inputs(mut source: String) -> String {
    let operands = "        .w0(d_w[0]),.w1(d_w[1]),.w2(d_w[2]),.w3(d_w[3]),\n        .c0(d_col[0][c]),.c1(d_col[1][c]),\n        .c2(d_col[2][c]),.c3(d_col[3][c]),.sum(part[c]));";
    for (old, new) in [
        (
            "  reg [8:0]  d_w   [0:3];\n  reg [7:0]  d_col [0:3][0:2];".to_owned(),
            "`ifndef GPU_V2_GOWIN_DSP\n  reg [8:0]  d_w   [0:3];\n`endif\n  reg [7:0]  d_col [0:3][0:2];".to_owned(),
        ),
        (
            operands.to_owned(),
            format!("`ifdef GPU_V2_GOWIN_DSP\n{}\n`else\n{operands}\n`endif", operands.replace("d_w", "w_in")),
        ),
        (
            "      for (k=0;k<4;k=k+1) d_w[k] <= w_in[k];".to_owned(),
            "`ifndef GPU_V2_GOWIN_DSP\n      for (k=0;k<4;k=k+1) d_w[k] <= w_in[k];\n`endif".to_owned(),
        ),
        (
            ".AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b0),".to_owned(),
            ".AREG(1'b1),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b0),".to_owned(),
        ),
        (
            "// Four products and their full unsigned sum, captured on one advancing edge.".to_owned(),
            "// Native E0 captures DSP weights and soft RGB; E1 captures the full sum. Portable retains soft E0 decode.".to_owned(),
        ),
    ] {
        assert_eq!(source.matches(&old).count(), 1, "native Serial weight anchor {old}");
        source = source.replace(&old, &new);
    }
    source
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
    r#"// Color: two advancing edges, unchanged pre-edge credit ownership.
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

  // One full tap-sum edge (age1), then feedback/normalization (age2).
  reg p_valid,p_first,p_last;
  reg [5:0] p_key;
  wire [18:0] part [0:2];

  // Feedback accumulator and stream owner.
  reg        a_valid;
  reg [5:0]  a_key;
  reg [16:0] a_val [0:2];
  reg        own_valid;
  reg [5:0]  own_key;

  // The asynchronous native RAM output is the head; there is no head register.
  wire [31:0] result_do;
  reg [3:0]  f_head, f_tail;
  reg [4:0]  f_count;

  // Select/expand precedes three one-edge fabric four-tap sums.

  wire [16:0] prev [0:2];
  wire [19:0] sumv [0:2];
  wire [7:0] normalized [0:2];
  generate
    for(c=0;c<3;c=c+1) begin: accumulate
      gpu_v2_texture_color_sum4 m(.clk(clk),.ce(ce && !fault),
        .w0(d_w[0]),.w1(d_w[1]),.w2(d_w[2]),.w3(d_w[3]),
        .c0(d_col[0][c]),.c1(d_col[1][c]),
        .c2(d_col[2][c]),.c3(d_col[3][c]),.sum(part[c]));
      assign prev[c] = p_first ? 17'd0 : a_val[c];
      assign sumv[c] = {1'b0,part[c]} + {3'b0,prev[c]};
      // Exact nearest /511, N<=255*511. No division or quantization change.
      wire [7:0] high = sumv[c][16:9];
      wire [8:0] high_low = {1'b0,high} + {1'b0,sumv[c][7:0]};
      assign normalized[c] = high + (sumv[c][8] | high_low[8]);
    end
  endgenerate

  // Fault detection. Sum-domain checks happen at the same stage boundaries as
  // the emulator; all latch `fault` and freeze the datapath.
  // Unsigned prev>=0, and the full20-bit sum cannot overflow.
  // sum_bad therefore covers every partial-domain failure on the same edge.
  wire sum_bad = p_valid && (
    sumv[0]>MAX_SUM || sumv[1]>MAX_SUM || sumv[2]>MAX_SUM);
  wire owner_bad = accepted && (first_in ? own_valid : (!own_valid || own_key!=key_in));
  wire packet_bad = accepted && (in_payload[7:4]>4'd10);
  wire feedback_bad = p_valid && (p_first ? a_valid : (!a_valid || a_key!=p_key));

  // Result FIFO traffic and credit accounting (pop before push, as the emu).
  wire do_pop = out_ready && (f_count!=0);
  wire do_push = p_valid && p_last;
  wire [4:0] count_pre_push = f_count - (do_pop ? 5'd1 : 5'd0);
  wire overflow_bad = do_push && (count_pre_push == 5'd16);
  wire [4:0] count_next = f_count + (do_push ? 5'd1 : 5'd0) - (do_pop ? 5'd1 : 5'd0);
  wire [4:0] credits_next =
      credits + ((accepted && last_in) ? 5'd1 : 5'd0) - (do_pop ? 5'd1 : 5'd0);

  // Cancel incoming/pop terms modulo32 without placing admission arithmetic
  // on the main ownership comparison. Keep the original six-bit total's
  // overflow: modulo equality alone would accept corrupt states at30/31.
  wire pending_last=d_valid && d_last;
  wire incoming_last=accepted && last_in;
  wire [4:0] expected_credits=
      f_count + (do_push ? 5'd1 : 5'd0) + (pending_last ? 5'd1 : 5'd0);
  wire total_wrap=(&count_next[4:1]) &&
      ((count_next[0] && (pending_last || incoming_last)) ||
       (pending_last && incoming_last));
  wire credit_bad=(credits!=expected_credits) || total_wrap;

  wire fault_now=packet_bad || owner_bad || sum_bad || feedback_bad || overflow_bad || credit_bad;
  wire result_write=!reset && ce && !fault && !fault_now && do_push;
  wire [3:0] result_read_address=f_head;
  wire [31:0] result_write_data={2'b0,p_key,normalized[2],normalized[1],normalized[0]};
`ifdef GPU_V2_GOWIN_DSP
  genvar q; generate for(q=0;q<8;q=q+1) begin:result_bank
    RAM16SDP4 ram(.CLK(clk),.WRE(result_write),.WAD(f_tail),
      .RAD(result_read_address),.DI(result_write_data[q*4+:4]),.DO(result_do[q*4+:4]));
  end endgenerate
`else
  reg [31:0] result_ram [0:15];
  assign result_do=result_ram[result_read_address];
  always @(posedge clk) begin
    if(result_write) result_ram[f_tail]<=result_write_data;
  end
`endif
  integer a, b, j, k;
  always @(posedge clk) begin
    if (reset) begin
      d_valid <= 1'b0;
      p_valid <= 1'b0;
      a_valid <= 1'b0; own_valid <= 1'b0;
      f_head <= 4'd0; f_tail <= 4'd0; f_count <= 5'd0;
      credits <= 5'd0; fault <= 1'b0;
    end else if (ce && !fault) begin
      if(fault_now) fault<=1'b1;
      else begin
      if (do_pop) f_head <= f_head + 4'd1;
      if (do_push) begin
        f_tail <= f_tail + 4'd1;
      end
      f_count <= count_next;
      credits <= credits_next;

      if(p_valid) begin
        a_valid <= !p_last; a_key <= p_key;
        for(j=0;j<3;j=j+1) a_val[j] <= sumv[j][16:0];
      end
      p_valid <= d_valid; p_key <= d_key; p_first <= d_first; p_last <= d_last;

      d_valid <= accepted; d_key <= key_in; d_first <= first_in; d_last <= last_in;
      for (k=0;k<4;k=k+1) d_w[k] <= w_in[k];
      for (k=0;k<4;k=k+1) for (b=0;b<3;b=b+1) d_col[k][b] <= col_in[k][b];

      if (accepted) begin
        own_valid <= !last_in;
        own_key <= key_in;
      end

      end
    end
  end

  assign out_valid = !fault && (f_count!=0);
  assign out_key   = result_do[29:24];
  assign out_r     = result_do[7:0];
  assign out_g     = result_do[15:8];
  assign out_b     = result_do[23:16];
endmodule

// Four products and their full unsigned sum, captured on one advancing edge.
module gpu_v2_texture_color_sum4(input wire clk,ce,
 input wire [8:0] w0,w1,w2,w3,input wire [7:0] c0,c1,c2,c3,
 output wire [18:0] sum);
 wire [16:0] p0,p1,p2,p3;
 gpu_v2_texture_color_mul9 m0(.clk(clk),.ce(ce),.a(w0),.b(c0),.product(p0));
 gpu_v2_texture_color_mul9 m1(.clk(clk),.ce(ce),.a(w1),.b(c1),.product(p1));
 gpu_v2_texture_color_mul9 m2(.clk(clk),.ce(ce),.a(w2),.b(c2),.product(p2));
 gpu_v2_texture_color_mul9 m3(.clk(clk),.ce(ce),.a(w3),.b(c3),.product(p3));
 wire [17:0] pair0={1'b0,p0}+{1'b0,p1};
 wire [17:0] pair1={1'b0,p2}+{1'b0,p3};
 wire [18:0] full_sum={1'b0,pair0}+{1'b0,pair1};
 reg [18:0] q;
 always @(posedge clk) if(ce) q<=full_sum;
 assign sum=q;
endmodule

module gpu_v2_texture_color_mul9(input wire clk,ce,input wire [8:0] a,
 input wire [7:0] b,output wire [16:0] product);
`ifdef GPU_V2_GOWIN_DSP
 wire [17:0] raw;
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b0),
 .ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(raw),.SOA(),.SOB(),.A(a),.B({1'b0,b}),.SIA(9'd0),.SIB(9'd0),
 .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(1'b0));
 assign product=raw[16:0];
`else
 assign product=a*b;
`endif
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
        assert_eq!(latency(), 2);
        assert_eq!(initiation_interval(), 1);
        assert_eq!(Allocation::default().total_state_bits(), 291);
    }
}
