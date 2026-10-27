// Producer16/Group32 share64 rows. One held native Q is the consumer head.
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
 reg[5:0]wptr,tptr,rptr,gc;reg[4:0]pc;reg head_valid;
 wire[5:0]alloc=tptr+{1'b0,pc},missing=alloc-wptr;
 wire[5:0]rows={1'b0,pc}+gc;
 wire running=ce&&!reset&&!fault;
 assign bad=running&&((reserve_valid&&(pc==16||rows==48))||
  (write_valid&&missing==0)||(consume&&!head_valid));
 wire commit=running&&!bad;
 wire transfer=commit&&gc<32&&tptr!=wptr;
 wire take=commit&&consume;
 wire pr=commit&&rptr!=tptr&&(!head_valid||consume);
 wire pw=commit&&write_valid;
 wire ow=commit&&reserve_valid;
 wire[34:0]data_do;
 assign producer_ready=!reset&&!fault&&pc<16;
 assign front_valid=head_valid;assign front_payload=data_do;
 // The consumer has a two-edge Group cadence. It reads the next tag after
 // this native Q is replaced, rather than retaining a second payload head.
 assign next_valid=0;assign next_key=0;
 assign producer_count=pc;assign unwritten_count=missing[4:0];
 assign group_count=gc;assign row_count=rows;
 assign allocate_row=alloc;assign write_row=wptr;assign transfer_row=tptr;
 assign read_row=rptr;assign reclaim_row=rptr-{5'd0,head_valid};
 assign owner_address=wptr[3:0]+write_valid;
 assign strobes={take,pr,pr,transfer,pw,ow};
 assign head_valid_out={1'b0,head_valid};assign pending_out=0;
 assign owner_value=0;assign returned_payload=data_do;
 always @(posedge clk)begin
  if(reset)begin wptr<=0;tptr<=0;rptr<=0;gc<=0;pc<=0;head_valid<=0;fault<=0;end
  else if(running)begin
   if(bad)fault<=1;
   else begin
    if(pw)wptr<=wptr+1'b1;
    if(transfer)tptr<=tptr+1'b1;
    if(pr)rptr<=rptr+1'b1;
    if(pr||take)head_valid<=pr;
    case({reserve_valid,transfer})2'b10:pc<=pc+1'b1;2'b01:pc<=pc-1'b1;default:begin end endcase
    case({transfer,consume})2'b10:gc<=gc+1'b1;2'b01:gc<=gc-1'b1;default:begin end endcase
   end
  end
 end
 generate if(NATIVE)begin:payload_hardware
  wire[35:0]raw;assign data_do=raw[34:0];
  SDPX9B #(.READ_MODE(0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.RESET_MODE("SYNC")) bank(
   .DO(raw),.DI({1'b0,write_payload}),.BLKSELA(3'b0),.BLKSELB(3'b0),
   .ADA({3'b0,wptr,1'b0,4'b1111}),.ADB({3'b0,rptr,5'b0}),
   .CLKA(clk),.CLKB(clk),.CEA(pw),.CEB(pr),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
 end else begin:payload_model
  reg[34:0]mem[0:63];reg[34:0]q;
  always @(posedge clk)begin
   if(pw)mem[wptr]<=write_payload;
   if(reset)q<=0;else if(pr)q<=mem[rptr];
  end
  assign data_do=q;
 end endgenerate
`ifndef SYNTHESIS
 reg held;reg[34:0]held_payload;
 always @(posedge clk)begin
  if(reset)held<=0;
  else begin
   if(pc>16||gc>32||rows>48||missing>{1'b0,pc})$fatal(1,"native Group queue prefix");
   if(pr&&pw&&rptr==wptr)$fatal(1,"native Group queue read/write collision");
   if(held&&(!front_valid||front_payload!==held_payload))$fatal(1,"native Group head Q changed while held");
   held<=front_valid&&!take;held_payload<=front_payload;
  end
 end
`endif
endmodule
