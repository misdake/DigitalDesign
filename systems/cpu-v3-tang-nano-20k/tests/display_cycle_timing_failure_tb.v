`timescale 1ns/1ps
module tb;
reg pixel_clock=0,boot_enable=0,boot_failed=0,memory_error_sticky=0,underflow_sync=1;
wire[1:0]enable_sync,failure_sync;
FailureLeaf dut(.*);
integer i;
reg[1:0]expected;
initial begin
 for(i=0;i<11;i=i+1)begin
  boot_failed=i==3||i==4;memory_error_sticky=i==7||i==8;
  case(i)3,7:expected=1;4,8:expected=3;5,9:expected=2;default:expected=0;endcase
  #1;pixel_clock=1;#1;
  if(failure_sync!==expected)$fatal(1,"FAILURE_PIPE edge=%0d expected=%h actual=%h",i,expected,failure_sync);
  $display("FAILURE %0d %h",i,failure_sync);
  pixel_clock=0;#1;
 end
 $display("PASS FAILURE");$finish;
end
initial begin #100;$fatal(1,"failure leaf edge cap");end
endmodule
