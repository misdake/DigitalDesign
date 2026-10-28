// All seven production owners contend simultaneously; no synthetic completion.
integer seven_accept[0:6],seven_words[0:6],seven_done[0:6];
integer seven_index;
always @(negedge logic_clk)if(seven_active)begin
 display_request_valid=seven_accept[0]==0;display_address='h5000/2;display_response_ready=1;
 instruction_request_valid=seven_accept[1]==0;instruction_address='h5100/2;instruction_response_ready=1;
 data_request_valid=seven_accept[2]==0;data_address='h5200/2;data_line=1;data_write=0;data_response_ready=1;
 dma_request_valid=seven_accept[3]==0;dma_address='h5300/2;dma_write=1;dma_write_data=16'hface;dma_response_ready=1;
 gpu_ro_request_valid=seven_accept[4]==0;gpu_ro_address='h5400/2;gpu_ro_write=0;gpu_ro_line_count_minus_1=1;
 gpu_fb_r_request_valid=seven_accept[5]==0;gpu_fb_r_address='h5500/2;gpu_fb_r_write=0;gpu_fb_r_line_count_minus_1=3;
 gpu_fb_w_request_valid=seven_accept[6]==0;gpu_fb_w_address='h5600/2;gpu_fb_w_write=1;gpu_fb_w_line_count_minus_1=1;
 gpu_fb_w_write_data={write32(seven_words[6]*2+1),write32(seven_words[6]*2)};
end
task seven_read;
 input integer owner,address,length;
 input[63:0]value;
 input last,error;
 begin
  if(seven_accept[owner]!=1||error||seven_words[owner]>=length)$fatal(1,"seven-client response ownership %0d",owner);
  if(value!=={seed32(address+seven_words[owner]*8+4),seed32(address+seven_words[owner]*8)})$fatal(1,"seven-client payload %0d beat%0d",owner,seven_words[owner]);
  if(last!==(seven_words[owner]==length-1))$fatal(1,"seven-client LAST %0d",owner);
  seven_words[owner]=seven_words[owner]+1;
  if(last)seven_done[owner]=seven_done[owner]+1;
 end
endtask
always @(posedge logic_clk)if(seven_active&&!reset)begin
 if(display_request_valid&&display_request_ready)seven_accept[0]=seven_accept[0]+1;
 if(instruction_request_valid&&instruction_request_ready)seven_accept[1]=seven_accept[1]+1;
 if(data_request_valid&&data_request_ready)seven_accept[2]=seven_accept[2]+1;
 if(dma_request_valid&&dma_request_ready)seven_accept[3]=seven_accept[3]+1;
 if(gpu_ro_request_valid&&gpu_ro_request_ready)seven_accept[4]=seven_accept[4]+1;
 if(gpu_fb_r_request_valid&&gpu_fb_r_request_ready)seven_accept[5]=seven_accept[5]+1;
 if(gpu_fb_w_request_valid&&gpu_fb_w_request_ready)seven_accept[6]=seven_accept[6]+1;
 if(display_response_valid)seven_read(0,'h5000,4,display_read_data,display_response_last,display_error);
 if(instruction_response_valid)seven_read(1,'h5100,4,instruction_read_data,seven_words[1]==3,instruction_error);
 if(data_response_valid)seven_read(2,'h5200,4,data_read_data,seven_words[2]==3,data_error);
 if(dma_response_valid)begin
  if(dma_error||seven_accept[3]!=1||pin.mem[nw('h5300)]!==32'h1000face)$fatal(1,"DMA completed before masked physical write");
  seven_done[3]=seven_done[3]+1;
 end
 if(gpu_ro_response_valid)seven_read(4,'h5400,8,gpu_ro_read_data,gpu_ro_response_last,gpu_ro_error);
 if(gpu_fb_r_response_valid)seven_read(5,'h5500,16,gpu_fb_r_read_data,gpu_fb_r_response_last,gpu_fb_r_error);
 if(gpu_fb_w_write_data_ready)seven_words[6]=seven_words[6]+1;
 if(gpu_fb_w_response_valid)begin
  if(gpu_fb_w_error||!gpu_fb_w_response_last||seven_words[6]!=8||pin.mem[nw('h5600+60)]!==write32(15))$fatal(1,"GPU write completed before final pin commit");
  seven_done[6]=seven_done[6]+1;
 end
end
task seven_clients;
 begin
  for(seven_index=0;seven_index<7;seven_index=seven_index+1)begin
   seven_accept[seven_index]=0;seven_words[seven_index]=0;seven_done[seven_index]=0;
  end
  @(posedge logic_clk);#1;seven_active=1;
  wait(seven_done[0]&&seven_done[1]&&seven_done[2]&&seven_done[3]&&seven_done[4]&&seven_done[5]&&seven_done[6]);
  repeat(4)@(negedge logic_clk);
  for(seven_index=0;seven_index<7;seven_index=seven_index+1)
   if(seven_accept[seven_index]!=1||seven_done[seven_index]!=1)$fatal(1,"seven-client request/terminal conservation %0d",seven_index);
  for(seven_index=0;seven_index<16;seven_index=seven_index+1)
   if(pin.mem[nw('h5600+seven_index*4)]!==write32(seven_index))$fatal(1,"seven-client write payload");
  seven_active=0;display_request_valid=0;instruction_request_valid=0;data_request_valid=0;dma_request_valid=0;
  gpu_ro_request_valid=0;gpu_fb_r_request_valid=0;gpu_fb_w_request_valid=0;
  $display("SEVEN_CLIENTS_PASS requests=7 terminals=7 read_words=36 gpu_write_words=8 dma_write_words=1");
 end
endtask
