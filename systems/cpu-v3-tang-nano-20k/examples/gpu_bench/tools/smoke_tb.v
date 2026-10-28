`timescale 1ns/1ps
// Functional, independent delayed memory peer. These cycles are NOT board timing.
module tb;
reg clk=0,reset=1;
always #5 clk=~clk;
integer cycle=0;
always @(posedge clk)begin
 cycle<=cycle+1;
 if(cycle>=3000000)$fatal(1,"benchmark smoke cycle limit");
end
GSR GSR(.GSRI(1'b1));
reg[63:0]memory[0:1048575];
reg[3:0]channel=0;reg wen=0;reg[15:0]value=0;
wire busy,fault;
wire rq,frq,fwq;wire[21:0]ra,fra,fwa;wire[1:0]rc,frc,fwc;wire[63:0]fwd;
reg active=0,ractive=0,wactive=0,wresponse=0;
integer addr=0,beat=0,beats=0,raddr=0,rbeat=0,rbeats=0,waddr=0,wbeat=0,wbeats=0;
wire rv=active&&cycle%5!=0;
wire frv=ractive&&cycle%7!=0;
wire wready=wactive&&cycle%6!=0;
always @(posedge clk)begin
 wresponse<=0;
 if(!reset)begin
  if(rq&&!active)begin active<=1;addr<=ra>>2;beat<=0;beats<=4*(rc+1);end
  else if(rv)begin if(beat==beats-1)active<=0;else beat<=beat+1;end
  if(frq&&!ractive)begin ractive<=1;raddr<=fra>>2;rbeat<=0;rbeats<=4*(frc+1);end
  else if(frv)begin if(rbeat==rbeats-1)ractive<=0;else rbeat<=rbeat+1;end
  if(fwq&&!wactive)begin wactive<=1;waddr<=fwa>>2;wbeat<=0;wbeats<=4*(fwc+1);end
  else if(wready)begin
   memory[waddr+wbeat]<=fwd;
   if(wbeat==wbeats-1)begin wactive<=0;wresponse<=1;end else wbeat<=wbeat+1;
  end
 end
end
gpu_v2_vliw_command gpu(.clk(clk),.reset(reset),.ce(1'b1),.abort(1'b0),
 .device_index(3'd4),.device_channel(channel),.device_read_enable(1'b0),
 .device_write_enable(wen),.device_write_data(value),.busy(busy),.fault(fault),.output_ready(1'b1),
 .ro_request_valid(rq),.ro_request_ready(!active),.ro_address(ra),.ro_count(rc),
 .ro_response_valid(rv),.ro_response_last(beat==beats-1),.ro_response_error(1'b0),.ro_data(memory[addr+beat]),
 .fb_r_request_valid(frq),.fb_r_request_ready(!ractive),.fb_r_address(fra),.fb_r_count(frc),
 .fb_r_response_valid(frv),.fb_r_response_last(rbeat==rbeats-1),.fb_r_response_error(1'b0),.fb_r_data(memory[raddr+rbeat]),
 .fb_w_request_valid(fwq),.fb_w_request_ready(!wactive),.fb_w_address(fwa),.fb_w_count(fwc),
 .fb_w_data(fwd),.fb_w_data_ready(wready),.fb_w_response_valid(wresponse),.fb_w_response_last(1'b1),.fb_w_response_error(1'b0));
task tick;begin @(negedge clk);#1;end endtask
task submit;input integer base,count;begin
 channel=0;value=count-1;wen=1;tick;
 channel=1;value=base/128;tick;wen=0;
 tick;while(busy)tick;
 if(fault)$fatal(1,"production GPU fault");
end endtask
integer setup_base,setup_count,work_base,work_count,color_base,expected_pixels,i,nonzero;
reg[15:0]pixel;
initial begin
 if(!$value$plusargs("SETUP_BASE=%d",setup_base)||!$value$plusargs("SETUP_COUNT=%d",setup_count)||
    !$value$plusargs("WORK_BASE=%d",work_base)||!$value$plusargs("WORK_COUNT=%d",work_count)||
    !$value$plusargs("COLOR_BASE=%d",color_base)||!$value$plusargs("EXPECTED_PIXELS=%d",expected_pixels))
  $fatal(1,"explicit case parameters required");
 $readmemh("bench.hex",memory);
 for(i=0;i<24000;i=i+1)memory[color_base/8+i]=0;
 tick;tick;reset=0;tick;
 if(setup_count>0)submit(setup_base,setup_count);
 submit(work_base,work_count);
 nonzero=0;
 for(i=0;i<24000;i=i+1)begin
  if(memory[color_base/8+i][15:0]!=0)nonzero=nonzero+1;
  if(memory[color_base/8+i][31:16]!=0)nonzero=nonzero+1;
  if(memory[color_base/8+i][47:32]!=0)nonzero=nonzero+1;
  if(memory[color_base/8+i][63:48]!=0)nonzero=nonzero+1;
 end
 if(expected_pixels>=0&&nonzero!=expected_pixels)$fatal(1,"coverage mismatch actual=%0d expected=%0d",nonzero,expected_pixels);
 $display("PASS pixels=%0d functional_cycles=%0d",nonzero,cycle);
 $finish;
end
endmodule
