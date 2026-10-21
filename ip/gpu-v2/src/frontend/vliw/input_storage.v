// Positions have a separate bank. T coefficients skip the V N36/attr32 rows
// at 192..319. The two consumers retain independent synchronous Qs.
// 每bank独立1W1R，Vertex与T原写地址和数据保持；读沿之后各Q独立保持。
module rp_results_split(input clk,reset,ce,
 input position_we,export_we,input[8:0]wa,input[35:0]wd,
 input position_re,input[8:0]position_ra,input export_re,input[8:0]export_ra,
 output[35:0]position_q,export_q);
 iz_ram36 positions(clk,reset,ce,position_we,wa,wd,position_re,position_ra,position_q);
 iz_ram36 exported(clk,reset,ce,export_we,wa,wd,export_re,export_ra,export_q);
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset)begin
  if(position_we&&wa[8:6]>=3)$fatal(1,"位置写超出plane0..2");
  if(position_re&&position_ra[8:6]>=3)$fatal(1,"T位置读超出plane0..2");
 end
`endif
endmodule

// Immutable queue heads expose refs throughout export without a second copy.
module rp_triangle_descriptors #(parameter AB=4)(input clk,we,input[AB-1:0]wa,input[35:0]wd,
 input[AB-1:0]ra,output[35:0]q);
`ifdef PORTABLE
 reg[35:0]mem[0:(1<<AB)-1];
 always @(posedge clk)if(we)mem[wa]<=wd;
 assign q=mem[ra];
`else
 genvar i;
 generate for(i=0;i<9;i=i+1)begin:lanes
  if(AB<=4)begin:single
   RAM16SDP4 ram(.DO(q[i*4+:4]),.DI(wd[i*4+:4]),
    .WAD({{(4-AB){1'b0}},wa}),.RAD({{(4-AB){1'b0}},ra}),.WRE(we),.CLK(clk));
  end else begin:double
   wire[3:0]lo,hi;
   RAM16SDP4 a(.DO(lo),.DI(wd[i*4+:4]),.WAD(wa[3:0]),.RAD(ra[3:0]),.WRE(we&&!wa[4]),.CLK(clk));
   RAM16SDP4 b(.DO(hi),.DI(wd[i*4+:4]),.WAD(wa[3:0]),.RAD(ra[3:0]),.WRE(we&&wa[4]),.CLK(clk));
   assign q[i*4+:4]=ra[4]?hi:lo;
  end
 end endgenerate
`endif
endmodule
