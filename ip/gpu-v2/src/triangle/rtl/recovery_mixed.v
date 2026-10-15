// Fixed mixed recovery: load-decoded Delta9 normal, exact UV, RGB9.
module gpu_v2_threefield_recovery_mixed #(
 parameter UV_BITS=__UV_BITS__, MAX_WEIGHT_EXP=__MAX_EXP__, TOKEN_BITS=__TOKEN_BITS__,
 parameter ACC_WIDTH=15+UV_BITS+MAX_WEIGHT_EXP, COMPUTE_EXP_BITS=(MAX_WEIGHT_EXP<2)?1:2
)(
 input wire clk,reset,ce,
 input wire config_valid,config_texture,
 input wire [71:0] config_base,
 input wire [77:0] config_delta1,config_delta2,
 input wire [5:0] config_active,
 output wire config_ready,output reg config_error,
 input wire pixel_valid,input wire [27:0] beta1,beta2,
 input wire [TOKEN_BITS-1:0] pixel_token,input wire pixel_fallback,
 output wire pixel_ready,
 output reg result_valid,input wire result_ready,
 output reg [TOKEN_BITS-1:0] result_token,output reg result_fallback,
 output wire [6*ACC_WIDTH-1:0] result_data
);
reg phase,loaded,texture_q;
reg [71:0] bases;
// Three normal14 and three albedo13 fields: each delta vector adds3 bits.
reg [80:0] deltas1,deltas2;
reg [5:0] active;
reg [23+COMPUTE_EXP_BITS:0] held_beta;
reg ret_valid,ret_last,ret_fallback,ret_normal_endpoint;
reg [COMPUTE_EXP_BITS-1:0] ret_exp;
reg [TOKEN_BITS-1:0] ret_token;
reg [5:0] ret_sign;
wire [35:0] products18[0:4];
wire [17:0] products9[0:2];
reg signed [ACC_WIDTH-1:0] accumulator[0:5],result[0:5];
wire step=ce&&(!result_valid||result_ready);
assign config_ready=!phase&&!ret_valid&&!result_valid;
assign pixel_ready=loaded&&!phase&&(!result_valid||result_ready)&&!config_valid;
wire issue=phase||(pixel_valid&&pixel_ready);
wire [23+COMPUTE_EXP_BITS:0] weight=phase?held_beta:{beta1[27:26],beta1[22+:COMPUTE_EXP_BITS],beta1[21:0]};
wire [COMPUTE_EXP_BITS-1:0] compute_exp=weight[22+:COMPUTE_EXP_BITS];
wire normal_endpoint=weight[22+COMPUTE_EXP_BITS];
wire uv_endpoint=weight[23+COMPUTE_EXP_BITS];
wire [TOKEN_BITS-1:0] issue_token=phase?ret_token:pixel_token;
initial begin
 if(UV_BITS<9||UV_BITS>12||MAX_WEIGHT_EXP<0||MAX_WEIGHT_EXP>3||TOKEN_BITS<1||TOKEN_BITS>16
    ||ACC_WIDTH!=15+UV_BITS+MAX_WEIGHT_EXP||COMPUTE_EXP_BITS!=((MAX_WEIGHT_EXP<2)?1:2))
  $error("unsupported mixed recovery parameters");
end
function descriptor_ok;
 input t;input [71:0] b;input [77:0] d1,d2;input [5:0] a;
 integer g,j;reg [12:0] d;reg [8:0] m;reg [2:0] e;
 begin
 descriptor_ok=1;
 for(g=0;g<6;g=g+1)begin
  if(g>=3&&!t&&b[g*12+8+:4]!=0)descriptor_ok=0;
  if(t&&g==5&&(b[g*12+:12]!=0||a[g]))descriptor_ok=0;
  if(a[g])for(j=0;j<2;j=j+1)begin
   if(j==0)d=d1[g*13+:13];else d=d2[g*13+:13];
   m=d[8:0];e=d[11:9];
   if(g>=3&&t)begin end
   else if(g>=3&&!t)begin if(d[11:8]!=0)descriptor_ok=0;end
   // Equivalent to scale<=4 AND (mag<<scale)<=4096 for all9+3-bit inputs.
   else if(e>4||(e==4&&m>256))descriptor_ok=0;
  end
 end
 end
