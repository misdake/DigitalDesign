`timescale 1ns/1ps
module tb;
reg write_clock=0,write_enable=0;
reg [1:0] write_address=0,read_address=0;
reg [47:0] write_data=0;
wire [47:0] read_data;
DisplayPairFifo dut(.*);
always #5 write_clock=~write_clock;
initial begin repeat(50) @(posedge write_clock); $fatal(1,"pair memory watchdog"); end
reg [47:0] expected [0:3];
integer i,j;
initial begin
 for(i=0;i<4;i=i+1) begin
  expected[i]=48'h1023456789ab ^ (48'h814020100804*i);
  @(negedge write_clock); write_enable=1; write_address=i; write_data=expected[i];
  @(posedge write_clock); #1;
 end
 @(negedge write_clock); write_enable=0;
 // Read selection is asynchronous, including while a different slot is written.
 for(i=0;i<4;i=i+1) begin
  read_address=i; #1;
  if(read_data!==expected[i]) $fatal(1,"pair memory read %0d",i);
 end
 for(i=0;i<4;i=i+1) begin
  @(negedge write_clock); write_enable=1; write_address=i;
  expected[i]=~expected[i]; write_data=expected[i]; read_address=(i+1)%4;
  #1; if(read_data!==expected[(i+1)%4]) $fatal(1,"independent read/write addresses");
  @(posedge write_clock); #1;
  for(j=0;j<4;j=j+1) begin
   read_address=j; #1;
   if(read_data!==expected[j]) $fatal(1,"pair memory overwrite %0d read %0d",i,j);
  end
 end
 $display("DIGITAL_DESIGN_PASS"); $finish;
end
endmodule
