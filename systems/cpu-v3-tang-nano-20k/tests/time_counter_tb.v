module tb;
reg clk=0,reset=1;
always #5 clk=~clk;
reg [3:0] channel=10;
reg write_enable=0,done=0;
reg [15:0] watch_data=0;
wire [15:0] exact_ms,fast_ms;
wire held;
__EXACT__ exact(.clk(clk),.reset(reset),.device_index(3'd0),.device_channel(channel),
 .device_read_enable(1'b1),.device_write_enable(write_enable),.device_write_data(16'd0),
 .dcache_maintenance_ready(1'b1),.dcache_maintenance_done(done),.dcache_maintenance_error(1'b0),
 .watch_read_data(watch_data),.device_read_data(exact_ms));
__FAST__ fast(.clk(clk),.reset(reset),.device_index(3'd0),.device_channel(channel),
 .device_read_enable(1'b1),.device_write_enable(write_enable),.device_write_data(16'd0),
 .dcache_maintenance_ready(1'b1),.dcache_maintenance_done(done),.dcache_maintenance_error(1'b0),
 .watch_read_data(watch_data),.device_read_data(fast_ms),.cpu_hold(held));
integer cycle;
initial begin #2000000; $fatal(1,"timer watchdog"); end
initial begin
 @(posedge clk);#1;if(exact_ms!==0 || fast_ms!==0)$fatal(1,"reset");
 for(cycle=1;cycle<=131080;cycle=cycle+1)begin
  @(negedge clk);reset=0;
  write_enable=(cycle==100 || cycle==600 || cycle==1000);
  channel=(cycle==100)?4:((cycle==600)?7:10);
  done=cycle==500; watch_data=cycle>=900?1:0;
  @(posedge clk);#1;
  if(channel==10 && (exact_ms!==cycle/54000 || fast_ms!==((cycle/2)&65535)))$fatal(1,"tick %0d",cycle);
  if(((cycle>=100 && cycle<500)||(cycle>=600 && cycle<900)) && !held)$fatal(1,"missing hold");
 end
 @(negedge clk);reset=1;@(posedge clk);#1;
 if(exact_ms!==0 || fast_ms!==0)$fatal(1,"restart reset");
 @(negedge clk);reset=0;@(posedge clk);#1;
 if(exact_ms!==0 || fast_ms!==0)$fatal(1,"divider reset");
 $display("TIME_PASS exact division, wrap, maintenance/watch hold, read-only, reset");$finish;
end
endmodule
