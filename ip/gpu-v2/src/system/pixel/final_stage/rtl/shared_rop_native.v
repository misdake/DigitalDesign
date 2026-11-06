module gpu_v2_final_rop_bank(input wire clk,reset,rop_ce,spec_ce,input wire[26:0]rop_a,input wire[7:0]rop_b,
 input wire[8:0]spec_a,input wire[23:0]spec_b,output wire[53:0]rop_product,output wire[50:0]spec_product);
 // Both clients advance together. Spec is consumed before the phase1 ROP
 // overwrite; ROP is consumed before the next phase0 spec overwrite.
 genvar c;generate for(c=0;c<3;c=c+1)begin:lanes
 wire[8:0]a=rop_ce?rop_a[c*9+:9]:spec_a;
 wire[7:0]b=rop_ce?rop_b:spec_b[c*8+:8];wire[17:0]raw;
 assign rop_product[c*18+:18]=raw;assign spec_product[c*17+:17]=raw[16:0];
 MULT9X9 #(.AREG(1'b0),.BREG(1'b0),.PIPE_REG(1'b0),.OUT_REG(1'b1),.ASIGN_REG(1'b0),.BSIGN_REG(1'b0),.SOA_REG(1'b0),.MULT_RESET_MODE("SYNC")) dsp(
 .DOUT(raw),.SOA(),.SOB(),.A(a),.B({1'b0,b}),.SIA(9'd0),.SIB(9'd0),.ASEL(1'b0),.BSEL(1'b0),.ASIGN(rop_ce),.BSIGN(1'b0),.CLK(clk),.CE(rop_ce||spec_ce),.RESET(1'b0));
 end endgenerate
endmodule
