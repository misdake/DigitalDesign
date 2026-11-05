// Common profiling event v2. Passive only; no assignment to a DUT signal.
// The posedge block samples old state. The observation-only delay below waits
// for NBA commits, retaining the original edge tick for CP commit events.
`define PF @GPU@.core.front
`define PI @GPU@.core.front.interpolator
`define PV @GPU@.core.front.meshlet.core
`define PB @GPU@.core.back
`define PQ @GPU@.core.back.front.quad
`define PC @GPU@.core.back.sink.cache
`define PD @GPU@.dma
`define PP @GPU@.processor
`define PE0(N) $fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"%0s\",[]]]",pr_tick,pr_submission,N);
`define PE1(N,A) $fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"%0s\",[%0d]]]",pr_tick,pr_submission,N,A);
`define PE2(N,A,B) $fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"%0s\",[%0d,%0d]]]",pr_tick,pr_submission,N,A,B);
`define PE3(N,A,B,C) $fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"%0s\",[%0d,%0d,%0d]]]",pr_tick,pr_submission,N,A,B,C);
`define IMAGE_BANK(B) \
 if(!`PC.ui_active&&`PC.banks[B].a_en&&`PC.banks[B].a_we) \
  $fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"cache_write\",[%0d,%0d,%0d,65535,%0d]]]",pr_tick,pr_submission,B,`PC.banks[B].a_addr,`PC.banks[B].a_data,`PC.result_context);
integer pr_events,pr_submission=0,pr_count=0,pr_limit=12000000,pr_max_submissions=1,pr_finish=0;
reg[63:0]pr_tick;
reg[31:0]pr_word=0;
integer pr_i,pr_vkind,pr_ltokens=0,pr_stokens=0;
integer pr_old_pc,pr_old_state,pr_exec_pc=-1;
reg pr_live=0,pr_capturing=0,pr_done=0,pr_old_busy,pr_old_ce,pr_old_stop,pr_old_target,pr_old_failed;
reg pr_seen_reset=0;
reg pr_stopped_capture=0;
reg[63:0]pr_logic_zero_fs=0;
reg pr_clock_seen=0;
task profiler_physical_write;input[63:0]source_fs,period_fs;input[31:0]address,data;input[3:0]mask;
 reg[63:0]numerator;
 reg[31:0]required_bits;
 begin
  if(pr_capturing&&!pr_done)begin
   if(!pr_clock_seen||source_fs<pr_logic_zero_fs||period_fs==0)$fatal(1,"profiler native clock reference");
   required_bits={{8{mask[3]}},{8{mask[2]}},{8{mask[1]}},{8{mask[0]}}};
   if((pr_known32(data)&required_bits)!=required_bits)$fatal(1,"profiler unknown enabled native write data");
   numerator=(source_fs-pr_logic_zero_fs)*148;
   $fdisplay(pr_events,"[[%0d,%0d],\"physical_write\",[%0d,%0d,%0d]]",numerator,period_fs,address,pr_value32(data),mask);
  end
 end
endtask
function[31:0]pr_known32;input[31:0]v;integer j;begin
 for(j=0;j<32;j=j+1)pr_known32[j]=(v[j]===1'b0||v[j]===1'b1);
end endfunction
function[31:0]pr_value32;input[31:0]v;integer j;begin
 for(j=0;j<32;j=j+1)pr_value32[j]=(v[j]===1'b1);
end endfunction
initial begin
 pr_events=$fopen("@EVENTS@","w");if(!pr_events)$fatal(1,"profiler output open");
 if($value$plusargs("PROFILE_LIMIT=%d",pr_limit))begin end
 if($value$plusargs("PROFILE_SUBMISSIONS=%d",pr_max_submissions))begin end
 if($value$plusargs("PROFILE_FINISH=%d",pr_finish))begin end
 if(pr_limit<1||pr_max_submissions<1)$fatal(1,"profiler requires bounds");
end
task profiler_cpu_begin;input integer span_id,frame_id;input[767:0]label;begin
 if(!pr_done)$fdisplay(pr_events,"[%0d,\"cpu_begin\",[%0d,%0d,\"%0s\"]]",@CYCLE@*64'd148,span_id,frame_id,label);
end endtask
task profiler_cpu_end;input integer span_id;begin
 if(!pr_done)$fdisplay(pr_events,"[%0d,\"cpu_end\",[%0d]]",@CYCLE@*64'd148,span_id);
end endtask
always @(posedge @CLOCK@)if(!pr_done)begin
 if(!pr_clock_seen)begin pr_logic_zero_fs=$realtime*1000000;pr_clock_seen=1;end
 pr_tick=@CYCLE@*64'd148;
 if(pr_seen_reset)pr_capturing=1;
 if(@RESET@)pr_seen_reset=1;
 pr_old_pc=`PP.diagnostic_pc;pr_old_state=`PP.state;pr_old_busy=@GPU@.busy;
 pr_old_ce=@ENABLE@;pr_old_stop=`PP.stop;pr_old_target=`PP.target_valid;pr_old_failed=`PP.failed;
 if(!@RESET@)begin
  if(@GPU@.load_count&&@GPU@.submit_ready)pr_count=@GPU@.device_write_data+1;
  if(@GPU@.submit&&@GPU@.submit_ready)begin
   if(pr_live||pr_count<1)$fatal(1,"profiler submission overlap/count");
   pr_submission=pr_submission+1;pr_live=1;pr_capturing=1;pr_exec_pc=-1;
   $fdisplay(pr_events,"[%0d,\"submit\",%0d]",pr_tick,pr_submission);
   $fdisplay(pr_events,"[%0d,\"cb_submit\",[%0d,%0d,%0d]]",pr_tick,pr_submission,@GPU@.device_write_data*128,pr_count*4);
  end
 end
 if(pr_capturing)begin
  if(@RESET@)begin
   if(pr_live)begin
    pr_stopped_capture=1;
    $fdisplay(pr_events,"[%0d,\"cb_reset\",[%0d]]",pr_tick,pr_submission);
    $fdisplay(pr_events,"[%0d,\"gpu_stopped\",[%0d,\"reset\"]]",pr_tick,pr_submission);pr_live=0;
   end
   pr_ltokens=0;pr_stokens=0;
  end
  if(!@RESET@&&pr_live&&@ENABLE@&&!`PP.stop&&`PP.state==11)begin
   pr_word=`PP.read_word;
   $fdisplay(pr_events,"[%0d,\"cb_latch\",[%0d,%0d,%0d]]",pr_tick,pr_submission,`PP.diagnostic_pc,`PP.read_word);
  end
  if(!@RESET@&&pr_live&&@ENABLE@&&!`PP.stop&&`PP.state==2&&pr_exec_pc!=`PP.diagnostic_pc)begin
   pr_exec_pc=`PP.diagnostic_pc;
   $fdisplay(pr_events,"[%0d,\"cb_execute\",[%0d,%0d,%0d]]",pr_tick,pr_submission,pr_exec_pc,pr_word);
  end
  $fdisplay(pr_events,"[%0d,\"gpu_control\",[%0d,[%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d],%0d]]",pr_tick,pr_submission,
   `PP.diagnostic_pc,`PP.state,`PP.render_idle,`PP.work_busy,`PP.dma_idle,`PP.memory_busy,`PP.flush_complete,@ENABLE@,@GPU@.busy,`PP.command);
  $fwrite(pr_events,"[%0d,\"gpu_cache\",[[",pr_tick);
  for(pr_i=0;pr_i<8;pr_i=pr_i+1)begin
   if(pr_i)$fwrite(pr_events,",");$fwrite(pr_events,"%0s",`PC.tvalid[pr_i]?"true":"false");
  end
  $fwrite(pr_events,"],[");
  for(pr_i=0;pr_i<8;pr_i=pr_i+1)begin
   if(pr_i)$fwrite(pr_events,",");$fwrite(pr_events,"%0d",`PC.tvalid[pr_i]?`PC.tag[pr_i]:0);
  end
  $fdisplay(pr_events,"],%0d,%0d]]",@GPU@.color_base,@GPU@.depth_base);
  pr_vkind=0;
  if(`PF.meshlet.busy)begin
   if(!@ENABLE@)pr_vkind=5;
   else if(`PV.step)pr_vkind=`PV.triangle_mode?2:1;
   else if(`PF.meshlet.state==13&&!`PF.meshlet.triangle_ready)pr_vkind=3;
   else pr_vkind=4;
  end
  $fdisplay(pr_events,"[%0d,\"gpu_load\",[%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,0,%0d,0,0,%0d,0,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d]]",
   pr_tick,!`PD.idle,`PF.meshlet.busy,@ENABLE@&&`PV.step,`PV.count,`PF.raster_busy,`PI.p_busy,
   `PQ.global_used,`PQ.producer_used,`PQ.light_pending,`PQ.sample_pending,
   $countones(`PB.front.light.core.valid_pipe),$countones(`PB.public_live),(`PQ.publish_ptr[4:0]-`PB.issue_slot)&31,`PB.final_color.leaf.credits,
   (`PC.ctx_valid[0]?1:0)+(`PC.ctx_valid[1]?1:0),`PQ.allocate,
   @ENABLE@&&`PF.depth_event_valid,`PB.front.pixel_accepted,`PB.sample_fire,`PB.final_issue,`PB.lane_accept,
   pr_vkind,`PI.s_busy,`PB.work_count,`PB.work_reserved,$countones(`PB.public_live),`PC.hint_count,`PC.mt_active,
   @GPU@.ro_request_valid&&!@GPU@.ro_request_ready,@GPU@.fb_r_request_valid&&!@GPU@.fb_r_request_ready,@GPU@.fb_w_request_valid&&!@GPU@.fb_w_request_ready,@ENABLE@,
   `PB.front.light_compute_tick&&(|`PB.front.light.core.valid_pipe),
   `PB.front.sampling.advance&&(`PB.front.d_issue||`PB.front.l_issue||`PB.front.c_issue),@RESET@);
  if(!@RESET@)begin
   if(`PV.step)begin
    $fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"vt_step\",[%0d,%0d,%0d,%0d,%0d]]]",pr_tick,pr_submission,`PV.pc,`PV.triangle_mode,`PV.control[31:0],`PV.control[63:32],`PV.control[71:64]);
    if(`PV.rf_wsrc!=0)$fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"vt_rf_write\",[%0d,%0d,%0d]]]",pr_tick,pr_submission,`PV.rf_dst,pr_value32(`PV.rf_write),pr_known32(`PV.rf_write));
   end
   if(`PV.word_take)$fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"t_word\",[%0d,%0d,%0d,%0d,%0d,%0d]]]",pr_tick,pr_submission,`PV.read_slot,`PV.word_index,pr_value32(`PV.word_data[31:0]),pr_value32({10'd0,`PV.word_data[53:32]}),pr_known32(`PV.word_data[31:0]),pr_known32({10'd0,`PV.word_data[53:32]})&32'h3fffff);
   if(`PB.front.pixel_accepted)begin `PE3("light_accept",`PB.front.light_descriptor[4:0],`PB.front.light_lane,`PB.front.light_descriptor[8:5]) end
   if(`PB.light_result)$fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"light_result\",[%0d,%0d,%0d,%0d,%0d,%0d]]]",pr_tick,pr_submission,`PB.lk,`PB.ll,pr_value32({23'd0,`PB.lg}),pr_known32({23'd0,`PB.lg})&511,pr_value32({23'd0,`PB.lh}),pr_known32({23'd0,`PB.lh})&511);
   if(@GPU@.ro_request_valid&&@GPU@.ro_request_ready)begin `PE3("master_request",0,@GPU@.ro_address,@GPU@.ro_count) end
   if(@GPU@.fb_r_request_valid&&@GPU@.fb_r_request_ready)begin `PE3("master_request",1,@GPU@.fb_r_address,@GPU@.fb_r_count) end
   if(@GPU@.fb_w_request_valid&&@GPU@.fb_w_request_ready)begin
    `PE3("master_request",2,@GPU@.fb_w_address,@GPU@.fb_w_count)
    `PE1("write_owner",@GPU@.clear_active?1:(@GPU@.serial_active?(@GPU@.copy_active?3:2):0))
   end
   if(@GPU@.ro_response_valid)$fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"master_payload\",[0,%0d,%0d,255,%0d]]]",pr_tick,pr_submission,@GPU@.ro_data[31:0],@GPU@.ro_data[63:32],@GPU@.ro_response_error);
   if(@GPU@.fb_r_response_valid)$fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"master_payload\",[1,%0d,%0d,255,%0d]]]",pr_tick,pr_submission,@GPU@.fb_r_data[31:0],@GPU@.fb_r_data[63:32],@GPU@.fb_r_response_error);
   if(@GPU@.fb_w_data_ready)$fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"master_payload\",[2,%0d,%0d,255,0]]]",pr_tick,pr_submission,@GPU@.fb_w_data[31:0],@GPU@.fb_w_data[63:32]);
   if(@GPU@.ro_response_valid&&(@GPU@.ro_response_last||@GPU@.ro_response_error))begin `PE2("master_terminal",0,@GPU@.ro_response_error) end
   if(@GPU@.fb_r_response_valid&&(@GPU@.fb_r_response_last||@GPU@.fb_r_response_error))begin `PE2("master_terminal",1,@GPU@.fb_r_response_error) end
   if(@GPU@.fb_w_response_valid&&(@GPU@.fb_w_response_last||@GPU@.fb_w_response_error))begin `PE2("master_terminal",2,@GPU@.fb_w_response_error) end
   if(`PD.pop)begin
    `PE1("dma_return",`PD.rh[0])
    if(!`PD.stop&&!`PD.rk[`PD.rh[0]]&&`PD.rl[`PD.rh[0]])begin `PE1("dma_done",`PD.retired[0]) end
   end
   if(`PD.accept_work)$fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"dma_job\",[%0d,%0d,%0d,%0d,%0d,%0d]]]",pr_tick,pr_submission,`PD.put[0],`PD.work_mesh,`PD.work_slot,`PD.work_source,`PD.work_size,`PD.work_blocks);
   if(`PD.take)$fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"dma_request\",[%0d,%0d,%0d,%0d]]]",pr_tick,pr_submission,`PD.rp[0],`PD.offer_cache?2:`PD.issued[0],`PD.request_address,`PD.request_count);
   if(@ENABLE@&&@GPU@.start_fire)begin `PE1("draw",@GPU@.slot) end
   if(@ENABLE@&&@GPU@.meshlet_done)begin `PE0("vt_end") end
   if(@ENABLE@&&`PV.publish)begin `PE1("t_publish",`PV.write_slot) end
   if(`PF.header_take)begin `PE1("prepare",`PV.read_slot) end
   if(`PV.release_slot)begin `PE1("t_release",`PV.read_slot) end
   if(@ENABLE@&&`PF.raster_start)begin `PE0("r_start") end
   if(@ENABLE@&&`PF.done_raster)begin `PE0("r_end") end
   if(@ENABLE@&&`PI.cfg_apply)begin `PE0("i_context") end
   if(`PQ.regular_retire||`PQ.skip_empty)begin `PE2("q_retire",`PQ.retire_slot,`PQ.skip_empty) end
   if(`PQ.allocate)begin `PE1("q_alloc",`PQ.allocated[4:0]) end
   if(`PQ.published)begin `PE2("q_publish",`PQ.published_slot,`PQ.published_begin[3:0]) end
   if(`PB.final_issue)begin `PE2("final",`PB.issue_slot,`PB.issue_lane) end
   if(`PC.row0_fire)begin `PE2("rop_begin",`PC.tail,`PB.retire_slot) end
   `IMAGE_BANK(0)
   `IMAGE_BANK(1)
   `IMAGE_BANK(2)
   `IMAGE_BANK(3)
   if(`PC.color_fire_write&&`PC.result_last)begin `PE1("q_commit",`PC.result_context) end
   if(`PB.public_publish)begin `PE1("sample_publish",`PB.sample_descriptor[4:0]) end
   if(`PB.sample_fire)begin `PE2("sample_accept",`PB.sample_descriptor[3:0],`PB.sample_row) end
   if(`PB.sample_result)$fdisplay(pr_events,"[%0d,\"gpu_event\",[%0d,\"sample_result\",[%0d,%0d,%0d,%0d]]]",pr_tick,pr_submission,`PB.sk,`PB.sl,pr_value32({8'd0,`PB.sRGB}),pr_known32({8'd0,`PB.sRGB})&32'hffffff);
   if(`PB.sample_result&&`PB.sl)begin `PE1("sample_last",`PB.sk[5:2]) end
   if(@GPU@.memory.offer_owner==1&&@GPU@.ro_request_valid&&@GPU@.ro_request_ready)begin `PE0("texture_request") end
  end
  #0.001;
  if(!@RESET@&&pr_live)begin
   if(pr_old_ce&&!pr_old_stop&&pr_old_state==2&&(`PP.state!=2||`PP.diagnostic_pc!=pr_old_pc))
    $fdisplay(pr_events,"[%0d,\"cb_issue\",[%0d,%0d,%0d]]",pr_tick,pr_submission,pr_old_pc,pr_word);
   if(pr_old_ce&&!pr_old_stop&&pr_old_busy&&(`PP.diagnostic_pc!=pr_old_pc||(pr_old_state==2&&`PP.state==9)))begin
    $fdisplay(pr_events,"[%0d,\"cb_release\",[%0d,%0d,%0d]]",pr_tick,pr_submission,pr_old_pc,pr_word);
    if(pr_word[31:28]==10)begin `PE3("target",@GPU@.color_base*512,@GPU@.depth_base*512,`PP.target_valid) end
   end
   if(!pr_old_failed&&`PP.failed)begin
    pr_stopped_capture=1;
    $fdisplay(pr_events,"[%0d,\"cb_abort\",[%0d]]",pr_tick,pr_submission);
    $fdisplay(pr_events,"[%0d,\"gpu_stopped\",[%0d,\"abort\"]]",pr_tick,pr_submission);
   end
   if(pr_old_busy&&!@GPU@.busy&&!`PP.failed)begin
    $fdisplay(pr_events,"[%0d,\"cb_drain\",[%0d]]",pr_tick,pr_submission);pr_live=0;
   end
  end
  if(pr_old_target&&!`PP.target_valid)begin `PE1("target_unbind",@RESET@) end
  if(!@RESET@&&pr_old_busy&&!@GPU@.busy&&!`PP.failed)
   $fdisplay(pr_events,"[%0d,\"gpu_done\",%0d]",pr_tick+148,pr_submission);
 end
 if(pr_capturing&&!pr_live&&pr_submission>=pr_max_submissions)begin
  $fdisplay(pr_events,"[%0d,\"capture_end\",%0s]",pr_tick+148,pr_stopped_capture?"false":"true");$fclose(pr_events);pr_done=1;
  if(pr_finish)$finish;
 end else if(@CYCLE@>=pr_limit)begin
  $fdisplay(pr_events,"[%0d,\"capture_end\",false]",pr_tick+148);$fclose(pr_events);pr_done=1;if(pr_finish)$finish;
 end
end
`undef PF
`undef PI
`undef PV
`undef PB
`undef PQ
`undef PC
`undef PD
`undef PP
`undef PE0
`undef PE1
`undef PE2
`undef PE3
`undef IMAGE_BANK
