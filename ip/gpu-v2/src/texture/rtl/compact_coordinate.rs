//! Coordinate RTL for the checked physical-mip domain of `CoordinateEmu::Input`.
//! The independent generic calendar/emulator remains the numerical reference.
//! One checked-domain axis kernel is reused on the two enabled edges.
//! U writes its final result slices; V's captured input survives one edge.
//! Two enabled edges, II2, old phase admission and CE holds are unchanged.

/// Module selected by the serial preparation controller.
pub const MODULE: &str = "gpu_v2_texture_serial_coord";
/// V input16, configuration16 and joint fine result36; no U payload shadow.
pub const NUMERIC_BITS: usize = 32 + 36;
/// One-hot phase2 and valid3, matching the generic reference's edge contract.
pub const CONTROL_BITS: usize = 2 + 3;
pub const LATENCY: usize = 2;
pub const INITIATION_INTERVAL: usize = 2;

/// Internal checked-domain kernel, not the generic full-shift-domain lowering.
/// The caller must supply wrapped UV16, power-of-two sides2..1024,
/// `shift=log2(side0)-8`, `side1=max(2,side0/2)` and `halve=(side0>2)`.
/// Production LOD/Ready decoding supplies these relationships. Standalone
/// callers use the same input validation as `CoordinateEmu`; generic diagnostic
/// calendars retain `derivative::rtl::emit` and their complete amount domain.
/// Only off-phase admission asserts the combinational fault, as in that emitter.
pub fn verilog() -> &'static str {
    r#"// Legal physical mip domain: shift=log2(side0)-8, side1=max(2,side0/2).
module gpu_v2_texture_serial_coord (
  input clk,ce,reset,in_valid,
  output in_ready,fault,out_valid,
  input in_flags_0,in_flags_1,
  input signed [17:0] in_coordinate_shift_0,
  input [15:0] in_wrapped_uv_0,in_wrapped_uv_1,
  input signed [11:0] in_side_0,in_side_1,
  output [7:0] out_f0_0,out_f0_1,out_f1_0,out_f1_1,
  output [9:0] out_t0_0_0,out_t0_0_1,out_t0_1_0,out_t0_1_1,
  output [9:0] out_t1_0_0,out_t1_0_1,out_t1_1_0,out_t1_1_1
);
  reg [35:0] result_bank;
  reg [15:0] v_input;
  reg [3:0] distance_bank;
  reg nearest_bank,halve_bank;
  reg [9:0] mask_bank;
  reg [1:0] phase;
  reg [2:0] valid;
  assign in_ready=phase[0];
  assign fault=ce && in_valid && !in_ready;
  assign out_valid=valid[2];
  wire [3:0] input_distance=4'd2-in_coordinate_shift_0[3:0];
  wire [9:0] input_mask=in_side_0[9:0]-10'd1;
  wire [15:0] axis_uv=phase[0] ? in_wrapped_uv_0 : v_input;
  wire [3:0] distance=phase[0] ? input_distance : distance_bank;
  wire nearest=phase[0] ? in_flags_0 : nearest_bank;
  wire halve=phase[0] ? in_flags_1 : halve_bank;
  wire [9:0] mask0=phase[0] ? input_mask : mask_bank;
  // Validated power-of-two sides: coarse side is max(2,fine side/2).
  wire [9:0] mask1={1'b0,mask0[9:1]} | 10'd1;
  wire [17:0] extended_uv={axis_uv,2'b0};
  wire [17:0] scaled_uv=extended_uv>>distance;
  wire signed [18:0] fine=$signed({1'b0,scaled_uv})
    - (nearest ? 19'sd0 : 19'sd128);
  wire [9:0] index0=fine[17:8];
  wire [17:0] axis_result={index0 & mask0,fine[7:0]};
  assign out_f0_0=result_bank[7:0]; assign out_f0_1=result_bank[15:8];
  assign out_t0_0_0=result_bank[25:16];assign out_t0_1_0=result_bank[35:26];
  // Compatibility outputs are decoded views, not additional retained fields.
  wire [9:0] base_x=result_bank[25:16],base_y=result_bank[35:26];
  wire [9:0] cmask=(mask_bank>>1)|10'd1;
  wire [9:0] coarse_x=(base_x>>1)-(!base_x[0]&&!result_bank[7]);
  wire [9:0] coarse_y=(base_y>>1)-(!base_y[0]&&!result_bank[15]);
  assign out_t0_0_1=(base_x+10'd1)&mask_bank;assign out_t0_1_1=(base_y+10'd1)&mask_bank;
  assign out_t1_0_0=halve_bank?(coarse_x & cmask):base_x;
  assign out_t1_1_0=halve_bank?(coarse_y & cmask):base_y;
  assign out_t1_0_1=(out_t1_0_0+10'd1)&cmask;assign out_t1_1_1=(out_t1_1_0+10'd1)&cmask;
  assign out_f1_0=halve_bank?{base_x[0]^!result_bank[7],!result_bank[7],result_bank[6:1]}:result_bank[7:0];
  assign out_f1_1=halve_bank?{base_y[0]^!result_bank[15],!result_bank[15],result_bank[14:9]}:result_bank[15:8];
  always @(posedge clk) begin
    if(reset) begin phase<=2'b01; valid<=3'b0; end
    else if(ce) begin
      phase<={phase[0],phase[1]};
      valid<={valid[1],in_valid && in_ready,1'b0};
      if(in_valid && in_ready) begin
        {result_bank[25:16],result_bank[7:0]}<=axis_result;
        v_input<=in_wrapped_uv_1;
        distance_bank<=input_distance;mask_bank<=input_mask;
        nearest_bank<=in_flags_0;halve_bank<=in_flags_1;
      end
      if(valid[1])
        {result_bank[35:26],result_bank[15:8]}<=axis_result;
    end
  end
endmodule
"#
}
