`timescale 1ns/1ps
module tb;
`ifndef PORTABLE
 GSR GSR(.GSRI(1'b1));
`endif
 reg clk=0;always #5 clk=~clk;
 reg reset=1,ce=1,we=0,re=0;reg[3:0]wa=0;reg[6:0]ra=0;reg[16:0]wd=0;
 wire[17:0]q;
 iz_operand_table dut(clk,reset,ce,we,wa,wd,re,ra,q);
 integer i,cycle=0;reg[17:0]expected,held;
 always @(posedge clk)begin cycle=cycle+1;if(cycle>600)$fatal(1,"operand table cycle bound");end
 task tick;begin @(posedge clk);#1;@(negedge clk);end endtask
 task read_row;input[6:0]address;input[17:0]value;begin
  ra=address;re=1;tick();if(q!==value)$fatal(1,"table row%0d got%h want%h",address,q,value);re=0;
 end endtask
 initial begin
  tick();reset=0;
  for(i=0;i<32;i=i+1)begin
   expected=0;if(i<=17)expected=18'd131072>>i;
   read_row(32+i,expected);
  end
  for(i=0;i<16;i=i+1)begin
   case(i%8)0:expected=32;1:expected=64;2:expected=128;3:expected=256;4:expected=512;default:expected=32;endcase
   if(i>=8)expected=18'd0-expected;
   read_row(64+i,expected);
  end
  read_row(80,30);read_row(81,18'h3ffff);
  for(i=0;i<16;i=i+1)begin
   expected=18'd128>>(i%8);if(i>=8)expected=18'd0-expected;
   read_row(96+i,expected);
  end
  // Exercise every data row and both physical nine-bit write lanes.
  for(i=0;i<16;i=i+1)begin wa=i;wd=17'h10001+i*513;we=1;tick();end
  we=0;
  for(i=0;i<16;i=i+1)read_row(i,18'h10001+i*513);
  held=q;ce=0;we=1;wa=0;wd=0;re=1;ra=80;tick();
  if(q!==held)$fatal(1,"CE hold");ce=1;we=0;read_row(0,18'h10001);
  held=q;ra=81;re=0;tick();if(q!==held)$fatal(1,"read hold");
  reset=1;we=1;wa=1;wd=0;re=1;tick();if(q!==0)$fatal(1,"reset Q");
  reset=0;we=0;read_row(1,18'h10001+513);read_row(81,18'h3ffff);
  $display("PASS operand table constants, data, CE, read hold and reset cycles=%0d",cycle);$finish;
 end
 initial begin #6100;$fatal(1,"operand table absolute bound");end
endmodule
