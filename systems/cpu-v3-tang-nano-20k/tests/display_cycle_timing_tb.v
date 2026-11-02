`timescale 1ps/1ps
module tb;
reg clk=0,pixel_clock=0,serial_clock=0;
reg reset=0,video_locked=1,boot_enable=0,boot_failed=0;
reg memory_request_ready=0,memory_data_valid=0,memory_last=0,memory_error=0;
reg[63:0]memory_read_data=0;
reg[2:0]device_index=3;reg[3:0]device_channel=3;
reg device_read_enable=1,device_write_enable=0;reg[15:0]device_write_data=0;
wire memory_request_valid,memory_urgent,underflow,started_ack,startup_error,frame_event;
wire[21:0]memory_address;wire[15:0]device_read_data;
wire tmds_clk_p,tmds_clk_n;wire[2:0]tmds_data_p,tmds_data_n;
FramebufferHdmi dut(.*);
integer phase=17,fd,cycle=0,pixels=0,events=0,mask;
time lt=37,pt,now_tick,target;
reg[2047:0]trace_path;
wire[127:0]pins={reset,video_locked,boot_enable,boot_failed,memory_request_ready,
 memory_data_valid,memory_read_data,memory_last,memory_error,device_index,device_channel,
 device_read_enable,device_write_enable,device_write_data};
wire[127:0]logic_view={memory_request_valid,memory_urgent,memory_address,dut.line_write,
 dut.line_write_address,(dut.line_write?memory_read_data:64'd0),dut.published,
 dut.group_valid,dut.group_linear,device_read_data,underflow,started_ack,startup_error,frame_event};
wire[63:0]pixel_view={dut.h_count,dut.v_count,dut.hsync,dut.vsync,dut.active,
 dut.released,dut.frame_toggle,dut.started,dut.underflow_sticky,dut.display_slot,
 dut.display_second,dut.vertical_repeat,dut.scan_valid,dut.scan_line_ready,dut.scan_linear,
 dut.pixel_reset,dut.publish_sync,dut.enable_sync,dut.failure_sync,dut.pixel_reset_sync};
task drive;
 begin
 boot_enable=cycle>=8;reset=cycle>=64&&cycle<72;
 boot_failed=cycle>=100&&cycle<108;
 // In phase17, cycle20 logic/pixel rises coincide; pixel must see old sticky error.
 memory_error=cycle==20;
 end
endtask
initial begin
 if(!$value$plusargs("phase=%d",phase))phase=17;
 if(!$value$plusargs("trace=%s",trace_path))$fatal(1,"trace argument");
 fd=$fopen(trace_path,"w");if(!fd)$fatal(1,"trace open");
 pt=60+phase;
 while(pixels<1200)begin
  now_tick=lt<pt?lt:pt;target=now_tick*64'd250000/999;
  #(target-$time);
  mask=0;
  if(now_tick==lt)begin
   clk=~clk;lt=lt+37;
   if(clk)mask=mask|1;else drive;
  end
  if(now_tick==pt)begin
   pixel_clock=~pixel_clock;pt=pt+60;
   if(pixel_clock)mask=mask|2;
  end
  #0;
  if(mask!=0)begin
   $fwrite(fd,"E %0d %032h %032h %016h\n",mask,pins,logic_view,pixel_view);
   events=events+1;
   if(mask&1)cycle=cycle+1;
   if(mask&2)pixels=pixels+1;
  end
  #1;
  if(events>4000)$fatal(1,"event cap");
 end
 $fwrite(fd,"F 0 %032h %032h %016h\n",pins,logic_view,pixel_view);
 $fclose(fd);$display("PASS TIMING %0d %0d",events,pixels);$finish;
end
initial begin #100000000;$fatal(1,"short timing cap");end
endmodule
