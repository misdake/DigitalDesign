// Eight 16x8 half-tile lines, four TDP banks with fixed zero padding. A compute clock-enable never gates B maintenance.
module gpu_v2_framebuffer_tdp #(
 parameter[31:0]COLOR_BASE=0,DEPTH_BASE=16384,
 parameter[9:0]WIDTH=400,parameter[8:0]HEIGHT=240,
 parameter PREFETCH_ENABLE=1,parameter PREFETCH_DIRTY=0
)(
 input clk,reset,ce,commit_allow,shade_phase,
 input[53:0]shared_rop_product,output shared_rop_offer,shared_rop_ce,
 output[26:0]shared_rop_a,output[7:0]shared_rop_b,output preflight_bad,
 input row_valid,input[8:0]row_tile,row_x,input[7:0]row_y,input[3:0]row_mask,
 input[2:0]row_index,input[31:0]row_data,output row_accept,row_ready,
 input[2:0]depth_func,input depth_write,blend,
 output reg commit_valid,output reg[8:0]commit_x,output reg[7:0]commit_y,output reg[3:0]commit_mask,
 input flush_request,output reg flush_complete,output fault,idle,
 input prefetch_abort,prefetch_valid,input[9:0]prefetch_tile,
 output mem_req_valid,mem_req_write,output[31:0]mem_req_addr,input mem_req_ready,
 input[63:0]mem_read_data,input mem_read_valid,
 output mem_write_valid,output[63:0]mem_write_data,input mem_write_ready,mem_complete_valid,mem_complete_ok
);
 localparam M_IDLE=0,M_RPRE=1,M_RREQ=2,M_RBEAT=3,M_RACK=4,M_WPRE=5,M_WREQ=6,M_WBEAT=7,M_WACK=8;
 reg[3:0]mst;reg mt_active;reg[2:0]mt_line;reg mt_tvalid;
 reg[9:0]mt_target;reg mt_write,mt_plane;reg[1:0]mt_remaining;reg mt_sector;
 reg[4:0]mt_beat;reg mt_accepted,mt_presented,req_valid,wr_beat_valid,flushing;
 reg[9:0]tag[0:7];reg tvalid[0:7],dirc[0:7],dird[0:7];reg[2:0]victim;
 // Lossy wall-clock hints never reserve a ROP context or a public credit.
 reg[9:0]hint0,hint1;reg[1:0]hint_count;
 reg mt_prefetch,prefetch_protect_valid;reg[2:0]prefetch_protect_line;
 reg fault_c,fault_m;assign fault=fault_c||fault_m;
 reg[1:0]cal;reg[2:0]fill_row;
 reg ctx_valid[0:1];reg[2:0]ctx_line[0:1];reg[8:0]ctx_x[0:1];reg[7:0]ctx_y[0:1];reg[3:0]ctx_mask[0:1];
 reg tail,result_owner;reg[1:0]return_key;
 reg[31:0]source_rgba;reg[1:0]compare_bank;
 reg depth_pending,color_pending;reg[15:0]depth_data,color_data;
 reg[8:0]result_addr;reg[1:0]result_bank;reg result_depth_flag,result_color_flag,result_last,result_context;
 wire actual_ce=ce&&commit_allow;
 wire rop_active=ctx_valid[0]||ctx_valid[1];
 wire[9:0]row_key={row_tile[8:5],row_y[3],row_tile[4:0]};
 wire[7:0]resident_hits;genvar n;
 generate for(n=0;n<8;n=n+1)begin:resident_compare
  assign resident_hits[n]=tvalid[n]&&!(mt_active&&mt_line==n)&&tag[n]==row_key;
 end endgenerate
 wire hit=|resident_hits;
 wire[2:0]hit_line={|resident_hits[7:4],resident_hits[2]|resident_hits[3]|resident_hits[6]|resident_hits[7],resident_hits[1]|resident_hits[3]|resident_hits[5]|resident_hits[7]};
 wire row_credit=!ctx_valid[tail]||fill_row!=0;
 wire aligned=(cal[1]==row_index[1])&&(cal[0]==row_index[0]);
 wire source_ready=aligned&&(fill_row!=0||(!flushing&&hit));
 wire ingress_error=ce&&!reset&&!fault&&row_valid&&row_credit&&(
  row_index!=fill_row || (row_index[0]&&|row_data[31:16]) ||
  (fill_row==0&&(row_x[0]||row_y[0]||{1'b0,row_x}+10'd1>=WIDTH||{1'b0,row_y}+9'd1>=HEIGHT||row_mask==0)));
 wire req_accept=req_valid&&mem_req_ready;
 wire extra_read=mt_active&&mem_read_valid&&(mt_write||!(mt_accepted||req_accept)||mt_beat>=16);
 wire leaf_out_valid,leaf_cw,leaf_dw,shared_mul_pending;
 wire[15:0]leaf_new_color,leaf_new_depth;
 wire return_bad=ce&&!reset&&!fault&&leaf_out_valid&&(!ctx_valid[result_owner]||depth_pending||!shade_phase);
 wire source_bad=ingress_error||extra_read||return_bad;
 assign preflight_bad=!reset&&source_bad;
 assign row_ready=!reset&&!fault&&row_credit&&source_ready&&!source_bad;
 assign row_accept=actual_ce&&row_valid&&row_ready;
 wire color_fire=row_accept&&!row_index[0];
 wire depth_input_fire=row_accept&&row_index[0];
 wire row0_fire=color_fire&&row_index==0;
 wire leaf_ce=actual_ce&&!reset&&!fault&&!source_bad;
 wire return_fire=leaf_ce&&leaf_out_valid;
 wire depth_fire=leaf_ce&&depth_pending;
 wire color_fire_write=leaf_ce&&color_pending;
 wire[2:0]incoming_line=fill_row==0?hit_line:ctx_line[tail];
 wire[1:0]incoming_bank=row_index[2:1]^{row_x[1],1'b0};
 wire[9:0]incoming_addr={row_index[0],1'b0,incoming_line,row_y[2:1],row_index[2],row_x[3:2]};
 wire incoming_covered=ctx_mask[tail][row_index[2:1]];
 wire need_old_color=row_mask[row_index[2:1]]&&blend;
 wire need_old_depth=row_mask[row_index[2:1]]&&depth_func!=0&&depth_func!=7;
 wire rop_read=(color_fire&&need_old_color)||(depth_input_fire&&need_old_depth);
 wire[15:0]a_q[0:3],b_q[0:3];
 wire[15:0]leaf_old_color=(blend&&incoming_covered)?a_q[incoming_bank]:16'd0;
 gpu_v2_rop_tdp leaf(.clk(clk),.reset(reset||fault),.ce(leaf_ce),.shade_phase(shade_phase),
  .in_valid(depth_input_fire),.blend(blend),.depth_func(depth_func),.depth_write(depth_write),.covered(incoming_covered),
  .old_color(leaf_old_color),.old_depth(a_q[compare_bank]),.src_r(source_rgba[7:0]),.src_g(source_rgba[15:8]),
  .src_b(source_rgba[23:16]),.src_a(source_rgba[31:24]),.src_depth(row_data[15:0]),.in_ready(),
  .out_valid(leaf_out_valid),.new_color(leaf_new_color),.new_depth(leaf_new_depth),.color_written(leaf_cw),.depth_written(leaf_dw),
  .shared_rop_product(shared_rop_product),.shared_mul_pending(shared_mul_pending),.shared_rop_ce(shared_rop_ce),.shared_rop_a(shared_rop_a),.shared_rop_b(shared_rop_b));
 assign shared_rop_offer=ce&&!reset&&!fault&&shared_mul_pending;
 wire wbeat=mt_active&&mt_write&&wr_beat_valid&&mem_write_ready&&(mt_accepted||req_accept);
 wire rbeat=mt_active&&!mt_write&&(mt_accepted||req_accept)&&mt_beat<16&&mem_read_valid;
 wire refill_write=rbeat;
 wire use_nxt=(mst==M_WREQ)||(mst==M_WBEAT);
 wire m_rd_issue=mt_active&&mt_write&&((mst==M_WPRE)||(mst==M_WREQ&&wbeat)||(mst==M_WBEAT&&wbeat&&mt_beat<15));
 wire[3:0]maintenance_read_beat=use_nxt?mt_beat[3:0]+4'd1:mt_beat[3:0];
 wire[9:0]maintenance_read_addr={mt_plane,1'b0,mt_line,mt_sector,maintenance_read_beat};
 wire[9:0]maintenance_write_addr={mt_plane,1'b0,mt_line,mt_sector,mt_beat[3:0]};
 wire[5:0]beats_total={1'b0,mt_beat}+{5'd0,rbeat}+{5'd0,wbeat};
 wire stop_publication=fault||source_bad;
 wire[9:0]mt_tile=mt_write?tag[mt_line]:mt_target;
 // Raw {Y/8[4:0],X/16[4:0]} key is shared by the queue/cache. Linearize
 // the held victim/new 16x16 tile, retaining key bit5 as its half selector.
 wire[8:0]mt_tile_y={5'd0,mt_tile[9:6]};
 // WIDTH is elaboration-time constant. Explicit constant shifts keep this
 // address conversion in fabric rather than spending a DSP on multiply-add.
 wire[8:0]mt_linear_sum[0:6];assign mt_linear_sum[0]={4'd0,mt_tile[4:0]};
 generate for(n=0;n<6;n=n+1)begin:tile_stride
  if(WIDTH[n+4])assign mt_linear_sum[n+1]=mt_linear_sum[n]+(mt_tile_y<<n);
  else assign mt_linear_sum[n+1]=mt_linear_sum[n];
 end endgenerate
 wire[8:0]mt_linear_tile=mt_linear_sum[6];
 assign mem_req_valid=req_valid;assign mem_req_write=mt_write;
 assign mem_req_addr=(mt_plane?DEPTH_BASE:COLOR_BASE)+{14'd0,mt_linear_tile,mt_tile[5],mt_sector,7'b0};
 assign mem_write_valid=wr_beat_valid;
 // B's native Q is the single held beat: next read occurs only when old beat fires.
 assign mem_write_data=mt_beat[2]?{b_q[1],b_q[0],b_q[3],b_q[2]}:{b_q[3],b_q[2],b_q[1],b_q[0]};
 generate for(n=0;n<4;n=n+1)begin:banks
  wire a_write_d=depth_fire&&result_depth_flag&&result_bank==n;
  wire a_write_c=color_fire_write&&result_color_flag&&result_bank==n;
  wire a_read=rop_read&&incoming_bank==n;
  wire a_en=a_read||a_write_d||a_write_c;
  wire a_we=a_write_d||a_write_c;
  wire[9:0]a_addr=a_we?{a_write_d,result_addr}:incoming_addr;
  wire[15:0]a_data=depth_pending?depth_data:color_data;
  wire b_en=m_rd_issue||refill_write;wire b_we=refill_write;
  wire[9:0]b_addr=refill_write?maintenance_write_addr:maintenance_read_addr;
  wire[15:0]b_data=mt_beat[2]?mem_read_data[(n^2)*16+:16]:mem_read_data[n*16+:16];
  gpu_v2_framebuffer_tdp_bank ram(.clk(clk),.reset(reset),.a_en(a_en),.a_we(a_we),.a_addr(a_addr),.a_data(a_data),.a_q(a_q[n]),
   .b_en(b_en),.b_we(b_we),.b_addr(b_addr),.b_data(b_data),.b_q(b_q[n]));
`ifndef SYNTHESIS
  always @(posedge clk)if(!reset)begin
   if(a_read&&(a_write_d||a_write_c))$fatal(1,"TDP A static calendar collision");
   if(a_write_d&&a_write_c)$fatal(1,"TDP D/C write collision");
  end
`endif
 end endgenerate
 // Physical ownership, not global ROP busy, excludes maintenance victims.
 reg[7:0]pinned;reg free_found,dirty_found,any_dirty;reg[2:0]free_line,dirty_line;
 integer k;
 wire[2:0]victim1=victim+3'd1,victim2=victim+3'd2;
 wire[2:0]select_line=!pinned[victim]?victim:(!pinned[victim1]?victim1:victim2);
 always @*begin
  pinned=0;
  for(k=0;k<8;k=k+1)pinned[k]=(ctx_valid[0]&&ctx_line[0]==k)||(ctx_valid[1]&&ctx_line[1]==k);
  free_found=0;dirty_found=0;any_dirty=0;free_line=0;dirty_line=0;
  for(k=0;k<8;k=k+1)begin
   if(!pinned[k]&&!tvalid[k]&&!free_found)begin free_found=1;free_line=k;end
   if(dirc[k]||dird[k])begin
    any_dirty=1;if(!pinned[k]&&!dirty_found)begin dirty_found=1;dirty_line=k;end
   end
  end
 end
 wire[2:0]sel_line=free_found?free_line:select_line;
 wire sel_dirc=dirc[sel_line],sel_dird=dird[sel_line],sel_dirty=sel_dirc||sel_dird,sel_plane=!sel_dirc;
 // At most two distinct pinned lines, hence one of these three is available.
 wire maintenance_available=1'b1;
 // A queued hint is rechecked when it reaches service, not merely on arrival.
 reg hint_resident,incoming_resident;integer hk;
 // Reuse the demand candidate. XY may veto a hint, never choose its line.
 wire[2:0]spec_line=sel_line;
 wire spec_found=!(row_valid&&resident_hits[sel_line])&&
                 !(prefetch_protect_valid&&prefetch_protect_line==sel_line)&&
                 (PREFETCH_DIRTY||!sel_dirty);
 always @*begin
  hint_resident=0;incoming_resident=0;
  for(hk=0;hk<8;hk=hk+1)begin
   if(tvalid[hk]&&tag[hk]==hint0)hint_resident=1;
   if(tvalid[hk]&&tag[hk]==prefetch_tile)incoming_resident=1;

  end
 end
 wire demand_wait=row_valid&&fill_row==0&&!hit;
 wire hint_service=PREFETCH_ENABLE&&!mt_active&&!fault&&!source_bad&&
                   !prefetch_abort&&!flush_request&&!flushing&&!demand_wait&&hint_count!=0;
 wire hint_start=hint_service&&!hint_resident&&spec_found;
 wire hint_unsafe=hint_service&&!hint_resident&&!spec_found;
 wire hint_pop=hint_service; // Stale/no-victim hints are deliberately dropped.
 wire hint_duplicate=(hint_count!=0&&hint0==prefetch_tile)||(hint_count==2&&hint1==prefetch_tile)||
                     incoming_resident||(mt_active&&mt_tvalid&&mt_target==prefetch_tile);
 wire hint_tile_legal=(prefetch_tile[4:0]<(WIDTH>>4))&&(prefetch_tile[9:5]<(HEIGHT>>3));
 wire hint_push=PREFETCH_ENABLE&&prefetch_valid&&hint_tile_legal&&hint_count<2&&
                !hint_duplicate&&!prefetch_abort&&!fault&&!source_bad&&!flushing&&!flush_request;
 // This queue uses old-state capacity; a full hint is lossy even on a pop edge.
 always @(posedge clk)begin
  if(reset||prefetch_abort||fault||source_bad||flush_request||flushing)begin hint_count<=0;hint0<=0;hint1<=0;end
  else if(PREFETCH_ENABLE)begin
   case({hint_pop,hint_push})
    2'b01:begin if(hint_count==0)hint0<=prefetch_tile;else hint1<=prefetch_tile;hint_count<=hint_count+1'b1;end
    2'b10:begin hint0<=hint1;hint_count<=hint_count-1'b1;end
    2'b11:begin if(hint_count==1)hint0<=prefetch_tile;else begin hint0<=hint1;hint1<=prefetch_tile;end end
    default:begin end
   endcase
  end
 end
 assign idle=(hint_count==0)&&!flushing&&!rop_active&&!mt_active&&fill_row==0&&!depth_pending&&!color_pending&&!leaf_out_valid&&!shared_mul_pending;
 integer f;
 always @(posedge clk)begin
  if(reset)begin
   mst<=M_IDLE;mt_active<=0;mt_accepted<=0;mt_presented<=0;req_valid<=0;wr_beat_valid<=0;mt_remaining<=0;
   mt_write<=0;mt_plane<=0;mt_sector<=0;mt_beat<=0;mt_line<=0;mt_tvalid<=0;mt_target<=0;
   flushing<=0;flush_complete<=0;fault_c<=0;fault_m<=0;commit_valid<=0;victim<=0;
   cal<=0;fill_row<=0;tail<=0;result_owner<=0;return_key<=0;depth_pending<=0;color_pending<=0;
   ctx_valid[0]<=0;ctx_valid[1]<=0;compare_bank<=0;
   mt_prefetch<=0;prefetch_protect_valid<=0;prefetch_protect_line<=0;
   for(f=0;f<8;f=f+1)begin tvalid[f]<=0;dirc[f]<=0;dird[f]<=0;tag[f]<=0;end
  end else begin
   commit_valid<=0;
   // Consuming the previous protected line must not clear a different line
   // published by maintenance on this edge. Publication below has priority.
   if(row0_fire&&prefetch_protect_valid&&prefetch_protect_line==hit_line)
    prefetch_protect_valid<=0;
   if(ingress_error||return_bad)fault_c<=1;
   if(extra_read)fault_m<=1;
            // A fault cancels only work that has never been presented. A
            // presented request stays stable until accepted; accepted transport
            // runs to its terminal response.
            if (mt_active && (fault || (prefetch_abort && mt_prefetch)) && !mt_accepted && !mt_presented) begin
                mt_active <= 1'b0;
                mst <= M_IDLE;
                req_valid <= 1'b0;
                wr_beat_valid <= 1'b0;
            end else if (mt_active) begin
                if ((mt_accepted || req_accept) && mem_complete_valid) begin
                    // Terminal response: validate the accumulated beat count.
                    if (stop_publication) begin
                        // Already faulted: terminate without publishing.
                        mt_active <= 1'b0; mst <= M_IDLE;
                        req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                    end else if (!mem_complete_ok) begin
                        fault_m <= 1'b1;
                        mt_active <= 1'b0; mst <= M_IDLE;
                        req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                    end else if (beats_total != 6'd16) begin
                        // A success terminal before 16 beats is a protocol fault.
                        fault_m <= 1'b1;
                        mt_active <= 1'b0; mst <= M_IDLE;
                        req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                    end else if (!mt_sector) begin
                        mt_sector <= mt_sector + 1'b1;
                        mt_beat <= 5'd0;
                        mt_accepted <= 1'b0;
                        mt_presented <= 1'b0;
                        req_valid <= 1'b0;
                        wr_beat_valid <= 1'b0;
                        mst <= mt_write ? M_WPRE : M_RPRE;
                    end else if (mt_write) begin
                        if (mt_plane == 1'b0) dirc[mt_line] <= 1'b0;
                        else dird[mt_line] <= 1'b0;
                        mt_remaining[mt_plane] <= 1'b0;
                        if (mt_plane ? mt_remaining[0] : mt_remaining[1]) begin
                            mt_plane <= !mt_plane;
                            mt_sector <= 1'b0; mt_beat <= 5'd0;
                            mt_accepted <= 1'b0; mt_presented <= 1'b0;
                            req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                            mst <= M_WPRE;
                        end else if (mt_tvalid) begin
                            mt_write <= 1'b0;
                            mt_plane <= 1'b0;
                            mt_sector <= 1'b0; mt_beat <= 5'd0;
                            mt_accepted <= 1'b0; mt_presented <= 1'b0;
                            req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                            tvalid[mt_line] <= 1'b0;
                            mst <= M_RPRE;
                        end else begin
                            mt_active <= 1'b0; mst <= M_IDLE;
                        end
                    end else if (mt_plane == 1'b0) begin
                        mt_plane <= 1'b1;
                        mt_sector <= 1'b0; mt_beat <= 5'd0;
                        mt_accepted <= 1'b0; mt_presented <= 1'b0;
                        req_valid <= 1'b0; wr_beat_valid <= 1'b0;
                        mst <= M_RPRE;
                    end else begin
                        tag[mt_line] <= mt_target;
                        tvalid[mt_line] <= 1'b1;
                        if(mt_prefetch)begin prefetch_protect_valid<=1;prefetch_protect_line<=mt_line;end
                        mt_active <= 1'b0; mst <= M_IDLE;
                    end
                end else begin
                    case (mst)
                        M_RPRE: begin
                            req_valid <= 1'b1;
                            mt_presented <= 1'b1;
                            mst <= M_RREQ;
                        end
                        M_RREQ: begin
                            if (req_accept) begin
                                req_valid <= 1'b0;
                                mt_accepted <= 1'b1;
                                if (rbeat) mt_beat <= 5'd1;
                                mst <= M_RBEAT;
                            end
                        end
                        M_RBEAT: begin
                            if (rbeat && mt_beat == 5'd15) begin
                                mt_beat <= 5'd16;
                                mst <= M_RACK;
                            end else if (rbeat) begin
                                mt_beat <= mt_beat + 5'd1;
                            end
                        end
                        M_RACK: begin
                        end
                        M_WPRE: begin
                            req_valid <= 1'b1;
                            wr_beat_valid <= 1'b1;
                            mt_presented <= 1'b1;
                            mst <= M_WREQ;
                        end
                        M_WREQ: begin
                            if (req_accept) begin
                                req_valid <= 1'b0;
                                mt_accepted <= 1'b1;
                                if (wbeat) mt_beat <= 5'd1;
                                mst <= M_WBEAT;
                            end
                        end
                        M_WBEAT: begin
                            if (wbeat) begin
                                if (mt_beat == 5'd15) begin
                                    mt_beat <= 5'd16;
                                    wr_beat_valid <= 1'b0;
                                    mst <= M_WACK;
                                end else begin
                                    mt_beat <= mt_beat + 5'd1;
                                end
                            end
                        end
                        M_WACK: begin
                        end
                        default: mst <= M_IDLE;
                    endcase
                end
            end
   if(!flush_request&&!flushing)flush_complete<=0;
   if(flush_request&&!flushing&&!flush_complete&&!fault)begin flushing<=1;flush_complete<=0;end
   if(fault)begin ctx_valid[0]<=0;ctx_valid[1]<=0;fill_row<=0;depth_pending<=0;color_pending<=0;end
   if(!mt_active&&!fault&&!source_bad)begin
    if(flushing)begin
     if(dirty_found)begin
      mt_active<=1;mt_prefetch<=0;mt_line<=dirty_line;mt_tvalid<=0;mt_write<=1;
      mt_remaining<={dird[dirty_line],dirc[dirty_line]};mt_plane<=!dirc[dirty_line];mt_sector<=0;mt_beat<=0;
      mt_accepted<=0;mt_presented<=0;req_valid<=0;wr_beat_valid<=0;mst<=M_WPRE;
     end else if(!any_dirty&&!rop_active)begin
      flushing<=0;flush_complete<=1;prefetch_protect_valid<=0;
      for(f=0;f<8;f=f+1)tvalid[f]<=0;
     end
    end else if(ce&&row_valid&&fill_row==0&&!hit&&maintenance_available)begin
     mt_active<=1;mt_prefetch<=0;mt_line<=sel_line;victim<=sel_line+3'd1;mt_tvalid<=1;
     if(prefetch_protect_line==sel_line)prefetch_protect_valid<=0;mt_target<=row_key;
     mt_remaining<={sel_dird,sel_dirc};mt_write<=sel_dirty;mt_plane<=sel_dirty?sel_plane:1'b0;
     mt_sector<=0;mt_beat<=0;mt_accepted<=0;mt_presented<=0;req_valid<=0;wr_beat_valid<=0;
     if(!sel_dirty)begin tvalid[sel_line]<=0;mst<=M_RPRE;end else mst<=M_WPRE;
    end else if(hint_start)begin
     mt_active<=1;mt_prefetch<=1;mt_line<=sel_line;victim<=sel_line+3'd1;mt_tvalid<=1;mt_target<=hint0;
     mt_remaining<={sel_dird,sel_dirc};mt_write<=sel_dirty;mt_plane<=sel_plane;
     mt_sector<=0;mt_beat<=0;mt_accepted<=0;mt_presented<=0;req_valid<=0;wr_beat_valid<=0;
     if(sel_dirty)mst<=M_WPRE;
     else begin tvalid[sel_line]<=0;mt_plane<=0;mst<=M_RPRE;end
    end else if(hint_unsafe)begin
     // Lossy hint: no retry state and no same-edge demand credit is borrowed.
     // Advance once so later hints do not stick on one protected/dirty line.
     victim<=sel_line+3'd1;
    end
   end
   if(leaf_ce)begin
    cal<=cal+2'd1;
    depth_pending<=return_fire;color_pending<=depth_pending;
    if(row_accept)begin
     fill_row<=fill_row+3'd1;
     if(!row_index[0])source_rgba<=row_data;
     else begin compare_bank<=incoming_bank;if(row_index==7)tail<=!tail;end
     if(row0_fire)begin
      ctx_valid[tail]<=1;ctx_line[tail]<=hit_line;ctx_x[tail]<=row_x;ctx_y[tail]<=row_y;ctx_mask[tail]<=row_mask;end
    end
    if(return_fire)begin
     depth_data<=leaf_new_depth;color_data<=leaf_new_color;result_depth_flag<=leaf_dw;result_color_flag<=leaf_cw;
     result_bank<=return_key^{ctx_x[result_owner][1],1'b0};
     result_addr<={1'b0,ctx_line[result_owner],ctx_y[result_owner][2:1],return_key[1],ctx_x[result_owner][3:2]};
     result_last<=return_key==3;result_context<=result_owner;
     return_key<=return_key+2'd1;if(return_key==3)result_owner<=!result_owner;
    end
    if(depth_fire&&result_depth_flag)dird[result_addr[7:5]]<=1;
    if(color_fire_write)begin
     if(result_color_flag)dirc[result_addr[7:5]]<=1;
     if(result_last)begin
      commit_valid<=1;commit_x<=ctx_x[result_context];commit_y<=ctx_y[result_context];commit_mask<=ctx_mask[result_context];ctx_valid[result_context]<=0;
     end
    end
   end
  end
 end
`ifndef SYNTHESIS
 integer pin_count,pi,ti,tj;
 always @*begin pin_count=0;for(pi=0;pi<8;pi=pi+1)pin_count=pin_count+pinned[pi];end
 always @(posedge clk)if(!reset)begin
  for(ti=0;ti<8;ti=ti+1)for(tj=ti+1;tj<8;tj=tj+1)
   if(tvalid[ti]&&tvalid[tj]&&tag[ti]==tag[tj])$fatal(1,"duplicate resident tag");
  // All B launch classes exclude a simultaneous new owner independently:
  // demand is a miss, flush cannot admit a new quad, speculative tags exclude
  // the offered demand even before the A handshake.
  if(row0_fire&&!mt_active)begin
   if(flushing)$fatal(1,"flush admitted a new ROP owner");
   if(ce&&row_valid&&fill_row==0&&!hit)$fatal(1,"demand miss admitted a hit owner");
   if(hint_start&&spec_line==hit_line)$fatal(1,"speculation stole same-edge owner");
  end
  if(hint_start&&(pinned[spec_line]||(!PREFETCH_DIRTY&&(dirc[spec_line]||dird[spec_line]))))$fatal(1,"unsafe speculative victim");
  if(hint_count>2)$fatal(1,"hint queue overflow");
  if(pin_count>2||pinned[select_line])$fatal(1,"three-candidate victim proof violated");
  if(leaf_ce&&cal[0]!==shade_phase)$fatal(1,"TDP/common shared DSP phase mismatch");
  if(depth_pending&&color_pending)$fatal(1,"overlapping fixed result slots");
  if(shared_rop_ce&&!shade_phase)$fatal(1,"ROP shared DSP wrong phase");
  if(mt_active&&pinned[mt_line])$fatal(1,"maintenance target is ROP pinned");
  if(row0_fire&&ctx_valid[tail])$fatal(1,"new owner overwrote a pinned context");
  if(leaf_ce&&fill_row[0]&&!depth_input_fire)$fatal(1,"paired source D missing after accepted C");
 end
`endif
endmodule
