`timescale 1ns/1ps
// Test-only credit sink: this is not the adopted physical result pool.
module tb;
reg clk=0; always #5 clk=~clk;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
localparam DATA="@DATA@", NV=@NV@, NT=@NT@, NP=@NP@, MAXC=@MAXC@, STRESS=@STRESS@;
reg reset=1,ce=1,start=0,slot=@SLOT@,dma_we=0,output_ready=1;
reg[9:0]dma_addr=0;reg[63:0]dma_data=0;wire dma_ready;
reg[31:0]cfg_far_z=@FAR@;reg[4:0]cfg_tex_w_log2=@TW@,cfg_tex_h_log2=@TH@,cfg_max_mip=@MIP@;
reg signed[7:0]cfg_lod_bias=@BIAS@;
wire quad_valid,quad_ready,quad_rgb;wire[2:0]quad_id;wire[7:0]quad_x;wire[6:0]quad_y;wire[1:0]phase;
wire quad_end_valid;wire[2:0]quad_end_id;wire[3:0]quad_end_mask;
wire result_we;wire[6:0]result_wa;wire[35:0]result_wd;
wire depth_event_valid;wire[4:0]depth_event_pid;wire[15:0]depth_event_z16;wire depth_event_pass;
wire lod_valid;wire[2:0]lod_id;wire[7:0]lod_value;
wire frontend_busy,raster_busy,interpolator_busy,meshlet_done,meshlet_ready,done_raster,idle;
wire[3:0]frontend_error,raster_error;wire[16:0]interpolator_error;wire[6:0]published_vertices;
wire emit_valid;wire[9:0]emit_addr;wire[35:0]emit_data;
gpu_v2_vliw_frontend dut(.*);
// Behavioral test sink only: eight paid old-state credits, no shader or math.
reg[31:0]allocated=0,released=0;
wire[3:0]occupancy=allocated-released;
assign quad_ready=occupancy<8&&!reset;
assign quad_id=allocated[2:0];
reg out_ready=0;
wire consumer_busy=allocated!=released;
reg[11:0]written_words[0:7];reg[3:0]written_depth[0:7];
reg written_lod[0:7],written_geometry[0:7];
integer clear_slot;
always @(posedge clk)begin
 if(reset)begin
  allocated<=0;released<=0;
  for(clear_slot=0;clear_slot<8;clear_slot=clear_slot+1)begin
   written_words[clear_slot]<=0;written_depth[clear_slot]<=0;written_lod[clear_slot]<=0;written_geometry[clear_slot]<=0;
  end
 end else if(ce)begin
  if(quad_valid&&quad_ready)begin
   allocated<=allocated+1;
   written_words[quad_id]<=0;written_depth[quad_id]<=0;written_lod[quad_id]<=0;written_geometry[quad_id]<=0;
  end
  if(result_we)written_words[result_wa[6:4]][result_wa[3:2]*3+result_wa[1:0]]<=1;
  if(depth_event_valid)written_depth[depth_event_pid[4:2]][depth_event_pid[1:0]]<=1;
  if(lod_valid)written_lod[lod_id]<=1;
  if(quad_end_valid)written_geometry[quad_end_id]<=1;
  if(consumer_busy&&out_ready&&(&written_words[released[2:0]])&&(&written_depth[released[2:0]])&&written_lod[released[2:0]]&&written_geometry[released[2:0]])released<=released+1;
 end
end
reg[35:0]vertices[0:383];reg[31:0]descriptors[0:95],admissions[0:95];reg[53:0]words[0:1823];
integer cycle=0,en=0,fd,dm,rc,addr,j,vi=0,ti=0,pi=0,wi=0,qn=0,qw=0,qr=0,due[0:7];
integer slot_seq[0:7],sequence_id=0,slot_packet[0:7],slot_qi[0:7],saw_done=0;
integer peak=0,full_ce=0,headers_with_pending=0,ce0_checks=0;
reg[255:0]frozen_state;
integer qid,packet,qi,index,seq,slot_index;
reg[63:0]dma_value;reg[89:0]h00;reg scalar_active=0;reg[1:0]scalar_lane;
reg[467:0]record;reg accepted,dma_taken,start_taken;
reg[31:0]rng=32'h92cd3471;reg oh=0,dh=0;reg[39:0]held_out;reg[32:0]held_depth;
always @(posedge clk)begin
 cycle=cycle+1;if(cycle>MAXC)$fatal(1,"frontend bounded watchdog");

 // DMA has an independent clock-enable contract in the adopted input bank.
 dma_taken=!reset&&dma_we&&dma_ready;
 start_taken=ce&&!reset&&start&&idle&&meshlet_ready;
 if(!reset&&ce)begin
  en=en+1;if(meshlet_done)saw_done=1;
  if(occupancy>peak)peak=occupancy;
  if(occupancy==8)full_ce=full_ce+1;
  if(frontend_error||raster_error||interpolator_error)$fatal(1,"frontend error %h/%h/%h",frontend_error,raster_error,interpolator_error);
  if(dut.header_take)begin
   if(pi>=NP||dut.fd!==descriptors[pi])$fatal(1,"packet descriptor %0d",pi);
   $fdisplay(fd,"P %0d %0d %h",cycle,pi,dut.fd);pi=pi+1;wi=0;
   if(consumer_busy)headers_with_pending=headers_with_pending+1;
  end
  if(dut.fwv&&dut.fwr)begin
   if(wi>=16||dut.fwi!==wi[4:0]||dut.fwd!==words[(pi-1)*16+wi])$fatal(1,"raw16 packet%0d word%0d",pi-1,wi);
   wi=wi+1;
  end
  if(dut.meshlet.core.launch_t)begin
   if(vi!=NV*5||ti>=NT||dut.meshlet.core.triangle_descriptor!==admissions[ti])$fatal(1,"allV then allT");
   ti=ti+1;
  end
  if(emit_valid&&output_ready)begin
   if(vi>=NV*5||emit_addr!==vi[9:0]||emit_data!==vertices[vi])$fatal(1,"V96 output %0d",vi);
   vi=vi+1;
  end
  if(quad_valid&&quad_ready)begin
   if(occupancy>=8)$fatal(1,"same-edge free credit");
   slot_seq[quad_id]=sequence_id;slot_packet[quad_id]=pi-1;slot_qi[quad_id]=quad_y*200+quad_x;
   $fdisplay(fd,"A %0d %0d %0d %0d %0d %0d",cycle,sequence_id,pi-1,quad_id,quad_x,quad_y);sequence_id=sequence_id+1;
  end
  if(dut.sample_valid)begin scalar_active=1;scalar_lane=dut.sample_lane;end
  if(scalar_active&&phase<3)begin
   if(scalar_lane==0)h00[phase*30+:30]=dut.sample_h;
   if(phase==2)scalar_active=0;
  end
  if(quad_end_valid)$fdisplay(fd,"E %0d %0d %h %h",cycle,slot_seq[quad_end_id],quad_end_mask,h00);
  if(result_we)begin
   if(((result_wa[6:4]-released[2:0])&7)>=occupancy)$fatal(1,"result without owned slot");
   $fdisplay(fd,"W %0d %0d %0d %0d %h",cycle,slot_seq[result_wa[6:4]],result_wa[3:2],result_wa[1:0],result_wd);
  end
  if(depth_event_valid)begin
   if(((depth_event_pid[4:2]-released[2:0])&7)>=occupancy)$fatal(1,"depth without owned slot");
   $fdisplay(fd,"N %0d %0d %0d %0d %0d",cycle,slot_seq[depth_event_pid[4:2]],depth_event_pid[1:0],depth_event_z16,depth_event_pass);
  end
  if(lod_valid)begin
   if(((lod_id-released[2:0])&7)>=occupancy)$fatal(1,"LOD without owned slot");
   $fdisplay(fd,"L %0d %0d %0d",cycle,slot_seq[lod_id],lod_value);
  end

 end
end
task tick;begin
 @(negedge clk);rng=rng*32'd1664525+32'd1013904223;
 ce=!STRESS||rng[2:0]!=0;output_ready=!STRESS||rng[5:3]!=0;
 out_ready=!STRESS||(rng[8:6]!=0&&!(cycle%997>=100&&cycle%997<250));

 #1;

 frozen_state={phase,dut.interpolator.compute.valid_pipe,dut.raster.w_pc,dut.raster.life,allocated,released};
 @(posedge clk);#2;
 if(!reset&&!ce)begin
  if(frozen_state!=={phase,dut.interpolator.compute.valid_pipe,dut.raster.w_pc,dut.raster.life,allocated,released})$fatal(1,"CE0 architectural hold");
  ce0_checks=ce0_checks+1;
 end
end endtask
initial begin
 $readmemh({DATA,"/vertices.hex"},vertices,0,NV*5-1);$readmemh({DATA,"/admissions.hex"},admissions,0,NT-1);
 $readmemh({DATA,"/descriptors.hex"},descriptors,0,NP-1);$readmemh({DATA,"/words.hex"},words,0,NP*16-1);
 fd=$fopen({DATA,"/events.txt"},"w");if(fd==0)$fatal(1,"event file");tick();reset=0;
 dm=$fopen({DATA,"/dma.txt"},"r");
 while(!$feof(dm))begin rc=$fscanf(dm,"%d %h",addr,dma_value);if(rc==2)begin
  dma_addr=addr;dma_data=dma_value;dma_we=1;accepted=0;while(!accepted)begin tick();accepted=dma_taken;end
 end end $fclose(dm);dma_we=0;
 while(!idle||!meshlet_ready)tick();start=1;accepted=0;while(!accepted)begin tick();accepted=start_taken;end start=0;
 while(!saw_done||!idle||consumer_busy)tick();
 if(vi!=NV*5||ti!=NT||pi!=NP||published_vertices!=NV)$fatal(1,"final counts");
 $display("PASS FRONTEND cycles=%0d quads=%0d peak=%0d full_ce=%0d headers_pending=%0d ce0=%0d",cycle,sequence_id,peak,full_ce,headers_with_pending,ce0_checks);$fclose(fd);$finish;
end
initial begin #@MAXTIME@;$fatal(1,"HDL timeout");end
endmodule
