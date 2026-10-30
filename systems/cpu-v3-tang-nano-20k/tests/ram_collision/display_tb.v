`timescale 1ns/1ps
module tb;
reg clk=0,pixel_clock=0,serial_clock=0,reset=1,video_locked=0;
reg boot_enable=0,boot_failed=0;
wire started_ack,startup_error,frame_event;
always #9.259 clk=~clk;
initial begin #12.526;forever #15.015 pixel_clock=~pixel_clock;end
// Serializer is bypassed by production's existing __ICARUS__ branch; two real
// RAM/ownership domains retain their production rates and run continuously.
reg memory_request_ready=0,memory_data_valid=0,memory_last=0,memory_error=0;
reg[63:0]memory_read_data=0;
reg[2:0]device_index=3;reg[3:0]device_channel=0;
reg device_read_enable=0,device_write_enable=0;reg[15:0]device_write_data=0;
wire memory_request_valid,memory_urgent,underflow,tmds_clk_p,tmds_clk_n;
wire[21:0]memory_address;wire[2:0]tmds_data_p,tmds_data_n;wire[15:0]device_read_data;
FramebufferHdmi dut(.*);
integer logic_cycles=0,pixel_cycles=0,beat=0,requests=0,request_x=0,request_y=0,wa=0;
integer sampled=0,black_samples=0,blank_samples=0,col=0,pixel_y=0,consumers=0,tainted=0,line_writes=0;
reg fb_d=0,injected=0;reg[15:0]expected_pixel;reg[23:0]expected_rgb;
function[15:0]pattern;input integer x,y;begin pattern=x*73+y*977;end endfunction
function[23:0]color;input[15:0]p;begin color={p[15:11],p[15:13],p[10:5],p[10:9],p[4:0],p[4:2]};end endfunction
wire vis=dut.visible_pipe3;wire fb_vis=dut.framebuffer_pipe3&&vis;
// First legal memory descriptor is held until vertical blank after the black
// startup frame. This hits unowned default read Q while keeping enough time to
// publish both real groups before content becomes visible; Display remains enabled.
always @(negedge clk)memory_request_ready=pixel_cycles>2*1056*525+100 && logic_cycles%19>=4 && beat==0;
always @(posedge clk)begin
 logic_cycles=logic_cycles+1;memory_data_valid<=0;memory_last<=0;
 if(!reset && dut.line_write)begin
  line_writes=line_writes+1;
  if(dut.published[dut.fill_slot]!=dut.release_sync[dut.fill_slot])$fatal(1,"row group written before release");
 end
 if(!reset && memory_request_valid && memory_request_ready)begin
  if(beat!=0)$fatal(1,"overlapping unsupported descriptor");
  if(memory_address<22'h200000 || memory_address>=22'h219000 || memory_address[3:0]!=0)$fatal(1,"bad framebuffer address %h",memory_address);
  request_x=((memory_address-22'h200000)>>12)*16;
  request_y=(((memory_address-22'h200000)>>8)&15)*16+(((memory_address-22'h200000)>>4)&15);
  if(request_x>=400 || request_y>=240)$fatal(1,"out of image request");
  beat<=1;requests=requests+1;
 end else if(beat!=0 && logic_cycles%7!=0)begin
  wa=request_x+(beat-1)*4;
  memory_read_data<={pattern(wa+3,request_y),pattern(wa+2,request_y),pattern(wa+1,request_y),pattern(wa,request_y)};
  memory_data_valid<=1;memory_last<=beat==4;
  if(beat==4)beat<=0;else beat<=beat+1;
 end
end
task device_write;input[3:0]channel;input[15:0]value;begin
 @(negedge clk);device_channel=channel;device_write_data=value;device_write_enable=1;
 @(negedge clk);device_write_enable=0;
end endtask
initial begin
 repeat(4)@(posedge clk);#1 reset=0;video_locked=1;boot_enable=1;
 device_write(1,16'h0000);device_write(2,16'h0020);device_write(3,16'h0001);
end
always @(negedge pixel_clock)if($test$plusargs("FORCE_LIVE") && dut.conversion_active && dut.read_phase==1 && !dut.pair_bank && !injected)begin
 force dut.u_line_buffer.read_data_a=32'hxxxxxxxx;injected=1;
end
always @(posedge pixel_clock)begin
 pixel_cycles=pixel_cycles+1;
 if(pixel_cycles>4*1056*525)$fatal(1,"Display pixel cycle cap");
 if(!dut.pixel_reset)begin
  if(dut.conversion_active)begin
   if(dut.h_count<1012 && dut.read_phase==1)begin
    consumers=consumers+1;
    if(dut.pair_bank?dut.u_line_buffer.audit_taint_b:dut.u_line_buffer.audit_taint_a)tainted=tainted+1;
   end
   if((dut.h_count<1012 && (dut.read_phase==2 || dut.read_phase==3)) || dut.emit_pair)begin
    consumers=consumers+2;
    if(dut.u_line_buffer.audit_taint_a)tainted=tainted+1;
    if(dut.u_line_buffer.audit_taint_b)tainted=tainted+1;
   end
  end
  if(tainted!=0)$fatal(1,"AUDIT_LIVE_DISPLAY_TAINT effective=%0d",tainted);
  if(dut.h_count==216)pixel_y=(dut.v_count-32)/2;
  fb_d<=fb_vis;
  if(fb_vis&&!fb_d)col=0;
  if(fb_vis)begin
   expected_pixel=pattern(col/2,pixel_y);expected_rgb=color(expected_pixel);
   if(dut.rgb_pipe!==expected_rgb)$fatal(1,"AUDIT_LIVE_PIXEL x=%0d y=%0d got=%h want=%h",col,pixel_y,dut.rgb_pipe,expected_rgb);
   sampled=sampled+1;col=col+1;
  end else if(vis)begin
   black_samples=black_samples+1;
   if(dut.rgb_pipe!==0)$fatal(1,"AUDIT_LIVE_PIXEL startup black got=%h",dut.rgb_pipe);
  end else blank_samples=blank_samples+1;
  if(sampled==32000)begin
   if(underflow || startup_error)$fatal(1,"unexpected display fault underflow=%b startup=%b",underflow,startup_error);
   if(dut.u_line_buffer.audit_exact_windows<2)$fatal(1,"zero or incomplete exact collision coverage");
   if(line_writes<800 || consumers<10000 || black_samples<100000 || blank_samples<100000)$fatal(1,"missing scanout coverage");
   $display("DIGITAL_DESIGN_PASS DISPLAY pixel_cycles=%0d logic_cycles=%0d pixels=%0d black=%0d blank=%0d consumers=%0d tainted_effective=%0d read_windows=%0d held_windows=%0d exact_windows=%0d requests=%0d writes=%0d",pixel_cycles,logic_cycles,sampled,black_samples,blank_samples,consumers,tainted,dut.u_line_buffer.audit_read_windows,dut.u_line_buffer.audit_held_windows,dut.u_line_buffer.audit_exact_windows,requests,line_writes);$finish;
  end
 end
end
endmodule
