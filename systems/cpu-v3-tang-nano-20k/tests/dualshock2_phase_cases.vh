// Append to the independent packet peer. Real clocks, production Display CDC,
// native snapshot/line RAMs; serializer clock is irrelevant to frame timing.
reg pixel_clock=0;always #15.015015 pixel_clock=~pixel_clock;
reg beacons_enabled=1;
wire display_frame_event;
assign frame_event=beacons_enabled&&display_frame_event;
__DISPLAY__ display(.clk(clk),.reset(reset),.pixel_clock(pixel_clock),
 .serial_clock(1'b0),.video_locked(!reset),.boot_enable(!reset),.boot_failed(1'b0),
 .memory_request_ready(1'b1),.memory_data_valid(1'b0),.memory_read_data(64'd0),
 .memory_last(1'b0),.memory_error(1'b0),.device_index(3'd0),.device_channel(4'd0),
 .device_read_enable(1'b0),.device_write_enable(1'b0),.device_write_data(16'd0),
 .frame_event(display_frame_event));
integer frames=0,last_publish=-1,last_buttons=-1,published_buttons=-1;
integer age_min=9999999,age_max=0,input_min=9999999,input_max=0,checks=0;
integer measured_mode=0,raw_frames=0,raw_cycle=-1,raw_age=0;
integer previous_frame=-1,period_min=9999999,period_max=0;
reg previous_front=0;
always @(posedge display.frame_toggle)if(!reset)begin
 raw_frames=raw_frames+1;raw_cycle=cycle;
end
always @(negedge display.frame_toggle)if(!reset)begin
 raw_frames=raw_frames+1;raw_cycle=cycle;
end
always @(posedge ds2_clk)if(!reset&&!att&&wire_byte==5&&wire_bit==0)last_buttons=cycle;
always @(posedge clk)begin
 if(!reset&&display_frame_event)begin
  frames=frames+1;
  if(previous_frame>=0)begin
   if(cycle-previous_frame<period_min)period_min=cycle-previous_frame;
   if(cycle-previous_frame>period_max)period_max=cycle-previous_frame;
  end
  previous_frame=cycle;
  if(cycle-raw_cycle<2||cycle-raw_cycle>4)$fatal(1,"Display CDC event latency");
  if(measured_mode!=0)begin
   if(last_publish<0||cycle-last_publish<1900||cycle-last_publish>10300)
    $fatal(1,"phase publication age mode%0d age%0d",measured_mode,cycle-last_publish);
   if(cycle-last_publish<age_min)age_min=cycle-last_publish;
   if(cycle-last_publish>age_max)age_max=cycle-last_publish;
   raw_age=cycle-published_buttons;
   if(raw_age<input_min)input_min=raw_age;
   if(raw_age>input_max)input_max=raw_age;
   checks=checks+1;
   $display("DS2_PHASE mode=%0d frame=%0d publicationAge=%0d buttonsAge=%0d",measured_mode,frames,cycle-last_publish,raw_age);
  end
 end
 #1;
 if(!reset&&dut.front!=previous_front)begin
  previous_front=dut.front;last_publish=cycle;published_buttons=last_buttons;
 end
end
task next_frames;input integer count;integer stop;begin
 stop=frames+count;while(frames<stop)tick();
end endtask
task read_after_present;integer read_begin;begin
 // Representative present return after old-read drain, then immediate CPU
 // snapshot. No millisecond software delay or UART transmission intervenes.
 repeat(80)tick();read_begin=cycle;coherent_snapshot();
 if(cycle-read_begin>32||cycle-last_publish>10450)$fatal(1,"fresh actual input read");
 $display("DS2_READ publicationAge=%0d buttonsAge=%0d",cycle-last_publish,cycle-published_buttons);
end endtask
integer before_transactions,before_pubs;
integer target_expected=0,target_deadline=0,target_error=0,transition_checks=0;
always @(posedge clk)if(!reset&&dut.frame_start&&dut.phase_start)begin
 target_expected=(dut.operation==0&&dut.kind==3)?21:9;
 target_deadline=previous_frame+899027;
end
always @(posedge att)if(!reset&&target_expected!=0)begin
 target_error=cycle-target_deadline;
 if(operation==0&&frame_connected&&!frame_bad_marker&&!frame_bad_prefix&&!frame_unknown)begin
  if(target_expected!=wire_byte)begin
   if(target_error < -109000 || target_error > 97000)$fatal(1,"mode transition bound");
   transition_checks=transition_checks+1;
   $display("DS2_TRANSITION expected=%0d actual=%0d completionMinusFrame=%0d",target_expected,wire_byte,target_error);
  end else if(target_error > -1900 || target_error < -10300)$fatal(1,"target completion bound");
 end
 target_expected=0;
end
task change_at_target;input integer id;begin
 while(!dut.phase_start)begin @(negedge clk);#1;end
 mode_id=id;
end endtask
initial begin
 mode_id=8'h41;
 tick();tick();@(negedge clk);reset=0;
 // Hardware configuration completes without CPU. Wait for a settled phase.
 next_frames(3);if(!dut.valid||mode_id!=8'h73)$fatal(1,"configuration before phase check");
 measured_mode=9;next_frames(2);read_after_present();measured_mode=0;
 change_at_target(8'h79);next_frames(2);
 measured_mode=21;next_frames(2);read_after_present();measured_mode=0;
 change_at_target(8'h73);next_frames(2);
 measured_mode=9;next_frames(1);read_after_present();measured_mode=0;
 // Reading is optional: multiple autonomous updates must force a retry.
 begin_snapshot();before_pubs=pubs;next_frames(1);read_status();
 if(pubs<=before_pubs||!status[1])$fatal(1,"slow CPU freshness marker");
 coherent_snapshot();
 beacons_enabled=0;before_transactions=transactions;next_frames(2);
 if(transactions-before_transactions<4)$fatal(1,"missing beacon stopped polling");
 beacons_enabled=1;next_frames(2);measured_mode=9;next_frames(1);measured_mode=0;
 // Disconnect and digital/config reconnect while phase correction continues.
 connected=0;next_frames(1);if(dut.valid)$fatal(1,"disconnect stale validity");
 connected=1;mode_id=8'h41;next_frames(2);
 if(!dut.valid||mode_id!=8'h73)$fatal(1,"autonomous phase reconnect");
 measured_mode=9;next_frames(1);read_after_present();measured_mode=0;
 if(checks!=7||transition_checks!=2||period_min<899025||period_max>899029)$fatal(1,"real dual-clock coverage");
 $display("DS2_PASS phase checks=%0d publicationAge=%0d..%0d buttonsAge=%0d..%0d mainCycles framePeriod=%0d..%0d",checks,age_min,age_max,input_min,input_max,period_min,period_max);
 $finish;
end
endmodule
