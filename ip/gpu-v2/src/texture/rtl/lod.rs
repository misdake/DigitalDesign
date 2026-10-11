//! Checked-domain LOD RTL with grouped normalization and one registered ROM read.
//! Independent LodEmu retains the generic calendar and arithmetic reference.
use std::fmt::Write;

pub const MODULE: &str = "gpu_v2_texture_serial_lod";
pub const NUMERIC_BITS: usize = 80;
pub const CONTROL_BITS: usize = 3 + 4;
pub const LATENCY: usize = 4;
pub const INITIATION_INTERVAL: usize = 8;

pub fn verilog() -> String {
    let mut init = String::new();
    for (i, value) in crate::texture::format::LOG_RAW.iter().enumerate() {
        writeln!(init, "    log_rom[{i}]=8'd{value};").unwrap();
    }
    SOURCE.replace("// ROM_INIT", &init)
}

const SOURCE: &str = r#"module gpu_v2_texture_serial_lod (
  input clk,
  input ce,
  input reset,
  input in_valid,
  output in_ready,
  output fault,
  output out_valid,
  input [17:0] in_slope_0,
  input [3:0] in_meta_0,
  input in_has_mip_0,
  input [1:0] in_filter_0,
  input signed [15:0] in_bias_0,
  input [3:0] in_meta_1,
  input [3:0] in_meta_2,
  input [3:0] in_meta_3,
  output wire signed [17:0] out_shift0,
  output wire out_nearest,
  output wire out_halve,
  output wire signed [11:0] out_side0,
  output wire signed [11:0] out_side1,
  output wire [8:0] out_parent0,
  output wire [8:0] out_parent1,
  output wire [3:0] out_n0,
  output wire [3:0] out_n1,
  output wire out_last_fine,
  output wire [3:0] out_quad,
  output wire [3:0] out_mask,
  output wire [3:0] out_slot
);
  reg [2:0] phase;
  reg [3:0] valid;
  assign in_ready=phase==0;
  wire accepted=ce && in_valid && in_ready;
  assign fault=ce && in_valid && !in_ready;
  assign out_valid=valid[3];

  // Grouped leading-zero normalization; only legal positive left shifts.
  wire is_zero=in_slope_0==0, is_over=in_slope_0>18'd131072;
  wire [17:0] safe=(is_zero || is_over) ? 18'd1 : in_slope_0;
  wire z16=!(|safe[17:2]);
  wire [17:0] lod_norm_after16=z16 ? {safe[1:0],16'b0} : safe;
  wire z8=!(|lod_norm_after16[17:10]);
  wire [17:0] lod_norm_after8=z8 ? {lod_norm_after16[9:0],8'b0} : lod_norm_after16;
  wire z4=!(|lod_norm_after8[17:14]);
  wire [17:0] lod_norm_after4=z4 ? {lod_norm_after8[13:0],4'b0} : lod_norm_after8;
  wire z2=!(|lod_norm_after4[17:16]);
  wire [17:0] lod_norm_after2=z2 ? {lod_norm_after4[15:0],2'b0} : lod_norm_after4;
  wire z1=!lod_norm_after2[17];
  wire [17:0] norm=z1 ? {lod_norm_after2[16:0],1'b0} : lod_norm_after2;
  wire [4:0] h=5'd17-{z16,z8,z4,z2,z1};
  wire round_up=norm[10] && ((|norm[9:0]) || norm[11]);
  wire [6:0] k={1'b0,norm[16:11]}+round_up;
  wire signed [4:0] exponent=$signed(h)-5'sd16+$signed({4'b0,k[6]});

  // Checked admission guarantees max_n<=10 and bias in [-8192,8192].
  // The scale exponent is [-16,1]; offset Q8 fits signed fifteen bits.
  reg [3:0] max_n;
  reg signed [14:0] bias;
  reg [1:0] filter;
  reg has_mip,zero_flag,over_flag;
  reg [11:0] identity;
  reg signed [4:0] exp_q;
  reg [5:0] index_q;
  reg [7:0] log_q;
  reg [3:0] fine_d;
  reg [7:0] fraction_d;
  reg [3:0] fine_q;
  reg [8:0] lambda_q;
  reg [7:0] log_rom [0:63];
  initial begin
// ROM_INIT
  end
  wire signed [14:0] integer_term=$signed({{2{exp_q[4]}},exp_q,8'b0});
  wire signed [14:0] raw=integer_term+$signed({7'b0,log_q})+bias;
  // raw is the LOD offset without max_n*256. The upper clamp is
  // simply offset>=0; a negative integer offset directly gives fine mip.
  wire [5:0] mip_distance=(~raw[13:8])+6'd1;
  reg [3:0] clamped_fine;
  reg [7:0] clamped_fraction;
  always @* begin
    clamped_fine=max_n;clamped_fraction=0;
    if(has_mip && !zero_flag) begin
      if(over_flag || !raw[14]) clamped_fine=0;
      else if(mip_distance<={2'b0,max_n}) begin
        clamped_fine=mip_distance[3:0];clamped_fraction=raw[7:0];
      end
    end
  end
  wire [8:0] lambda=filter==2 ? {fraction_d,1'b0}-(fraction_d>8'd128) : 9'd0;
  wire [3:0] physical=fine_q==0 ? 4'd1 : fine_q;
  wire signed [4:0] shift=$signed({1'b0,physical})-5'sd8;
  assign out_shift0={{13{shift[4]}},shift};
  assign out_nearest=filter==0;
  assign out_halve=fine_q>1;
  assign out_side0=12'd1<<physical;
  assign out_side1=out_halve ? out_side0>>1 : 12'd2;
  assign out_parent1=lambda_q;
  assign out_parent0=~lambda_q;
  assign out_n0=fine_q;
  assign out_n1=fine_q==0 ? 4'd0 : fine_q-1'b1;
  assign out_last_fine=lambda_q==0;
  assign out_quad=identity[3:0];
  assign out_mask=identity[7:4];
  assign out_slot=identity[11:8];
  always @(posedge clk) begin
    if(reset) begin phase<=0;valid<=0;end
    else if(ce) begin
      phase<=phase+1'b1;valid<={valid[2:0],accepted};
      if(accepted) begin
        max_n<=in_meta_0;bias<=in_bias_0[14:0];filter<=in_filter_0;has_mip<=in_has_mip_0;
        identity<={in_meta_3,in_meta_2,in_meta_1};
        zero_flag<=is_zero;over_flag<=is_over;exp_q<=exponent;index_q<=k[5:0];
      end
      if(valid[0]) log_q<=log_rom[index_q];
      if(valid[1]) begin fine_d<=clamped_fine;fraction_d<=clamped_fraction;end
      if(valid[2]) begin fine_q<=fine_d;lambda_q<=lambda;end
    end
  end
endmodule
"#;