endfunction
function [12:0] decode_normal;
 input [8:0] m;input [2:0] e;
 begin case(e)
  0:decode_normal={4'b0,m};1:decode_normal={3'b0,m,1'b0};
  2:decode_normal={2'b0,m,2'b0};3:decode_normal={1'b0,m,3'b0};
  4:decode_normal={m,4'b0};default:decode_normal=0;
 endcase end
endfunction
function [80:0] bind_deltas;
 input [77:0] d;input [5:0] a;integer g;reg [12:0] x;
 begin bind_deltas=0;for(g=0;g<6;g=g+1)if(a[g])begin
  x=d[g*13+:13];
  if(g<3)bind_deltas[g*14+:14]={x[12],decode_normal(x[8:0],x[11:9])};
  else bind_deltas[42+(g-3)*13+:13]=x;
 end end
endfunction
function beta_unsupported;
 input [27:0] b;input t;
 begin
 beta_unsupported=(b[25:22]>MAX_WEIGHT_EXP)||(b[26]&&b[8:0]!=0);
 if(t&&(b[20:18]>=(1<<(UV_BITS-9))||(b[27]&&(b[17:9]!=0||b[20:18]!=0))))beta_unsupported=1;
 end
endfunction
wire admission_fallback=pixel_fallback||beta_unsupported(beta1,texture_q)||beta_unsupported(beta2,texture_q);
wire issue_fallback=phase?ret_fallback:admission_fallback;
wire [9:0] normal_code=normal_endpoint?10'd512:{1'b0,weight[8:0]};
wire [12:0] uv_code=uv_endpoint?(13'd1<<UV_BITS):
 (({4'b0,weight[17:9]}<<(UV_BITS-9))|{10'b0,weight[20:18]});
genvar g;
generate for(g=0;g<5;g=g+1)begin:wide_lane
 wire [17:0] magnitude;
 if(g<3)begin
  assign magnitude=phase?{5'b0,deltas2[g*14+:13]}:{5'b0,deltas1[g*14+:13]};
 end else begin
  assign magnitude=phase?{6'b0,deltas2[42+(g-3)*13+:12]}:{6'b0,deltas1[42+(g-3)*13+:12]};
 end
 wire [17:0] multiplier_b=(g<3)?{8'b0,normal_code}:{5'b0,uv_code};
 wire dsp_ce=step&&!reset&&issue&&!issue_fallback&&active[g]&&((g<3)||texture_q);
`ifdef GPU_RECOVERY_NATIVE
 MULT18X18 #(.AREG(0),.BREG(0),.PIPE_REG(0),.OUT_REG(1),.ASIGN_REG(0),.BSIGN_REG(0),.MULT_RESET_MODE("SYNC")) dsp(
  .DOUT(products18[g]),.SOA(),.SOB(),.A(magnitude),.B(multiplier_b),.SIA(18'd0),.SIB(18'd0),
  .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(dsp_ce),.RESET(reset));
`else
 reg [35:0] product;
 always @(posedge clk)if(reset)product<=0;else if(dsp_ce)product<=magnitude*multiplier_b;
 assign products18[g]=product;
`endif
end
for(g=0;g<3;g=g+1)begin:rgb_lane
 wire [8:0] magnitude=phase?{1'b0,deltas2[42+g*13+:8]}:{1'b0,deltas1[42+g*13+:8]};
 wire [8:0] multiplier_b=normal_endpoint?9'd1:weight[8:0];
 wire dsp_ce=step&&!reset&&issue&&!issue_fallback&&active[g+3]&&!texture_q;
`ifdef GPU_RECOVERY_NATIVE
 MULT9X9 #(.AREG(0),.BREG(0),.PIPE_REG(0),.OUT_REG(1),.ASIGN_REG(0),.BSIGN_REG(0),.SOA_REG(0),.MULT_RESET_MODE("SYNC")) dsp(
  .DOUT(products9[g]),.SOA(),.SOB(),.A(magnitude),.B(multiplier_b),.SIA(9'd0),.SIB(9'd0),
  .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(dsp_ce),.RESET(reset));
`else
 reg [17:0] product;
 always @(posedge clk)if(reset)product<=0;else if(dsp_ce)product<=magnitude*multiplier_b;
 assign products9[g]=product;
`endif
end
for(g=0;g<6;g=g+1)begin:lane
 wire [ACC_WIDTH-1:0] fixed_product;
 if(g<3)begin
  // Decoded4096 * endpoint512 <=2^21,22 useful unsigned bits.
  assign fixed_product={{(ACC_WIDTH-22){1'b0}},products18[g][21:0]}<<(UV_BITS-9);
 end else if(g<5)begin
  wire [23:0] rgb_product=ret_normal_endpoint?({6'b0,products9[g-3]}<<9):{6'b0,products9[g-3]};
  assign fixed_product=texture_q?{{(ACC_WIDTH-24){1'b0}},products18[g][23:0]}:(rgb_product<<(UV_BITS-9));
 end else begin
  wire [23:0] rgb_product=ret_normal_endpoint?({6'b0,products9[g-3]}<<9):{6'b0,products9[g-3]};
  assign fixed_product=rgb_product<<(UV_BITS-9);
 end
 wire [ACC_WIDTH-1:0] unsigned_term=fixed_product<<ret_exp;
 wire signed [ACC_WIDTH-1:0] base_value;
 if(g<3)begin assign base_value=$signed({{(ACC_WIDTH-12){bases[g*12+11]}},bases[g*12+:12]});end
 else begin assign base_value=$signed({{(ACC_WIDTH-12){1'b0}},bases[g*12+:12]});end
 wire [ACC_WIDTH-1:0] base_aligned=base_value<<UV_BITS;
 wire [ACC_WIDTH-1:0] seed=ret_last?accumulator[g]:base_aligned;
 wire term_enabled=active[g]&&!ret_fallback;
 wire subtract=term_enabled&&ret_sign[g];
 wire [ACC_WIDTH-1:0] operand=(term_enabled?unsigned_term:{ACC_WIDTH{1'b0}})^{ACC_WIDTH{subtract}};
 wire [ACC_WIDTH:0] carry_sum={seed,subtract}+{operand,1'b1};
 wire signed [ACC_WIDTH-1:0] accumulated=$signed(carry_sum[ACC_WIDTH:1]);
 assign result_data[g*ACC_WIDTH+:ACC_WIDTH]=result[g];
 always @(posedge clk)begin
  if(reset)begin accumulator[g]<=0;result[g]<=0;end
  else if(step&&ret_valid)begin
   if(ret_last)begin if(ret_fallback)result[g]<=0;else result[g]<=accumulated;end
   else accumulator[g]<=accumulated;
  end
 end
end endgenerate
integer q;
always @(posedge clk)begin
 if(reset)begin
  phase<=0;loaded<=0;config_error<=0;texture_q<=0;bases<=0;deltas1<=0;deltas2<=0;active<=0;
  held_beta<=0;ret_valid<=0;ret_last<=0;ret_token<=0;ret_fallback<=0;
  ret_normal_endpoint<=0;ret_exp<=0;ret_sign<=0;
  result_valid<=0;result_token<=0;result_fallback<=0;
 end else if(ce&&config_valid&&config_ready)begin
  if(descriptor_ok(config_texture,config_base,config_delta1,config_delta2,config_active))begin
   loaded<=1;config_error<=0;texture_q<=config_texture;bases<=config_base;
   deltas1<=bind_deltas(config_delta1,config_active);deltas2<=bind_deltas(config_delta2,config_active);active<=config_active;
  end else begin loaded<=0;config_error<=1;end
 end else if(step)begin
  if(result_valid&&result_ready)result_valid<=0;
  if(ret_valid&&ret_last)begin result_valid<=1;result_token<=ret_token;result_fallback<=ret_fallback;end
  ret_valid<=issue;
  if(issue)begin
   ret_last<=phase;ret_token<=issue_token;ret_fallback<=issue_fallback;
   ret_normal_endpoint<=normal_endpoint;
   if(issue_fallback)ret_exp<=0;else ret_exp<=compute_exp;
   for(q=0;q<6;q=q+1)begin
    if(q<3)begin
     if(phase)ret_sign[q]<=deltas2[q*14+13]^weight[21];else ret_sign[q]<=deltas1[q*14+13]^weight[21];
    end else begin
     if(phase)ret_sign[q]<=deltas2[42+(q-3)*13+12]^weight[21];else ret_sign[q]<=deltas1[42+(q-3)*13+12]^weight[21];
    end
   end
   phase<=!phase;
   if(!phase)held_beta<={beta2[27:26],beta2[22+:COMPUTE_EXP_BITS],beta2[21:0]};
  end
 end
end
endmodule
