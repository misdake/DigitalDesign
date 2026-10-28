// Format 2 session observer. Included at the explicit harness marker. No DUT writes.
// Identity counters span all submissions; SEND acceptance is independent of CE.
// Every record samples old state on a physical edge. Accepted events at c
// complete at c+1; state samples represent the interval [c,c+1).
`define PF @GPU@.core.front
`define PI @GPU@.core.front.interpolator
`define PV @GPU@.core.front.meshlet.core
`define PB @GPU@.core.back
`define PQ @GPU@.core.back.front.quad
`define PC @GPU@.core.back.sink.cache
`define PD @GPU@.dma
`define PP @GPU@.processor
integer pr_submission=0,pr_count=0,pr_max_submissions=1,pr_finish=0;
reg pr_capturing=0;
integer pr_events,pr_load,pr_n=0,pr_mesh=0,pr_prepare=0,pr_raster=0,pr_interp=0;
integer pr_tri_owner[0:15],pr_tri_live[0:15];
integer pr_quad_owner[0:31],pr_quad_seq[0:31],pr_quad_live[0:31];
integer pr_ctx_quad[0:1],pr_ctx_owner[0:1];
integer pr_job_slot[0:1],pr_page_job[0:1],pr_ret_job[0:1],pr_ret_tx[0:1];
integer pr_job=0,pr_tx=0,pr_quad=0,pr_pub=0,pr_ret=0,pr_commit=0,pr_empty=0;
integer pr_cmd_pc=-1,pr_cmd_start=0,pr_cmd_word=0,pr_cmd_exec=-1;
integer pr_ltokens=0,pr_stokens=0,pr_i,pr_vkind,pr_limit=12000000;
reg pr_live=0,pr_done=0,pr_seen_busy=0;
reg[30:0]pr_state_old=31'h7fffffff;
wire[30:0]pr_state_now={`PP.diagnostic_pc,`PP.state,`PP.render_idle,`PP.work_busy,`PP.dma_idle,`PP.memory_busy,`PP.flush_complete,`PP.waits_done,@ENABLE@,1'b0};
initial begin
 pr_events=$fopen("@EVENTS@","w");pr_load=$fopen("@LOAD@","w");
 if(!pr_events||!pr_load)$fatal(1,"profiler output open");
 $fdisplay(pr_events,"FORMAT 2");
 if($value$plusargs("PROFILE_LIMIT=%d",pr_limit))begin end
 if($value$plusargs("PROFILE_SUBMISSIONS=%d",pr_max_submissions))begin end
 if($value$plusargs("PROFILE_FINISH=%d",pr_finish))begin end
 if(pr_limit<1||pr_max_submissions<1)$fatal(1,"profiler requires bounds");
 for(pr_i=0;pr_i<16;pr_i=pr_i+1)begin pr_tri_owner[pr_i]=0;pr_tri_live[pr_i]=0;end
 for(pr_i=0;pr_i<32;pr_i=pr_i+1)begin pr_quad_owner[pr_i]=0;pr_quad_seq[pr_i]=0;pr_quad_live[pr_i]=0;end
 for(pr_i=0;pr_i<2;pr_i=pr_i+1)begin pr_ctx_quad[pr_i]=0;pr_ctx_owner[pr_i]=0;pr_job_slot[pr_i]=0;pr_page_job[pr_i]=0;pr_ret_job[pr_i]=0;pr_ret_tx[pr_i]=0;end
 $fdisplay(pr_load,"cycle,dma,vt_owned,vt_step,t_queue,r_busy,i_busy,quad_total,quad_producer,light_pending,sample_pending,light_tokens,sample_quads,final_wait,final_tokens,rop_contexts,r_quad,i_pixel,light_issue,sample_issue,final_issue,rop_lane,vt_owner,vt_kind,r_owner,i_owner,prepare_busy,prepare_owner,texture_work,texture_reserved,texture_public,fb_hint,fb_maintenance,ro_wait,fb_read_wait,fb_write_wait,ce,light_tick,sample_tick");
end
task profiler_cpu_begin;input integer span_id,frame_id;input[767:0]label;begin
 if(!pr_done)$fdisplay(pr_events,"CPU_BEGIN %0d %0d %0d %0s",@CYCLE@,span_id,frame_id,label);
end endtask
task profiler_cpu_end;input integer span_id;begin
 if(!pr_done)$fdisplay(pr_events,"CPU_END %0d %0d",@CYCLE@,span_id);
end endtask
always @(posedge @CLOCK@)begin
 if(@RESET@&&pr_capturing&&!pr_done)$fatal(1,"profiler reset needs a new session");
 if(!@RESET@&&!pr_done)begin
 if(pr_live&&pr_seen_busy&&!@GPU@.busy)begin
  if(pr_cmd_pc>=0&&pr_cmd_exec>=0)$fdisplay(pr_events,"CMD_END %0d %0d %0d %0d %0d %h",@CYCLE@,pr_cmd_pc,pr_cmd_start,pr_cmd_exec,`PP.state,pr_cmd_word);
  pr_cmd_pc=-1;pr_cmd_exec=-1;
   if(pr_quad!=pr_pub||pr_quad!=pr_ret||pr_quad!=pr_commit+pr_empty||pr_ltokens!=0||pr_stokens!=0)$fatal(1,"profiler final conservation");
   for(pr_i=0;pr_i<16;pr_i=pr_i+1)if(pr_tri_live[pr_i])$fatal(1,"profiler live T at end");
   for(pr_i=0;pr_i<32;pr_i=pr_i+1)if(pr_quad_live[pr_i])$fatal(1,"profiler live quad at end");
   if(pr_ctx_quad[0]||pr_ctx_quad[1])$fatal(1,"profiler live ROP at end");
   $fdisplay(pr_events,"DONE %0d %0d %0d %0d %0d %0d %0d",@CYCLE@,pr_submission,pr_quad,pr_pub,pr_ret,pr_commit,pr_empty);
   pr_live=0;pr_seen_busy=0;
  end
 if(@GPU@.load_count&&@GPU@.submit_ready)pr_count=@GPU@.device_write_data+1;
 if(@GPU@.submit&&@GPU@.submit_ready)begin
  if(pr_live||pr_count<1)$fatal(1,"profiler submission overlap/count");
  pr_submission=pr_submission+1;pr_live=1;pr_capturing=1;pr_state_old=31'h7fffffff;
  $fdisplay(pr_events,"SUBMIT %0d %0d %0d %0d",@CYCLE@,pr_submission,@GPU@.device_write_data*128,pr_count*4);
 end
 if(pr_capturing)begin
  if(@GPU@.busy)pr_seen_busy=1;
  if(pr_state_old!==pr_state_now)begin
   $fdisplay(pr_events,"STATE %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d",@CYCLE@,`PP.diagnostic_pc,`PP.state,`PP.render_idle,`PP.work_busy,`PP.dma_idle,`PP.memory_busy,`PP.flush_complete,`PP.waits_done,@ENABLE@);
   pr_state_old=pr_state_now;
  end
  // Command residency is separate from asynchronous work launched by it.
  if(pr_cmd_pc!=`PP.diagnostic_pc||!@GPU@.busy)begin
   if(pr_cmd_pc>=0&&pr_cmd_exec>=0)$fdisplay(pr_events,"CMD_END %0d %0d %0d %0d %0d %h",@CYCLE@,pr_cmd_pc,pr_cmd_start,pr_cmd_exec,`PP.state,pr_cmd_word);
   pr_cmd_pc=-1;pr_cmd_exec=-1;
   if(@GPU@.busy)begin pr_cmd_pc=`PP.diagnostic_pc;pr_cmd_start=@CYCLE@;end
  end
  if(pr_cmd_pc>=0&&@ENABLE@&&`PP.state==2&&pr_cmd_exec<0)begin
   pr_cmd_exec=@CYCLE@;pr_cmd_word=`PP.command;
   $fdisplay(pr_events,"CMD_EXEC %0d %0d %h",@CYCLE@,pr_cmd_pc,pr_cmd_word);
  end
  if(@ENABLE@&&@GPU@.start_fire)begin
   pr_mesh=pr_mesh+1;
   $fdisplay(pr_events,"DRAW %0d %0d %0d %0d %0d",@CYCLE@,pr_mesh,`PP.diagnostic_pc,@GPU@.slot,pr_page_job[@GPU@.slot]);
  end
  if(@ENABLE@&&@GPU@.meshlet_done)$fdisplay(pr_events,"VT_END %0d %0d",@CYCLE@+1,pr_mesh);
  if(@ENABLE@&&`PV.publish)begin
   if(pr_tri_live[`PV.write_slot])$fatal(1,"profiler T slot overwrite");
   pr_tri_owner[`PV.write_slot]=pr_mesh;pr_tri_live[`PV.write_slot]=1;
   $fdisplay(pr_events,"T_PUBLISH %0d %0d %0d",@CYCLE@,pr_mesh,`PV.write_slot);
  end
  if(`PF.header_take)begin
   if(!pr_tri_live[`PV.read_slot])$fatal(1,"profiler T unowned header");
   pr_prepare=pr_tri_owner[`PV.read_slot];
   $fdisplay(pr_events,"PREPARE %0d %0d %0d",@CYCLE@,pr_prepare,`PV.read_slot);
  end
  if(`PV.release_slot)begin
   if(!pr_tri_live[`PV.read_slot])$fatal(1,"profiler T release");
   pr_tri_live[`PV.read_slot]=0;
  end
  if(@ENABLE@&&`PF.raster_start)begin
   pr_raster=pr_prepare;$fdisplay(pr_events,"R_START %0d %0d",@CYCLE@,pr_raster);
  end
  if(@ENABLE@&&`PF.done_raster)$fdisplay(pr_events,"R_END %0d %0d",@CYCLE@,pr_raster);
  if(@ENABLE@&&`PI.cfg_apply)begin
   pr_interp=pr_prepare;$fdisplay(pr_events,"I_CONTEXT %0d %0d",@CYCLE@,pr_interp);
  end
  // Physical pool slot + generation is the quad identity, never batch_id.
  if(`PQ.regular_retire||`PQ.skip_empty)begin
   if(!pr_quad_live[`PQ.retire_slot])$fatal(1,"profiler unowned retirement");
   $fdisplay(pr_events,"Q_RETIRE %0d %0d %0d %0d",@CYCLE@+1,pr_quad_seq[`PQ.retire_slot],pr_quad_owner[`PQ.retire_slot],`PQ.skip_empty);
   pr_ret=pr_ret+1;if(`PQ.skip_empty)pr_empty=pr_empty+1;
   // Keep identity until the next allocation; ROP holds its independent copy.
   pr_quad_live[`PQ.retire_slot]=0;
  end
  if(`PQ.allocate)begin
   if(pr_quad_live[`PQ.allocated[4:0]])$fatal(1,"profiler pool overwrite");
   if(pr_raster<1||pr_raster>pr_mesh)$fatal(1,"profiler invalid raster owner");
   pr_quad=pr_quad+1;pr_quad_owner[`PQ.allocated[4:0]]=pr_raster;
   pr_quad_seq[`PQ.allocated[4:0]]=pr_quad;pr_quad_live[`PQ.allocated[4:0]]=1;
   $fdisplay(pr_events,"Q_ALLOC %0d %0d %0d %0d",@CYCLE@,pr_quad,pr_raster,`PQ.allocated[4:0]);
  end
  if(`PQ.published)begin
   if(!pr_quad_live[`PQ.published_slot])$fatal(1,"profiler publish without allocation");
   pr_pub=pr_pub+1;
   $fdisplay(pr_events,"Q_PUBLISH %0d %0d %0d %0d",@CYCLE@,pr_quad_seq[`PQ.published_slot],pr_quad_owner[`PQ.published_slot],`PQ.published_begin[3:0]);
  end
  // Final can read the next quad before the old one retires.
  if(`PB.final_issue)$fdisplay(pr_events,"FINAL %0d %0d %0d %0d",@CYCLE@,pr_quad_seq[`PB.issue_slot],pr_quad_owner[`PB.issue_slot],`PB.issue_lane);
  if(`PC.row0_fire)begin
   if(pr_ctx_quad[`PC.tail]!=0)$fatal(1,"profiler ROP context overwrite");
   pr_ctx_quad[`PC.tail]=pr_quad_seq[`PB.retire_slot];pr_ctx_owner[`PC.tail]=pr_quad_owner[`PB.retire_slot];
   $fdisplay(pr_events,"ROP_BEGIN %0d %0d %0d",@CYCLE@,pr_ctx_quad[`PC.tail],pr_ctx_owner[`PC.tail]);
  end
  if(`PC.color_fire_write&&`PC.result_last)begin
   if(pr_ctx_quad[`PC.result_context]==0)$fatal(1,"profiler unowned cache commit");
   $fdisplay(pr_events,"Q_COMMIT %0d %0d %0d",@CYCLE@+1,pr_ctx_quad[`PC.result_context],pr_ctx_owner[`PC.result_context]);
   pr_ctx_quad[`PC.result_context]=0;pr_commit=pr_commit+1;
  end
  // Logical DMA job ownership outlives request issue until the real LAST.
  if(`PD.accept_work)begin
   pr_job=pr_job+1;pr_job_slot[`PD.put[0]]=pr_job;
   if(`PD.work_mesh)pr_page_job[`PD.work_slot]=pr_job;
   $fdisplay(pr_events,"DMA_JOB %0d %0d %0d %0d %0d %0d %0d",@CYCLE@,pr_job,`PP.diagnostic_pc,`PD.work_mesh,`PD.work_slot,`PD.work_source,`PD.work_size);
  end
  // Retire the old descriptor before a simultaneous successor takes its slot.
  if(`PD.pop)begin
   $fdisplay(pr_events,"DMA_RETURN %0d %0d %0d",@CYCLE@+1,pr_ret_tx[`PD.rh[0]],pr_ret_job[`PD.rh[0]]);
   if(!`PD.rk[`PD.rh[0]]&&`PD.rl[`PD.rh[0]])$fdisplay(pr_events,"DMA_DONE %0d %0d",@CYCLE@+1,pr_job_slot[`PD.retired[0]]);
  end
  if(`PD.take)begin
   pr_tx=pr_tx+1;
   pr_ret_job[`PD.rp[0]]=`PD.offer_cache?0:pr_job_slot[`PD.issued[0]];
   pr_ret_tx[`PD.rp[0]]=pr_tx;
   $fdisplay(pr_events,"DMA_REQUEST %0d %0d %0d %0d %0d",@CYCLE@,pr_tx,pr_ret_job[`PD.rp[0]],`PD.request_address,`PD.request_count);
  end
  pr_vkind=0;
  if(`PF.meshlet.busy)begin
   if(!@ENABLE@)pr_vkind=5;
   else if(`PV.step)pr_vkind=`PV.triangle_mode?2:1;
   else if(`PF.meshlet.state==13&&!`PF.meshlet.triangle_ready)pr_vkind=3;
   else pr_vkind=4;
  end
  $fdisplay(pr_load,"%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d",
   @CYCLE@,!`PD.idle,`PF.meshlet.busy,@ENABLE@&&`PV.step,`PV.count,`PF.raster_busy,`PI.p_busy,
   `PQ.global_used,`PQ.producer_used,`PQ.light_pending,`PQ.sample_pending,
   pr_ltokens,pr_stokens,(`PQ.publish_ptr[4:0]-`PB.issue_slot)&31,`PB.final_color.leaf.credits,
   (`PC.ctx_valid[0]?1:0)+(`PC.ctx_valid[1]?1:0),`PQ.allocate,
   @ENABLE@&&`PF.depth_event_valid,`PB.front.pixel_accepted,`PB.sample_fire,`PB.final_issue,`PB.lane_accept,
   pr_mesh,pr_vkind,pr_raster,pr_interp,`PI.s_busy,pr_prepare,
   `PB.work_count,`PB.work_reserved,$countones(`PB.public_live),`PC.hint_count,`PC.mt_active,
   @GPU@.ro_request_valid&&!@GPU@.ro_request_ready,@GPU@.fb_r_request_valid&&!@GPU@.fb_r_request_ready,@GPU@.fb_w_request_valid&&!@GPU@.fb_w_request_ready,@ENABLE@,
   `PB.front.light_compute_tick&&(|`PB.front.light.core.valid_pipe),
   `PB.front.sampling.advance&&(`PB.front.d_issue||`PB.front.l_issue||`PB.front.c_issue));
  pr_ltokens=pr_ltokens+(`PB.front.pixel_accepted?1:0)-(`PB.light_result?1:0);
  pr_stokens=pr_stokens+(`PB.public_publish?1:0)-((`PB.sample_result&&`PB.sl)?1:0);
  if(pr_ltokens<0||pr_stokens<0)$fatal(1,"profiler branch token underflow");

 end
 if(pr_capturing&&!pr_live&&pr_submission>=pr_max_submissions)begin
  $fdisplay(pr_events,"CAPTURE_END %0d",@CYCLE@);$fclose(pr_events);$fclose(pr_load);pr_done=1;
  if(pr_finish)$finish;
 end
 end
 if(@CYCLE@>=pr_limit&&!pr_done)begin
  $fdisplay(pr_events,"BOUNDED_STOP %0d",@CYCLE@);$fclose(pr_events);$fclose(pr_load);
  $display("PROFILER_BOUNDED_STOP @CYCLE@=%0d",@CYCLE@);$finish;
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
