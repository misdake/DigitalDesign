module tb;
reg clk=0,reset=1;
always #5 clk=~clk;
reg [2:0] index=0;
reg [3:0] channel=11;
reg read_enable=0,write_enable=0,done=0;
reg [15:0] data=0,watch_data=0;
wire [15:0] exact_value,fast_value;
wire held;
__EXACT__ exact(.clk(clk),.reset(reset),.device_index(index),.device_channel(channel),
 .device_read_enable(read_enable),.device_write_enable(write_enable),.device_write_data(data),
 .dcache_maintenance_ready(1'b1),.dcache_maintenance_done(done),.dcache_maintenance_error(1'b0),
 .watch_read_data(watch_data),.device_read_data(exact_value));
__FAST__ fast(.clk(clk),.reset(reset),.device_index(index),.device_channel(channel),
 .device_read_enable(read_enable),.device_write_enable(write_enable),.device_write_data(data),
 .dcache_maintenance_ready(1'b1),.dcache_maintenance_done(done),.dcache_maintenance_error(1'b0),
 .watch_read_data(watch_data),.device_read_data(fast_value),.cpu_hold(held));
integer cycle,epoch=0,expected_exact,expected_fast;
initial begin #1000000;$fatal(1,"microsecond timer watchdog");end
initial begin
 #1;read_enable=1;
 @(posedge clk);#1;if(exact_value!==0 || fast_value!==0)$fatal(1,"reset exact=%h fast=%h",exact_value,fast_value);
 for(cycle=1;cycle<=67000;cycle=cycle+1)begin
  @(negedge clk);reset=0;index=0;channel=11;read_enable=1;write_enable=0;data=0;done=0;watch_data=0;
  // Clear exactly on the physical tick edge. Bit 0 alone authorizes clear.
  if(cycle==54 || cycle==108)begin write_enable=1;data=1;epoch=cycle;end
  if(cycle==109)begin write_enable=1;data=2;end
  if(cycle==110)begin index=1;write_enable=1;data=1;end
  if(cycle==300)begin channel=4;write_enable=1;end
  if(cycle==600)done=1;
  if(cycle==700)begin channel=7;write_enable=1;end
  if(cycle>=1000)watch_data=1;
  if(cycle>=400 && cycle<500)read_enable=0;
  @(posedge clk);#1;
  expected_exact=(cycle-epoch)/54;
  expected_fast=cycle-epoch;if(expected_fast>65535)expected_fast=65535;
  if(channel==11 && index==0 && read_enable &&
    (exact_value!==expected_exact || fast_value!==expected_fast))
   $fatal(1,"interval edge=%0d exact=%0d/%0d fast=%0d/%0d",cycle,exact_value,expected_exact,fast_value,expected_fast);
  if((index!=0 || !read_enable) && (exact_value!==0 || fast_value!==0))$fatal(1,"unselected read");
  if(((cycle>=300 && cycle<600)||(cycle>=700 && cycle<1000)) && !held)$fatal(1,"hold coverage");
 end
 // Saturation is not a wrap; an explicit clear restarts the complete divisor.
 if(fast_value!==65535)$fatal(1,"saturation");
 @(negedge clk);write_enable=1;data=3;
 @(posedge clk);#1;if(exact_value!==0 || fast_value!==0)$fatal(1,"restart clear");
 @(negedge clk);write_enable=0;
 repeat(53)@(posedge clk);#1;if(exact_value!==0)$fatal(1,"clear prescaler");
 @(posedge clk);#1;if(exact_value!==1)$fatal(1,"54-clock microsecond");
 @(negedge clk);reset=1;@(posedge clk);#1;
 if(exact_value!==0 || fast_value!==0)$fatal(1,"final reset");
 $display("TIME_PASS microseconds: 54-clock tick, clear priority/bit/index, saturation, reset, CPU holds and disabled reads");$finish;
end
endmodule

