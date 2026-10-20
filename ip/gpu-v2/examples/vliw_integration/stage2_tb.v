`timescale 1ns/1ps
// Bounded full-factory replay. The memory peer pays every 128-byte transaction.
module tb;
`ifdef GPU_V2_GOWIN_DSP
GSR GSR(.GSRI(1'b1));
`endif
reg clk=0,reset=1,ce=1,abort=0,start=0,slot=0,dma_we=0,output_ready=1;
reg[9:0]dma_addr=0;reg[63:0]dma_data=0;wire dma_ready;
reg table_valid=0;wire table_ready;reg[7:0]table_index=0;reg[35:0]table_data=0;
reg[31:0]cfg_far_z=199*65536;reg[4:0]cfg_tex_w_log2=6,cfg_tex_h_log2=6,cfg_max_mip=6;reg signed[7:0]cfg_lod_bias=0;
reg draw_bank=0,draw_unlit=0;reg[26:0]draw_config=(2<<25)|(1<<24)|(6<<20);
reg[23:0]specular=0;reg[7:0]alpha=255;reg[2:0]depth_func=1;reg depth_write=1,blend=0,draw_rebind=0;
reg[15:0]context_epoch=1;reg[1:0]context_mode=2;reg[4:0]context_code=8;
reg signed[15:0]light_x=-11585,light_y=0,light_z=-11585,ray_x=-8192,ray_y=-8192,ray_k=8192;
reg[8:0]ambient=32,directional=192;
reg flush_request=0;wire flush_complete,pipeline_idle,idle,fault,frontend_idle,meshlet_done,meshlet_ready;
wire[6:0]published_vertices;wire emit_valid;wire[9:0]emit_addr;wire[35:0]emit_data;
wire quad_admitted,quad_empty_skipped,quad_published,light_result,sample_result,final_issue,final_result,quad_accepted,commit_valid;
wire[8:0]commit_x;wire[7:0]commit_y;wire[3:0]commit_mask;wire[5:0]global_used,producer_used;wire[4:0]work_reserved,work_count;
wire mem_req_valid,mem_req_write,mem_write_valid;wire[31:0]mem_req_addr;wire[63:0]mem_write_data;
reg mem_req_ready=0,mem_read_valid=0,mem_write_ready=0,mem_complete_valid=0,mem_complete_ok=1;reg[63:0]mem_read_data=0;
gpu_v2_vliw dut(.*);
localparam STRESS=@STRESS@,MAXC=@MAXC@,BYTES=@BYTES@,FB=192000,TEX=16384;
reg[7:0]memory[0:BYTES-1];reg[35:0]vertices[0:383],admissions[0:95],descriptors[0:95],rom[0:255];reg[53:0]words[0:1823];
integer cycle=0,en=0,pending=0,address=0,beat=0,delay_count=0,requests=0,paid_ce0=0,k,mi0;
integer fd,bd,ff,dm,rc,addr,j,task_id=-1,vi=0,ti=0,pi=0,wi=0,all_pi=0,nv,nt,np;
integer pending_packet=-1,executing_packet=-1;
integer slot_seq[0:7],physical_seq[0:31],sequence_id=0,peak=0,full_ce=0,headers_pending=0,starts_pending=0;
integer qpub=0,skips=0,commits=0,nlight=0,nsample=0,nfinal=0,ce0_checks=0;
integer memory_hold_until=0;
reg[63:0]dma_value;reg[89:0]h00;reg scalar_active=0;reg[1:0]scalar_lane;
reg table_taken,dma_taken,start_taken,saw_done=0,inputs_done=0;
reg[31:0]rng=32'h193fa2a7;reg[127:0]frozen;
function integer mi;input[31:0]a;begin
 if(a>=32'h140000)mi=TEX+FB+a-32'h140000;
 else if(a>=32'h100000)mi=TEX+a-32'h100000;
 else mi=a-32'h1000;
 if(mi<0||mi>=BYTES)$fatal(1,"memory address %h",a);
end endfunction
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>MAXC)$fatal(1,"bounded full GPU watchdog");
 table_taken=ce&&!reset&&table_valid&&table_ready;dma_taken=!reset&&dma_we&&dma_ready;
 start_taken=ce&&!reset&&start&&frontend_idle&&meshlet_ready&&!dut.table_allow;
 if(!reset)begin
  if(fault)$fatal(1,"full GPU fault cycle%0d front%h R%h I%h backend%0d",cycle,dut.frontend_error,dut.raster_error,dut.interpolator_error,dut.backend_fault);
  // ROP commit is a paid wall-clock event pulse, including a following CE0 edge.
  if(commit_valid)begin $fdisplay(bd,"C %0d %0d %0d %h",cycle,commit_x,commit_y,commit_mask);commits=commits+1;end
  if(ce)begin
   en=en+1;if(meshlet_done)saw_done=1;
   if(global_used>peak)peak=global_used;if(global_used==24)full_ce=full_ce+1;
   if(global_used>24||producer_used>8)$fatal(1,"physical pool credits");
   if(dut.front.header_take)begin
    pending_packet=all_pi;
    if(pi>=np||dut.front.fd!==descriptors[pi])$fatal(1,"descriptor task%0d packet%0d",task_id,pi);
    $fdisplay(fd,"P %0d %0d %h",cycle,all_pi,dut.front.fd);pi=pi+1;all_pi=all_pi+1;wi=0;
    if(global_used)headers_pending=headers_pending+1;
    if(STRESS&&task_id==0&&pi==np)memory_hold_until=cycle+1500;
   end
   if(dut.front.raster.packet_take)executing_packet=pending_packet;
   if(dut.front.fwv&&dut.front.fwr)begin
    if(wi>=16||dut.front.fwi!==wi[4:0]||dut.front.fwd!==words[(pi-1)*16+wi])$fatal(1,"raw16 task%0d packet%0d word%0d",task_id,pi-1,wi);
    wi=wi+1;
   end
   if(dut.front.meshlet.core.launch_t)begin
    if(vi!=nv*5||ti>=nt||dut.front.meshlet.core.triangle_descriptor!==admissions[ti])$fatal(1,"all V then T order");ti=ti+1;
   end
   if(emit_valid&&output_ready)begin
    if(vi>=nv*5||emit_addr!==vi[9:0]||emit_data!==vertices[vi])$fatal(1,"V output task%0d word%0d",task_id,vi);vi=vi+1;
   end
   if(quad_admitted)begin
    slot_seq[dut.qid]=sequence_id;physical_seq[dut.back.front.quad.allocated[4:0]]=sequence_id;
    if(executing_packet<0)$fatal(1,"quad before execution packet");
    $fdisplay(fd,"A %0d %0d %0d %0d %0d %0d",cycle,sequence_id,executing_packet,dut.qid,dut.qx,dut.qy);sequence_id=sequence_id+1;
   end
   if(dut.front.sample_valid)begin scalar_active=1;scalar_lane=dut.front.sample_lane;end
   if(scalar_active&&dut.phase<3)begin if(scalar_lane==0)h00[dut.phase*30+:30]=dut.front.sample_h;if(dut.phase==2)scalar_active=0;end
   if(dut.qend)$fdisplay(fd,"E %0d %0d %h %h",cycle,slot_seq[dut.qend_id],dut.qmask,h00);
   if(dut.result_we)$fdisplay(fd,"W %0d %0d %0d %0d %h",cycle,slot_seq[dut.result_wa[6:4]],dut.result_wa[3:2],dut.result_wa[1:0],dut.result_wd);
   if(dut.depth_valid)$fdisplay(fd,"N %0d %0d %0d %0d %0d",cycle,slot_seq[dut.depth_pid[4:2]],dut.depth_pid[1:0],dut.depth_z,dut.depth_pass);
   if(dut.lod_valid)$fdisplay(fd,"L %0d %0d %0d",cycle,slot_seq[dut.lod_id],dut.lod_value);
   if(quad_published)begin $fdisplay(bd,"P %0d %0d %h",cycle,physical_seq[dut.back.published_slot],dut.back.published_begin);qpub=qpub+1;end
   if(quad_empty_skipped)begin $fdisplay(bd,"E %0d %0d",cycle,physical_seq[dut.back.retire_slot]);skips=skips+1;end
   if(light_result)begin $fdisplay(bd,"L %0d %0d %0d %0d %0d",cycle,physical_seq[dut.back.lk[6:2]],dut.back.lk[1:0],dut.back.lg,dut.back.lh);nlight=nlight+1;end
   if(sample_result)begin $fdisplay(bd,"S %0d %0d %0d %h",cycle,physical_seq[dut.back.sample_address[6:2]],dut.back.sk[1:0],dut.back.sRGB);nsample=nsample+1;end
   if(final_result)begin $fdisplay(bd,"F %0d %0d %0d %h",cycle,physical_seq[dut.back.retire_slot],dut.back.out_lane,dut.back.final_RGB);nfinal=nfinal+1;end
   if(quad_accepted)$fdisplay(bd,"R %0d %0d",cycle,physical_seq[dut.back.retire_slot]);
   if(start_taken&&global_used)starts_pending=starts_pending+1;
  end
 end
end
task tick;begin
 #1;rng=rng*1664525+1013904223;
 ce=!STRESS||rng[3:0]!=0;output_ready=!STRESS||rng[6:4]!=0;
 mem_req_ready=pending==0&&cycle%7!=1&&(!STRESS||cycle%1009<800);
 mem_read_valid=pending==1&&delay_count==0&&cycle%5!=2;
 mem_write_ready=pending==3&&delay_count==0&&cycle%7!=2&&(!STRESS||cycle%727<600);
 mem_complete_valid=(pending==2||pending==4)&&delay_count==0;
 if(cycle<memory_hold_until)begin mem_req_ready=0;mem_read_valid=0;mem_write_ready=0;mem_complete_valid=0;end
 mem_read_data=0;if(mem_read_valid)for(k=0;k<8;k=k+1)mem_read_data[k*8+:8]=memory[mi(address+beat*8+k)];
 #2;frozen={dut.phase,dut.front.interpolator.compute.valid_pipe,dut.front.raster.w_pc,dut.back.front.quad.allocated,dut.back.front.quad.retired};
 // The memory peer progresses during CE0. Every return still pays all beats.
 if(mem_req_valid&&mem_req_ready)begin address=mem_req_addr;beat=0;delay_count=3;pending=mem_req_write?3:1;requests=requests+1;end
 else if(delay_count>0)delay_count=delay_count-1;
 if(mem_read_valid)begin if(!ce)paid_ce0=paid_ce0+1;beat=beat+1;if(beat==16)begin pending=2;delay_count=2;end end
 if(mem_write_valid&&mem_write_ready)begin for(k=0;k<8;k=k+1)memory[mi(address+beat*8+k)]=mem_write_data[k*8+:8];beat=beat+1;if(beat==16)begin pending=4;delay_count=2;end end
 if(mem_complete_valid)pending=0;
 clk=1;#2;clk=0;#1;
 if(!reset&&!ce)begin
  if(frozen!=={dut.phase,dut.front.interpolator.compute.valid_pipe,dut.front.raster.w_pc,dut.back.front.quad.allocated,dut.back.front.quad.retired})$fatal(1,"CE0 architectural hold");ce0_checks=ce0_checks+1;
 end
end endtask
task load_and_start;input integer taskno;input integer vn,tn,pn;input use_slot;
reg[1023:0]path;begin
 while(!frontend_idle||!meshlet_ready)tick();task_id=taskno;nv=vn;nt=tn;np=pn;slot=use_slot;vi=0;ti=0;pi=0;wi=0;saw_done=0;
 $sformat(path,"task%0d/vertices.hex",taskno);$readmemh(path,vertices,0,nv*5-1);
 $sformat(path,"task%0d/admissions.hex",taskno);$readmemh(path,admissions,0,nt-1);
 $sformat(path,"task%0d/descriptors.hex",taskno);$readmemh(path,descriptors,0,np-1);
 $sformat(path,"task%0d/words.hex",taskno);$readmemh(path,words,0,np*16-1);
 $sformat(path,"task%0d/dma.txt",taskno);dm=$fopen(path,"r");
 while(!$feof(dm))begin rc=$fscanf(dm,"%d %h",addr,dma_value);if(rc==2)begin
  dma_addr=addr;dma_data=dma_value;dma_we=1;dma_taken=0;while(!dma_taken)tick();
 end end $fclose(dm);dma_we=0;start=1;start_taken=0;while(!start_taken)tick();start=0;
 while(!saw_done||!frontend_idle)tick();
 if(vi!=nv*5||ti!=nt||pi!=np||published_vertices!=nv)$fatal(1,"task completion counts");
end endtask
initial begin
 fd=$fopen("front.txt","w");bd=$fopen("back.txt","w");$readmemh("initial.mem",memory);$readmemh("task0/rom.hex",rom);
 tick();tick();reset=0;
 for(j=0;j<256;j=j+1)begin table_valid=1;table_index=j;table_data=rom[j];table_taken=0;while(!table_taken)tick();end table_valid=0;
 @TASKS@
 inputs_done=1;while(!idle)tick();flush_request=1;while(!flush_complete)tick();
 ff=$fopen("memory.mem","w");for(j=0;j<BYTES;j=j+1)$fdisplay(ff,"%h",memory[j]);$fclose(ff);
 $display("PASS FULL cycles=%0d CE=%0d Q=%0d published=%0d empty=%0d peak=%0d fullCE=%0d headersPending=%0d startsPending=%0d light=%0d sample=%0d final=%0d commits=%0d requests=%0d CE0returns=%0d CE0checks=%0d",cycle,en,sequence_id,qpub,skips,peak,full_ce,headers_pending,starts_pending,nlight,nsample,nfinal,commits,requests,paid_ce0,ce0_checks);
 $fclose(fd);$fclose(bd);$finish;
end
initial begin #@MAXTIME@;$fatal(1,"HDL bounded timeout");end
endmodule
