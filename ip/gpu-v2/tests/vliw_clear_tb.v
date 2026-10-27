`timescale 1ns/1ps
module tb;
@PORTS@
reg[31:0]command_base=0,command_bytes=0;
`ifndef PORTABLE
GSR GSR(.GSRI(1'b1));
`endif
always #5 clk=~clk;
gpu_v2_vliw_command dut(.*);
reg[63:0]memory[0:8191],image[0:65535];
reg[35:0]vertices[0:59];
integer cycle=0,mode=0,bi=0,bn=0,ba=0,starts=0,done_count=0,vi=0,overlap=0,ce0=0;
integer cachefills=0,material_changes=0,j,rbase=0,ri=0,wbase=0,wi=0,wdelay=0;
integer write_requests=0,write_terms=0,write_beats=0,clear_writes=0,preserve_writes=0;
integer held_requests=0,ce0_beats=0,same_terminal=0,delayed_terminal=0,commits=0,changed=0;
integer ce_hold_left=0,ce_hold_checks=0;
reg[16:0]ce_hold_pc=0;
reg ce_hold_started=0,ce_hold_edge=0;
reg active=0,ractive=0,wactive=0,held=0;
reg[23:0]held_request=0;
reg[63:0]descriptor;
function integer index_of;
 input integer halfword;
 integer bytes;
 begin
  bytes=halfword*2;
  if(bytes>=32'h110000&&bytes<32'h13ee00)index_of=(bytes-32'h110000)/8;
  else if(bytes>=32'h150000&&bytes<32'h17ee00)index_of=32768+(bytes-32'h150000)/8;
  else begin $fatal(1,"unexpected framebuffer address %h",bytes);index_of=0;end
 end
endfunction
wire fail_write=(mode==3||mode==5)&&write_requests==1&&wi==5;
wire same_write=(write_requests%2)==0;
always @(negedge clk)begin
 ce=cycle%7!=2;
 ro_request_ready=!active&&cycle%5!=1;
 ro_response_valid=active&&cycle%3!=1;ro_data=memory[(ba>>2)+bi];
 ro_response_error=0;ro_response_last=bi==bn-1;
 fb_r_request_ready=!ractive;fb_r_response_valid=ractive&&cycle%3!=2;
 fb_r_response_last=ri==15;fb_r_response_error=0;
 fb_r_data=ractive?image[rbase+ri]:0;
 fb_w_request_ready=!wactive&&cycle%5!=1&&(mode!=1||abort);
 fb_w_data_ready=wactive&&wi<16&&cycle%4!=2&&!fail_write;
 fb_w_response_valid=wactive&&(fail_write||(wi==16&&wdelay==0)||
  (same_write&&wi==15&&fb_w_data_ready));
 fb_w_response_last=1;fb_w_response_error=fail_write;
 // First abort is presented on the same edge as a successful true terminal.
 if(mode==7&&write_requests==1&&fb_w_response_valid&&!fb_w_response_error)abort=1;
 // Freeze the decoder on the final terminal of the first CLEAR. Memory
 // completion is wall-clock work, while CLEAR_DONE retirement must await CE.
 if(mode==8&&!ce_hold_started&&write_requests==1500&&fb_w_response_valid&&!fb_w_response_error)begin
  ce_hold_started=1;ce_hold_left=4;ce_hold_pc=dut.processor.pc;
 end
 ce_hold_edge=ce_hold_left!=0;
 if(ce_hold_edge)begin ce=0;ce_hold_left=ce_hold_left-1;end
end
always @(posedge clk)if(!reset&&ce_hold_edge)begin
 #1;
 if(dut.processor.state!=14||dut.processor.pc!==ce_hold_pc||!busy||
    dut.clear_active||dut.processor.work_valid||dut.start_fire||write_requests!=1500)
  $fatal(1,"CLEAR final terminal did not retire to held CLEAR_DONE under CE0");
 ce_hold_checks=ce_hold_checks+1;
end
always @(posedge clk)begin
 cycle=cycle+1;
 if(cycle>200000)$fatal(1,"CLEAR watchdog state=%0d pc=%0d err=%0d writes=%0d beat=%0d",dut.processor.state,dut.processor.pc,device_read_data[1],write_requests,wi);
 if(!reset)begin
  if(!ce)ce0=ce0+1;
  if(ro_request_valid&&ro_request_ready)begin
   if(active)$fatal(1,"multiple RO owners");
   active=1;ba=ro_address;bi=0;bn=ro_count==3?16:ro_count==1?8:4;
   if(ro_address<22'h1000)cachefills=cachefills+1;
  end else if(ro_response_valid)begin
   if(ro_response_last||ro_response_error)active=0;else bi=bi+1;
  end
  if(fb_r_request_valid&&fb_r_request_ready)begin
   if(ractive||wactive||fb_r_count!=3)$fatal(1,"bad framebuffer read ownership");
   ractive=1;rbase=index_of(fb_r_address);ri=0;
  end else if(fb_r_response_valid)begin
   if(fb_r_response_last)ractive=0;else ri=ri+1;
  end
  if(held&&(!fb_w_request_valid||{fb_w_count,fb_w_address}!==held_request))$fatal(1,"offered write revoked or changed");
  held=fb_w_request_valid&&!fb_w_request_ready;
  held_request={fb_w_count,fb_w_address};if(held)held_requests=held_requests+1;
  if(mode==1&&held)abort<=1;
  if(fb_w_request_valid&&fb_w_request_ready)begin
   if(wactive||ractive||fb_w_count!=3)$fatal(1,"bad framebuffer write ownership");
   wactive=1;wbase=index_of(fb_w_address);wi=0;wdelay=3;write_requests=write_requests+1;
   if(mode==0||mode==6||mode==8)begin
    if(dut.clear_active)begin
     if(clear_writes<1500)begin
      if(fb_w_address!==22'h88000+clear_writes*64)$fatal(1,"color clear address progression");
     end else if(fb_w_address!==22'ha8000+(clear_writes-1500)*64)$fatal(1,"depth clear address progression");
     clear_writes=clear_writes+1;
    end else preserve_writes=preserve_writes+1;
   end
  end else if(wactive)begin
   if(mode==2&&wi==4)abort<=1;
   if(fb_w_data_ready)begin
    if(wi>=16)$fatal(1,"write accepted seventeenth beat");
    if(dut.clear_active&&fb_w_data!=={4{dut.clear_value}})$fatal(1,"clear pattern mismatch");
    image[wbase+wi]=fb_w_data;wi=wi+1;write_beats=write_beats+1;if(!ce)ce0_beats=ce0_beats+1;
   end
   if(fb_w_response_valid)begin
    if(!fb_w_response_error&&wi!=16)$fatal(1,"successful short write");
    if(fb_w_data_ready)same_terminal=same_terminal+1;else delayed_terminal=delayed_terminal+1;
    write_terms=write_terms+1;wactive=0;
   end else if(wi==16&&wdelay!=0)wdelay=wdelay-1;
  end
  if(dut.input_active&&dut.work_busy)overlap=overlap+1;
  if(dut.start_fire)begin
   if(dut.slot!==(starts%2))$fatal(1,"pingpong changed");starts=starts+1;
  end
  if(ce&&meshlet_done)begin done_count=done_count+1;if(published_vertices!=3)$fatal(1,"vertex count");end
  if(ce&&emit_valid&&output_ready)begin
   if(vi>=60||emit_data!==vertices[vi])$fatal(1,"Vertex golden mismatch at%0d",vi);
   vi=vi+1;
  end
  if(ce&&!dut.stop&&dut.processor.state==5&&
     dut.processor.object_index==(dut.processor.apply_render?1:2)&&
     (dut.processor.opcode==5||dut.processor.opcode==6))material_changes=material_changes+1;
  if(commit_valid)commits=commits+1;
  if(mode!=6&&(fb_r_request_valid||quad_admitted))$fatal(1,"rejected fixture produced pixels");
 end
end
task tick;begin @(negedge clk);#1;end endtask
initial begin
 j=$value$plusargs("MODE=%d",mode);
 $readmemh("memory.hex",memory);$readmemh("vertices.hex",vertices);
 for(j=0;j<65536;j=j+1)image[j]=j>=32768&&j<56768?64'hffffffffffffffff:{4{16'h2222}};
 if(mode==4)memory[@CLEAR_Q@]=memory[@CLEAR_Q@]|(64'h20000<<@CLEAR_BIT@);
 reset=1;output_ready=1;command_base=mode==6?32'h1800:32'h1000;
 command_bytes=mode==6?@DIRTY_BYTES@:@BYTES@;
 tick;tick;reset=0;submit_commands(command_base,command_bytes);
 if(mode==1||mode==2||mode==7)begin
  while(!(dut.processor.state==10&&dut.dma_idle&&!dut.memory_busy))tick;
  if(!busy||!fault)$fatal(1,"abort falsely completed");
 end else while(busy)tick;
 if(wactive||active||ractive)$fatal(1,"result published before memory drain");
 if(mode==0||mode==6||mode==8)begin
  if(device_read_data[1]||starts!=(mode==6?4:3)||done_count!=starts||vi!=starts*15||
     overlap==0||cachefills<3||material_changes<3||clear_writes!=3000||
     same_terminal==0||delayed_terminal==0||ce0_beats==0)
   $fatal(1,"CLEAR conservation e%0d starts%0d V%0d writes%0d preserved%0d",device_read_data[1],starts,vi,clear_writes,preserve_writes);
  for(j=0;j<24000;j=j+1)begin
   if(image[j]!=={4{16'h6b4d}})begin
    if(mode!=6)$fatal(1,"color clear mismatch %0d",j);
    changed=changed+1;
   end
  end
  for(j=32768;j<56768;j=j+1)if(image[j]!=={4{16'h1357}})$fatal(1,"depth clear mismatch");
  if(image[24000]!=={4{16'h2222}}||image[32767]!=={4{16'h2222}}||image[56768]!=={4{16'h2222}})
   $fatal(1,"clear changed guard");
  if(mode==6)begin
   if(commits==0||preserve_writes==0||changed==0)$fatal(1,"no real dirty cache data preserved");
  end else if(preserve_writes!=0||commits!=0)$fatal(1,"backface unexpectedly dirty");
 end else begin
  if(mode==4)begin
   if(device_read_data[1]!=5||write_requests!=0)$fatal(1,"reserved parameter not rejected");
  end else begin
   if(!fault||(write_requests<1||write_requests>2)||write_terms!=write_requests)
    $fatal(1,"clear error/abort wrong e%0d writes%0d",device_read_data[1],write_requests);
   if((mode==1||mode==2||mode==7)&&write_beats!=16*write_requests)$fatal(1,"abort did not drain offered write");
  end
 end
 if(mode==8&&ce_hold_checks!=4)$fatal(1,"missing final-terminal CE0 hold checks got%0d",ce_hold_checks);
 repeat(5)begin tick;if((mode==0||mode==6||mode==8)&&busy)$fatal(1,"busy reasserted after completion");end
 $display("PASS CLEAR mode=%0d cycles=%0d starts=%0d V=%0d writes=%0d beats=%0d clear=%0d preserved=%0d changed=%0d commits=%0d held=%0d CE0beats=%0d same_terminal=%0d late_terminal=%0d CE0retire=%0d error=%0d",
  mode,cycle,starts,vi,write_requests,write_beats,clear_writes,preserve_writes,changed,commits,held_requests,ce0_beats,same_terminal,delayed_terminal,ce_hold_checks,device_read_data[1]);
 $finish;
end
endmodule
