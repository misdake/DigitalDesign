// Observe the real production cold gate and CPU bus; never force a GPU reset.
integer gpu_count_sends=0,gpu_base_sends=0,gpu_reads=0;
reg gpu_cold_released=0;
always @(posedge clk)begin
 if(start_cycle>3&&!dut.cold_ready)begin
  if(dut.u_gpu.core.busy!==0||dut.gpu_ro_memory_request_valid!==0)
   $fatal(1,"GPU lacks defined state during production cold start");
 end
 if(dut.cold_ready)gpu_cold_released=1;
 if(gpu_cold_released&&dut.u_gpu.core.reset!==0)
  $fatal(1,"GPU cold initialization reasserted at runtime");
 if(dut.device_write_enable&&dut.device_index==4)begin
  if(!dut.boot_done)$fatal(1,"Device4 write before boot release");
  if(dut.device_channel==0)gpu_count_sends=gpu_count_sends+1;
  if(dut.device_channel==1)gpu_base_sends=gpu_base_sends+1;
 end
 if(dut.gpu_ro_memory_request_valid&&dut.gpu_ro_memory_request_ready)gpu_reads=gpu_reads+1;
 if(released&&start_cycle-release_cycle>10000)$fatal(1,"Device4 completion bound after boot");
end
task gpu_cold_complete;
 integer result_file;
 begin
  if(gpu_count_sends!=1||gpu_base_sends!=1||gpu_reads!=1||dut.u_gpu.core.busy!==0)
   $fatal(1,"Production CPU/Device4 command completion");
  result_file=$fopen("__GPU_COLD_RESULT_PATH__","w");
  if(result_file==0)$fatal(1,"Cannot save GPU cold result");
  $fdisplay(result_file,"{\"count_sends\":%0d,\"base_sends\":%0d,\"reads\":%0d,\"release_cycle\":%0d,\"end_cycle\":%0d}",gpu_count_sends,gpu_base_sends,gpu_reads,release_cycle,start_cycle);
  $fclose(result_file);
  $display("GPU_COLD_PASS count=%0d base=%0d reads=%0d",gpu_count_sends,gpu_base_sends,gpu_reads);
 end
endtask
