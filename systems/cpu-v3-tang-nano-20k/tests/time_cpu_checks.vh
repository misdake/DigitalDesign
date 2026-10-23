integer active_edges=0,time_reads=0,maintenance_ticks=0,watch_ticks=0,dwait_ticks=0;
integer previous_ms=0;
reg [15:0] last_time_read=0;
always @(posedge clk)begin
 if(reset)begin active_edges=0;time_reads=0;end
 else begin
  if(sysctl.milliseconds!==((active_edges/32)&65535))$fatal(1,"CPU wall-clock drift");
  if(device_read_enable && device_index==0 && device_channel==10)begin
   if(device_read_data!==((active_edges/32)&65535))$fatal(1,"RECV value");
   last_time_read=device_read_data;time_reads=time_reads+1;
  end
  if(sysctl.milliseconds!=previous_ms)begin
   if(held && !watch_enable)maintenance_ticks=maintenance_ticks+1;
   if(watch_enable)watch_ticks=watch_ticks+1;
   if(core.state==2 && core.instruction==16'h6e80 && !copy_ready)dwait_ticks=dwait_ticks+1;
  end
  previous_ms=sysctl.milliseconds;
  active_edges=active_edges+1;
 end
end
integer index;
initial begin
 for(index=0;index<256;index=index+1)rom[index]=16'h6c00;
 for(index=0;index<65536;index=index+1)memory[index]=16'h55aa;
 __PROGRAM__
 repeat(3)@(posedge clk);@(negedge clk);reset=0;
 wait(halted||fault);#1;
 if(fault || halt_signal!==last_time_read || time_reads!=5)$fatal(1,"CPU RECV/retire");
 if(maintenance_ticks==0 || watch_ticks==0 || dwait_ticks==0)$fatal(1,"hold coverage maintenance=%0d watch=%0d dwait=%0d",maintenance_ticks,watch_ticks,dwait_ticks);
 if(memory[16'h4000]!==16'h1234)$fatal(1,"cache maintenance regression");
 $display("TIME_PASS real CPU/cache RECV10 reads=%0d, held ticks=%0d/%0d/%0d",time_reads,maintenance_ticks,watch_ticks,dwait_ticks);$finish;
end
endmodule
