// Independent two-edge tap-group owner: six MULT9X9 instead of twelve.
// One group contains four taps; an eight-tap trilinear pixel needs two groups.
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
 reg busy,done,p_first,p_last,q_done,q_last;
 reg[5:0]p_key,q_key;
 reg[4:0]credits,f_count;
 reg[3:0]f_head,f_tail;
 
 
 wire[7:0]pair_sum[0:2],sumv[0:2];
 wire[31:0]result_do;
 assign in_ready=proposal_ce&&!fault&&busy&&(!last_offer||credits<5'd16);
 wire accepted=in_valid&&in_ready;
 wire start=in_valid&&proposal_ce&&!fault&&!busy&&(!last_offer||credits<5'd16);
 wire do_pop=out_ready&&(f_count!=0),do_push=q_done&&q_last;
 wire[4:0]count_pre_push=f_count-(do_pop?5'd1:5'd0);
 wire[4:0]count_next=f_count+(do_push?5'd1:5'd0)-(do_pop?5'd1:5'd0);
 wire[4:0]credits_next=credits+((accepted&&last_in)?5'd1:5'd0)-(do_pop?5'd1:5'd0);
 wire pending_last=done&&p_last;
 wire[4:0]expected_credits=f_count+(do_push?5'd1:5'd0)+(pending_last?5'd1:5'd0);
 wire packet_bad=accepted&&(in_payload[7:4]>4'd10);
 wire fault_now=packet_bad||(do_push&&count_pre_push==5'd16)||(credits!=expected_credits);
 assign preflight_bad=fault_now;
 wire advance=ce&&!fault&&!fault_now;
 wire mul_ce=advance&&(accepted||start);
 wire result_write=!reset&&advance&&do_push;
 wire[31:0]result_write_data={2'b0,q_key,sumv[2],sumv[1],sumv[0]};
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
`ifdef GPU_V2_GOWIN_DSP
 genvar q;generate for(q=0;q<8;q=q+1)begin:result_bank
  RAM16SDP4 ram(.CLK(clk),.WRE(result_write),.WAD(f_tail),.RAD(f_head),.DI(result_write_data[q*4+:4]),.DO(result_do[q*4+:4]));
 end endgenerate
`else
 reg[31:0]result_ram[0:15];assign result_do=result_ram[f_head];
 always@(posedge clk)if(result_write)result_ram[f_tail]<=result_write_data;
`endif
 integer k;
 always@(posedge clk)begin
  if(reset)begin busy<=0;done<=0;q_done<=0;credits<=0;f_count<=0;f_head<=0;f_tail<=0;fault<=0;end
  else if(ce&&!fault)begin
   if(fault_now)fault<=1;
   else begin
    busy<=start;done<=accepted;q_done<=done;
    if(done)begin q_key<=p_key;q_last<=p_last;end
    if(accepted)begin
     p_key<=key_in;p_first<=first_in;p_last<=last_in;
    end

    if(do_pop)f_head<=f_head+1'b1;
    if(do_push)f_tail<=f_tail+1'b1;
    f_count<=count_next;credits<=credits_next;
   end
  end
 end
 assign out_valid=!fault&&(f_count!=0);
 assign out_key=result_do[29:24];assign out_r=result_do[7:0];assign out_g=result_do[15:8];assign out_b=result_do[23:16];
endmodule

module color_serial_registered_mul9(input wire clk,ce,input wire[8:0]a,input wire[7:0]b,output wire[16:0]product);
`ifdef GPU_V2_GOWIN_DSP
 wire[17:0]raw;
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b1),
 .ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(raw),.SOA(),.SOB(),.A(a),.B({1'b0,b}),.SIA(9'd0),.SIB(9'd0),
 .ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(1'b0));
 assign product=raw[16:0];
`else
 reg[16:0]q;always@(posedge clk)if(ce)q<=a*b;assign product=q;
`endif
endmodule

module color_packed_accumulator(input wire clk,ce,add,input wire[53:0]a,output wire[53:0]q);
`ifdef GPU_V2_GOWIN_DSP
 ALU54D #(.AREG(1'b0),.BREG(1'b0),.ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.ACCLOAD_REG(1'b0),
 .OUT_REG(1'b1),.B_ADD_SUB(1'b0),.C_ADD_SUB(1'b0),.ALUD_MODE(0),.ALU_RESET_MODE("SYNC")) alu(
 .DOUT(q),.CASO(),.A(a),.B(54'd0),.CASI(55'd0),.ACCLOAD(add),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(ce),.RESET(1'b0));
`else
 reg[53:0]storage;always@(posedge clk)if(ce)begin if(add)storage<=storage+a;else storage<=a;end
 assign q=storage;
`endif
endmodule
