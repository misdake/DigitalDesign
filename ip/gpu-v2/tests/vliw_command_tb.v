`timescale 1ns/1ps
module tb;
@PORTS@
reg[31:0]command_base=0,command_bytes=0;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
always #5 clk=~clk;
gpu_v2_vliw_command dut(.*);
reg[63:0]memory[0:8191];reg[35:0]vertices[0:44];
integer cycle=0,mode=0,bi=0,bn=0,ba=0,starts=0,done_count=0,vi=0,overlap=0,ce0=0;
integer bursts32=0,bursts64=0,bursts128=0,cachefills=0,material_changes=0,j;
reg active=0,force_ce0=0;
integer submission;
always @(negedge clk)begin
 ce=!force_ce0&&cycle%7!=2;ro_request_ready=!active&&cycle%5!=1;
 ro_response_valid=active&&cycle%3!=1;ro_data=memory[(ba>>2)+bi];
 ro_response_error=active&&((mode==1&&ba==@ERROR_ADDRESS@&&bi==0)||(mode==3&&bi==bn-1));
 ro_response_last=bi==bn-1;
 fb_r_request_ready=1;fb_w_request_ready=1;
end
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>12000)$fatal(1,"CP watchdog state%0d pc%0d error%0d",dut.processor.state,dut.processor.pc,device_read_data[1]);
 if(!reset)begin
  if(!ce)ce0=ce0+1;
  if(ro_request_valid&&ro_request_ready)begin
   if(active)$fatal(1,"multiple physical RO owners");
   active=1;ba=ro_address;bi=0;bn=ro_count==3?16:ro_count==1?8:4;
   case(ro_count)0:bursts32=bursts32+1;1:bursts64=bursts64+1;3:bursts128=bursts128+1;default:$fatal(1,"burst size");endcase
   if(ro_address<22'h1000)cachefills=cachefills+1;
  end else if(ro_response_valid)begin
   if(ro_response_last||ro_response_error)active=0;else bi=bi+1;
  end
  if(mode==2&&dut.input_active&&dut.work_busy)abort<=1;
  if(dut.input_active&&dut.work_busy)overlap=overlap+1;
  if(dut.start_fire)begin
   if(dut.slot!==(starts%2))$fatal(1,"pingpong slot");
   if((mode==0||mode==3)&&dut.draw_unlit!==(starts<2))$fatal(1,"material binding");
   starts=starts+1;
  end
  if(ce&&meshlet_done)begin done_count=done_count+1;if(published_vertices!=3)$fatal(1,"vertex count");end
  if(ce&&emit_valid&&output_ready)begin
   if((mode==0||mode==3)&&(vi>=45||emit_data!==vertices[vi]))$fatal(1,"V changed at%0d got%h want%h",vi,emit_data,vertices[vi]);
   vi=vi+1;
  end
  if(ce&&!dut.stop&&dut.processor.state==5&&
     dut.processor.object_index==(dut.processor.apply_render?1:2)&&
     (dut.processor.opcode==5||dut.processor.opcode==6))material_changes=material_changes+1;
  if(fb_r_request_valid||fb_w_request_valid||quad_admitted)$fatal(1,"backface fixture escaped to raster");
 end
end
task tick;begin @(negedge clk);#1;end endtask
initial begin
 j=$value$plusargs("MODE=%d",mode);$readmemh("memory.hex",memory);$readmemh("vertices.hex",vertices);
 reset=1;output_ready=1;command_base=32'h1000;command_bytes=@BYTES@;
 tick();tick();reset=0;
 for(submission=0;submission<(mode==0?2:1);submission=submission+1)begin
 starts=0;done_count=0;vi=0;
 force_ce0=1;
 submit_commands(command_base,command_bytes);
 repeat(3)begin tick();if(dut.processor.state!=1||!busy)$fatal(1,"CE0 launch was lost");end
 force_ce0=0;
 if(mode==2)begin
  while(!(dut.processor.state==10&&dut.dma_idle&&!dut.memory_busy))tick();
  if(!busy||!fault)$fatal(1,"abort falsely completed");
 end else while(busy)tick();
 if(mode==0||mode==3)begin
  if(device_read_data[1]!=(mode==3)||starts!=3||done_count!=3||vi!=45||overlap==0||cachefills<3||bursts32==0||bursts64==0||bursts128==0||material_changes<3)
   $fatal(1,"CP success conservation e%0d starts%0d done%0d V%0d overlap%0d cache%0d",device_read_data[1],starts,done_count,vi,overlap,cachefills);
 end else begin
  if(!fault||dut.processor.page_valid[1])$fatal(1,"failed DMA published a page or lost error");
 end
 repeat(5)begin tick();if((mode==0||mode==3)&&busy)$fatal(1,"busy reasserted after completion");end
 if(mode==0||mode==3)begin
  device_index=3;#1;if(device_read_data!==0)$fatal(1,"wrong device selected");
  device_index=4;device_channel=3;#1;if(device_read_data!==0)$fatal(1,"wrong channel selected");
  device_channel=2;#1;
  if(device_read_data!==(mode==3?16'd2:16'd0))$fatal(1,"RECV changed completion/error");
 end
 $display("PASS CP32 mode=%0d cycles=%0d starts=%0d V=%0d DMAoverlap=%0d cachefills=%0d CE0=%0d error=%0d",mode,cycle,starts,vi,overlap,cachefills,ce0,device_read_data[1]);
 end
 $finish;
end
endmodule
