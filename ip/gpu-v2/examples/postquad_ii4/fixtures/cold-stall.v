module tb;integer unique_i,unique_j;integer stream_fd,matched_line,resident_count,stream_tile;initial stream_fd=$fopen("stream-context.txt","w");integer order_fd;initial order_fd=$fopen("cache-order.txt","w");integer own_dist,own_pub,own_alloc,owned_reads=0,full_edges=0,full_tail_edges=0,unfinished_reads=0,wrap_publications=0,description_checks=0;reg[22:0]canonical_gold[0:31];integer issue_fd,color_fd;initial begin issue_fd=$fopen("issues.txt","w");color_fd=$fopen("color-stream.txt","w");end
 `ifdef GPU_V2_GOWIN_DSP
 GSR GSR(.GSRI(1'b1));
 `endif
 reg clk=0,reset=1,ce=0,abort=0,in_valid=0,draw_rebind=0,flush_request=0;
 reg[22:0]in_begin=0;reg[31:0]in_basic=0;reg[35:0]in_light=0,in_uv=0;
 reg[26:0]draw_config=27'h5600080;reg[23:0]specular=24'h382818;reg[7:0]alpha=128;
 reg[2:0]depth_func=3;reg depth_write=1,blend=1;
 reg[15:0]context_epoch=1;reg[1:0]context_mode=3;reg[4:0]context_code=8;
 reg signed[15:0]light_x=0,light_y=0,light_z=16384,ray_x=-8192,ray_y=-8192,ray_k=8192;
 reg[8:0]ambient=32,directional=192;
 wire in_ready;wire[2:0]in_row;wire flush_complete,pipeline_idle,idle,fault,published;
 wire[4:0]published_slot,work_reserved,work_count;wire[5:0]global_used;
 wire light_result,sample_result,final_issue,final_result,quad_accepted,commit_valid;
 wire[8:0]commit_x;wire[7:0]commit_y;wire[3:0]commit_mask;
 wire mem_req_valid,mem_req_write,mem_write_valid;wire[31:0]mem_req_addr;wire[63:0]mem_write_data;
 reg mem_req_ready=0,mem_read_valid=0,mem_write_ready=0,mem_complete_valid=0,mem_complete_ok=1;reg[63:0]mem_read_data=0;
 gpu_v2_postquad dut(.*);
 localparam PRIME=128,TOTAL=384,STALL=1,N=256;
 reg[126:0]rom[0:TOTAL*8-1];reg[23:0]rgb_gold[0:TOTAL*4-1],sample_gold[0:TOTAL*4-1];reg[17:0]light_gold[0:TOTAL-1];
 reg[7:0]memory[0:416767],gold[0:416767];
 reg[23:0]actual_rgb[0:TOTAL*4-1];reg[20:0]actual_commit[0:TOTAL-1];
 integer slot_job[0:31];integer pub_time[0:255],retire_time[0:255],commit_time[0:255];
 integer wall,cursor=0,pubs=0,retires=0,commits=0,phase=0,pending=0,beat=0,delay_count=0;
 integer reqs=0,txreqs=0,fbreads=0,fbwrites=0,readbeats=0,writebeats=0,ce0paid=0;
 integer measured_start=0,source_done=0,drained=0,first_final=0,last_final=0,finals=0,peak_global=0,peak_work=0;
 integer address=0,i,k,job,lane,idx,prime_done=0;reg[31:0]held_req;reg held_valid=0,held_write;
 reg[63:0]held_data;reg held_wvalid=0;reg[126:0]held_source;reg held_source_valid=0;
 function integer mi;input[31:0]a;begin
  if(a>=32'h140000)mi=224768+a-32'h140000;
  else if(a>=32'h100000)mi=32768+a-32'h100000;else mi=a;
  if(mi<0||mi>416767)$fatal(1,"memory address %h",a);
 end endfunction
 initial begin #2000000;$fatal(1,"HDL watchdog");end
 initial begin
  $readmemh("rom.mem",rom);$readmemh("initial.mem",memory);$readmemh("gold.mem",gold);
  $readmemh("rgb.mem",rgb_gold);$readmemh("sample.mem",sample_gold);$readmemh("light.mem",light_gold);
  for(i=0;i<32;i=i+1)slot_job[i]=-1;
  #1;clk=1;#1;clk=0;#1;reset=0;
  for(wall=1;wall<200000;wall=wall+1)begin
   ce=!STALL||(wall%29!=3&&wall%29!=4);
   if(phase==0&&cursor==PRIME*8)phase=1;
   if(phase==1&&idle)begin phase=2;measured_start=wall;prime_done=wall;end
   in_valid=(phase==0||phase==2)&&cursor<TOTAL*8;
   if(in_valid)begin
    in_begin=rom[cursor][126:104];in_basic=rom[cursor][103:72];in_light=rom[cursor][71:36];in_uv=rom[cursor][35:0];
   end
   mem_req_ready=pending==0&&(!STALL||wall%7!=1);
   mem_read_valid=pending==1&&delay_count==0&&(!STALL||wall%5!=2);
   mem_write_ready=pending==3&&delay_count==0&&(!STALL||wall%7!=2);
   mem_complete_valid=(pending==2||pending==4)&&delay_count==0;
   mem_read_data=0;if(mem_read_valid)for(k=0;k<8;k=k+1)mem_read_data[k*8+:8]=memory[mi(address+beat*8+k)];
   #2;
   if(dut.sink.cache.leaf_ce&&dut.sink.cache.leaf.v2&&!dut.shade_phase)$fatal(1,"ROP phase");
   if(final_issue&&dut.shade_phase)$fatal(1,"Final phase");
   if(dut.sink.cache.row_accept&&dut.sink.phase!==dut.shade_phase)$fatal(1,"sink phase");
   if(dut.shared_rop_ce&&dut.shared_spec_ce)$fatal(1,"shared conflict");
   if(dut.sink.cache.leaf_ce!==dut.advance)$fatal(1,"shared pause mismatch");
   own_pub=(dut.front.quad.publish_ptr-dut.front.quad.retire_ptr)&31;
   own_alloc=(dut.front.quad.alloc_ptr-dut.front.quad.retire_ptr)&31;
   own_dist=(dut.basic_address[7:3]-dut.front.quad.retire_ptr[3:0])&15;
   if(own_pub!=dut.published_count||own_alloc>16||own_pub>own_alloc||own_alloc-own_pub>1)$fatal(1,"canonical owner pointer invariant");
   if(own_alloc==16)begin full_edges=full_edges+1;if(own_alloc>own_pub)full_tail_edges=full_tail_edges+1;end
   if(in_valid&&in_ready&&in_row==0&&in_begin[3:0]!=0)begin
    if(own_alloc>=16)$fatal(1,"allocation paid from same edge retirement");
    canonical_gold[dut.front.quad.alloc_ptr[3:0]]=in_begin;
   end
   if(published)begin
    if(dut.published_begin!==canonical_gold[published_slot])$fatal(1,"published canonical description changed");
    if(published_slot==15)wrap_publications=wrap_publications+1;
   end
   if(dut.advance&&dut.basic_read)begin
    owned_reads=owned_reads+1;if(own_alloc>own_pub)unfinished_reads=unfinished_reads+1;
    if(own_dist>=own_pub)$fatal(1,"actual Basic read unpublished");
    if(dut.issue_header!==canonical_gold[dut.issue_slot])$fatal(1,"issued canonical description changed");
    description_checks=description_checks+1;
   end
   if(dut.active&&dut.advance&&dut.lane_valid)begin
    if({dut.header[22:8],dut.header[3:0]}!=={canonical_gold[dut.retire_slot][22:8],canonical_gold[dut.retire_slot][3:0]})$fatal(1,"retirement description changed");
    description_checks=description_checks+1;
   end
   if(dut.advance&&dut.raw_retire_proposal!==quad_accepted)$fatal(1,"paid raw retirement equivalence");
   if(dut.advance&&dut.raw_head_retire!==quad_accepted)$fatal(1,"exact raw retirement equivalence");
   if(dut.front.color.input_fire)$fdisplay(order_fd,"%h %h %h %h",dut.front.demand.res_payload,{dut.front.demand.res_tex[3],dut.front.demand.res_tex[2],dut.front.demand.res_tex[1],dut.front.demand.res_tex[0]},dut.front.color.in_payload,dut.front.color.in_texels);
   if(dut.sink.cache.row0_fire||dut.sink.cache.depth0_fire)begin
    if({dut.sink.cache.rde3,dut.sink.cache.rde2,dut.sink.cache.rde1,dut.sink.cache.rde0}!==4'hf)$fatal(1,"input read port enable");
    if({dut.sink.cache.rda0,dut.sink.cache.rda1,dut.sink.cache.rda2,dut.sink.cache.rda3}!=={dut.sink.cache.source_addr0,dut.sink.cache.source_addr1,dut.sink.cache.source_addr2,dut.sink.cache.source_addr3})$fatal(1,"input read port addresses");
   end
   if(dut.sink.cache.row0_fire)begin
    stream_tile=(dut.sink.cache.row_y>>4)*25+(dut.sink.cache.row_x>>4);resident_count=0;matched_line=0;
    for(k=0;k<8;k=k+1)if(dut.sink.cache.tvalid[k]&&dut.sink.cache.tag[k]==stream_tile)begin resident_count=resident_count+1;matched_line=k;end
    if(resident_count!=1||matched_line!=dut.sink.cache.hit_line)$fatal(1,"independent resident line identity");
    if({dut.sink.cache.row_x,dut.sink.cache.row_y,dut.sink.cache.row_mask}!=={canonical_gold[dut.retire_slot][15:8],1'b0,canonical_gold[dut.retire_slot][22:16],1'b0,canonical_gold[dut.retire_slot][3:0]})$fatal(1,"canonical context row0");
   end
   $fdisplay(stream_fd,"%0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d %0d",wall,0,dut.advance,dut.sink.cache.row_accept,dut.sink.cache.row_index,dut.retire_slot,dut.sink.cache.hit_line,dut.sink.cache.row_x,dut.sink.cache.row_y,dut.sink.cache.row_mask,dut.sink.cache.return_fire,dut.sink.cache.return_key,dut.sink.cache.depth_fire,dut.sink.cache.depth_key,dut.sink.cache.depth_pending,dut.sink.cache.result_bank,dut.sink.cache.depth_bank,dut.sink.cache.leaf_cw,dut.sink.cache.depth_flag,dut.sink.cache.result_addr0,dut.sink.cache.result_addr1,dut.sink.cache.result_addr2,dut.sink.cache.result_addr3,dut.sink.cache.source_addr0,dut.sink.cache.source_addr1,dut.sink.cache.source_addr2,dut.sink.cache.source_addr3,dut.sink.cache.row0_fire,dut.sink.cache.depth0_fire,commit_valid,commit_x,commit_y,commit_mask,{dut.sink.cache.wre3,dut.sink.cache.wre2,dut.sink.cache.wre1,dut.sink.cache.wre0});
   if(dut.sink.cache.row0_fire||dut.sink.cache.depth0_fire)begin
    if({dut.sink.cache.rde3,dut.sink.cache.rde2,dut.sink.cache.rde1,dut.sink.cache.rde0}!==4'hf)$fatal(1,"input read port enable");
    if({dut.sink.cache.rda0,dut.sink.cache.rda1,dut.sink.cache.rda2,dut.sink.cache.rda3}!=={dut.sink.cache.source_addr0,dut.sink.cache.source_addr1,dut.sink.cache.source_addr2,dut.sink.cache.source_addr3})$fatal(1,"input read port addresses");
   end
   for(unique_i=0;unique_i<8;unique_i=unique_i+1)for(unique_j=unique_i+1;unique_j<8;unique_j=unique_j+1)if(dut.sink.cache.tvalid[unique_i]&&dut.sink.cache.tvalid[unique_j]&&dut.sink.cache.tag[unique_i]==dut.sink.cache.tag[unique_j])$fatal(1,"valid tag duplicate");
   if(fault)$fatal(1,"top fault wall%0d source%0d phase%0d global%0d state%0d front%0d sink%0d final%0d",wall,cursor,phase,global_used,dut.state,dut.front_fault,dut.sink_fault,dut.final_fault);
   if(global_used>peak_global)peak_global=global_used;if(work_reserved>peak_work)peak_work=work_reserved;
   if(held_valid&&(!mem_req_valid||mem_req_addr!==held_req||mem_req_write!==held_write))$fatal(1,"request hold");
   held_valid=mem_req_valid&&!mem_req_ready;held_req=mem_req_addr;held_write=mem_req_write;
   if(held_wvalid&&(!mem_write_valid||mem_write_data!==held_data))$fatal(1,"write hold");
   held_wvalid=mem_write_valid&&!mem_write_ready;held_data=mem_write_data;
   if(held_source_valid&&in_valid&&rom[cursor]!==held_source)$fatal(1,"source changed while blocked");
   held_source_valid=in_valid&&!in_ready;held_source=in_valid?rom[cursor]:0;
   if(published)begin
    slot_job[published_slot]=pubs;
    if(pubs>=PRIME)pub_time[pubs-PRIME]=wall;
    pubs=pubs+1;
   end
   if(light_result)begin
    job=slot_job[dut.lk[6:2]];
    if(job<0||{dut.lh,dut.lg}!==light_gold[job])$fatal(1,"light job%0d g%0d h%0d expected%h",job,dut.lg,dut.lh,light_gold[job]);
   end
   if(sample_result)begin
    job=slot_job[dut.sample_address[6:2]];lane=dut.sk[1:0];
    if(job<0)$fatal(1,"sample owner missing"); // Actual quantized payload matched to unchanged reference.
   end
   if(dut.front.color.input_fire) $fdisplay(color_fd,"I %h %h",dut.front.color.in_payload,dut.front.color.in_texels);
   if(dut.front.color.result_fire) $fdisplay(color_fd,"O %h %h",dut.front.color.out_key,dut.front.color.out_rgb);
   if(final_issue)begin
    $fdisplay(issue_fd,"%0d %0d %h %h %h %0d %0d",slot_job[dut.issue_slot],dut.issue_lane,dut.basic_data,dut.final_texture,specular,dut.final_g,dut.final_h);
   end
   if(final_result)begin
    job=slot_job[dut.retire_slot];lane=dut.out_lane;
    actual_rgb[job*4+lane]=dut.final_RGB;
    // SH9/Color9/511/alpha6 Final is compared exactly against the unchanged actual SH9/Color9/511/alpha6 reference above.
    // Exact-profile RGB gold is not a SH9/Color9/511/alpha6 arithmetic oracle.
    if(job>=PRIME)begin if(finals==0)first_final=wall;last_final=wall;finals=finals+1;end
   end
   if(quad_accepted)begin if(retires>=PRIME)retire_time[retires-PRIME]=wall;retires=retires+1;end
   if(commit_valid)begin
    actual_commit[commits]={commit_x,commit_y,commit_mask}; if(commits>=PRIME)commit_time[commits-PRIME]=wall;commits=commits+1;end
   if(in_valid&&in_ready)begin
    if(in_row!==((cursor%8)&3'b111))$fatal(1,"source row cursor");cursor=cursor+1;
    if(cursor==TOTAL*8)begin source_done=wall;phase=3;end
   end
   if(mem_req_valid&&mem_req_ready)begin
    if(pending!=0)$fatal(1,"overlapping request");
    address=mem_req_addr;beat=0;delay_count=6;pending=mem_req_write?3:1;
    if(phase>=2)begin
     reqs=reqs+1;
     if(address<32'h100000)txreqs=txreqs+1;else if(mem_req_write)fbwrites=fbwrites+1;else fbreads=fbreads+1;
    end
   end else if(delay_count>0)delay_count=delay_count-1;
   if(mem_read_valid)begin
    if(phase>=2)readbeats=readbeats+1;
    if(!ce)ce0paid=ce0paid+1;
    beat=beat+1;if(beat==16)begin pending=2;delay_count=2;end
   end
   if(mem_write_valid&&mem_write_ready)begin
    for(k=0;k<8;k=k+1)memory[mi(address+beat*8+k)]=mem_write_data[k*8+:8];
    if(phase>=2)writebeats=writebeats+1;
    beat=beat+1;if(beat==16)begin pending=4;delay_count=2;end
   end
   if(mem_complete_valid)pending=0;
   if(phase==3&&idle)begin drained=wall;phase=4;flush_request=1;end
   if(phase==4&&flush_complete)begin
    if(pubs!=TOTAL||retires!=TOTAL||commits!=TOTAL||finals!=1024)$fatal(1,"completion counts %0d %0d %0d %0d",pubs,retires,commits,finals);
    // Full DRAM equivalence follows identical initial memory, every request/address,
    // and every accepted write word/handshake matched against the original SH9/Color9/511/alpha6 DUT.
    $writememh("actual-memory.mem",memory);$writememh("actual-rgb.mem",actual_rgb);$writememh("actual-commit.mem",actual_commit);
    $display("METRICS start=%0d prime=%0d source_done=%0d drain=%0d flush=%0d pub_first=%0d pub_last=%0d retire_first=%0d retire_last=%0d commit_first=%0d commit_last=%0d pub_window=%0d retire_window=%0d commit_window=%0d window_intervals=127 final_first=%0d final_last=%0d finals=%0d requests=%0d tx=%0d fbread=%0d fbwrite=%0d readbeats=%0d writebeats=%0d ce0paid=%0d peak_global=%0d peak_work=%0d",measured_start,prime_done,source_done,drained,wall,pub_time[0],pub_time[255],retire_time[0],retire_time[255],commit_time[0],commit_time[255],pub_time[223]-pub_time[96],retire_time[223]-retire_time[96],commit_time[223]-commit_time[96],first_final,last_final,finals,reqs,txreqs,fbreads,fbwrites,readbeats,writebeats,ce0paid,peak_global,peak_work);
    $display("PASS standalone complete chain, bounded completion, independent Lighting gold; Final/DRAM compared offline to frozen actual profile");$finish;
   end
   clk=1;#2;clk=0;#1;
  end
  $fatal(1,"wall bound phase%0d source%0d pub%0d ret%0d commit%0d global%0d state%0d pending%0d",phase,cursor,pubs,retires,commits,global_used,dut.state,pending);
 end endmodule
 