`timescale 1ns/1ps
// Real production GPU; a bounded memory peer and CPU markers, no CPU timing claim.
module tb;
reg clk=0,reset=1,force_ce0=0;
wire ce=!force_ce0;
always #5 clk=~clk;
integer cycle=0;
always @(negedge clk)cycle<=cycle+1;
GSR GSR(.GSRI(1'b1));
reg[2:0]di=4;reg[3:0]ch=0;reg wen=0;reg[15:0]wdata=0;
wire busy,roq;wire[21:0]roa;wire[1:0]roc;
reg[63:0]memory[0:2047];
reg active=0;integer address=0,beat=0,beats=0;
wire ror=!active&&cycle%5!=0;
wire rov=active&&cycle%3!=0,rol=beat==beats-1;
wire[63:0]rod=memory[(address>>2)+beat];
always @(posedge clk)if(reset)active<=0;else begin
 if(roq&&ror)begin active<=1;address<=roa;beat<=0;beats<=4*(roc+1);end
 else if(rov)begin if(rol)active<=0;else beat<=beat+1;end
end
gpu_v2_vliw_command gpu(.clk(clk),.reset(reset),.ce(ce),.abort(1'b0),
 .device_index(di),.device_channel(ch),.device_read_enable(1'b0),
 .device_write_enable(wen),.device_write_data(wdata),.busy(busy),.output_ready(1'b1),
 .ro_request_valid(roq),.ro_request_ready(ror),.ro_address(roa),.ro_count(roc),
 .ro_response_valid(rov),.ro_response_last(rol),.ro_response_error(1'b0),.ro_data(rod),
 .fb_r_request_ready(1'b1),.fb_r_response_valid(1'b0),.fb_r_response_last(1'b0),
 .fb_r_response_error(1'b0),.fb_r_data(64'd0),
 .fb_w_request_ready(1'b1),.fb_w_data_ready(1'b1),.fb_w_response_valid(1'b0),
 .fb_w_response_last(1'b0),.fb_w_response_error(1'b0));
task tick;begin @(negedge clk);#1;end endtask
task send;input[3:0]channel;input[15:0]value;begin
 ch=channel;wdata=value;wen=1;tick;wen=0;
end endtask
integer iteration;
initial begin
 $readmemh("commands.hex",memory);
 tick;tick;reset=0;
 profiler_cpu_begin(1,1,"Prepare A");
 repeat(3)tick;
 profiler_cpu_end(1);
 for(iteration=0;iteration<3;iteration=iteration+1)begin
  while(busy)tick;
  force_ce0=1;
  send(0,3);send(1,iteration==1?16'h40:16'h20);
  repeat(3)begin tick;if(!busy||gpu.processor.state!=1)$fatal(1,"CE0 submit lost");end
  force_ce0=0;
  if(iteration<2)begin
   profiler_cpu_begin(iteration+2,iteration+2,iteration==0?"Prepare B":"Prepare A again");
   repeat(8)tick;
   profiler_cpu_end(iteration+2);
  end
  while(busy)tick;
  if(gpu.fault)$fatal(1,"GPU fault");
  tick;
 end
 $fatal(1,"Observer failed to stop at requested submission count");
end
// GPU_PROFILER_OBSERVER
endmodule
