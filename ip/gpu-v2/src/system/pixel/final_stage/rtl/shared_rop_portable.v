module gpu_v2_final_rop_bank(input wire clk,reset,rop_ce,spec_ce,input wire[26:0]rop_a,input wire[7:0]rop_b,
 input wire[8:0]spec_a,input wire[23:0]spec_b,output wire[53:0]rop_product,output wire[50:0]spec_product);
 // Both clients advance together. Spec is consumed before the phase1 ROP
 // overwrite; ROP is consumed before the next phase0 spec overwrite.
 genvar c;generate for(c=0;c<3;c=c+1)begin:lanes
 wire[8:0]a=rop_ce?rop_a[c*9+:9]:spec_a;
 wire[7:0]b=rop_ce?rop_b:spec_b[c*8+:8];wire[17:0]raw;
 assign rop_product[c*18+:18]=raw;assign spec_product[c*17+:17]=raw[16:0];
 reg[17:0]p;assign raw=p;always @(posedge clk)if(rop_ce||spec_ce)begin if(rop_ce)p<=$signed(a)*$signed({1'b0,b});else p<=a*b;end
 end endgenerate
endmodule
