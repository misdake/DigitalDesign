// Closed single-result Final; byte zero of each RGB bus is red.
module gpu_v2_final_ordered_single(
 input wire clk,reset,ce,commit_allow,abort,in_valid,
 input wire [23:0] in_tint,in_tex,in_spec,
 input wire [8:0] in_g,in_h,
 output wire in_ready,in_accept,advance,preflight_bad,fault,
 input wire out_ready,
 output wire out_valid,out_fire,output wire [23:0] out_rgb,
 output wire [3:0] credits,queued
);
 localparam IDLE=2'd0,SPEC=2'd1,BASE=2'd2,PUBLISH=2'd3;
 reg stopped,result_valid;
 reg [1:0] phase;
 reg [23:0] result;
 @INPUT_STORAGE@
 wire running=ce&&!reset&&!fault;
 wire occupied=(phase!=IDLE)||result_valid;
 assign fault=stopped||abort;
 assign in_ready=running&&!occupied;
 assign preflight_bad=in_ready&&in_valid&&(in_h>9'd256);
 assign advance=running&&commit_allow&&!preflight_bad;
 assign in_accept=advance&&in_valid&&!occupied;
 assign out_valid=!reset&&!fault&&result_valid;
 assign out_fire=advance&&out_ready&&out_valid;
 assign out_rgb=result;
 assign credits={3'b0,occupied};assign queued={3'b0,result_valid};
 wire publish=advance&&(phase==PUBLISH);
 wire mul_ce=advance&&(in_accept||(phase==SPEC)||(phase==BASE));
 wire [23:0] color;
 genvar c;
 generate for(c=0;c<3;c=c+1)begin:channels
  reg [8:0] a;
  reg [7:0] b;
  wire [16:0] product;
  reg [7:0] base;
  reg [15:0] sh;
  wire [15:0] plus128=product[15:0]+16'd128;
  wire [15:0] folded=plus128+{8'd0,plus128[15:8]};
  wire [17:0] sum={1'b0,product}+{2'b0,sh};
  wire round_up=sum[7]&&((|sum[6:0])||sum[8]);
  wire saturated=(|sum[17:16])||(&sum[15:8]);
  wire [7:0] rounded=sum[15:8]+{7'd0,round_up};
  assign color[c*8+:8]=saturated?8'd255:rounded;
  always @*begin
   a={1'b0,in_tint[c*8+:8]};b=in_tex[c*8+:8];
   case(phase)
    SPEC:begin a=@H@;b=@SPEC@[c*8+:8];end
    BASE:begin a=@G@;b=base;end
   endcase
  end
  gpu_v2_final_single_mul9 multiplier(.clk(clk),.ce(mul_ce),.a(a),.b(b),.product(product));
  always @(posedge clk)if(advance)begin
   if(phase==SPEC)base<=folded[15:8];
   if(phase==BASE)sh<=product[15:0];
  end
 end endgenerate
 always @(posedge clk)begin
  if(reset)begin stopped<=0;result_valid<=0;phase<=IDLE;end
  else begin
   if(abort||preflight_bad)stopped<=1;
   if(advance)begin
    case(phase)
     IDLE:if(in_accept)phase<=SPEC;
     SPEC:phase<=BASE;
     BASE:phase<=PUBLISH;
     PUBLISH:phase<=IDLE;
    endcase
    @INPUT_CAPTURE@
    if(publish)begin result<=color;result_valid<=1;end
    else if(out_fire)result_valid<=0;
   end
  end
 end
endmodule

module gpu_v2_final_single_mul9(input wire clk,ce,input wire [8:0] a,
 input wire [7:0] b,output wire [16:0] product);
 @MULTIPLIER@
endmodule
