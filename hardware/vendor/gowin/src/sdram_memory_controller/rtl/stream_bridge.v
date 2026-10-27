// Related 2:1 clocks. The adapter owns transaction credits and read completion.
// This bridge owns a request mailbox, native offer and the existing pair slots.
module TangNano20KSdramNativeBridge108M54M #(
 parameter PREPARE_NEXT=1, COLD_START_ONLY=0, CHAIN_GROUP_FOUR=0, BANK_BIT=5,
 parameter CAS=2, RCD_CYCLES=2, RP_CYCLES=2, CAPTURE_LATENCY=2, REGISTER_READ_RETURN=0,
 parameter DQ_INPUT_DELAY_TAPS=0
)(
 input logic_clk,controller_clk,sdram_clk,reset,
 input request_valid,input request_continuation,input writing,input[20:0]address,
 input[5:0]words,input[3:0]write_mask,input[63:0]write_data,input write_data_valid,
 input next_valid,input[20:0]next_address,
 output request_ready,stream_active,write_data_ready,output[63:0]read_data,output read_valid,
 output reg write_done=0,output initialized,
 output O_sdram_clk,O_sdram_cke,O_sdram_cs_n,O_sdram_cas_n,O_sdram_ras_n,O_sdram_wen_n,
 output[3:0]O_sdram_dqm,output[10:0]O_sdram_addr,output[1:0]O_sdram_ba,inout[31:0]IO_sdram_dq
);
 wire transport_reset=COLD_START_ONLY?1'b0:reset;
 reg[20:0]qa;reg[5:0]qn;reg[3:0]qm;reg qw;reg qcontinuation;
 reg request_toggle=0,request_seen_native=0,request_seen_logic=0;
 reg[63:0]pair_head=0,pair_next=0;reg[1:0]pair_count=0;
 reg initialized_q=0;reg stream_q=0;
 reg offer_valid=0,hint_valid=0,offer_write=0;reg[20:0]offer_address=0;reg[5:0]offer_words=0;
 reg[3:0]offer_mask=0;reg offer_continuation=0;
 reg active_write=0,active_scalar=0;
 reg next_write_ready_q=0;
 reg offer_generation=0;
 // The key is used only by the passive ownership proof below.
