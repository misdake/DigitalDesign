module gpu_v2_texture_runtime_color_checked(
 input wire clk,reset,ce,proposal_ce,
 output wire preflight_bad,input wire in_valid,input wire[71:0]in_payload,
 input wire[15:0]in_tex0,in_tex1,in_tex2,in_tex3,
 output wire in_ready,input wire out_ready,output wire out_valid,
 output wire[5:0]out_key,output wire[7:0]out_r,out_g,out_b,
 output reg fault
);
 wire[5:0]key_in={in_payload[69:66],in_payload[71:70]};
 wire first_in=in_payload[64],last_in=in_payload[65];
 wire last_offer=in_valid?last_in:1'b1;
 // Closed VLIW parent writes every completed pixel to its final bank.
 reg busy,done,p_last,q_done,q_last;
 reg[5:0]p_key,q_key;
 wire[7:0]pair_sum[0:2],sumv[0:2];
 assign in_ready=proposal_ce&&!fault&&busy;
 wire accepted=in_valid&&in_ready;
 wire start=in_valid&&proposal_ce&&!fault&&!busy;
 wire packet_bad=accepted&&(in_payload[7:4]>4'd10);
 assign preflight_bad=packet_bad;
 wire advance=ce&&!fault&&!packet_bad;
 wire mul_ce=advance&&(accepted||start);
 wire[15:0]low_tex[0:1];assign low_tex[0]=in_tex0;assign low_tex[1]=in_tex1;
 genvar c,t;
 generate for(c=0;c<3;c=c+1)begin:channels
  wire[16:0]product[0:1];
  for(t=0;t<2;t=t+1)begin:taps
   wire[8:0]w=busy?in_payload[46+9*t+:9]:in_payload[28+9*t+:9];
   wire[15:0]tex=busy?(t==0?in_tex2:in_tex3):low_tex[t];
   wire[7:0]col;
   if(c==0)assign col={tex[15:11],tex[15:13]};
   if(c==1)assign col={tex[10:5],tex[10:9]};
   if(c==2)assign col={tex[4:0],tex[4:2]};
   color_serial_registered_mul9 multiplier(.clk(clk),.ce(mul_ce),.a(w),.b(col),.product(product[t]));
  end
  assign pair_sum[c]=product[0][16:9]+product[1][16:9]+{7'b0,product[0][8]}+{7'b0,product[1][8]};

 end endgenerate
 wire[53:0]packed_half={6'd0,8'd0,pair_sum[2],8'd0,pair_sum[1],8'd0,pair_sum[0]};
 wire[53:0]packed_sum;
 wire accum_ce=advance&&(accepted||done);
 wire accum_add=!(accepted&&first_in);
 color_packed_accumulator accumulator(.clk(clk),.ce(accum_ce),.add(accum_add),.a(packed_half),.q(packed_sum));
 assign sumv[0]=packed_sum[7:0];assign sumv[1]=packed_sum[23:16];assign sumv[2]=packed_sum[39:32];
 always@(posedge clk)begin
  if(reset)begin busy<=0;done<=0;q_done<=0;fault<=0;end
  else if(ce&&!fault)begin
   if(packet_bad)fault<=1;
   else begin
    busy<=start;done<=accepted;q_done<=done;
    if(done)begin q_key<=p_key;q_last<=p_last;end
    if(accepted)begin p_key<=key_in;p_last<=last_in;end
   end
  end
 end
 // Consume the old accumulator Q before any next-group update on this edge.
 assign out_valid=!fault&&q_done&&q_last;
 assign out_key=q_key;assign out_r=sumv[0];assign out_g=sumv[1];assign out_b=sumv[2];
`ifndef SYNTHESIS
 always@(posedge clk)if(!reset&&advance&&out_valid&&!out_ready)
  $fatal(1,"closed Color result bank did not accept completion");
`endif
endmodule
