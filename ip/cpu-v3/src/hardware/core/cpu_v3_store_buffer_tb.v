module tb;
reg clk=0;
always #5 clk=~clk;
reg we=0;
reg[1:0]wa=0,ra=0;
reg[31:0]wd=0;
wire[31:0]rd;
CpuV3StoreBuffer dut(clk,we,wa,wd,ra,rd);
reg[31:0]expected[0:3];
integer round,lane,read_lane;
initial begin
 for(round=0;round<4;round=round+1)begin
  for(lane=0;lane<4;lane=lane+1)begin
   @(negedge clk);we=1;wa=lane;ra=lane;
   wd=32'h932751af^(32'h10230507*round)^(32'h01010001*lane);
   #1;if(round!=0 && rd!==expected[lane])$fatal(1,"old data before write");
   @(posedge clk);expected[lane]=wd;
   #1;if(rd!==expected[lane])$fatal(1,"new data after write");
  end
  @(negedge clk);we=0;wd=~wd;
  for(read_lane=0;read_lane<4;read_lane=read_lane+1)begin
   ra=read_lane;#1;if(rd!==expected[read_lane])$fatal(1,"addressed read");
   @(negedge clk);
  end
 end
 $display("DIGITAL_DESIGN_PASS");$finish;
end
initial begin repeat(100)@(posedge clk);$fatal(1,"watchdog");end
endmodule