`ifndef SYNTHESIS
 function[12:0]bank_row_key;
  input[20:0]logical_address;
  reg[18:0]remaining;reg[1:0]bank;
  begin
   remaining=(logical_address&((21'd1<<BANK_BIT)-1))|
             ((logical_address>>(BANK_BIT+2))<<BANK_BIT);
   bank=logical_address>>BANK_BIT;
   bank_row_key={bank,remaining[18:8]};
  end
 endfunction
`endif
 assign initialized=initialized_q;assign stream_active=stream_q;
 // Cold-only users cannot submit before initialized_q. Keep the native PLL
 // startup reset out of the logic-domain grant/payload combinational paths.
 assign request_ready=initialized_q&&request_toggle==request_seen_logic&&!transport_reset;
 assign write_data_ready=pair_count<2&&!transport_reset;
 wire push=write_data_valid&&write_data_ready;
 wire core_ready,core_done,core_initialized,core_read_valid,chain_accept,read_chain_accept;
 wire[31:0]core_read_data;wire[5:0]core_write_index;wire[7:0]core_phase;
 wire pop=core_phase[4:0]==15&&core_write_index[0]&&pair_count!=0;
 wire[31:0]core_write_data=core_write_index[0]?pair_head[63:32]:pair_head[31:0];
 wire take=offer_valid&&core_ready&&(!offer_write||pair_count>=(offer_words==1?1:2));
 // The adapter alone proves same-owner, same-direction line overlap.
 // Descriptor ownership and write supply are different proofs. A READ chain
 // must not depend on a logic-domain write-pair credit.
 wire fast_next=offer_valid&&offer_continuation;
 reg write_retire_delay=0,write_event=0,write_visible=0,write_seen=0;
 reg read_half=0;reg[31:0]read_low=0;reg[63:0]read_pair=0;
 reg read_event=0,read_visible=0,read_seen=0;
 reg cpu_edge=0,cpu_edge_seen=0,slot_detect=0,slot_pipe=0,write_slot=0;
 wire raw_read_valid=read_visible!=read_seen&&!transport_reset;
 generate if(REGISTER_READ_RETURN)begin:registered_read_return
  // Invalid payload is unowned; keep the event out of the wide data enable.
  reg[63:0]payload=0;reg valid=0;
  always @(posedge logic_clk)begin
   if(transport_reset)begin payload<=0;valid<=0;end
   else begin valid<=raw_read_valid;payload<=read_pair;end
  end
  assign read_data=payload;assign read_valid=valid&&!transport_reset;
 end else begin:edge_read_return
  assign read_data=read_pair;assign read_valid=raw_read_valid;
 end endgenerate
 always @(posedge logic_clk)begin
  if(transport_reset)begin
   request_toggle<=0;request_seen_logic<=0;pair_count<=0;pair_head<=0;pair_next<=0;write_done<=0;write_seen<=0;
   initialized_q<=0;stream_q<=0;read_seen<=0;cpu_edge<=0;
  end else begin
   cpu_edge<=!cpu_edge;initialized_q<=core_initialized;request_seen_logic<=request_seen_native;
   stream_q<=core_phase[4:0]==15||core_phase[4:0]==18;write_done<=0;
   if(request_valid&&request_ready)begin
    qa<=address;qn<=words;qm<=write_mask;
    qw<=writing;qcontinuation<=request_continuation;request_toggle<=!request_toggle;
   end
   case({push,pop})
    2'b10:begin if(pair_count==0)pair_head<=write_data;else pair_next<=write_data;pair_count<=pair_count+1'b1;end
    2'b01:begin pair_head<=pair_next;pair_count<=pair_count-1'b1;end
    2'b11:begin
     if(pair_count==2)begin pair_head<=pair_next;pair_next<=write_data;end
     else pair_head<=write_data;
    end
   endcase
   if(raw_read_valid)read_seen<=read_visible;
   if(write_seen!=write_visible)begin
    write_done<=1;write_seen<=write_visible;
    // A scalar cannot overlap another owner: its unused upper half is dead.
    if(active_scalar)pair_count<=0;
   end
  end
 end
 // The mailbox stays immutable until this complete native-edge snapshot.
 always @(posedge controller_clk)begin
  if(transport_reset)begin
   next_write_ready_q<=0;
   offer_generation<=0;request_seen_native<=0;offer_valid<=0;hint_valid<=0;active_write<=0;active_scalar<=0;offer_continuation<=0;
   read_half<=0;read_event<=0;write_event<=0;write_retire_delay<=0;slot_pipe<=0;write_slot<=0;
  end else begin
   // Even-length streams retire on the logic pop edge. The preceding native
   // sample is exactly that edge's pre-pop credit, without a cross-domain
   // combinational path through the final command/address qualification.
   next_write_ready_q<=pair_count!=0;
   slot_pipe<=slot_detect;write_slot<=slot_pipe;
   if(!offer_valid&&request_seen_native!=request_toggle)begin
    // A continuation promotes the already published hint. No identity is
    // reconstructed here; a fresh, unhinted offer invalidates the old lookup.
    if(!qcontinuation||!hint_valid)offer_generation<=!offer_generation;
    offer_address<=qa;offer_words<=qn;offer_mask<=qm;
    offer_write<=qw;offer_continuation<=qcontinuation;offer_valid<=1;hint_valid<=0;
    request_seen_native<=request_toggle;
   end
   if(!offer_valid&&request_seen_native==request_toggle)begin
    hint_valid<=next_valid;
    if(next_valid&&!hint_valid)begin
     offer_generation<=!offer_generation;
     offer_address<=next_address;
    end
   end
   if(take||chain_accept||read_chain_accept)begin
    offer_valid<=0;active_write<=offer_write;active_scalar<=offer_words==1;
   end
   write_retire_delay<=chain_accept;
   if(write_retire_delay||(core_done&&active_write))write_event<=!write_event;
   if(core_read_valid)begin
    if(active_scalar)begin read_pair<={32'd0,core_read_data};read_event<=!read_event;end
    else if(!read_half)begin read_low<=core_read_data;read_half<=1;end
    else begin read_pair<={core_read_data,read_low};read_event<=!read_event;read_half<=0;end
   end
  end
 end
 always @(negedge controller_clk)begin
  if(transport_reset)begin cpu_edge_seen<=0;slot_detect<=0;write_visible<=0;read_visible<=0;end
  else begin
   slot_detect<=cpu_edge!=cpu_edge_seen;cpu_edge_seen<=cpu_edge;
   write_visible<=write_event;read_visible<=read_event;
  end
 end
 SdramController #(.CAS(CAS),.RCD_CYCLES(RCD_CYCLES),.RP_CYCLES(RP_CYCLES),.CAPTURE_LATENCY(CAPTURE_LATENCY),.DQ_INPUT_DELAY_TAPS(DQ_INPUT_DELAY_TAPS),.BANK_BIT(BANK_BIT),.PREPARE_NEXT(PREPARE_NEXT),.CHAIN(1),.READ_CHAIN(1),.CONTINUE(1),.NEXT_TAGGED(1)) u_controller(
  .clk(controller_clk),.sdram_clk(sdram_clk),.reset(reset),
  .request(offer_valid&&(!offer_write||pair_count>=(offer_words==1?1:2))),.writing(offer_write),.address(offer_address),.words(offer_words),
  .write_slot(write_slot),.next_valid(offer_valid||hint_valid),.next_address(offer_address),.next_words(offer_words),.next_chainable(fast_next),.next_write_ready(next_write_ready_q),.next_generation(offer_generation),
  .chain_accept(chain_accept),.read_chain_accept(read_chain_accept),.read_boundary(1'b0),.read_chain_pending(),
  .ready(core_ready),.done(core_done),.write_data(core_write_data),.write_mask(offer_mask),.write_index(core_write_index),
  .read_data(core_read_data),.read_valid(core_read_valid),.initialized(core_initialized),.phase(core_phase),.stream_stats(),
  .O_sdram_clk(O_sdram_clk),.O_sdram_cke(O_sdram_cke),.O_sdram_cs_n(O_sdram_cs_n),.O_sdram_cas_n(O_sdram_cas_n),
  .O_sdram_ras_n(O_sdram_ras_n),.O_sdram_wen_n(O_sdram_wen_n),.O_sdram_dqm(O_sdram_dqm),
  .O_sdram_addr(O_sdram_addr),.O_sdram_ba(O_sdram_ba),.IO_sdram_dq(IO_sdram_dq)
 );
`ifndef SYNTHESIS
 always @(posedge controller_clk)if(!reset&&offer_valid&&offer_continuation&&
  core_phase[4:0]==15&&core_write_index==u_controller.saved_last)
  if(next_write_ready_q!==(pair_count!=0))$fatal(1,"native continuation credit calendar");
 always @(posedge controller_clk)if(!reset&&!offer_valid)begin
  if(request_seen_native!=request_toggle&&qcontinuation&&hint_valid&&
     bank_row_key(qa)!=bank_row_key(offer_address))$fatal(1,"Continuation did not promote its published hint");
  if(request_seen_native==request_toggle&&hint_valid&&next_valid&&next_address!=offer_address)
   $fatal(1,"Published native hint changed while valid");
 end
 always @(posedge logic_clk)if(!reset)begin
  if(core_phase[4:0]==15&&core_write_index[0]&&pair_count==0)$fatal(1,"bridge write pair underrun");
 end
`endif
endmodule
