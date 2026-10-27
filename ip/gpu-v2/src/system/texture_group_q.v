// One48-entry FIFO in64 rows. A copied head precedes the registered native lookahead Q.
module gpu_v2_packet_store_unique #(parameter MODE=1,NATIVE=1)(
 input clk,reset,ce,input reserve_valid,input[5:0]reserve_owner,
 input write_valid,input[34:0]write_payload,input owner_request,consume,
 output producer_ready,front_valid,output[34:0]front_payload,
 output next_valid,output[15:0]next_key,
 output reg fault,output bad,
 output[4:0]producer_count,unwritten_count,output[5:0]group_count,row_count,
 output[5:0]allocate_row,write_row,transfer_row,read_row,reclaim_row,
 output[3:0]owner_address,output[5:0]strobes,
 output[1:0]head_valid_out,output pending_out,
 output[5:0]owner_value,output[34:0]returned_payload
);
 reg[5:0]wptr,rptr,used;reg head_valid,q_valid,pending;
 reg[34:0]head_payload;
 wire[5:0]reclaimed=rptr-{5'd0,head_valid}-{5'd0,q_valid}-{5'd0,pending};
 wire[5:0]alloc=reclaimed+used,missing=alloc-wptr;
 wire[5:0]written=wptr-reclaimed;
 wire running=ce&&!reset&&!fault;
 assign bad=running&&((reserve_valid&&used==48)||
  (write_valid&&missing==0)||(consume&&!head_valid));
 wire commit=running&&!bad;
 wire transfer=commit&&write_valid;
 wire take=commit&&consume;
 wire promote=commit&&q_valid&&(!head_valid||consume);
 wire load=commit&&pending;
 wire pr=commit&&!pending&&rptr!=wptr&&(!q_valid||promote);
 wire pw=commit&&write_valid;
 wire ow=commit&&reserve_valid;
 wire[34:0]data_do;
 assign producer_ready=!reset&&!fault&&used<48;
 assign front_valid=head_valid;assign front_payload=head_payload;
 // The next key is already visible before consumption. Tag lookup overlaps
 // promotion of native Q into the copied head and the following payload read.
 assign next_valid=q_valid;assign next_key=data_do[15:0];
 assign producer_count=missing[4:0];assign unwritten_count=missing[4:0];
 assign group_count=written;assign row_count=used;
 assign allocate_row=alloc;assign write_row=wptr;assign transfer_row=wptr;
 assign read_row=rptr;assign reclaim_row=reclaimed;
 assign owner_address=wptr[3:0]+write_valid;
 assign strobes={take,promote,pr,transfer,pw,ow};
 assign head_valid_out={q_valid,head_valid};assign pending_out=pending;
 assign owner_value=0;assign returned_payload=data_do;
 always @(posedge clk)begin
  if(reset)begin wptr<=0;rptr<=0;used<=0;head_valid<=0;q_valid<=0;pending<=0;fault<=0;end
  else if(running)begin
   if(bad)fault<=1;
   else begin
    if(pw)wptr<=wptr+1'b1;
    if(pr)rptr<=rptr+1'b1;
    if(promote)head_payload<=data_do;
    if(promote||take)head_valid<=promote;
    if(promote||load)q_valid<=load;
    if(pr||load)pending<=pr;
    case({reserve_valid,consume})2'b10:used<=used+1'b1;2'b01:used<=used-1'b1;default:begin end endcase
   end
  end
 end
 generate if(NATIVE)begin:payload_hardware
  wire[35:0]raw;assign data_do=raw[34:0];
  SDPX9B #(.READ_MODE(1),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.RESET_MODE("SYNC")) bank(
   .DO(raw),.DI({1'b0,write_payload}),.BLKSELA(3'b0),.BLKSELB(3'b0),
   .ADA({3'b0,wptr,1'b0,4'b1111}),.ADB({3'b0,rptr,5'b0}),
   .CLKA(clk),.CLKB(clk),.CEA(pw),.CEB(pr),.OCE(load),.RESETA(1'b0),.RESETB(reset));
 end else begin:payload_model
  reg[34:0]mem[0:63];reg[34:0]raw_q,q;
  always @(posedge clk)begin
   if(pw)mem[wptr]<=write_payload;
   if(pr)raw_q<=mem[rptr];
   if(reset)q<=0;else if(load)q<=raw_q;
  end
  assign data_do=q;
 end endgenerate
`ifndef SYNTHESIS
 reg held;reg[34:0]held_payload;
 always @(posedge clk)begin
  if(reset)held<=0;
  else begin
   if(used>48||missing>2)$fatal(1,"native Group queue prefix");
   if(pr&&pw&&rptr==wptr)$fatal(1,"native Group queue read/write collision");
   if(pw&&(q_valid||pending)&&wptr==rptr-1'b1)$fatal(1,"native Group lookahead Q overwritten");
   if(held&&(!front_valid||front_payload!==held_payload))$fatal(1,"native Group head Q changed while held");
   held<=front_valid&&!take;held_payload<=front_payload;
  end
 end
`endif
endmodule
