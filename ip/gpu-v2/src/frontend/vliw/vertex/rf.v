module shared_rf(input clk,we,input [3:0] wa,aa,ba,input [31:0] wd,
 output [31:0] aq,bq);
`ifdef PORTABLE
 reg [31:0] mem[0:15]; integer i;
 initial for(i=0;i<16;i=i+1) mem[i]=0;
 assign aq=mem[aa]; assign bq=mem[ba];
 always @(posedge clk) if(we) mem[wa]<=wd;
`else
 genvar i;
 generate for(i=0;i<8;i=i+1) begin:lane
   RAM16SDP4 a(.DO(aq[4*i +: 4]),.DI(wd[4*i +: 4]),.WAD(wa),.RAD(aa),.WRE(we),.CLK(clk));
   RAM16SDP4 b(.DO(bq[4*i +: 4]),.DI(wd[4*i +: 4]),.WAD(wa),.RAD(ba),.WRE(we),.CLK(clk));
 end endgenerate
`endif
endmodule

