module gpu_v2_texture_runtime_color_checked(
 input wire clk,reset,ce,proposal_ce,
 output wire preflight_bad,input wire in_valid,input wire[71:0]in_payload,
 input wire[15:0]in_tex0,in_tex1,in_tex2,in_tex3,
`ifdef GPU_V2_COMMAND_PORTS
 input wire texture_rgba4444,output wire[7:0]out_a,
`endif
 output wire in_ready,input wire out_ready,output wire out_valid,
 output wire[5:0]out_key,output wire[7:0]out_r,out_g,out_b,
 output reg fault
);
// Packet validation below remains a functional gate in either policy.
`ifdef GPU_V2_VLIW_COLOR_COMMAND
`define GPU_V2_COLOR_CLEAR begin end
`ifndef SYNTHESIS
`define GPU_V2_COLOR_BAD begin if(!reset&&((1)!=0))$fatal(1,"GPU contract: fault");end
`else
`define GPU_V2_COLOR_BAD begin end
`endif
`else
`define GPU_V2_COLOR_CLEAR fault<=0;
`define GPU_V2_COLOR_BAD fault<=1;
`endif

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
 wire packet_bad=@PACKET_CHECK@
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
   wire[7:0]rgb565;
   if(c==0)assign rgb565={tex[15:11],tex[15:13]};
   if(c==1)assign rgb565={tex[10:5],tex[10:9]};
   if(c==2)assign rgb565={tex[4:0],tex[4:2]};
`ifdef GPU_V2_COMMAND_PORTS
   wire[3:0]nibble=tex[12-4*c+:4];
   assign col=texture_rgba4444?{nibble,nibble}:rgb565;
`else
   assign col=rgb565;
`endif
   color_serial_registered_mul9 multiplier(.clk(clk),.ce(mul_ce),.a(w),.b(col),.product(product[t]));
  end
  assign pair_sum[c]=product[0][16:9]+product[1][16:9]+{7'b0,product[0][8]}+{7'b0,product[1][8]};

 end endgenerate
`ifdef GPU_V2_COMMAND_PORTS
 // Conserved weights keep A4's raw sum in13 bits (511*15=7665).
 // RGB retains its original rounded pair8 values. Twelve-bit slots leave
 // ample guard space for every group and reuse the existing ALU54.
 // A4 needs four conditional shifts, not a new hard multiplier. Register
 // the pair sum once, at the same edge as the existing RGB tap products.
 wire[8:0]alpha_w0=busy?in_payload[46+:9]:in_payload[28+:9];
 wire[8:0]alpha_w1=busy?in_payload[55+:9]:in_payload[37+:9];
 wire[3:0]alpha_a0=busy?in_tex2[3:0]:in_tex0[3:0];
 wire[3:0]alpha_a1=busy?in_tex3[3:0]:in_tex1[3:0];
 // Conserved weights prove w0+w1<=511. Share that sum across all four
 // bit planes instead of rebuilding two independent nibble multipliers.
 wire[8:0]alpha_weights=alpha_w0+alpha_w1;
 function[8:0]alpha_bit;
  input a0,a1;input[8:0]w0,w1,wsum;
  begin alpha_bit=a1?(a0?wsum:w1):(a0?w0:9'd0);end
 endfunction
 wire[8:0]alpha_s[0:3];
 generate for(t=0;t<4;t=t+1)begin:alpha_bits
  assign alpha_s[t]=alpha_bit(alpha_a0[t],alpha_a1[t],alpha_w0,alpha_w1,alpha_weights);
 end endgenerate
 wire[10:0]alpha_lo={2'd0,alpha_s[0]}+{1'd0,alpha_s[1],1'b0};
 wire[10:0]alpha_hi={2'd0,alpha_s[2]}+{1'd0,alpha_s[3],1'b0};
 reg[12:0]alpha_pair;
 always@(posedge clk)if(mul_ce)alpha_pair<={2'd0,alpha_lo}+{alpha_hi,2'd0};
 wire[53:0]packed_half={1'b0,alpha_pair,8'd0,pair_sum[2],4'd0,pair_sum[1],4'd0,pair_sum[0]};
 wire[53:0]packed_low={5'd0,alpha_pair,36'd0};
`else
 wire[53:0]packed_half={6'd0,8'd0,pair_sum[2],8'd0,pair_sum[1],8'd0,pair_sum[0]};
`endif
 wire[53:0]packed_sum;
 wire accum_ce=advance&&(accepted||done);
 wire accum_add=!(accepted&&first_in);
 color_packed_accumulator accumulator(.clk(clk),.ce(accum_ce),.add(accum_add),.a(packed_half),
`ifdef GPU_V2_COMMAND_PORTS
 .b(packed_low),
`endif
 .q(packed_sum));
`ifdef GPU_V2_COMMAND_PORTS
 assign sumv[0]=packed_sum[7:0];assign sumv[1]=packed_sum[19:12];assign sumv[2]=packed_sum[31:24];
 // The existing ALU computes17*raw sum as A=(raw<<4) and B=raw.
 // For P<=130305, nearest P/511 is q+[r+q>=256], q=P>>9.
 // No fabric multiply-by17 or rounding-bias adder is needed.
 wire[9:0]alpha_tail={1'b0,packed_sum[44:36]}+{2'd0,packed_sum[52:45]};
 wire[7:0]alpha8=packed_sum[52:45]+{7'd0,(|alpha_tail[9:8])};
 assign out_a=texture_rgba4444?alpha8:8'd255;
`else
 assign sumv[0]=packed_sum[7:0];assign sumv[1]=packed_sum[23:16];assign sumv[2]=packed_sum[39:32];
`endif
 always@(posedge clk)begin
  if(reset)begin busy<=0;done<=0;q_done<=0;`GPU_V2_COLOR_CLEAR end
  else if(ce&&!fault)begin
   if(packet_bad)`GPU_V2_COLOR_BAD
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
`ifdef GPU_V2_VLIW_COLOR_COMMAND
 always @(posedge clk)fault<=0;
`endif
`undef GPU_V2_COLOR_CLEAR
`undef GPU_V2_COLOR_BAD
endmodule
