// Explicit six-lane attribute-code recovery research leaf; no geometry or transport conversion.
module gpu_v2_threefield_recovery #(
 parameter UV_BITS=__UV_BITS__, MAX_WEIGHT_EXP=__MAX_EXP__, UV_DELTA_EXACT=__EXACT_UV__,
 parameter TOKEN_BITS=__TOKEN_BITS__, ACC_WIDTH=15+UV_BITS+MAX_WEIGHT_EXP,
 parameter COMPUTE_EXP_BITS=(MAX_WEIGHT_EXP<2)?1:2
)(
 input wire clk,reset,ce,
 input wire config_valid,config_texture,
 input wire [71:0] config_base,
 input wire [77:0] config_delta1,config_delta2,
 input wire [5:0] config_active,
 output wire config_ready,output reg config_error,
 input wire pixel_valid, input wire [27:0] beta1,beta2,
 input wire [TOKEN_BITS-1:0] pixel_token,input wire pixel_fallback,
 output wire pixel_ready,
 output reg result_valid,input wire result_ready,
 output reg [TOKEN_BITS-1:0] result_token,output reg result_fallback,
 output wire [6*ACC_WIDTH-1:0] result_data
);
reg phase,loaded,texture_q;
reg [71:0] bases;
reg [77:0] deltas1,deltas2;
reg [5:0] active;
reg [23+COMPUTE_EXP_BITS:0] held_beta;
reg ret_valid,ret_last,ret_fallback,ret_normal_endpoint,ret_uv_endpoint;
reg [TOKEN_BITS-1:0] ret_token;
reg [5:0] ret_sign;
reg [3:0] ret_shift[0:5];
wire [17:0] products[0:5];
reg [23:0] correction[0:5];
reg signed [ACC_WIDTH-1:0] accumulator[0:5],result[0:5];
wire step=ce&&(!result_valid||result_ready);
assign config_ready=!phase&&!ret_valid&&!result_valid;
assign pixel_ready=loaded&&!phase&&(!result_valid||result_ready)&&!config_valid;
wire issue=phase||(pixel_valid&&pixel_ready);
wire [23+COMPUTE_EXP_BITS:0] weight=phase?held_beta:{beta1[27:26],beta1[22+:COMPUTE_EXP_BITS],beta1[21:0]};
wire [COMPUTE_EXP_BITS-1:0] compute_exp=weight[22+:COMPUTE_EXP_BITS];
wire [TOKEN_BITS-1:0] issue_token=phase?ret_token:pixel_token;
// Width follows the legal decoded delta bound, including Q9->Q12 zero extension.
initial begin
 if(UV_BITS<9||UV_BITS>12||MAX_WEIGHT_EXP<0||MAX_WEIGHT_EXP>3||TOKEN_BITS<1||TOKEN_BITS>16
    ||ACC_WIDTH!=15+UV_BITS+MAX_WEIGHT_EXP||COMPUTE_EXP_BITS!=((MAX_WEIGHT_EXP<2)?1:2))
  $error("unsupported recovery parameters");
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
   if(g>=3&&t&&UV_DELTA_EXACT)begin end
   else if(g>=3&&!t)begin if(d[11:8]!=0)descriptor_ok=0;end
   else if(e>4||({4'b0,m}<<e)>13'd4096)descriptor_ok=0;
  end
 end
 end
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
function [11:0] fabric9x3;
 input [8:0] a;input [2:0] b;integer k;
 begin fabric9x3=0;for(k=0;k<3;k=k+1)if(b[k])fabric9x3=fabric9x3+({3'b0,a}<<k);end
endfunction
genvar g;
generate for(g=0;g<6;g=g+1)begin:lane
 wire [12:0] delta=phase?deltas2[g*13+:13]:deltas1[g*13+:13];
 wire uv=texture_q&&(g==3||g==4);
 wire [11:0] magnitude=(uv&&UV_DELTA_EXACT)?delta[11:0]:{3'b0,delta[8:0]};
 wire [2:0] delta_exp=(g>=3&&!texture_q)||(uv&&UV_DELTA_EXACT)?3'd0:delta[11:9];
 wire endpoint=uv?weight[23+COMPUTE_EXP_BITS]:weight[22+COMPUTE_EXP_BITS];
 wire [8:0] multiplier_b=endpoint?9'd1:(uv?weight[17:9]:weight[8:0]);
 wire dsp_ce=step&&!reset&&issue&&!issue_fallback&&active[g];
`ifdef GPU_RECOVERY_NATIVE
 MULT9X9 #(.AREG(0),.BREG(0),.PIPE_REG(0),.OUT_REG(1),.ASIGN_REG(0),.BSIGN_REG(0),.SOA_REG(0),.MULT_RESET_MODE("SYNC")) dsp(
  .DOUT(products[g]),.SOA(),.SOB(),.A(magnitude[8:0]),.B(multiplier_b),.SIA(9'd0),.SIB(9'd0),
  .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(dsp_ce),.RESET(reset));
`else
 reg [17:0] product;
 always @(posedge clk)if(reset)product<=0;else if(dsp_ce)product<=magnitude[8:0]*multiplier_b;
 assign products[g]=product;
`endif
 wire [11:0] high_term=fabric9x3(weight[17:9],magnitude[11:9]);
 wire [11:0] low_term=fabric9x3(magnitude[8:0],weight[20:18]);
 wire [11:0] cross_term=fabric9x3({6'b0,magnitude[11:9]},weight[20:18]);
 wire [23:0] new_correction=endpoint?{12'b0,magnitude[11:9],9'b0}:
   ({12'b0,high_term}<<UV_BITS)+{12'b0,low_term}+({12'b0,cross_term}<<9);
 reg [23:0] full_product;
 always @*begin
  if(uv)begin
   if(ret_uv_endpoint)full_product=({6'b0,products[g]}+correction[g])<<UV_BITS;
   else full_product=({6'b0,products[g]}<<(UV_BITS-9))+correction[g];
  end else begin
   if(ret_normal_endpoint)full_product={6'b0,products[g]}<<9;
   else full_product={6'b0,products[g]};
  end
 end
 wire [ACC_WIDTH-1:0] unsigned_term={{(ACC_WIDTH-24){1'b0}},full_product}<<ret_shift[g];
 wire signed [ACC_WIDTH-1:0] base_value;
 if(g<3)begin assign base_value=$signed({{(ACC_WIDTH-12){bases[g*12+11]}},bases[g*12+:12]});end
 else begin assign base_value=$signed({{(ACC_WIDTH-12){1'b0}},bases[g*12+:12]});end
 wire [ACC_WIDTH-1:0] base_aligned=base_value<<UV_BITS;
 wire [ACC_WIDTH-1:0] seed=ret_last?accumulator[g]:base_aligned;
 wire term_enabled=active[g]&&!ret_fallback;
 wire subtract=term_enabled&&ret_sign[g];
 wire [ACC_WIDTH-1:0] operand=(term_enabled?unsigned_term:{ACC_WIDTH{1'b0}})^{ACC_WIDTH{subtract}};
 // One (ACC_WIDTH+1)-bit carry chain. The low pair subtract+1 generates
 // exactly the subtraction carry-in; no separate full-width negate/increment.
 // Truncating sum[ACC_WIDTH:1] implements seed + (term XOR sign) + sign.
 wire [ACC_WIDTH:0] carry_sum={seed,subtract}+{operand,1'b1};
 wire signed [ACC_WIDTH-1:0] accumulated=$signed(carry_sum[ACC_WIDTH:1]);
 assign result_data[g*ACC_WIDTH+:ACC_WIDTH]=result[g];
 always @(posedge clk)begin
  if(reset)begin correction[g]<=0;ret_shift[g]<=0;accumulator[g]<=0;result[g]<=0;end
  else if(step)begin
   if(ret_valid)begin
    if(ret_last)begin if(ret_fallback)result[g]<=0;else result[g]<=accumulated;end
    else accumulator[g]<=accumulated;
   end
   if(issue)begin
    if(issue_fallback||!active[g])ret_shift[g]<=0;
    else if(uv)ret_shift[g]<=delta_exp+compute_exp;
    else ret_shift[g]<=UV_BITS-9+delta_exp+compute_exp;
    if(uv&&active[g]&&!issue_fallback)correction[g]<=new_correction;else correction[g]<=0;
   end
  end
 end
end endgenerate
integer q;
always @(posedge clk)begin
 if(reset)begin
 phase<=0;loaded<=0;config_error<=0;texture_q<=0;bases<=0;deltas1<=0;deltas2<=0;active<=0;
 held_beta<=0;ret_valid<=0;ret_last<=0;ret_token<=0;ret_fallback<=0;
 ret_normal_endpoint<=0;ret_uv_endpoint<=0;ret_sign<=0;
 result_valid<=0;result_token<=0;result_fallback<=0;
 end else if(ce&&config_valid&&config_ready)begin
  if(descriptor_ok(config_texture,config_base,config_delta1,config_delta2,config_active))begin
   loaded<=1;config_error<=0;texture_q<=config_texture;bases<=config_base;
   deltas1<=config_delta1;deltas2<=config_delta2;active<=config_active;
  end else begin loaded<=0;config_error<=1;end
 end else if(step)begin
  if(result_valid&&result_ready)result_valid<=0;
  if(ret_valid&&ret_last)begin result_valid<=1;result_token<=ret_token;result_fallback<=ret_fallback;end
  ret_valid<=issue;
  if(issue)begin
   ret_last<=phase;ret_token<=issue_token;ret_fallback<=issue_fallback;
   ret_normal_endpoint<=weight[22+COMPUTE_EXP_BITS];ret_uv_endpoint<=weight[23+COMPUTE_EXP_BITS];
   for(q=0;q<6;q=q+1)begin
    if(phase)ret_sign[q]<=deltas2[q*13+12]^weight[21];
    else ret_sign[q]<=deltas1[q*13+12]^weight[21];
   end
   phase<=!phase;
   if(!phase)held_beta<={beta2[27:26],beta2[22+:COMPUTE_EXP_BITS],beta2[21:0]};
  end
 end
end
endmodule
