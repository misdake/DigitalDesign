//! Conserving UNORM9 coefficient RTL for the closed compact production domain.
//! Production LOD/Coordinate supply adjacent taps and corresponding mip fractions.
//! `emu::compact::Metadata::capture` checks this domain before selected emulation;
//! this internal RTL leaf trusts the closed producer, while checking scalar widths.
//! The independent generic coefficient module retains arbitrary legal input semantics.
//! Local parity selects fine/coarse operands on three shared one-edge DSP lanes.
//! Old row totals feed corners before the same-edge next-plane overwrite. Returning
//! products and complement subtracts feed the next operation/output capture
//! without separate subtract pipeline registers. Same CE and edges as the emu.
//! GPU_V2_GOWIN_DSP binds native MULT9X9 output registers; two explicit FF rows hold ready weights.
//! Portable simulation retains the same two weight rows and synchronous metadata ports.

/// Verilog module name emitted by [`verilog`].
pub const MODULE: &str = "gpu_v2_texture_coefficient";

/// Enabled-edge latency from acceptance to a visible result.
pub const LATENCY: usize = 6;
/// Initiation interval: one accepted cohort every two enabled edges.
pub const INITIATION_INTERVAL: usize = 2;
/// Fixed numeric stages: input37 + horizontal17 + row18 + fine corners36.
pub const NUMERIC_BITS: usize = 108;
/// Number of metadata owners.
pub const COHORT_CAPACITY: usize = 3;
/// Number of ready result rows.
pub const READY_CAPACITY: usize = 2;
/// Input bus width: explicit scalar fields plus the packed 99-bit metadata.
pub const IN_PAYLOAD_BITS: usize = 162;
/// Output bus width: eight 9-bit weights plus the packed 36-bit metadata.
pub const OUT_PAYLOAD_BITS: usize = 108;

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
    /// Metadata soft FF bits; admission rows are held in BSRAM until consumption.
    pub metadata_ff_bits: usize,
    pub metadata_bsram: usize,
    pub metadata_hard_bits: usize,
    /// Two explicit ready-weight rows.
    pub ready_ff_bits: usize,
    /// No native weight SRAM; metadata still uses three BSRAM banks.
    pub ready_sdp4_cells: usize,
    pub ready_ram16_bits: usize,
    /// One local parity bit and five valid bits.
    pub phase_valid_bits: usize,
    /// Ring pointers, cohort count, ready pointers, queue count and fault.
    pub control_bits: usize,
}
impl Default for Allocation {
    fn default() -> Self {
        Self {
            multiply_sites: 3,
            products_per_site: 1,
            multiply_latency: 1,
            initiation_interval: 2,
            select_sites: 4,
            subtract_sites: 3,
            numeric_ff_bits: NUMERIC_BITS,
            metadata_ff_bits: 0,
            metadata_bsram: 1,
            metadata_hard_bits: 36,
            ready_ff_bits: 2 * 72,
            ready_sdp4_cells: 0,
            ready_ram16_bits: 0,
            phase_valid_bits: 1 + 5,
            control_bits: 4 + 4 + 2 + 1 + 1 + 2 + 1,
        }
    }
}
impl Allocation {
    /// Register state bits only; native SRAM is listed separately.
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
    r#"module gpu_v2_texture_coefficient(
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
  output wire [35:0]  out_meta,
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

  wire [35:0] meta_in={key_in[5:0],slot_in[3:0],lv0,
     (!c02[0] && !f0v[7]),(!c00[0] && !f0u[7]),c02[9:0],c00[9:0]};
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

