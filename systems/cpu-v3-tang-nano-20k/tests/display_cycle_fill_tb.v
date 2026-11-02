`timescale 1ps/1ps
module tb;
reg clk=0,pixel_clock=0,serial_clock=0;
reg reset=0,video_locked=1,boot_enable=0,boot_failed=0;
reg memory_request_ready=0,memory_data_valid=0,memory_last=0,memory_error=0;
reg [63:0] memory_read_data=0;
reg [2:0] device_index=3;
reg [3:0] device_channel=3;
reg device_read_enable=1,device_write_enable=0;
reg [15:0] device_write_data=0;
wire memory_request_valid,memory_urgent,underflow,started_ack,startup_error,frame_event;
wire [21:0] memory_address;
wire [15:0] device_read_data;
wire tmds_clk_p,tmds_clk_n;
wire [2:0] tmds_data_p,tmds_data_n;
FramebufferHdmi dut(.*);
integer scenario=0,phase=0,fd,cycle=0,requests=0,writes=0,frames=0,blocked=0;
integer delay_count=0,beat=0,reset_at=0,epoch=0,reset_age=0;
reg busy=0,injected=0,swap2=0;
reg [21:0] held_address=0;
reg [2047:0] trace_path;
time logic_tick=37,pixel_tick;
initial forever begin
 #((logic_tick*64'd250000)/999-$time); clk=~clk; logic_tick=logic_tick+37;
end
initial begin
 if(!$value$plusargs("phase=%d",phase))phase=0;
 pixel_tick=60+phase;
 forever begin
  #((pixel_tick*64'd250000)/999-$time);pixel_clock=~pixel_clock;pixel_tick=pixel_tick+60;
 end
end
function [63:0] payload;
 input [21:0] address;
 input integer b;
 begin payload={32'hdead0000 ^ {10'b0,address},32'h12340000 ^ {10'b0,address}} ^ b; end
endfunction
reg [127:0] previous_input,previous_output,now_input,now_output;
integer run_count=0,samples=0,rows=0;
wire [127:0] sampled_input =
 {reset,boot_enable,boot_failed,memory_request_ready,memory_data_valid,
 memory_read_data,memory_last,memory_error,device_index,device_channel,
 device_read_enable,device_write_enable,device_write_data,dut.released,
 dut.frame_toggle,dut.underflow_sticky,dut.started};
wire [127:0] sampled_output =
 {memory_request_valid,memory_urgent,memory_address,dut.line_write,
 dut.line_write_address,(dut.line_write ? memory_read_data : 64'd0),dut.published,
 dut.group_valid,dut.group_linear,device_read_data,underflow,started_ack,startup_error,frame_event};
task flush_run;
 begin if(run_count!=0) begin
 $fwrite(fd,"E %0d %032h %032h\n",run_count,previous_input,previous_output);
 rows=rows+1;run_count=0;
 if(rows>150000)$fatal(1,"RLE row cap");
 end end
endtask
task finish_trace;
 integer a,query;
 begin
 flush_run;
 // Passive terminal reads qualify all Device3 channels without advancing time/state.
 for(query=0;query<5;query=query+1)begin
  device_channel=query;#0;
  $fwrite(fd,"F %032h %032h\n",sampled_input,sampled_output);
 end
 for(a=0;a<400;a=a+1)
 $fwrite(fd,"M %0d %08h %08h\n",a,dut.u_line_buffer.memory_a[a],dut.u_line_buffer.memory_b[a]);
 $fwrite(fd,"S %0d %0d %0d %0d %0d\n",samples,requests,writes,frames,blocked);
 $fclose(fd);
 end
endtask
initial begin
 if(!$value$plusargs("case=%d",scenario))scenario=0;
 if(!$value$plusargs("trace=%s",trace_path))$fatal(1,"missing trace");
 fd=$fopen(trace_path,"w");if(!fd)$fatal(1,"trace open");
end
always @(posedge clk) begin
 // Inactive region samples before every production NBA, including coincident pixel edges.
 #0;
 now_input=sampled_input;now_output=sampled_output;
 if(run_count!=0&&(now_input!==previous_input||now_output!==previous_output))flush_run;
 previous_input=now_input;previous_output=now_output;run_count=run_count+1;
 samples=samples+1;
 if(samples>4000000)begin finish_trace;$fatal(1,"logic edge cap");end
 if(dut.line_write)writes=writes+1;
 if(frame_event)frames=frames+1;
 if(dut.front_valid&&!dut.burst_active&&!dut.fill_slot_free)blocked=blocked+1;
end
always @(posedge clk) begin
 cycle<=cycle+1;
 if(reset)begin busy<=0;beat<=0;delay_count<=0;end
 else begin
  if(memory_request_valid&&memory_request_ready)begin
   busy<=1;beat<=0;delay_count<=2;held_address<=memory_address;requests<=requests+1;
  end
  if(busy&&delay_count!=0)delay_count<=delay_count-1;
  if(memory_data_valid)begin
   if(memory_last||memory_error)busy<=0;
   else beat<=beat+1;
  end
  if(memory_error)begin busy<=0;injected<=1;end
  if(scenario==5&&dut.memory_error_sticky&&reset_at==0)reset_at<=cycle+5;
 end
end
always @(negedge clk) begin
 reset=scenario==5&&reset_at!=0&&cycle>=reset_at&&cycle<reset_at+8;
 epoch=(scenario==5&&reset_at!=0&&cycle>=reset_at+8)?cycle-reset_at-8:cycle;
 boot_enable=epoch>=8;boot_failed=0;
 device_channel=3;device_write_enable=0;device_write_data=0;
 // First descriptor B/linear, invalid descriptors, then stage C/raw.
 case(epoch)
  12:begin device_channel=1;device_write_enable=1;device_write_data=0;end
  13:begin device_channel=2;device_write_enable=1;device_write_data=16'h22;end
  14:begin device_channel=4;device_write_enable=1;device_write_data=1;end
  15:begin device_channel=3;device_write_enable=1;device_write_data=1;end
  20:begin device_channel=1;device_write_enable=1;device_write_data=16'h7010;end
  21:begin device_channel=2;device_write_enable=1;device_write_data=16'h3e;end
  22:begin device_channel=3;device_write_enable=1;device_write_data=1;end
  25:begin device_channel=1;device_write_enable=1;device_write_data=1;end
  26:begin device_channel=2;device_write_enable=1;device_write_data=16'h20;end
  27:begin device_channel=3;device_write_enable=1;device_write_data=1;end
  30:begin device_channel=1;device_write_enable=1;device_write_data=0;end
  31:begin device_channel=2;device_write_enable=1;device_write_data=16'h24;end
  32:begin device_channel=4;device_write_enable=1;device_write_data=0;end
 endcase
 if(frame_event&&!swap2)begin
  device_channel=3;device_write_enable=1;device_write_data=1;swap2=1;
 end
 if(reset)swap2=0;
 memory_request_ready=!reset&&!busy&&(!memory_request_valid||cycle%7!=0);
 if(scenario==1&&requests>=50)memory_request_ready=0;
 if(scenario==2&&requests>=100)memory_request_ready=0;
 memory_data_valid=busy&&delay_count==0&&cycle%3!=0&&!reset;
 memory_read_data=memory_data_valid?payload(held_address,beat):0;
 memory_last=memory_data_valid&&beat==3;
 memory_error=0;
 if((scenario==3||scenario==5)&&!injected&&memory_data_valid&&beat==2)memory_error=1;
 if(scenario==4&&memory_data_valid&&beat==1)memory_last=1;
 if(scenario==6&&!injected&&busy&&delay_count==1)memory_error=1;
 if((scenario==0&&dut.frame_index>=2)||
    (scenario==1&&dut.frame_index>=2)||
    (scenario==2&&dut.underflow_sync)||
    ((scenario==3||scenario==4||scenario==6)&&dut.memory_error_sticky)||
    (scenario==5&&reset_at!=0&&cycle>=reset_at+28))begin
  finish_trace;
  $display("PASS case=%0d phase=%0d cycles=%0d requests=%0d writes=%0d",scenario,phase,cycle,requests,writes);
  $finish;
 end
end
initial begin
 #(64'd4*1056*525*120*250000/999+1000000);
 finish_trace;$fatal(1,"pixel/frame bound");
end
endmodule
