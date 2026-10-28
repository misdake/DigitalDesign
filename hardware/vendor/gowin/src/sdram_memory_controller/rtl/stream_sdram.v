// Ordered two-descriptor endpoint. Return tags and write supply are independent
// of physical retirement; only a qualified same-owner/direction stream overlaps.
module SharedSdramPort(
 input clk,reset,input cpu_request_valid,cpu_write,cpu_line,input[21:0]cpu_address,
 input[1:0]cpu_line_count_minus_1,cpu_chain_class,input[63:0]cpu_write_data,input cpu_response_ready,
 input[63:0]controller_read_data,input controller_read_valid,controller_init_done,controller_request_ready,
 input controller_stream_active,controller_write_done,controller_write_data_ready,
 output cpu_request_ready,cpu_lookahead_window,cpu_write_data_ready,
 output reg cpu_response_valid=0,output reg[63:0]cpu_read_data=0,output reg cpu_response_last=0,cpu_error=0,
 output controller_request_valid,controller_next_valid,output[20:0]controller_next_address,
 output controller_write,output[20:0]controller_address,output[3:0]controller_write_mask,
 output[63:0]controller_write_data,output controller_write_data_valid,output[5:0]controller_words,
 output controller_request_continuation
);
 (* syn_ramstyle="registers" *) reg[21:0]qa[0:1];(* syn_ramstyle="registers" *) reg[1:0]qc[0:1],qlen[0:1];(* syn_ramstyle="registers" *) reg qw[0:1],ql[0:1],qe[0:1];reg[15:0]scalar_payload;
 reg[3:0]tx_beat=0;reg supplied[0:1];reg[1:0]put=0,head=0,sent=0,rx=0;reg[4:0]rx_beat=0;
 wire[1:0]used=put-head;
 wire legal=!cpu_line||(cpu_line_count_minus_1==0?cpu_address[3:0]==0:
  cpu_line_count_minus_1==1?cpu_address[4:0]==0:cpu_line_count_minus_1==3&&cpu_address[5:0]==0);
 wire[4:0]rx_words=!ql[rx[0]]?5'd1:({1'b0,qlen[rx[0]],2'd0}+5'd4);
 wire[1:0]forwarded=sent-head;
 wire same_next=used==2&&qc[head[0]]!=0&&qc[head[0]]==qc[!head[0]]&&
  qw[head[0]]==qw[!head[0]]&&ql[head[0]]&&ql[!head[0]];
 wire send_early=sent==head||same_next;
 assign cpu_request_ready=controller_init_done&&used<2&&(legal||used==0)&&(!cpu_write||cpu_line||used==0)&&!reset;
 wire accept=cpu_request_valid&&cpu_request_ready;
 assign controller_request_valid=sent!=put&&send_early&&!qe[sent[0]]&&!reset;
 assign controller_address=qa[sent[0]][21:1];assign controller_write=qw[sent[0]];
 assign controller_request_continuation=sent!=head;
 assign controller_words=!ql[sent[0]]?6'd1:qlen[sent[0]]==0?6'd8:qlen[sent[0]]==1?6'd16:6'd32;
 assign controller_write_mask=qw[sent[0]]&&!ql[sent[0]]?(qa[sent[0]][0]?4'b0011:4'b1100):4'd0;
 assign controller_next_valid=used==2;assign controller_next_address=qa[!head[0]][21:1];
 // Descriptor credit, not delayed return data, bounds successor admission.
 // CL3 plus registered read return must leave time for the native next-row
 // lookup/PRE/ACT before the old column stream reaches its chain boundary.
 assign cpu_lookahead_window=used==1&&ql[head[0]]&&
  forwarded!=0&&controller_stream_active;
 wire next_feed=forwarded==2&&same_next&&qw[head[0]]&&supplied[head[0]];
 wire tx_slot=next_feed?!head[0]:head[0];
 wire[4:0]tx_words=!ql[tx_slot]?5'd1:({1'b0,qlen[tx_slot],2'd0}+5'd4);
 assign controller_write_data_valid=used!=0&&forwarded!=0&&qw[tx_slot]&&!supplied[tx_slot]&&!reset;
 wire[31:0]scalar_word=qa[tx_slot][0]?{scalar_payload,16'd0}:{16'd0,scalar_payload};
 assign controller_write_data=ql[tx_slot]?cpu_write_data:{32'd0,scalar_word};
 wire feed=controller_write_data_valid&&controller_write_data_ready;
 assign cpu_write_data_ready=feed&&ql[tx_slot];
 wire retire=cpu_response_valid&&cpu_response_last&&cpu_response_ready;
 always @(posedge clk)begin
  if(reset)begin
   put<=0;head<=0;sent<=0;rx<=0;rx_beat<=0;
   cpu_response_valid<=0;cpu_response_last<=0;cpu_error<=0;cpu_read_data<=0;
   tx_beat<=0;supplied[0]<=0;supplied[1]<=0;
  end else begin
   if(accept)begin
    qa[put[0]]<=cpu_address;qc[put[0]]<=cpu_chain_class;qlen[put[0]]<=cpu_line_count_minus_1;
    qw[put[0]]<=cpu_write;ql[put[0]]<=cpu_line;qe[put[0]]<=!legal;
    if(cpu_write&&!cpu_line)scalar_payload<=cpu_write_data[15:0];
    supplied[put[0]]<=0;put<=put+1'b1;
   end
   if(controller_request_valid&&controller_request_ready)sent<=sent+1'b1;
   // Capture data independently of the half-cycle return-valid event. Only a
   // held response owns the old value; invalid-cycle data has no lifetime.
   // This keeps return-valid off the 64 payload-register enables.
   if(!cpu_response_valid||cpu_response_ready)
    cpu_read_data<=ql[rx[0]]?controller_read_data:
     {48'd0,(qa[rx[0]][0]?controller_read_data[31:16]:controller_read_data[15:0])};
   if(feed)begin
    if({1'b0,tx_beat}==tx_words-1'b1)begin supplied[tx_slot]<=1;tx_beat<=0;end
    else tx_beat<=tx_beat+1'b1;
   end
   if(cpu_response_ready)begin cpu_response_valid<=0;cpu_response_last<=0;cpu_error<=0;end
   if(controller_read_valid)begin
    cpu_response_valid<=1;cpu_response_last<=rx_beat==rx_words-1'b1;
    if(rx_beat==rx_words-1'b1)begin rx_beat<=0;rx<=rx+1'b1;end
    else rx_beat<=rx_beat+1'b1;
   end
   if(controller_write_done)begin
    cpu_response_valid<=1;cpu_response_last<=1;cpu_read_data<=0;rx<=rx+1'b1;
   end
   if(used!=0&&qe[head[0]]&&!cpu_response_valid)begin cpu_response_valid<=1;cpu_response_last<=1;cpu_error<=1;end
   if(retire)begin
    head<=head+1'b1;
    if(qe[head[0]])begin sent<=head+1'b1;rx<=head+1'b1;end
   end
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(!reset)begin
  if(controller_request_valid&&controller_request_continuation&&!same_next)$fatal(1,"continuation owner/direction proof");
  if(accept&&cpu_write&&!cpu_line&&used!=0)$fatal(1,"scalar payload ownership proof");
  if(used>2)$fatal(1,"shared port descriptor capacity");
  if(controller_read_valid&&cpu_response_valid&&!cpu_response_ready)$fatal(1,"read response backpressure crossed stream");
 end
`endif
// A missing physical terminal cannot be replaced by an error/LAST: that
 // would release an owner while its native transaction could still be alive.
 // A debug timeout observes the same datapath and never manufactures a reply.
`ifndef SYNTHESIS
 reg[19:0]timer=0;
 always @(posedge clk)begin
  if(reset||used==0||retire)timer<=0;
  else if(!cpu_response_valid)begin
   if(&timer)$fatal(1,"Shared SDRAM physical completion timeout");
   timer<=timer+1'b1;
  end
 end
`endif
endmodule