  reg [36:0] input_stage;
  reg [16:0] horizontal_delay;
  reg [17:0] row_totals;
  reg [35:0] fine_corners;
  wire [16:0] product [0:2];
  reg ph;
  reg [4:0] valid;
  reg [3:0] cwr,mrd;
  reg [1:0] cohorts;
  reg rrd,rwr;
  reg [1:0] queued;
  wire advance=ce && !fault && queued<2;
  wire v1=valid[0],v2=valid[1],v3=valid[2],v4=valid[3];
  // All operands are old stage values. Next admission on age2 cannot
  // replace the parent/fraction until after the current DSP reads.
  wire [8:0] mul0_a=ph ? input_stage[8:0] : input_stage[17:9];
  wire [7:0] fine_u=input_stage[25:18],fine_v=input_stage[33:26];
  wire [7:0] coarse_u=input_stage[36]?{input_stage[34]^!fine_u[7],!fine_u[7],fine_u[6:1]}:fine_u;
  wire [7:0] coarse_v=input_stage[36]?{input_stage[35]^!fine_v[7],!fine_v[7],fine_v[6:1]}:fine_v;
  wire [7:0] mul0_b=ph ? fine_v : coarse_v;
  gpu_v2_texture_coefficient_mul9 m0(.clk(clk),.ce(advance),.a(mul0_a),.b(mul0_b),.product(product[0]));
  wire [7:0] horizontal_u=ph ? horizontal_delay[16:9] : input_stage[25:18];
  wire [8:0] horizontal_parent=ph ? horizontal_delay[8:0] : input_stage[8:0];
  wire [8:0] top_difference=horizontal_parent-product[0][16:8];
  gpu_v2_texture_coefficient_mul9 m1(.clk(clk),.ce(advance),.a(product[0][16:8]),.b(horizontal_u),.product(product[1]));
  gpu_v2_texture_coefficient_mul9 m2(.clk(clk),.ce(advance),.a(top_difference),.b(horizontal_u),.product(product[2]));
  wire [8:0] bottom_left=row_totals[8:0]-product[1][16:8];
  wire [8:0] top_left=row_totals[17:9]-product[2][16:8];
  wire [35:0] returning_corners={product[1][16:8],bottom_left,product[2][16:8],top_left};
  wire [71:0] out_w={returning_corners,fine_corners};
  wire [1:0] cost_in={1'b0,(p0!=0)}+{1'b0,(p1!=0)};
  wire [1:0] cost=in_valid?cost_in:2'd0;
  wire phase_even=~ph;
  assign in_ready=advance && cohorts<3 && phase_even && work_available>={6'b0,cost};
  wire accepted=in_valid && in_ready;
  wire do_consume=ce && !fault && out_ready && queued!=0;
  wire do_capture=advance && valid[4];
  assign in_accept=accepted;
  assign work_reserved=accepted?cost_in:2'd0;
  assign out_valid=!fault && queued!=0;
  // Metadata remains in its admission row until public consumption.
  // Sixteen physical metadata rows, at most three cohorts plus two queued results.
  // No extra public credit and no copied metadata in the ready-weight bank.
  wire ready_we=!reset && advance && v4 && !(accepted && (bad_input || work_available>16));
  wire [71:0] ready_value;
  reg [71:0] ready_ff0,ready_ff1;
  always @(posedge clk) begin
    if(ready_we && !rwr) ready_ff0<=out_w;
    if(ready_we && rwr) ready_ff1<=out_w;
  end
  assign ready_value=rrd ? ready_ff1 : ready_ff0;
  wire input_fault=accepted && (bad_input || work_available>16);
  wire meta_we=!reset && !fault && accepted && !input_fault;
  // A pop from the sole ready row has no published successor to read.
  // Hold DO until publication; speculative reads can collide with admission.
  wire meta_re=!reset && !fault && !input_fault &&
    (do_capture || (do_consume && queued[1]));
  wire [3:0] meta_ra=do_consume ? mrd+1'b1 : mrd;
  wire [35:0] meta_data=meta_in;
  wire [35:0] meta_do;
  // The first head returns on publication; a pop reads the next head.
  // Native DO registers are the only retained metadata return.
`ifdef GPU_V2_GOWIN_DSP
  genvar m;
  generate for(m=0;m<1;m=m+1) begin:metadata_bank
    SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),
      .BLK_SEL_0(0),.BLK_SEL_1(0),.RESET_MODE("SYNC")) bank(
      .DO(meta_do[m*36+:36]),.DI(meta_data[m*36+:36]),
      .BLKSELA(3'b0),.BLKSELB(3'b0),
      .ADA({5'b0,cwr,1'b0,4'b1111}),.ADB({5'b0,meta_ra,5'b0}),
      .CLKA(clk),.CLKB(clk),.CEA(meta_we),.CEB(meta_re),
      .OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
  end endgenerate
`else
  reg [35:0] meta_ram [0:15];
  reg [35:0] meta_return;
  always @(posedge clk) begin
    if(meta_we) meta_ram[cwr]<=meta_data;
    if(reset) meta_return<=0;
    else if(meta_re) meta_return<=meta_ram[meta_ra];
  end
  assign meta_do=meta_return;
`endif
  assign out_weights=ready_value; assign out_meta=meta_do[35:0];
  always @(posedge clk) begin
    if(reset) begin
      input_stage<=0; horizontal_delay<=0; row_totals<=0; fine_corners<=0;
      ph<=0; valid<=0; mrd<=0; cwr<=0; cohorts<=0;
      rrd<=0; rwr<=0; queued<=0; fault<=0;
    end else if(!fault) begin
      if(accepted && (bad_input || work_available>16)) fault<=1;
      else begin
        queued<=queued+do_capture-do_consume;
        cohorts<=cohorts+accepted-do_capture;
        if(do_consume) begin rrd<=~rrd; mrd<=mrd+1'b1; end
        if(advance) begin
          if(accepted) begin
            input_stage<={(lv0>1 && !nearest),c02[0],c00[0],
                          nearest?8'd0:f0v,nearest?8'd0:f0u,p1[8:0],p0[8:0]};
            cwr<=cwr+1'b1;
          end
          if(v2) horizontal_delay<={coarse_u,input_stage[17:9]};
          // Read fine row totals at age3, then replace them with coarse.
          // Age4 ready write reads coarse before the next cohort's fine write.
          if(v2 || v3) row_totals<={top_difference,product[0][16:8]};
          if(v3) fine_corners<=returning_corners;
          if(do_capture) rwr<=~rwr;
          valid<={valid[3:0],accepted}; ph<=~ph;
        end
      end
    end
  end
endmodule

module gpu_v2_texture_coefficient_mul9(input wire clk,ce,input wire [8:0] a,
 input wire [7:0] b,output wire [16:0] product);
`ifdef GPU_V2_GOWIN_DSP
 wire [17:0] raw;
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b1),
 .ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(raw),.SOA(),.SOB(),.A(a),.B({1'b0,b}),.SIA(9'd0),.SIB(9'd0),
 .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(1'b0));
 assign product=raw[16:0];
`else
 reg [16:0] q; always @(posedge clk) if(ce) q<=a*b;
 assign product=q;
`endif
endmodule
"#
}
pub fn source() -> &'static str {
    verilog()
}
/// Closed Serial specialization. The caller holds one lane/plane at a time.
/// Admission requires old queued==0 and old cohorts==0, including a consuming
/// edge. Arithmetic CE/parity still follow the reviewed ready2 calendar, so
/// E4 write/E5 publication/E6 consumption and all closed Serial latencies stay.
/// Runtime and the independent generic interface continue to use `verilog()`.
pub fn serial_verilog() -> String {
    let mut source = verilog().to_owned();
    for (old, new) in [
        (
            "assign in_ready=advance && cohorts<3 && phase_even && work_available>={6'b0,cost};",
            "assign in_ready=advance && cohorts<1 && queued==0 && phase_even && work_available>={6'b0,cost};",
        ),
        (
            "if(do_consume) begin rrd<=~rrd; mrd<=mrd+1'b1; end",
            "if(do_consume) begin rrd<=1'b0; mrd<=mrd+1'b1; end",
        ),
        ("if(do_capture) rwr<=~rwr;", "if(do_capture) rwr<=1'b0;"),
        ("reg [71:0] ready_ff0,ready_ff1;", "reg [71:0] ready_ff0;"),
        (
            "if(ready_we && !rwr) ready_ff0<=out_w;",
            "if(ready_we) ready_ff0<=out_w;",
        ),
        ("    if(ready_we && rwr) ready_ff1<=out_w;\n", ""),
        (
            "assign ready_value=rrd ? ready_ff1 : ready_ff0;",
            "assign ready_value=ready_ff0;",
        ),
    ] {
        assert_eq!(
            source.matches(old).count(),
            1,
            "Serial specialization anchor {old}"
        );
        source = source.replace(old, new);
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn module_declares_actual_shared_calendar() {
        assert_eq!(latency(), 6);
        assert_eq!(initiation_interval(), 2);
        assert_eq!(Allocation::default().multiply_sites, 3);
        assert_eq!(Allocation::default().multiply_latency, 1);
        assert_eq!(
            verilog()
                .matches("gpu_v2_texture_coefficient_mul9 m")
                .count(),
            3
        );
    }
}
