// 连续静态发射R：固定工作Q/stepQ，静态指令提前取，几何谓词注册后分支。
(* syn_dspstyle="logic" *)
module rc_raster #(parameter INTERLEAVE=1,parameter EARLY_OUT=1)(
 input clk,reset,ce,input[1:0]phase,
 input bbox_done,bbox_empty,input[4:0]bbox_xlo,bbox_xhi,input[3:0]bbox_ylo,bbox_yhi,
 input run_enable,
 input start,
 input geo_we,input[3:0]geo_wa,input[29:0]geo_wd,
 output quad_valid,input quad_ready,output quad_take,output[7:0]quad_x,output[6:0]quad_y,
 output sample_valid,output[1:0]sample_lane,output[29:0]sample_h,
 output sample_cover,sample_anchor,output quad_end_valid,output[3:0]quad_mask,
 output reg busy,done,output reg[3:0]error,
 output evt_import,evt_prep,evt_traverse,evt_row,evt_align,evt_credit_wait,evt_leaf,evt_empty_quad,evt_node,
 output full_scan
);
 localparam IDLE=0,IMPORT=1,EXEC=2,BOUND=3;
 localparam OFF=0,LOAD=1,CHECK=2,ALIGN=3,SCAN=4;
 reg[1:0]life;reg[2:0]scan_mode;
 wire[3:0]root_level;wire[4:0]bound_xlo,bound_xhi;wire[3:0]bound_ylo,bound_yhi;
 reg[4:0]w_pc;reg[2:0]tau;
 reg[1:0]w_edge,s_edge;reg[3:0]prep_level,level;reg[8:0]nx,ny;
 reg walk_done,found_full;reg[1:0]w_pass;
 reg w_epoch;reg row_first;
 reg[2:0]tile_head,tile_tail;
 reg[22:0]tile_meta0,tile_meta1;
 reg scan_slot,scan_full,scan_qvalid,scan_inflight,anchor_seen;reg[2:0]check_mask;
 reg scan_bank,prefetch_active,scan_classify_pending;wire prefetch_handoff;
 reg s_valid;reg[2:0]s_kind;reg[1:0]s_return_edge;reg s_sub;
 localparam S_LOAD=0,S_ROW=1,S_SCAN=2,S_PREFETCH=3,S_CHECK=4;
 reg[8:0]origin_x,sx,sy,end_x,end_y;reg[1:0]lane;reg cover_acc;reg[3:0]cover_mask;
 wire[2:0]tile_count=tile_tail-tile_head;
 wire tile_empty=tile_head==tile_tail,tile_full=tile_count==2;
 wire[22:0]head_meta=tile_head[0]?tile_meta1:tile_meta0;
 wire[9:0]side=10'd1<<level;
 // mailbox起点按2^level对齐；终点只需把低level位置1。
 wire[9:0]tile_mask;genvar tm;
 generate for(tm=0;tm<10;tm=tm+1)begin:tile_extent
  assign tile_mask[tm]=head_meta[21:18]>tm;
 end endgenerate
 wire[9:0]tile_end_x={1'b0,head_meta[8:0]}|tile_mask;
 wire[9:0]tile_end_y={1'b0,head_meta[17:9]}|tile_mask;
 wire xbit=level<9?nx[level]:1'b0,ybit=level<9?ny[level]:1'b0;
 wire[8:0]return_side=9'd1<<level;
 reg[4:0]node_tile_mask;
 always @*begin
  case(level)9:node_tile_mask=31;8:node_tile_mask=15;7:node_tile_mask=7;6:node_tile_mask=3;5:node_tile_mask=1;default:node_tile_mask=0;endcase
 end
 wire outside=nx>=400||ny>=240||nx[8:4]>bound_xhi||{1'b0,ny[7:4]}>{1'b0,bound_yhi}||
  ((nx[8:4]|node_tile_mask)<bound_xlo)||((ny[8:4]|node_tile_mask)<{1'b0,bound_ylo});
 wire row_end=({1'b0,sx}+10'd2)>{1'b0,end_x};
 wire scan_end=row_end&&(({1'b0,sy}+10'd2)>{1'b0,end_y});
 wire check_finish=life==EXEC&&scan_mode==CHECK&&scan_inflight&&s_kind==S_CHECK&&phase==3;
 wire check_accept=&check_mask;
 // Partial tiles are aligned 4x4: sy[1] marks their final quad row.
 // Stored successor predicates can retire a rejected tail without another
 // four-edge CHECK. No scan ALU result bypass enters this decision.
 wire reject_tail=phase==3&&!scan_full&&
  ((scan_classify_pending&&!check_accept&&scan_end)||
   (check_finish&&(scan_end||(!check_accept&&!row_end&&sy[1]))));
 wire scanner_finishing=scan_mode==OFF||reject_tail;
 function pass_h;input[29:0]h;input t;begin pass_h=!h[29]&&(!t||(|h));end endfunction

 wire packet_take=start&&!reset;
 wire input_take=geo_we&&!reset;
 wire import_we=input_take;


 // Walker独占ROM、16×30工作池、同步step B口和30位ALU。
 reg rom_valid;reg[4:0]rom_pc;wire[35:0]uop;
 reg rom_re;reg[4:0]rom_ra;
 wire[8:0]bbox_origin_x,bbox_origin_y;
 rc_raster_program program_store(clk,reset,ce,rom_re,{2'd0,rom_ra},uop);
 wire rom_match=rom_valid&&rom_pc==w_pc;
 wire start_tile=life==EXEC&&(scanner_finishing||prefetch_handoff)&&!tile_empty;
 // Base rows and scanner row conversions precede one level-major loop:
 // X, Y, lower support, upper support. Bounds cap every completed level.
 // Only origin restoration depends on bounds; its independent DSP is unchanged.
 wire early_prep_pc=w_pc!=14;
 wire walk_allowed=(life==EXEC||(life==BOUND&&early_prep_pc))&&!walk_done&&
  !(bbox_done&&bbox_empty)&&(INTERLEAVE||scan_mode==OFF&&!start_tile);
 wire origin_op=uop[30:27]==8;
 wire[4:0]origin_next_pc=5'd10;
 wire fast_origin=origin_op&&nx==0&&ny==0;
 wire enqueue_block=uop[30:27]==6&&w_edge==0&&tile_full;
 wire fast_w=walk_allowed&&rom_match&&w_edge==0&&
  ((uop[30:27]==7&&level==root_level)||(uop[30:27]==4&&outside)||fast_origin);
 wire origin_issue,origin_finish;reg[1:0]origin_valid;
 wire micro_issue=walk_allowed&&rom_match&&w_edge<3&&!enqueue_block&&!fast_w&&
  (origin_op||!(origin_issue||(|origin_valid))||origin_finish);
 reg[5:0]micro_step;reg micro_sub;
 always @*begin
  micro_step=uop[11:6];micro_sub=uop[21:20]==1;
  case(uop[26:25])
   1:micro_step=uop[11:6]+level-1'b1;
   2:begin micro_step=(xbit==ybit?6'd32:6'd48)+level;micro_sub=ybit;end
   3:micro_step=uop[11:6]+prep_level-1'b1;
   default:begin end
  endcase
  if(origin_op)micro_step=6'd62;
 end
 wire[5:0]micro_tw=uop[17:12]+(uop[24]?prep_level:4'd0);
 wire[3:0]w_ra={uop[1:0],w_edge};
 wire[7:0]w_step_ra={w_edge,micro_step};
 wire[29:0]work_q,step_q;wire[35:0]packed_step_q;
 assign step_q=packed_step_q[29:0];
 reg r_valid,r_sub;reg[1:0]r_edge;reg r_tile,r_tile_slot;
 reg[7:0]r_tw;reg[35:0]r_uop;reg[4:0]r_pc;reg r_epoch;
 wire[1:0]r_dst=r_uop[4:3];
 // Fixed format controls are decoded before the wide operands. In particular,
 // step doubling consumes the step Q and does not pass through a work-Q mux.
 wire double_step=r_uop[30:27]==14;
 wire zero_work=r_uop[30:27]==12||r_uop[30:27]==13;
 wire work_live=!zero_work&&!double_step&&
  !(r_uop[19:18]==2&&!work_q[29])&&!(r_uop[19:18]==3&&work_q[29]);
 wire step_live=r_uop[30:27]!=12&&
  !(r_uop[21:20]==2&&!step_q[29])&&!(r_uop[21:20]==3&&step_q[29]);
 wire[29:0]formatted_work=r_uop[19:18]==1?{work_q[28:0],1'b0}:work_q;
 wire[29:0]fmt_a=(formatted_work&{30{work_live}})|(step_q&{30{double_step}});
 wire[29:0]fmt_b=step_q&{30{step_live}};
 // The threshold operation produces tau-support-1 for a strict signed compare.
 // Its carry is the edge's top-left bit, rather than the normal subtract carry.
 wire threshold_return=r_uop[30:27]==13;
 wire alu_carry=threshold_return?tau[r_edge]:r_sub;
 wire[29:0]alu=fmt_a+(fmt_b^{30{r_sub}})+alu_carry;
 wire cancelled=r_valid&&r_epoch!=w_epoch;
 wire r_origin=r_uop[30:27]==8;
 reg[1:0]origin_edge0,origin_edge1;
 assign origin_issue=r_valid&&r_origin&&!cancelled;
 wire origin_retire=origin_valid[1];
 assign origin_finish=origin_retire&&origin_edge1==2;
 wire[53:0]origin_result;
 // One packed Nx/Ny read and one old-h read feed a fused dual-product DSP.
 // The import writer uses the two existing 18-bit step lanes independently.
 rp_origin_mac origin_mac(clk,reset,ce,packed_step_q[17:0],packed_step_q[35:18],
  {8'd0,nx,1'b0},{8'd0,ny,1'b0},{{24{work_q[29]}},work_q},origin_result);
 wire w_retire=r_valid&&!cancelled&&!r_origin;
 wire w_cond_retire=w_retire&&(r_uop[30:27]==4||r_uop[30:27]==5);
 wire cond_pass=pass_h(alu,tau[r_edge]);
 wire w_early=EARLY_OUT&&w_cond_retire&&!cond_pass;
 wire w_decide=w_cond_retire&&(r_edge==2||w_early);
 wire w_last=w_retire&&r_edge==2;
 wire w_static_tail=micro_issue&&w_edge==2&&uop[30:27]!=4&&uop[30:27]!=5;
 wire[1:0]pass_next={w_pass[0],cond_pass};
 wire all_pass=w_pass==3&&cond_pass;
 wire rf_we=import_we||origin_retire||w_retire&&r_uop[22]&&!r_tile;
 wire[3:0]rf_wa=import_we?geo_wa:origin_retire?{2'd2,origin_edge1}:{r_dst,r_edge};
 wire ordinary_rf_write=w_retire&&r_uop[22]&&!r_tile;
 wire[29:0]rf_wd=(geo_wd&{30{import_we}})|(origin_result[29:0]&{30{origin_retire}})|
  (alu&{30{ordinary_rf_write}});
 wire table_we=w_retire&&r_uop[23];
 wire[4:0]bbox_difference=(bbox_xlo^bbox_xhi)|{1'b0,(bbox_ylo^bbox_yhi)};
 wire[3:0]bbox_root=bbox_difference[4]?4'd9:bbox_difference[3]?4'd8:bbox_difference[2]?4'd7:bbox_difference[1]?4'd6:bbox_difference[0]?4'd5:4'd4;
 assign root_level=bbox_root;assign bound_xlo=bbox_xlo;assign bound_xhi=bbox_xhi;assign bound_ylo=bbox_ylo;assign bound_yhi=bbox_yhi;
 reg[4:0]root_keep;
 always @*begin case(bbox_root)4:root_keep=31;5:root_keep=30;6:root_keep=28;7:root_keep=24;8:root_keep=16;default:root_keep=0;endcase end
 assign bbox_origin_x={(bbox_xlo&root_keep),4'd0};assign bbox_origin_y={({1'b0,bbox_ylo}&root_keep),4'd0};
 rp_raster_rf #(.AB(4)) work_store(clk,reset,ce,micro_issue,rf_we,
  w_ra,rf_wa,rf_wd,work_q);

 // 两槽tile邮箱只存六个h；walker写、scanner独立同步读，不复制工作池。
 wire load_issue=life==EXEC&&scan_mode==LOAD&&s_edge<3;
 // Once the final quad is admitted, phi2/phi3 of lanes 0..2 have a free
 // mailbox read / scan-store write slot. The next mailbox remains owned until
 // the handoff; missing that admission window takes the ordinary LOAD path.
 wire prefetch_issue=life==EXEC&&scan_mode==SCAN&&scan_inflight&&phase==2&&lane<3&&prefetch_active;
 wire mailbox_we=w_retire&&r_tile;
 wire[29:0]mailbox_q;
 rp_raster_rf #(.AB(3)) tile_store(clk,reset,ce,load_issue||prefetch_issue,mailbox_we,
  prefetch_issue?{tile_head[0],lane}:{scan_slot,s_edge},{r_tile_slot,r_edge},alu,mailbox_q);

 // Partial tiles are exactly 4x4. Two current vectors alternate only when a
 // rejected CHECK commits its already computed next-quad/next-row position.
 // The full-tile path retains its row-start vector and original sample calendar.
 wire checking=!scan_full&&(scan_mode==CHECK||scan_mode==ALIGN);
 wire[1:0]current_vec=scan_bank?2'd2:2'd1;
 wire[1:0]future_vec=scan_bank?2'd1:2'd2;
 wire[1:0]prefetch_vec=head_meta[22]?2'd0:scan_full?2'd2:future_vec;
 wire align_accept;
 // A stored vector carries its quad predicate. CHECK advances only a vector
 // already known to fail; its writeback also classifies the successor.
 wire check_advance=check_finish&&!reject_tail;
 wire fetch_check=(checking||scan_classify_pending)&&
  !(phase==3&&(scan_mode==ALIGN?align_accept:check_accept));
 wire fetch_row_end=check_advance?!row_end:row_end;
 // Scanner固定计数日历，不读取ROM；ROW/SCAN只占本地8×30池。
 assign full_scan=scan_full;
 assign quad_valid=life==EXEC&&scan_mode==SCAN&&lane==0&&phase==0&&scan_qvalid&&run_enable&&!reset;
 assign quad_take=quad_valid&&quad_ready;
 wire scan_start=life==EXEC&&scan_mode==SCAN&&phase==0&&scan_qvalid&&(lane!=0||quad_take);
 wire prefetch_start=scan_start&&lane==0&&scan_end&&!tile_empty;
 wire check_start=life==EXEC&&scan_mode==CHECK&&phase==0&&scan_qvalid;
 assign sample_valid=scan_start&&!reset;
 assign sample_lane=lane;
 assign quad_x=sx[8:1];assign quad_y=sy[7:1];
 wire scan_fetch0=life==EXEC&&(scan_mode==SCAN||scan_mode==ALIGN||scan_mode==CHECK)&&phase==3&&!reject_tail;
 wire scan_fetch1=scan_start||check_start;
 wire scan_fetch2=life==EXEC&&(scan_mode==SCAN||scan_mode==CHECK)&&scan_inflight&&phase==1;
 wire scan_read=scan_fetch0||scan_fetch1||scan_fetch2;
 // 地址由phase直接给出：phi0/1读edge1/2，phi2读ROW[lane]，phi3读edge0。
 wire[1:0]scan_read_edge=phase==2?lane:phase+2'd1;
 // 行末quad前三lane的phi2取ROW，phi3在原ALU空槽写回。
 wire row_issue=life==EXEC&&scan_full&&scan_mode==SCAN&&scan_inflight&&phase==2&&row_end&&!scan_end&&lane!=3;
 wire scan_rf_re=scan_read||row_issue;
 wire[1:0]s_read_edge=scan_read_edge;
 wire[1:0]s_read_vec=scan_full?{1'b0,(phase!=2&&!(row_first&&lane==0))}:
  check_advance?future_vec:current_vec;
 wire[5:0]s_step=fetch_check?(fetch_row_end?6'd58:6'd33):
  (!scan_full&&row_end&&lane==3)?6'd59:phase==2?6'd49:(lane[0]?6'd57:6'd32);
 wire[29:0]scan_work_q,scan_step_q;
 wire s_scan_retire=s_valid&&s_kind==S_SCAN&&(s_return_edge!=0||scan_start);
 wire s_check_retire=s_valid&&s_kind==S_CHECK&&(s_return_edge!=0||check_start);
 wire s_row_retire=s_valid&&s_kind==S_ROW;
 wire s_load_retire=s_valid&&s_kind==S_LOAD;
 wire s_prefetch_retire=s_valid&&s_kind==S_PREFETCH;
 wire[29:0]scan_alu=scan_work_q+(scan_step_q^{30{s_sub}})+s_sub;
 // |Nx|,|Ny|<=131071, so support<=524284 and tau-support-1 fits S20.
 // Negative h outside S20 cannot pass. Nonnegative h passes except the
 // h=threshold=0 case; only the bounded negative tail needs a comparator.
 wire[19:0]check_threshold_q;
 wire threshold_read=scan_rf_re||load_issue||prefetch_issue;
 wire[1:0]threshold_edge=load_issue?s_edge:prefetch_issue?lane:s_read_edge;
 rp_raster_rf #(.AB(2),.WIDTH(20)) check_thresholds(clk,reset,ce,threshold_read,
  w_retire&&threshold_return,threshold_edge,r_edge,alu[19:0],check_threshold_q);
 wire scan_we=s_scan_retire||s_check_retire||s_row_retire||s_load_retire||s_prefetch_retire;
 wire[1:0]scan_write_vec=s_kind==S_PREFETCH?prefetch_vec:s_kind==S_CHECK?future_vec:
  s_kind==S_LOAD?(scan_full?2'd0:2'd1):s_kind==S_ROW?2'd0:scan_full?2'd1:current_vec;
 wire[3:0]scan_wa={scan_write_vec,s_return_edge};
 wire[29:0]scan_wd=(s_kind==S_LOAD||s_kind==S_PREFETCH)?mailbox_q:scan_alu;
 wire next_quad_pass=(!scan_wd[29]&&((|scan_wd[28:0])||check_threshold_q[19]))||
  (scan_wd[29]&&(&scan_wd[28:19])&&check_threshold_q[19]&&scan_wd[18:0]>check_threshold_q[18:0]);
 wire predicate_write=s_load_retire||s_prefetch_retire||s_check_retire||
  (s_scan_retire&&!scan_full&&lane==3&&!scan_end);
 assign align_accept=check_mask[0]&&check_mask[1]&&
  ((s_load_retire&&s_return_edge==2)?next_quad_pass:check_mask[2]);
 rp_raster_rf #(.AB(4)) scan_store(clk,reset,ce,scan_rf_re,scan_we,
  {s_read_vec,s_read_edge},scan_wa,scan_wd,scan_work_q);
 rp_raster_steps step_store(clk,reset,ce,micro_issue,table_we,w_step_ra,r_tw,alu,
  input_take&&!geo_wa[3],geo_wa[2],geo_wa[1:0],geo_wd[18:1],packed_step_q,
  scan_rf_re,{s_read_edge,s_step},scan_step_q);
 assign sample_h=scan_work_q;
 wire scan_cover_valid=s_scan_retire&&s_return_edge==2&&scan_inflight&&phase==2;
 assign prefetch_handoff=prefetch_active&&scan_cover_valid&&lane==3&&scan_end;
 assign sample_cover=scan_full||(cover_acc&&pass_h(scan_work_q,tau[2]));
 assign sample_anchor=sample_cover&&!anchor_seen;
 assign quad_end_valid=scan_cover_valid&&lane==3&&!reset;
 assign quad_mask=cover_mask|((sample_cover?4'b0001:4'd0)<<lane);

 wire[3:0]prep_last=life==BOUND?4'd8:root_level-1'b1;
 reg[4:0]issue_next_pc;
 always @*begin
  issue_next_pc=uop[35:31];
  case(uop[30:27])
   3:if(prep_level<prep_last)issue_next_pc=2;
   // Once bounds arrive, all lower levels are already complete. An in-flight
   // unneeded high row may retire, then the remaining rows can be omitted.
   2,14:if(w_pc!=17&&life==EXEC&&prep_level>prep_last)issue_next_pc=14;
   7:if(!xbit&&ybit)issue_next_pc=13;
   default:begin end
  endcase
 end
 // The retiring predicate chooses the ROM read directly. Its speculative
 // successor still carries the old epoch and is discarded on the next edge.
 wire branch_all_pass=r_edge==2&&all_pass;
 wire terminal_tile=(branch_all_pass&&level<=4)||level<=2;
 // Full nodes above the framebuffer locality unit must descend regardless
 // of their lower-bound result. The upper test is sufficient at those levels.
 wire upper_split=branch_all_pass&&level>4;
 // Branch destinations 10..13 share upper bits. Within a predicate return,
 // opcode bit0 distinguishes upper (4) from lower (5); the two address bits
 // also encode publish/reject versus split/full-test without a 5-bit mux.
 wire branch_finish=r_uop[27]?terminal_tile:!branch_all_pass;
 wire branch_low=!r_uop[27]&&!upper_split;
 wire branch_split=!branch_finish&&!branch_low;
 wire[4:0]branch_target={2'b01,branch_finish,!branch_finish,branch_low};
 always @*begin
  rom_re=0;rom_ra=w_pc;
  if((life==EXEC||life==BOUND&&early_prep_pc)&&!rom_match)rom_re=1;
  if(w_static_tail)begin rom_re=1;rom_ra=issue_next_pc;end
  if(w_decide)begin rom_re=1;rom_ra=branch_target;end
  if(fast_w)begin
   if(origin_op)begin if(origin_next_pc!=w_pc)begin rom_re=1;rom_ra=origin_next_pc;end end
   else begin rom_re=1;rom_ra=13;end
  end
  if(life==IMPORT&&input_take&&geo_wa==4'ha)begin rom_re=1;rom_ra=0;end
 end
 wire evt_walk_issue=micro_issue;
 wire evt_walk_retire=w_retire||origin_retire||fast_w;
 wire evt_branch_wait=life==EXEC&&!walk_done&&(!rom_match||w_edge==3)&&!evt_walk_issue&&!evt_walk_retire;
 wire evt_tile_wait=life==EXEC&&walk_allowed&&rom_match&&enqueue_block;
 wire evt_early_out=w_early;
 wire evt_walk_discard=cancelled;
 assign evt_import=life==IMPORT;
 wire evt_early_prep=(micro_issue&&early_prep_pc)||
  (w_retire&&(r_pc<6||r_pc==17||r_pc==18));
 wire evt_origin=life==EXEC&&((!walk_done&&origin_op&&rom_match)||origin_issue||origin_retire);
 wire evt_support=(micro_issue&&w_pc>=6&&w_pc<=9)||(w_retire&&r_pc>=6&&r_pc<=9);
 assign evt_prep=life==BOUND||(life==EXEC&&((!walk_done&&(w_pc<10||w_pc>=14))||(r_valid&&(r_pc<10||r_pc>=14))));
 assign evt_traverse=life==EXEC&&((evt_walk_issue&&w_pc>=10&&w_pc<=13)||(w_retire&&r_pc>=10&&r_pc<=13)||(fast_w&&!origin_op));
 assign evt_node=evt_walk_issue&&w_pc==10&&w_edge==0||fast_w&&uop[30:27]==4;
 assign evt_row=load_issue||row_issue||s_load_retire||s_row_retire||prefetch_issue||s_prefetch_retire;
 assign evt_align=life==EXEC&&(scan_mode==ALIGN||scan_mode==SCAN&&lane==0&&!scan_inflight&&!scan_start&&run_enable&&quad_ready);
 assign evt_credit_wait=life==EXEC&&scan_mode==SCAN&&lane==0&&!scan_inflight&&!scan_start&&(!run_enable||!quad_ready);
 wire evt_check_quad=check_start;
 assign evt_leaf=scan_start||check_start||scan_inflight;
 assign evt_empty_quad=quad_end_valid&&quad_mask==0;

 always @(posedge clk)begin
  if(reset)begin
   life<=IDLE;busy<=0;done<=0;error<=0;tau<=0;
   w_pc<=0;w_edge<=0;s_edge<=0;prep_level<=0;level<=9;nx<=0;ny<=0;walk_done<=0;found_full<=0;w_pass<=0;
   w_epoch<=0;row_first<=0;
   tile_head<=0;tile_tail<=0;tile_meta0<=0;tile_meta1<=0;scan_slot<=0;
   scan_mode<=OFF;scan_full<=0;scan_qvalid<=0;scan_inflight<=0;anchor_seen<=0;scan_bank<=0;prefetch_active<=0;scan_classify_pending<=0;
   check_mask<=0;origin_x<=0;sx<=0;sy<=0;end_x<=0;end_y<=0;lane<=0;cover_acc<=0;cover_mask<=0;
   rom_valid<=0;rom_pc<=0;r_valid<=0;r_sub<=0;r_edge<=0;r_tile<=0;r_tile_slot<=0;r_tw<=0;r_uop<=0;r_pc<=0;r_epoch<=0;
   s_valid<=0;s_kind<=0;s_return_edge<=0;s_sub<=0;
   origin_valid<=0;origin_edge0<=0;origin_edge1<=0;
  end else if(ce)begin
   done<=0;
   origin_valid<={origin_valid[0],origin_issue};
   origin_edge1<=origin_edge0;if(origin_issue)origin_edge0<=r_edge;
   scan_classify_pending<=scan_cover_valid&&lane==3&&!scan_full&&!scan_end;
   if(rom_re)begin rom_valid<=1;rom_pc<=rom_ra;end
   r_valid<=micro_issue;
   if(micro_issue)begin
    r_edge<=w_edge;r_sub<=micro_sub;r_tile<=uop[5:3]==6;
    // Metadata is published at the first return. Hold the physical slot for
    // all three vector words while the producer tail has already advanced.
    if(w_edge==0)r_tile_slot<=tile_tail[0];
    r_tw<={w_edge,micro_tw};r_uop<=uop;r_pc<=rom_pc;r_epoch<=w_epoch;
   end
   s_valid<=load_issue||scan_rf_re||prefetch_issue;
   if(load_issue||scan_rf_re||prefetch_issue)begin
    s_kind<=load_issue?S_LOAD:prefetch_issue?S_PREFETCH:fetch_check?S_CHECK:phase==2?S_ROW:S_SCAN;
    s_return_edge<=load_issue?s_edge:prefetch_issue?lane:scan_read_edge;
    s_sub<=!fetch_check&&phase!=2&&lane==3;
   end
   if(packet_take)begin
    life<=IMPORT;busy<=1;error<=0;
    walk_done<=0;w_pc<=0;w_edge<=0;prep_level<=0;tau<=0;
    w_epoch<=0;row_first<=0;
    tile_head<=0;tile_tail<=0;scan_mode<=OFF;scan_inflight<=0;scan_qvalid<=0;prefetch_active<=0;scan_classify_pending<=0;
   end
   if(input_take&&geo_wa==4'ha)begin life<=BOUND;w_pc<=0;w_edge<=0;end
   if(micro_issue)w_edge<=w_edge+1'b1;
   if(w_retire)begin
    if(w_cond_retire)begin if(r_edge==0)w_pass<={1'b0,cond_pass};else w_pass<=pass_next;end
    if(r_uop[19:18]==2&&r_uop[23])tau[r_edge]<=!((!work_q[29]&&(|work_q))||(work_q==0&&!step_q[29]&&(|step_q)));
   end
   if(w_retire&&r_tile&&r_edge==0)begin
    if(tile_full)error[1]<=1;
    if(tile_tail[0])tile_meta1<={found_full,level,ny,nx};else tile_meta0<={found_full,level,ny,nx};
    tile_tail<=tile_tail+1'b1;
   end
   if(w_static_tail)begin
    w_edge<=0;w_pc<=issue_next_pc;
    case(uop[30:27])
     1:prep_level<=1;
     3:begin if(prep_level<prep_last)prep_level<=prep_level+1'b1;
       end
     7:begin
      // 加减方向正好由被翻转位决定，不可能发生进位/借位。
      if(xbit==ybit)nx<=nx^return_side;
      else ny<=ny^return_side;
      if(!xbit&&ybit)level<=level+1'b1;
     end
    endcase
   end
   if(w_decide)begin
    w_epoch<=!w_epoch;w_pass<=0;w_pc<=branch_target;w_edge<=0;
    if(r_uop[27]&&branch_finish)found_full<=branch_all_pass;
    if(branch_split)level<=level-1'b1;
   end
   if(fast_w)begin
    w_edge<=0;
    if(origin_op)begin
     w_pc<=origin_next_pc;
    end else if(uop[30:27]==7)walk_done<=1;else w_pc<=13;
   end
   if(bbox_done&&life==BOUND)begin
    life<=EXEC;
    if(bbox_empty)walk_done<=1;
    else begin nx<=bbox_origin_x;ny<=bbox_origin_y;level<=root_level;end
   end
   if(start_tile)begin
    scan_slot<=tile_head[0];scan_full<=head_meta[22];origin_x<=head_meta[8:0];sx<=head_meta[8:0];sy<=head_meta[17:9];
    end_x<=tile_end_x>399?9'd399:tile_end_x[8:0];end_y<=tile_end_y>239?9'd239:tile_end_y[8:0];
    scan_mode<=prefetch_handoff?ALIGN:LOAD;s_edge<=0;lane<=0;scan_qvalid<=0;anchor_seen<=0;cover_mask<=0;
    scan_bank<=prefetch_handoff&&prefetch_vec[1];prefetch_active<=0;
    if(prefetch_handoff)row_first<=1;
   end
   if(prefetch_start)prefetch_active<=1;
   if(load_issue)begin
    s_edge<=s_edge+1'b1;
    if(s_edge==2)begin s_edge<=0;scan_mode<=ALIGN;lane<=0;scan_qvalid<=0;row_first<=1;end
   end
   if((s_load_retire&&s_return_edge==2)||prefetch_handoff)tile_head<=tile_head+1'b1;
   if(scan_fetch0)begin scan_qvalid<=1;if(scan_mode==ALIGN)begin if(scan_full||align_accept)scan_mode<=SCAN;else scan_mode<=CHECK;end end
   if(scan_classify_pending&&!check_accept&&!reject_tail)scan_mode<=CHECK;
   if(check_start)begin scan_qvalid<=0;scan_inflight<=1;end
   if(predicate_write)check_mask[s_return_edge]<=next_quad_pass;
   if(check_finish)begin
     if(reject_tail)begin if(!start_tile)scan_mode<=OFF;end
     else begin
      scan_bank<=!scan_bank;
      if(row_end)begin sx<=origin_x;sy<=sy+2'd2;end
      else sx<=sx+2'd2;
      if(check_accept)scan_mode<=SCAN;
     end
   end
   if(scan_start)begin
    scan_qvalid<=0;scan_inflight<=1;cover_acc<=pass_h(scan_work_q,tau[0]);
    if(lane==0)begin cover_mask<=0;anchor_seen<=0;end
   end
   if(scan_inflight&&phase==1)cover_acc<=cover_acc&&pass_h(scan_work_q,tau[1]);
   if(scan_cover_valid)begin
    if(lane==0)row_first<=0;
    cover_mask[lane]<=sample_cover;if(sample_cover)anchor_seen<=1;
    if(lane!=3)lane<=lane+1'b1;
   end
   if(scan_cover_valid&&lane==3)begin
    lane<=0;
    if(scan_end)begin if(!prefetch_handoff)scan_mode<=OFF;end
    else begin
     if(row_end)begin sx<=origin_x;sy<=sy+2'd2;row_first<=1;end
     else sx<=sx+2'd2;
    end
   end
   if(reject_tail&&!start_tile)scan_mode<=OFF;
   if(phase==3)scan_inflight<=0;
   if(life==EXEC&&walk_done&&tile_empty&&scanner_finishing&&!r_valid&&!s_valid)begin life<=IDLE;busy<=0;done<=1;end
   if(scan_read&&row_issue)error[2]<=1;
   if(tile_count>2)error[3]<=1;
  end
 end
`ifndef SYNTHESIS
 always @(posedge clk)if(ce&&!reset&&origin_retire&&(import_we||(w_retire&&r_uop[22]&&!r_tile)))
  $fatal(1,"origin and walker RF writers overlap");
 always @(posedge clk)if(ce&&!reset&&life==BOUND&&micro_issue&&!(w_pc<=9||w_pc==16||w_pc==17||w_pc==18))
  $fatal(1,"bbox-dependent R instruction issued before bounds");
 always @(posedge clk)if(ce&&!reset&&r_valid&&r_uop[30:27]==14)begin
  if(r_uop[21:20]!=0||r_sub)$fatal(1,"step doubling B source is not fixed by ROM");
 end
 reg[2:0]sim_mail_written0,sim_mail_written1;
 always @(posedge clk)begin
  if(reset)begin sim_mail_written0<=0;sim_mail_written1<=0;end
  else if(ce)begin
   if(packet_take)begin sim_mail_written0<=0;sim_mail_written1<=0;end
   if(mailbox_we)begin
    if(r_tile_slot)begin
     if(r_edge==0)sim_mail_written1<=3'b001;else sim_mail_written1[r_edge]<=1;
    end else begin
     if(r_edge==0)sim_mail_written0<=3'b001;else sim_mail_written0[r_edge]<=1;
    end
   end
   if(load_issue||prefetch_issue)begin
    if((prefetch_issue?tile_head[0]:scan_slot)?
     !sim_mail_written1[prefetch_issue?lane:s_edge]:!sim_mail_written0[prefetch_issue?lane:s_edge])
     $fatal(1,"mailbox read before its statically scheduled write");
   end
  end
 end
 integer sim_geo_count;
 always @(posedge clk)begin
  if(reset)sim_geo_count<=0;
  else if(ce)begin
   if(start)sim_geo_count<=0;
   if(geo_we)begin
    if(sim_geo_count>=9||geo_wa!==(sim_geo_count%3)*4+sim_geo_count/3)$fatal(1,"最终行几何写入次序错误");
    sim_geo_count<=sim_geo_count+1;
   end
  end
 end
 always @(posedge clk)if(ce&&!reset&&start&&life!=IDLE)$fatal(1,"T配置提交时R尚未空闲");
 always @(posedge clk)if(ce&&!reset&&geo_we&&life!=IMPORT)$fatal(1,"固定几何写入不在IMPORT阶段");
 always @(posedge clk)if(ce&&!reset&&w_retire&&threshold_return&&alu[29:19]!={11{alu[19]}})
  $fatal(1,"quad threshold exceeds proven S20 range");
 always @(posedge clk)if(ce&&!reset&&start_tile)begin
  if(head_meta[21:18]>9)$fatal(1,"tile层级非法");
  if(!head_meta[22]&&head_meta[21:18]!=2)$fatal(1,"partial tile must be 4x4");
  if(({1'b0,head_meta[8:0]}&tile_mask)!=0||({1'b0,head_meta[17:9]}&tile_mask)!=0)$fatal(1,"tile起点没有按层级对齐");
 end
`endif
endmodule

// Two enabled edges; no external C delay or intermediate X-origin vector.
module rp_origin_mac(input clk,reset,ce,input signed[17:0]nx,ny,input[17:0]x,y,
 input signed[53:0]h,output signed[53:0]result);
`ifdef PORTABLE
 reg signed[17:0]nr0,nr1;reg[17:0]cr0,cr1;reg signed[53:0]hr,rr;
 wire signed[18:0]ux={1'b0,cr0},uy={1'b0,cr1};
 always @(posedge clk)begin
  if(reset)begin nr0<=0;nr1<=0;cr0<=0;cr1<=0;hr<=0;rr<=0;end
  else if(ce)begin nr0<=nx;nr1<=ny;cr0<=x;cr1<=y;hr<=h;rr<=nr0*ux+nr1*uy+hr;end
 end
 assign result=rr;
`else
 MULTADDALU18X18 #(.A0REG(1),.A1REG(1),.B0REG(1),.B1REG(1),.CREG(1),
  .PIPE0_REG(0),.PIPE1_REG(0),.OUT_REG(1),.ASIGN0_REG(0),.ASIGN1_REG(0),
  .BSIGN0_REG(0),.BSIGN1_REG(0),.ACCLOAD_REG0(0),.ACCLOAD_REG1(0),
  .B_ADD_SUB(0),.C_ADD_SUB(0),.MULTADDALU18X18_MODE(0),.MULT_RESET_MODE("SYNC")) m(
  .DOUT(result),.CASO(),.SOA(),.SOB(),.A0(x),.A1(y),.B0(nx),.B1(ny),.C(h),
  .SIA(18'd0),.SIB(18'd0),.CASI(55'd0),.ACCLOAD(1'b0),.ASEL(2'b00),.BSEL(2'b00),
  .ASIGN(2'b00),.BSIGN(2'b11),.CLK(clk),.CE(ce),.RESET(reset));
`endif
endmodule
