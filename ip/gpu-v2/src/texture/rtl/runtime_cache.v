// Borrowed Pool head; one paid data read and one captured Color row.
module gpu_v2_texture_runtime_cache # (parameter NATIVE=@NATIVE@)(
 input wire clk,reset,proposal_ce,commit_allow,abort,
 input wire in_valid,input wire[71:0]in_payload,
 output wire proposal_ready,in_ready,preflight_bad,advance,
 input wire out_ready,output wire out_valid,output wire[71:0]out_payload,
 output wire[15:0]out_tex0,out_tex1,out_tex2,out_tex3,
 output wire mem_req_valid,output wire[31:0]mem_req_addr,
 input wire mem_req_ready,mem_resp_valid,input wire[3:0]mem_resp_index,
 input wire[63:0]mem_resp_data,input wire mem_resp_complete,mem_resp_ok,
 output wire drained,output reg fault
);
@SLOT_FUNCTIONS@
 (* syn_ramstyle="distributed_ram" *) reg[7:0]state_rows[0:15];
 (* syn_ramstyle="distributed_ram" *) reg[2:0]plru_rows[0:15];
 reg[15:0]state_initialized,plru_initialized;
 wire[17:0]tag_q[0:3];reg tag_valid;
 // Payload is written once at actual read issue, held through capture/pop.
 reg[71:0]res_payload;reg[15:0]res_tex[0:3];reg res_valid,rd_valid;
 reg[5:0]pin_line;wire pin_valid=rd_valid;
 reg desc_valid,desc_started,desc_presented;reg[5:0]desc_line;
 reg[31:0]desc_addr;reg[4:0]desc_next;
 wire head_valid=in_valid;wire[71:0]head_payload=in_payload;
 reg hit_ready,look_filling,vic_valid;reg[5:0]hit_line,vic_line;
 integer wl,wv;reg[1:0]o0,o1;
 wire[21:0]head_key=head_payload[21:0];
 wire[2:0]head_tlx=head_payload[24:22],head_tly=head_payload[27:25];
 wire[3:0]head_set={head_key[16:15],head_key[9:8]};
 wire[3:0]hk_slot=head_key[3:0],hk_n=head_key[7:4];
 wire[6:0]hk_x=head_key[14:8],hk_y=head_key[21:15];
 wire[31:0]hk_lay=slot_full_f(hk_slot)?layer_off_f(hk_n):32'd0;
 wire[13:0]hk_row={7'd0,hk_y}<<((hk_n<4'd3)?4'd0:(hk_n-4'd3));
 wire[13:0]hk_tile=hk_row|{7'd0,hk_x};
 wire[14:0]hk_offset={1'b0,hk_tile}+{1'b0,hk_lay[13:0]};
 wire[31:0]hk_addr=slot_base_f(hk_slot)+{10'd0,hk_offset,7'd0};
 wire[6:0]ik_xy=hk_x|hk_y;
 wire ik_coord_bad=(ik_xy[0]&&hk_n<4'd4)||(ik_xy[1]&&hk_n<4'd5)||
 (ik_xy[2]&&hk_n<4'd6)||(ik_xy[3]&&hk_n<4'd7)||
 (ik_xy[4]&&hk_n<4'd8)||(ik_xy[5]&&hk_n<4'd9)||(ik_xy[6]&&hk_n<4'd10);
 wire running=proposal_ce&&!reset&&!fault&&!abort;
 wire input_bad=running&&head_valid&&(!slot_valid_f(hk_slot)||hk_n>10||
 hk_n>slot_max_f(hk_slot)||(!slot_full_f(hk_slot)&&hk_n!=slot_max_f(hk_slot))||ik_coord_bad);
 wire borrow_bad=running&&desc_valid&&(!head_valid||hk_addr!=desc_addr);
 wire req_present=!reset&&desc_valid&&!desc_started&&(!fault||desc_presented);
 wire req_accept=req_present&&mem_req_ready;
 wire eff_started=desc_valid&&(desc_started||req_accept);
 wire beat_present=mem_resp_valid;
 wire beat_ok=beat_present&&eff_started&&desc_next<16&&mem_resp_index==desc_next[3:0];
 wire[4:0]eff_next=desc_next+(beat_ok?5'd1:5'd0);
 wire terminal=mem_resp_complete;
 wire term_ok=terminal&&mem_resp_ok&&eff_started&&eff_next==16;
 wire proto_fault=!reset&&((beat_present&&!beat_ok)||(terminal&&!term_ok));
 wire return_bad=!reset&&rd_valid&&res_valid;
 assign preflight_bad=input_bad||borrow_bad||proto_fault||return_bad;
 wire fault_eff=fault||abort||preflight_bad;
 assign advance=running&&!preflight_bad&&commit_allow;
 assign proposal_ready=!reset&&!fault&&!abort&&head_valid&&tag_valid&&!desc_valid&&
 !rd_valid&&(!res_valid||out_ready)&&hit_ready;
 wire read_issue=advance&&proposal_ready;
 assign in_ready=read_issue;
 assign out_valid=!reset&&!fault&&!abort&&res_valid;
 assign out_payload=res_payload;
 // Canonical tap order: retain the existing two conditional bank swaps.
 wire[15:0]x0=res_payload[22]?res_tex[1]:res_tex[0];
 wire[15:0]x1=res_payload[22]?res_tex[0]:res_tex[1];
 wire[15:0]x2=res_payload[22]?res_tex[3]:res_tex[2];
 wire[15:0]x3=res_payload[22]?res_tex[2]:res_tex[3];
 wire y0=res_payload[25]^res_payload[23],y1=y0^res_payload[22];
 assign out_tex0=y0?x2:x0;assign out_tex1=y1?x3:x1;
 assign out_tex2=y0?x0:x2;assign out_tex3=y1?x1:x3;
 assign mem_req_valid=req_present&&(!(fault||abort)||desc_presented);
 assign mem_req_addr=desc_addr;assign drained=!desc_valid;
 wire[17:0]head_tag={head_key[21:17],head_key[14:10],head_key[7:0]};
 wire tag_write=advance&&head_valid&&tag_valid&&!desc_valid&&!hit_ready&&!look_filling&&vic_valid;
 // Speculative tag port only: Pool ownership is unchanged until read_issue.
 wire tag_read=!reset&&!fault_eff&&head_valid&&!tag_write;
 wire[3:0]tag_set=head_set;
 generate if(NATIVE)begin:tags_native
  genvar tg;for(tg=0;tg<2;tg=tg+1)begin:tag_pair
   wire[35:0]pair_q;wire[3:0]byte_mask=vic_line[0]?4'b1100:4'b0011;
   SDPX9B #(.READ_MODE(1'b0),.BIT_WIDTH_0(36),.BIT_WIDTH_1(36)) bank(
   .DO(pair_q),.DI({head_tag,head_tag}),.BLKSELA(3'b000),.BLKSELB(3'b000),
   .ADA({5'd0,vic_line[5:2],1'b0,byte_mask}),.ADB({5'd0,tag_set,5'd0}),
   .CLKA(clk),.CLKB(clk),.CEA(tag_write&&vic_line[1]==tg),.CEB(tag_read),
   .OCE(1'b1),.RESETA(1'b0),.RESETB(1'b0));
   assign tag_q[tg*2]=pair_q[17:0];assign tag_q[tg*2+1]=pair_q[35:18];
  end
 end else begin:tags_portable
  reg[17:0]tag_ram[0:63];reg[17:0]q[0:3];integer tr;
  always@(posedge clk)begin
   if(tag_write)tag_ram[vic_line]<=head_tag;
   if(tag_read)for(tr=0;tr<4;tr=tr+1)q[tr]<=tag_ram[{tag_set,2'b00}+tr];
  end
  genvar tp;for(tp=0;tp<4;tp=tp+1)begin:tag_port assign tag_q[tp]=q[tp];end
 end endgenerate
 wire[3:0]state_set=desc_valid?desc_line[5:2]:head_set;
 wire[7:0]head_state=state_initialized[state_set]?state_rows[state_set]:8'd0;
 wire[2:0]head_plru=plru_initialized[state_set]?plru_rows[state_set]:3'd0;
 wire publish=terminal&&!reset&&!fault_eff&&term_ok;
 wire state_write=tag_write||(desc_valid&&(terminal||fault_eff||(beat_present&&!beat_ok)));
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
  if(reset)begin state_initialized<=0;plru_initialized<=0;end
  else begin
   if(state_write)begin state_rows[state_set]<=state_next;state_initialized[state_set]<=1;end
   if(plru_write)begin plru_rows[state_set]<=plru_next;plru_initialized[state_set]<=1;end
  end
 end
@HIT_AND_BANK_MAP@
 wire data_write=!reset&&!fault_eff&&beat_ok;
 wire[9:0]data_waddr={desc_line,mem_resp_index[3:1],mem_resp_index[0]};
 wire[15:0]data_di[0:3],data_do[0:3];
 assign data_di[0]=mem_resp_index[1]?mem_resp_data[32+:16]:mem_resp_data[0+:16];
 assign data_di[1]=mem_resp_index[1]?mem_resp_data[48+:16]:mem_resp_data[16+:16];
 assign data_di[2]=mem_resp_index[1]?mem_resp_data[0+:16]:mem_resp_data[32+:16];
 assign data_di[3]=mem_resp_index[1]?mem_resp_data[16+:16]:mem_resp_data[48+:16];
 wire[9:0]data_ra[0:3];
 assign data_ra[0]={hit_line,bank_y0,bank_x0[2]};
 assign data_ra[1]={hit_line,bank_y1,bank_x1[2]};
 assign data_ra[2]={hit_line,bank_y2,bank_x2[2]};
 assign data_ra[3]={hit_line,bank_y3,bank_x3[2]};
 genvar db;generate for(db=0;db<4;db=db+1)begin:data_bank
  if(NATIVE)begin:native_bank
   wire[31:0]q;
   SDPB #(.READ_MODE(1'b0),.BIT_WIDTH_0(16),.BIT_WIDTH_1(16),.RESET_MODE("SYNC")) bank(
   .DO(q),.DI({16'b0,data_di[db]}),.BLKSELA(3'b0),.BLKSELB(3'b0),
   .ADA({data_waddr,4'b0011}),.ADB({data_ra[db],4'b0}),
   .CLKA(clk),.CLKB(clk),.CEA(data_write),.CEB(read_issue),.OCE(1'b1),
   .RESETA(1'b0),.RESETB(reset));
   assign data_do[db]=q[15:0];
  end else begin:portable_bank
   reg[15:0]mem[0:1023];reg[15:0]q;
   always@(posedge clk)begin
    if(data_write)mem[data_waddr]<=data_di[db];
    if(reset)q<=0;else if(read_issue)q<=mem[data_ra[db]];
   end
   assign data_do[db]=q;
  end
 end endgenerate
 integer b;
 always@(posedge clk)begin
  if(reset)begin
   res_valid<=0;rd_valid<=0;tag_valid<=0;fault<=0;
   desc_valid<=0;desc_started<=0;desc_presented<=0;desc_next<=0;
  end else begin
   if(fault_eff)fault<=1;
   if(!head_valid||read_issue||tag_write)tag_valid<=0;
   else if(tag_read)tag_valid<=1;
   // Paid returns capture on the wall clock, never borrowed as same-edge credit.
   if(rd_valid&&!res_valid)begin
    for(b=0;b<4;b=b+1)res_tex[b]<=data_do[b];
    res_valid<=1;rd_valid<=0;
   end
   if(mem_req_valid)desc_presented<=1;
   if(mem_req_valid&&mem_req_ready)begin desc_started<=1;desc_presented<=1;end
   if(beat_present&&desc_valid&&eff_started&&desc_next<16&&mem_resp_index==desc_next[3:0])desc_next<=eff_next;
   if(terminal)desc_valid<=0;
   if(fault_eff&&desc_valid&&!desc_presented&&!mem_req_valid)desc_valid<=0;
   if(advance)begin
    if(out_ready&&res_valid)res_valid<=0;
    if(read_issue)begin res_payload<=head_payload;rd_valid<=1;pin_line<=hit_line;end
    if(tag_write)begin
     desc_line<=vic_line;desc_addr<=hk_addr;desc_started<=0;
     desc_presented<=0;desc_next<=0;desc_valid<=1;
    end
   end
  end
 end
endmodule
