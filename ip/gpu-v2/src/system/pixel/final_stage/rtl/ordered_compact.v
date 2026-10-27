// Packed RGB byte0=red. No key, destination copy, or per-pixel context shadow.
module gpu_v2_final_ordered_compact(
 input wire clk,reset,ce,commit_allow,abort,in_valid,
 input wire [23:0] in_tint,in_tex,in_spec,
 input wire [8:0] in_g,in_h,
 output wire in_ready,in_accept,advance,preflight_bad,fault,
 input wire out_ready,
 output wire out_valid,out_fire,output wire [23:0] out_rgb,
 output wire [3:0] credits,queued
);
 localparam BASE_THRESHOLDS=@BASE_THRESHOLDS@;
 reg stopped;
 reg [3:0] used,count,valid;
 reg [2:0] rd,wr;
 reg [8:0] g0,g1;
 (* syn_ramstyle="distributed_ram" *) reg [23:0] result_rows [0:15];
 wire running=ce&&!reset&&!fault;
 assign fault=stopped||abort;
 assign in_ready=running&&(used<8);
 assign preflight_bad=in_ready&&in_valid&&(in_h>9'd256);
 assign advance=running&&commit_allow&&!preflight_bad;
 assign in_accept=advance&&in_valid&&(used<8);
 assign out_valid=!reset&&!fault&&(count!=0);
 assign out_fire=advance&&out_ready&&out_valid;
 assign out_rgb=result_rows[rd];
 assign credits=used;assign queued=count;
 wire publish=advance&&valid[3];
 wire [23:0] color;
 genvar c;
 generate for(c=0;c<3;c=c+1)begin:channels
  wire [16:0] tt,sh,bg;
  reg [7:0] base1;
  reg [15:0] sh1,sh2;
  reg [17:0] sum3;
  wire [15:0] plus128=tt[15:0]+16'd128;
  wire [15:0] folded=plus128+{8'd0,plus128[15:8]};
  wire [8:0] digits={1'b0,tt[15:8]}+{1'b0,tt[7:0]};
  wire inc1=digits[8]||digits[7];
  wire inc2=digits[8]&&(digits[7]||(&digits[6:0]));
  wire [7:0] threshold=tt[15:8]+{6'd0,inc2,inc1&&!inc2};
  wire [7:0] base=BASE_THRESHOLDS?threshold:folded[15:8];
  wire round_up=sum3[7]&&((|sum3[6:0])||sum3[8]);
  wire saturated=(|sum3[17:16])||(&sum3[15:8]);
  wire [7:0] rounded=sum3[15:8]+{7'd0,round_up};
  assign color[c*8+:8]=saturated?8'd255:rounded;
  gpu_v2_final_compact_mul9 tint_product(.clk(clk),.ce(advance),.a({1'b0,in_tint[c*8+:8]}),.b(in_tex[c*8+:8]),.product(tt));
  gpu_v2_final_compact_mul9 spec_product(.clk(clk),.ce(advance),.a(in_h),.b(in_spec[c*8+:8]),.product(sh));
  gpu_v2_final_compact_mul9 base_product(.clk(clk),.ce(advance),.a(g1),.b(base1),.product(bg));
  always @(posedge clk)if(advance)begin
   base1<=base;sh1<=sh[15:0];sh2<=sh1;
   sum3<={1'b0,bg}+{2'b0,sh2};
  end
 end endgenerate
 always @(posedge clk)begin
  if(reset)begin stopped<=0;used<=0;count<=0;valid<=0;rd<=0;wr<=0;end
  else begin
   if(abort||preflight_bad)stopped<=1;
   if(advance)begin
    valid<={valid[2:0],in_accept};g0<=in_g;g1<=g0;
    case({in_accept,out_fire})
     2'b10:used<=used+1'b1;2'b01:used<=used-1'b1;
    endcase
    case({publish,out_fire})
     2'b10:count<=count+1'b1;2'b01:count<=count-1'b1;
    endcase
    if(publish)begin result_rows[wr]<=color;wr<=wr+1'b1;end
    if(out_fire)rd<=rd+1'b1;
   end
  end
 end
endmodule

module gpu_v2_final_compact_mul9(input wire clk,ce,input wire [8:0] a,
 input wire [7:0] b,output wire [16:0] product);
 @MULTIPLIER@
endmodule
