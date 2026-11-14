// Command-only Group35 cache: physical H15/localXY7, eight sets/four ways.
// Parent lines remain FILLING across both128-byte child terminals. Failure
// invalidates the parent; explicit abort retains the existing request drain.
// Paid payload/pin and synchronous texel DO have independent last consumers.
// Monitors observe their original wall/CE edges without controlling progress.
module gpu_v2_texture_runtime_cache_unique # (parameter NATIVE=@NATIVE@)(
 input wire clk,reset,proposal_ce,commit_allow,abort,
 input wire in_valid,input wire[34:0]in_payload,input wire texture_invalidate,
 input wire weight_write,input wire[6:0]weight_address,input wire[35:0]weight_value,
 input wire next_valid,input wire[14:0]next_key,
 output wire proposal_ready,in_ready,preflight_bad,advance,
 input wire out_ready,output wire out_valid,output wire[71:0]out_payload,
 output wire[15:0]out_tex0,out_tex1,out_tex2,out_tex3,
 output wire mem_req_valid,output wire[31:0]mem_req_addr,
 input wire mem_req_ready,mem_resp_valid,input wire[3:0]mem_resp_index,
 input wire[63:0]mem_resp_data,input wire mem_resp_complete,mem_resp_ok,
 output wire drained,output reg fault
);
 // Canonical physical tags retain immutable resources across descriptor changes.



 (* syn_ramstyle="distributed_ram" *) reg[7:0]state_rows[0:7];
 (* syn_ramstyle="distributed_ram" *) reg[2:0]plru_rows[0:7];
 reg[7:0]state_initialized,plru_initialized;
 wire[17:0]tag_q[0:3];reg tag_valid;
 // Payload is written once at actual read issue, held through capture/pop.
 reg[34:0]res_payload,paid_payload;reg[35:0]res_weights;reg res_valid,rd_valid;
 reg[4:0]pin_line;wire pin_valid=rd_valid;
 reg desc_valid,desc_started,desc_presented;reg[4:0]desc_line;
 reg[4:0]desc_next;reg desc_sector;
 wire[15:0]data_di[0:3],data_do[0:3];
 wire head_valid=in_valid;wire[34:0]head_payload=in_payload;
 reg hit_ready,look_filling,vic_valid;reg[4:0]hit_line,vic_line;
 integer wl,wv;
 reg[1:0]o0,o1;
 wire[14:0]head_sector=head_payload[14:0];
 wire[3:0]head_tlx=head_payload[18:15];wire[2:0]head_tly=head_payload[21:19];
 wire[2:0]head_set=head_sector[2:0];
 wire[31:0]hk_addr={9'd0,head_sector,8'd0};
 wire running=proposal_ce&&!reset&&!fault&&!abort;
 wire req_present=!reset&&desc_valid&&!desc_started&&(!fault||desc_presented);
 wire req_accept=req_present&&mem_req_ready;
 wire eff_started=desc_valid&&(desc_started||req_accept);
 wire beat_present=mem_resp_valid;
 wire beat_ok=beat_present;
 wire[4:0]eff_next=desc_next+(beat_ok?5'd1:5'd0);
 wire terminal=mem_resp_complete;
 wire term_ok=terminal;
 wire proto_fault=!reset&&((beat_present&&!beat_ok)||(terminal&&!term_ok));
 // Two occupied stages are legal; the pending DO is held until capture.
 wire return_bad=1'b0;
 assign preflight_bad=1'b0;
 wire fault_eff=fault||abort||preflight_bad;
 assign advance=running&&!preflight_bad&&commit_allow;
 assign proposal_ready=!reset&&!fault&&!abort&&head_valid&&tag_valid&&!desc_valid&&
 !rd_valid&&hit_ready;
 wire read_issue=advance&&proposal_ready;
 assign in_ready=read_issue;
 assign out_valid=!reset&&!fault&&!abort&&res_valid;
 // Coissue immutable weight read with the actual texel-bank read, same paid lease.
 wire[6:0]weight_ra={head_payload[27:24],head_payload[29:28],head_payload[30]};
 wire[35:0]weight_do;
 generate if(NATIVE)begin:weights_native
  SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.RESET_MODE("SYNC")) bank(
   .DO(weight_do),.DI(weight_value),.BLKSELA(3'b0),.BLKSELB(3'b0),
   .ADA({2'b0,weight_address,1'b0,4'b1111}),.ADB({2'b0,weight_ra,5'b0}),
   .CLKA(clk),.CLKB(clk),.CEA(weight_write),.CEB(read_issue),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
 end else begin:weights_portable
  reg[35:0]mem[0:127];reg[35:0]q;
  always@(posedge clk)begin
   if(weight_write)mem[weight_address]<=weight_value;
   if(reset)q<=0;else if(read_issue)q<=mem[weight_ra];
  end
  assign weight_do=q;
 end endgenerate
 wire[35:0]masked_weights;
 wire[3:0]restored_mask=res_payload[34:31];
 genvar wt;generate for(wt=0;wt<4;wt=wt+1)begin:restore_mask
  assign masked_weights[wt*9+:9]=restored_mask[wt]?res_weights[wt*9+:9]:9'd0;
 end endgenerate
 // Keep the already qualified inverse coefficient permutation and raw texels.
 wire y0=res_payload[19]^res_payload[16],y1=y0^res_payload[15];
 wire[8:0]v0=y0?masked_weights[18+:9]:masked_weights[0+:9];
 wire[8:0]v1=y1?masked_weights[27+:9]:masked_weights[9+:9];
 wire[8:0]v2=y0?masked_weights[0+:9]:masked_weights[18+:9];
 wire[8:0]v3=y1?masked_weights[9+:9]:masked_weights[27+:9];
 wire[35:0]physical_weights=res_payload[15]?{v2,v3,v0,v1}:{v3,v2,v1,v0};
 assign out_payload={res_payload[29:22],physical_weights,28'd0};
 assign out_tex0=data_do[0];assign out_tex1=data_do[1];
 assign out_tex2=data_do[2];assign out_tex3=data_do[3];
 assign mem_req_valid=req_present&&(!(fault||abort)||desc_presented);
 assign mem_req_addr=hk_addr+{24'd0,desc_sector,7'd0};assign drained=!desc_valid;
 wire[17:0]head_tag={6'd0,head_sector[14:3]};
 wire tag_write=advance&&head_valid&&tag_valid&&!desc_valid&&!hit_ready&&!look_filling&&vic_valid;
 // Speculative tag port only: Pool ownership is unchanged until read_issue.
 wire tag_read=!reset&&!fault_eff&&head_valid&&!tag_write;
 wire[2:0]tag_set=(read_issue&&next_valid)?next_key[2:0]:head_set;
 wire result_pop=advance&&out_ready&&res_valid;
 wire return_capture=rd_valid&&(!res_valid||result_pop);
 generate if(NATIVE)begin:tags_native
  genvar tg;for(tg=0;tg<2;tg=tg+1)begin:tag_pair
   wire[35:0]pair_q;
   SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(18),.BIT_WIDTH_1(36)) bank(
   .DO(pair_q),.DI({18'd0,head_tag}),.BLKSELA(3'b000),.BLKSELB(3'b000),
   .ADA({6'd0,vic_line[4:2],vic_line[0],4'b0011}),.ADB({6'd0,tag_set,5'd0}),
   .CLKA(clk),.CLKB(clk),.CEA(tag_write&&vic_line[1]==tg),.CEB(tag_read),
   .OCE(1'b1),.RESETA(1'b0),.RESETB(1'b0));
   assign tag_q[tg*2]=pair_q[17:0];assign tag_q[tg*2+1]=pair_q[35:18];
  end
 end else begin:tags_portable
  reg[17:0]tag_ram[0:31];reg[17:0]q[0:3];integer tr;
  always@(posedge clk)begin
   if(tag_write)tag_ram[vic_line]<=head_tag;
   if(tag_read)for(tr=0;tr<4;tr=tr+1)q[tr]<=tag_ram[{tag_set,2'b00}+tr];
  end
  genvar tp;for(tp=0;tp<4;tp=tp+1)begin:tag_port assign tag_q[tp]=q[tp];end
 end endgenerate
 wire[2:0]state_set=desc_valid?desc_line[4:2]:head_set;
 wire[7:0]head_state=state_initialized[state_set]?state_rows[state_set]:8'd0;
 wire[2:0]head_plru=plru_initialized[state_set]?plru_rows[state_set]:3'd0;
 wire publish=terminal&&desc_sector&&mem_resp_ok&&!reset&&!fault_eff&&term_ok;
 wire state_write=tag_write||(desc_valid&&((terminal&&(desc_sector||!term_ok||!mem_resp_ok))||fault_eff||(beat_present&&!beat_ok)));
 wire[1:0]state_way=tag_write?vic_line[1:0]:desc_line[1:0];
 wire[1:0]state_value=tag_write?2'd1:(publish?2'd2:2'd0);
 wire[7:0]state_next;
 genvar sw;generate for(sw=0;sw<4;sw=sw+1)begin:state_update
 assign state_next[sw*2+:2]=(state_way==sw)?state_value:head_state[sw*2+:2];end endgenerate
 wire plru_write=publish||read_issue;
 wire[1:0]used_way=publish?desc_line[1:0]:hit_line[1:0];
 wire[2:0]plru_next=used_way[1]?{(used_way[0]^1'b1),head_plru[1],1'b0}:
 {head_plru[2],(used_way[0]^1'b1),1'b1};
 // While the refill is live, its borrowed Pool head owns this set.
 always@(posedge clk)begin
  if(reset||texture_invalidate)begin state_initialized<=0;plru_initialized<=0;end
  else begin
   if(state_write)begin state_rows[state_set]<=state_next;state_initialized[state_set]<=1;end
   if(plru_write)begin plru_rows[state_set]<=plru_next;plru_initialized[state_set]<=1;end
  end
 end
  wire [3:0] ready_hits;
  wire [3:0] filling_hits;
  genvar hw;
  generate for (hw=0;hw<4;hw=hw+1) begin: fixed_hit_decode
    assign ready_hits[hw] = head_state[hw*2+:2] == 2'd2 && tag_q[hw] == head_tag;
    assign filling_hits[hw] = head_state[hw*2+:2] != 2'd0 && head_state[hw*2+:2] != 2'd2 && tag_q[hw] == head_tag;
  end endgenerate
  always @(*) begin
    hit_ready = |ready_hits;
    look_filling = |filling_hits;
    // Preserve the original highest-way priority with two fixed Boolean decodes.
    hit_line = {head_set, ready_hits[3] | ready_hits[2], ready_hits[3] | (ready_hits[1] & ~ready_hits[2])};
  end
  always @(*) begin
    o0={head_plru[0],(head_plru[0]?head_plru[2]:head_plru[1])};
    o1=o0^2'd1;
    vic_valid = 1'b0; vic_line = 5'd0;
    for (wv = 0; wv < 4; wv = wv + 1) begin
      if (!vic_valid && (head_state[wv*2+:2] == 2'd0) &&
          !(pin_valid && (pin_line == head_set*4+wv))) begin
        vic_valid = 1'b1; vic_line = head_set*4+wv;
      end
    end
    if (!vic_valid) begin
      // No descriptor means every non-invalid line is READY. Only one
      // line may be pinned, so the PLRU first choice or its sibling suffices.
      vic_valid=1'b1;
      if(pin_valid && pin_line=={head_set,o0})vic_line={head_set,o1};
      else vic_line={head_set,o0};
    end
  end

 wire[3:0]data_tlx=paid_payload[18:15];wire[2:0]data_tly=paid_payload[21:19];
 wire bank_low0=1'b0^data_tlx[1]^(data_tlx[0]&1'b1);
 wire bank_carry0=data_tly[0]&!bank_low0;
 wire[4:0]bank_row0={data_tly[2]^(data_tly[1]&bank_carry0),data_tly[1]^bank_carry0,bank_low0,data_tlx[3]^(data_tlx[2]&data_tlx[1]&data_tlx[0]&1'b1),data_tlx[2]^(data_tlx[1]&data_tlx[0]&1'b1)};
 wire bank_low1=1'b0^data_tlx[1]^(data_tlx[0]&1'b0);
 wire bank_carry1=data_tly[0]&!bank_low1;
 wire[4:0]bank_row1={data_tly[2]^(data_tly[1]&bank_carry1),data_tly[1]^bank_carry1,bank_low1,data_tlx[3]^(data_tlx[2]&data_tlx[1]&data_tlx[0]&1'b0),data_tlx[2]^(data_tlx[1]&data_tlx[0]&1'b0)};
 wire bank_low2=1'b1^data_tlx[1]^(data_tlx[0]&1'b1);
 wire bank_carry2=data_tly[0]&!bank_low2;
 wire[4:0]bank_row2={data_tly[2]^(data_tly[1]&bank_carry2),data_tly[1]^bank_carry2,bank_low2,data_tlx[3]^(data_tlx[2]&data_tlx[1]&data_tlx[0]&1'b1),data_tlx[2]^(data_tlx[1]&data_tlx[0]&1'b1)};
 wire bank_low3=1'b1^data_tlx[1]^(data_tlx[0]&1'b0);
 wire bank_carry3=data_tly[0]&!bank_low3;
 wire[4:0]bank_row3={data_tly[2]^(data_tly[1]&bank_carry3),data_tly[1]^bank_carry3,bank_low3,data_tlx[3]^(data_tlx[2]&data_tlx[1]&data_tlx[0]&1'b0),data_tlx[2]^(data_tlx[1]&data_tlx[0]&1'b0)};
 wire data_write=!reset&&!fault_eff&&beat_ok;
 wire[9:0]data_waddr={desc_line,desc_sector,mem_resp_index};

 assign data_di[0]=mem_resp_index[2]?mem_resp_data[32+:16]:mem_resp_data[0+:16];
 assign data_di[1]=mem_resp_index[2]?mem_resp_data[48+:16]:mem_resp_data[16+:16];
 assign data_di[2]=mem_resp_index[2]?mem_resp_data[0+:16]:mem_resp_data[32+:16];
 assign data_di[3]=mem_resp_index[2]?mem_resp_data[16+:16]:mem_resp_data[48+:16];
 wire[9:0]data_ra[0:3];
 assign data_ra[0]={pin_line,bank_row0};
 assign data_ra[1]={pin_line,bank_row1};
 assign data_ra[2]={pin_line,bank_row2};
 assign data_ra[3]={pin_line,bank_row3};
 genvar db;generate for(db=0;db<4;db=db+1)begin:data_bank
  if(NATIVE)begin:native_bank
   wire[31:0]q;
   SDPB #(.READ_MODE(1'b0),.BIT_WIDTH_0(16),.BIT_WIDTH_1(16),.RESET_MODE("SYNC")) bank(
   .DO(q),.DI({16'b0,data_di[db]}),.BLKSELA(3'b0),.BLKSELB(3'b0),
   .ADA({data_waddr,4'b0011}),.ADB({data_ra[db],4'b0}),
   .CLKA(clk),.CLKB(clk),.CEA(data_write),.CEB(return_capture),.OCE(1'b1),
   .RESETA(1'b0),.RESETB(reset));
   assign data_do[db]=q[15:0];
  end else begin:portable_bank
   reg[15:0]mem[0:1023];reg[15:0]q;
   always@(posedge clk)begin
    if(data_write)mem[data_waddr]<=data_di[db];
    if(reset)q<=0;else if(return_capture)q<=mem[data_ra[db]];
   end
   assign data_do[db]=q;
  end
 end endgenerate
 integer b;
 always@(posedge clk)begin
  if(reset)begin
   res_valid<=0;rd_valid<=0;tag_valid<=0;begin end
   desc_valid<=0;desc_started<=0;desc_presented<=0;desc_next<=0;desc_sector<=0;
  end else begin
   if(fault_eff)begin
`ifndef SYNTHESIS
 if(!reset&&!abort&&((1)!=0))$fatal(1,"GPU contract: fault");
`endif
 end
   if(texture_invalidate||!head_valid||tag_write)tag_valid<=0;
   else if(read_issue)tag_valid<=next_valid;
   else if(tag_read)tag_valid<=1;
   // Paid returns capture on the wall clock, never borrowed as same-edge credit.
   if(return_capture)begin

    res_payload<=paid_payload;res_weights<=weight_do;res_valid<=1;rd_valid<=0;
   end
   if(mem_req_valid)desc_presented<=1;
   if(mem_req_valid&&mem_req_ready)begin desc_started<=1;desc_presented<=1;end
   if(beat_present&&desc_valid&&eff_started&&desc_next<16&&mem_resp_index==desc_next[3:0])desc_next<=eff_next;
   // The parent line owns the head across both separately acknowledged sectors.
   if(terminal)begin
    if(!desc_sector&&term_ok&&mem_resp_ok&&!fault_eff)begin
     desc_sector<=1;desc_started<=0;desc_presented<=0;desc_next<=0;
    end else desc_valid<=0;
   end
   if(fault_eff&&desc_valid&&!desc_presented&&!mem_req_valid)desc_valid<=0;
   if(advance)begin
    if(result_pop&&!return_capture)res_valid<=0;
    if(read_issue)begin paid_payload<=in_payload;rd_valid<=1;pin_line<=hit_line;end
    if(tag_write)begin
     desc_line<=vic_line;desc_sector<=0;desc_started<=0;
     desc_presented<=0;desc_next<=0;desc_valid<=1;
    end
   end
  end
 end

`ifndef SYNTHESIS
 reg descriptor_was_valid;reg[31:0]descriptor_address;
 always @(posedge clk)begin
  if(reset)descriptor_was_valid<=0;
  else begin
   if(descriptor_was_valid&&hk_addr!==descriptor_address)
    $fatal(1,"Group sector changed before its descriptor drained");
   descriptor_was_valid<=desc_valid;descriptor_address<=hk_addr;
  end
 end
`endif

`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&(!abort&&((mem_resp_valid&&(!eff_started||desc_next>=16||mem_resp_index!=desc_next[3:0]))||(mem_resp_complete&&(!eff_started||eff_next!=16)))))$fatal(1,"GPU contract: texture return framing");
`endif

`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&((mem_resp_complete)&&!(mem_resp_ok)))$fatal(1,"GPU contract: mem_resp_ok");
`endif

 always @(posedge clk)if(reset)fault<=0;else if(abort)fault<=1;

`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&((!abort)&&(proto_fault||return_bad)))$fatal(1,"GPU contract: preflight_bad");
`endif
endmodule
