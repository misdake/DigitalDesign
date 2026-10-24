// Original V/T arithmetic calendar; normals travel once as packed S12F10.
// 同一ROM、RF与双DSP，V/T分时；所有状态变更均采样旧值。
module shared_frontend #(parameter RESULT_COMPACT=0,parameter SP_COMPACT=0,parameter SOURCE_READ=0,parameter BOUNDS_CULL=0,
 parameter TRIANGLE_SLOT_BITS=4)(
 input clk,reset,ce,start_v,start_t,meshlet_reset,
 input [6:0] vertex_count,input [5:0] uniform_base,input [8:0] config_base,
 input [9:0] vertex_base,input [35:0] triangle_descriptor,input cull_negative,
 input dma_we,input [9:0] dma_addr,input [63:0] dma_data,input output_ready,
 input packet_ready,word_ready,
 output reg busy,done_v,done_t,output reg [2:0] error,
 output reg [6:0] published_vertices,
 output wire packet_valid,output wire [35:0] descriptor,
 output wire word_valid,output reg [4:0] word_index,output reg [53:0] word_data,
 output wire dma_ready,output wire emit_valid,output wire [9:0] emit_addr,output wire [35:0] emit_data,
 input source_re,input [9:0] source_addr,output [63:0] source_data,
 output source_ready,triangle_ready,meshlet_ready,drained
);
 reg [8:0] pc;reg [6:0] remaining;reg [5:0] ub;reg [8:0] cb;
 reg [10:0] vp;reg [9:0] op;reg [5:0] vid;reg [2:0] vfield;reg triangle_mode;
 reg [17:0] refs;reg [3:0] scale;reg cull;reg [35:0] building_desc;
 reg sigma,delta_zero,delta_negative,collecting;reg any_front;
 wire bounds_run=BOUNDS_CULL&&pc[8];reg bounds_safe;
 localparam TRIANGLE_SLOTS=1<<TRIANGLE_SLOT_BITS;
 reg [TRIANGLE_SLOT_BITS:0] produced,consumed;
 wire[TRIANGLE_SLOT_BITS-1:0]write_slot=produced[TRIANGLE_SLOT_BITS-1:0];
 wire[TRIANGLE_SLOT_BITS-1:0]read_slot=consumed[TRIANGLE_SLOT_BITS-1:0];
 wire[TRIANGLE_SLOT_BITS:0]count=produced-consumed;
 wire queue_empty=produced==consumed;
 wire queue_full=(produced[TRIANGLE_SLOT_BITS]!=consumed[TRIANGLE_SLOT_BITS])&&write_slot==read_slot;
 reg vertex_epoch;wire export_epoch;
 // At most two consecutive meshlets own attributes. FIFO order makes the
 // alternate epoch free once the head reaches this epoch, or the FIFO empties.
 assign meshlet_ready=!busy&&!reset&&(queue_empty||export_epoch==vertex_epoch);
 // 位置与导出数据分bank：两个独立同步Q，导出背压不会保持位置口。
 localparam Q_EMPTY=1'b0,Q_DATA=1'b1;
 reg result_owner;reg core_qvalid;reg [4:0] response_index,fetch_index;
 reg export_valid;reg [1:0] import_state;
 wire [35:0] pq,export_q;wire [17:0] export_hi;
 wire import_active=import_state!=0;
 assign packet_valid=!queue_empty && import_state==0 && !reset;
 assign word_valid=export_valid && !reset;
 wire packet_take=ce&&packet_valid&&packet_ready;
 wire word_take=ce&&word_valid&&word_ready;
 wire release_slot=word_take&&word_index==15;
 wire refs_valid=triangle_descriptor[5:0]<published_vertices && triangle_descriptor[11:6]<published_vertices && triangle_descriptor[17:12]<published_vertices;
 wire [10:0] vertex_last_word={vertex_base,1'b0}+{4'd0,vertex_count}+{3'd0,vertex_count,1'b0};
 wire compact_invalid=SP_COMPACT&&(uniform_base[5]||config_base[8]||vertex_base[9]||vertex_last_word>1024);
 wire launch_v=ce&&start_v&&!busy&&!reset;
 wire launch_t=ce&&start_t&&!busy&&!start_v&&!queue_full&&!reset;
 wire launch=launch_v||(launch_t&&refs_valid);
 assign source_ready=SOURCE_READ&&!busy&&!reset&&!start_v&&!start_t&&(!SP_COMPACT||!source_addr[9]);
 assign triangle_ready=!busy&&!reset&&!queue_full;
 assign drained=!busy&&queue_empty&&!import_active&&!reset;
 wire source_take=ce&&source_re&&source_ready;
 reg [53:0]acc;wire [35:0]lq,rq;
 wire [71:0] control;
 `include "control_fields.vh"
 wire vertex_emit=emit!=0&&!(vfield==3&&emit==1);
 wire core_base=ce&&busy&&!reset&&(!vertex_emit||triangle_mode||output_ready);
 wire core_uses_q=triangle_mode&&up_push==4;
 wire output_room=!export_valid||word_ready;
 wire q_can_retire=result_owner==Q_EMPTY||
   (result_owner==Q_DATA&&output_room);
 wire step=core_base&&(!core_uses_q||core_qvalid);
 wire core_read=step&&pos_re;
 wire export_read=ce&&!reset&&(import_state==1||packet_take)&&q_can_retire;
  assign dma_ready=!reset&&!(SP_COMPACT&&dma_addr[9]);
 wire publish=step&&triangle_mode&&flow==6;
 rp_triangle_descriptors #(.AB(TRIANGLE_SLOT_BITS)) descriptors(.clk(clk),.we(ce&&!reset&&publish),
  .wa(write_slot),.wd({vertex_epoch,building_desc[35:23],sigma,building_desc[21:0]}),
  .ra(read_slot),.q({export_epoch,descriptor}));
 reg [8:0] next_pc;
 always @*begin
 next_pc=pc+1'b1;
 case(flow)
  1,3,7:next_pc=target;
  2:if(remaining==1)next_pc=target;
  4:next_pc=pc;
  5:if(delta_negative&&!cull)next_pc=target;
  default:begin end
 endcase
 end
 shared_control_rom #(.SP_COMPACT(SP_COMPACT)) rom(.clk(clk),.reset(reset),.en(launch||step),.addr(launch?(launch_v?(BOUNDS_CULL?9'd256:9'd0):9'd128):next_pc),.q(control));
 wire [9:0] la=l_src==1?{ub,u_off[4:1]}:l_src==3?{cb,cfg_off[1]}:vp[10:1];
 wire [9:0] ra=(work_en||work_we)?{(SP_COMPACT?4'b0110:4'b1000),work_addr}:r_src==1?{ub,u_off[4:1]}:r_src==3?{cb,cfg_off[1]}:vp[10:1];
 
`ifdef GPU_V2_COMMAND_PORTS
 wire[35:0]source_rq;
 assign source_data={source_rq[31:0],lq[31:0]};
`else
 assign source_data={rq[31:0],lq[31:0]};
`endif
 wire [9:0] sp_la=source_take?source_addr:la;
 wire [9:0] sp_ra=source_take?source_addr:ra;
 // SDP的工作写地址独立；T普通指令复用target低六位，不改变V跳转语义。
 wire [9:0] work_wa=(SP_COMPACT&&triangle_mode)?{4'b0110,target[5:0]}:ra;
 rs_static_scratchpad #(.COMPACT(SP_COMPACT)) sp(
`ifdef GPU_V2_COMMAND_PORTS
 .source_rq(source_rq),
`endif
 .clk(clk),.reset(reset),.dma_we(dma_we&&dma_ready),.dma_addr(dma_addr),.dma_data(dma_data),
 .le(source_take||(step&&l_src!=0)),.re(source_take||(step&&(r_src!=0||work_en))),.work_we((step&&work_we)),.la(sp_la),.ra(sp_ra),.work_wa(work_wa),.work_data(emit_data),.lq(lq),.rq(rq));
 wire [35:0] left=swap?rq:lq;wire [35:0] right=(swap&&!triangle_mode)?lq:rq;
 reg [31:0] origin_x,origin_y,origin_z;reg [4:0] quant_shift;reg normal18;
 reg [31:0] unpacked;reg [2:0] unpack_kind;reg [1:0] phase;reg [31:0] origin;
 wire [9:0] component=unpacked[9:0];
 always @*case(phase)0:origin=origin_x;1:origin=origin_y;default:origin=origin_z;endcase
 wire [31:0] position=origin+({22'd0,component}<<quant_shift);
 wire [31:0] normal={{15{component[9]}},component,7'd0};
 // T只需signed18的有效值；高位越界由原始P的符号前缀直接判断。
 wire signed[17:0]scaled18=$signed(unpacked)>>>scale;
 wire signed[31:0]scaled={{14{scaled18[17]}},scaled18};
 wire[15:0]positive_over,negative_over;genvar pr;
 generate for(pr=0;pr<14;pr=pr+1)begin:range_prefix
  assign positive_over[pr]=|unpacked[30:17+pr];
  assign negative_over[pr]=!(&unpacked[30:17+pr]);
 end endgenerate
 assign positive_over[15:14]=0;assign negative_over[15:14]=0;
 wire scale_over=unpacked[31]?negative_over[scale]:positive_over[scale];
 wire excluded_min=scaled18==18'h20000;
 wire [31:0] negated=-$signed(unpacked);
 reg [31:0] unp_value;
 always @*case(unpack_kind)1:unp_value=position;2:unp_value=normal;4:unp_value=scaled;5:unp_value=negated;default:unp_value=unpacked;endcase
 wire raw_front=!unpacked[31]&&(|unpacked[30:0]);
 wire z_write=step&&triangle_mode&&rf_wsrc==1&&unpack_kind==4&&(rf_dst==3||rf_dst==6||rf_dst==9);
 wire reject_behind=z_write&&rf_dst==3&&!any_front&&!raw_front;
 wire p_range_error=unpack_kind==4&&(scale_over||excluded_min);
 wire raw_bound_front=!acc[47]&&(|acc[46:16]);
 wire bound_left=acc[53]&&!(&acc[52:32]);
 wire bound_right=!acc[53]&&((|acc[52:33])||(acc[32]&&(|acc[31:0])));
 wire [31:0] acc_wb=acc[47:16];reg [31:0] constant_value;
 always @*case(cfg_idx)0:constant_value=-32'sd399;1:constant_value=-32'sd239;default:constant_value=32'd512;endcase
 wire [31:0] rf_write=rf_wsrc==1?unp_value:rf_wsrc==3?constant_value:acc_wb;
 wire [31:0] mac_value,pack_value;
 shared_rf rf(.clk(clk),.we((step&&rf_wsrc!=0)),.wa(rf_dst),.wd(rf_write),.aa(rf_mac),.ba(rf_pack),.aq(mac_value),.bq(pack_value));
 wire acc_clear=acc_op==1;
 wire [53:0] acc_input=acc_clear?54'd0:acc;wire [53:0] acc_next;
 wire[35:0]mac_right=right|{{19{control[70]}},17'd0};
 shared_mac mac(.clk(clk),.reset(reset),.en(ce&&step&&mul_en),.a(mac_value),.b(mac_right),.c(acc_input),.y(acc_next));
 wire [17:0] normal16_pack={{2{pack_value[17]}},pack_value[17:2]};
 wire [17:0] packed_normal=normal18?pack_value[17:0]:normal16_pack;
 // Preserve the original normal16 low-two-bit discard before S12F10 RNE.
 function [11:0] normal12;
  input [17:0] raw;input mode;
  reg [17:0] value;reg signed [12:0] rounded;
  begin
   if(mode)value=raw;else value={raw[15:0],2'b0};
   rounded=$signed({value[17],value[17:6]});
   if(value[5]&&((|value[4:0])||value[6]))rounded=rounded+13'sd1;
   if(rounded>13'sd2047)normal12=12'h7ff;
   else if(rounded< -13'sd2048)normal12=12'h800;
   else normal12=rounded[11:0];
  end
 endfunction
 wire [11:0] packed_normal12=normal12(packed_normal,normal18);
 reg [53:0] packed_buffer;
 assign emit_valid=busy&&vertex_emit&&!triangle_mode&&!reset;
 assign emit_addr=op;
 assign emit_data=packed_buffer[35:0];
 wire vwrite=step&&!triangle_mode&&vertex_emit;
 // The exported upper half is exactly {1,attribute,epoch,vertex}.
 // Position writes need no epoch and never pass through the export address mux.
 wire[8:0]coefficient_wa={trow,write_slot};
 wire[8:0]export_wa=triangle_mode?coefficient_wa:{1'b1,vfield[2],vertex_epoch,vid};
 // The descriptor head stays immutable through item15 acceptance. Read its
 // indexed vertex directly; no copied/rotating refs or component state exists.
 reg[5:0]attribute_ref;
 always @*case(fetch_index[3:1])
  5:attribute_ref=descriptor[5:0];
  6:attribute_ref=descriptor[11:6];
  default:attribute_ref=descriptor[17:12];
 endcase
 wire[8:0]export_addr=fetch_index<10 ?
  {fetch_index[3:0],read_slot} : {1'b1,fetch_index[0],export_epoch,attribute_ref};
 rp_results_split results(.clk(clk),.reset(reset),.ce(ce),
  .position_we(vwrite&&vfield<3),.export_we((vwrite&&vfield>=3)||(step&&twrite&&emit==1)),
  .position_wa({vfield[1:0],vid}),.export_wa(export_wa),.wd(emit_data),.position_re(core_read),.position_ra({1'b0,u_off[1:0],refs[5:0]}),
  .export_re(export_read),.export_ra(export_addr),.position_q(pq),.export_q(export_q));
 // Coefficient high words have the same item address and synchronous read edge.
 iz_ram18 coefficient_high(clk,reset,ce,step&&twrite&&emit==2,coefficient_wa,packed_buffer[53:36],
  export_read&&fetch_index<10,export_addr,export_hi);
 always @(posedge clk)begin
 if(reset)begin
 any_front<=0;busy<=0;done_v<=0;done_t<=0;bounds_safe<=0;
 error<=0;published_vertices<=0;pc<=0;remaining<=0;ub<=0;cb<=0;vp<=0;op<=0;vid<=0;vfield<=0;triangle_mode<=0;
 refs<=0;scale<=0;cull<=0;building_desc<=0;sigma<=0;delta_zero<=0;delta_negative<=0;collecting<=0;
 produced<=0;consumed<=0;vertex_epoch<=0;import_state<=0;word_index<=0;word_data<=0;
 result_owner<=Q_EMPTY;core_qvalid<=0;response_index<=0;fetch_index<=0;export_valid<=0;
 origin_x<=0;origin_y<=0;origin_z<=0;quant_shift<=0;normal18<=0;unpacked<=0;unpack_kind<=0;phase<=0;acc<=0;packed_buffer<=0;
 end else if(ce)begin
 if(SP_COMPACT&&dma_we&&dma_addr[9])error<=4;
 done_v<=0;done_t<=0;
 if(publish)produced<=produced+1'b1;
 if(packet_take)import_state<=1;
 if(word_take)begin
 export_valid<=0;
 if(word_index==15)begin import_state<=0;consumed<=consumed+1'b1;fetch_index<=0;end
 end
 if(!busy||!triangle_mode||(step&&core_uses_q))core_qvalid<=0;
 if(core_read)core_qvalid<=1;
 if(q_can_retire)result_owner<=Q_EMPTY;
 if(result_owner==Q_DATA&&output_room)begin
 word_data<=response_index<10 ? {export_hi,export_q} : {18'd0,export_q};
 word_index<=response_index;export_valid<=1;
 end
 // 位置Q与导出Q各自消费旧态并发起下一读，不存在跨消费者所有权。
 if(export_read)begin
 response_index<=fetch_index;
 result_owner<=Q_DATA;
 if(fetch_index==15)import_state<=2;else fetch_index<=fetch_index+1'b1;
 end
 // The meshlet controller only pulses this after admission and header checks.
 // Export can only free epochs during that interval; rechecking FIFO readiness
 // here would put its complete predicate into the count/epoch reset fanout.
 if(meshlet_reset)begin published_vertices<=0;error<=0;if(!BOUNDS_CULL)vertex_epoch<=!vertex_epoch;end
 if(launch_v)begin
 error<=0;triangle_mode<=0;
 if(compact_invalid)begin busy<=0;done_v<=1;error<=4;end
 else if(vertex_count+published_vertices>64)begin busy<=0;done_v<=1;error<=3;end
 else begin
 busy<=vertex_count!=0;done_v<=vertex_count==0;pc<=BOUNDS_CULL?9'd256:9'd0;bounds_safe<=1;remaining<=vertex_count;ub<=uniform_base;cb<=config_base;
 vp<={vertex_base,1'b0};op<=0;vid<=published_vertices[5:0];vfield<=0;
 end
 end else if(launch_t)begin
 error<=0;triangle_mode<=1;any_front<=0;
 if(!refs_valid)begin done_t<=1;error<=2;end
 else begin busy<=1;pc<=128;refs<=triangle_descriptor[17:0];scale<=triangle_descriptor[21:18];cull<=cull_negative;building_desc<=triangle_descriptor;sigma<=0;collecting<=1;end
 end else if(step)begin
 pc<=next_pc;
 if(bounds_run)begin
 if(control[71])bounds_safe<=bounds_safe&&quant_shift<=14&&!
  ((control[43]&&(unpacked[31]?negative_over[2]:positive_over[2]))||
   (control[44]&&(unpacked[31]?negative_over[7]:positive_over[7]))||
   (control[45]&&(unpacked[31]?negative_over[12]:positive_over[12])));
 if(flow==7&&bounds_safe&&((cfg_idx==0&&!raw_bound_front)||(cfg_idx==1&&bound_left)||(cfg_idx==2&&bound_right)))begin busy<=0;done_v<=1;end
 if(flow==7&&control[70]&&!(bounds_safe&&((cfg_idx==0&&!raw_bound_front)||(cfg_idx==1&&bound_left)||(cfg_idx==2&&bound_right))))vertex_epoch<=!vertex_epoch;
 end
 if(!triangle_mode)begin
 if(flow==3)remaining<=remaining-1'b1;
 if(flow==4)begin busy<=0;done_v<=1;end
 if(l_src==2||r_src==2)vp<=vp+1'b1;
 if(vertex_emit)begin
 op<=op+1'b1;
 if(vfield==4)begin vfield<=0;vid<=vid+1'b1;published_vertices<=published_vertices+1'b1;end else vfield<=vfield+1'b1;
 end
 end else begin
 // 原Z读顺序为p1.z、p2.z、p0.z；第三个Z写回就在step15。
 if(z_write)any_front<=any_front||raw_front;
 if(reject_behind)begin busy<=0;done_t<=1;collecting<=0;end
 if(gather_end)collecting<=0;
 if(ref_rot)refs<={refs[5:0],refs[17:6]};
 if(flow==5&&(delta_zero||(delta_negative&&cull)))begin busy<=0;done_t<=1;end
 if(flow==6)begin busy<=0;done_t<=1;end
 if(rf_wsrc==1&&p_range_error)begin busy<=0;done_t<=1;error<=1;collecting<=0;end
 end
 if(acc_op!=0)acc<=acc_next;
 if(rf_wsrc==1)phase<=phase==2||unpack_kind>=3?2'd0:phase+1'b1;
 if(rf_wsrc==1&&unpack_kind<3)unpacked<={10'd0,unpacked[31:10]};
 if(up_push!=0)begin
 unpacked<=up_push==4?pq[31:0]:left[31:0];
 if(up_push==6)unpacked[9:0]<={10{left[31]==control[70]}};
 unpack_kind<=up_push==6?3'd1:up_push;
 phase<=up_push==6?cfg_idx:2'd0;
 
 end
 if(cfg_we)case(cfg_idx)0:origin_x<=left[31:0];1:origin_y<=left[31:0];2:origin_z<=left[31:0];3:begin quant_shift<=left[4:0];normal18<=left[8];end endcase
 case(pack_op)
 1:packed_buffer<={22'd0,pack_value};
 2:packed_buffer<={42'd0,packed_normal12};
 3:packed_buffer[23:12]<=packed_normal12;
 4:packed_buffer[35:24]<=packed_normal12;
 5:begin packed_buffer<=acc;if(delta_capture)begin delta_zero<=acc==0;delta_negative<=acc[53];sigma<=acc[53];end end
 6:packed_buffer<={{22{pack_value[31]}},pack_value};
 default:begin end
 endcase
 end
 end
 end
`ifndef SYNTHESIS
 reg sim_output_held;reg [58:0] sim_output;
 reg [4:0] sim_export_reads;
 always @(posedge clk)begin
  if(reset)begin sim_output_held<=0;sim_output<=0;sim_export_reads<=0;end
  else begin
   if(sim_output_held&&(!word_valid||{word_index,word_data}!==sim_output))
    $fatal(1,"T output changed before acceptance");
   sim_output_held<=word_valid&&(!ce||!word_ready);
   if(word_valid)sim_output<={word_index,word_data};
   if(ce)begin
    if(packet_take)sim_export_reads<=export_read?1:0;
    else if(export_read)sim_export_reads<=sim_export_reads+1'b1;
    if(release_slot&&sim_export_reads!=16)$fatal(1,"T release before sixteen paid reads");
    if(vwrite&&vfield==3&&emit!=2)$fatal(1,"N36 written outside final normal slot");
    if(vwrite&&vfield==4&&emit!=1)$fatal(1,"attribute written outside final vertex slot");
   end
  end
 end
 initial if(!RESULT_COMPACT||!SP_COMPACT)$fatal(1,"独立结果口候选仅支持选定compact布局");
 initial if(TRIANGLE_SLOT_BITS<1||TRIANGLE_SLOT_BITS>4)$fatal(1,"Pingpong supports T queue depths 2..16");
 always @(posedge clk)if(ce&&!reset)begin
  if(count>TRIANGLE_SLOTS)$fatal(1,"T result queue overflow");
  if(step&&triangle_mode&&twrite&&trow>=10)$fatal(1,"T coefficient row outside record");
  if(step&&triangle_mode&&twrite&&coefficient_wa[8])
   $fatal(1,"T coefficient write overlaps V attributes");
  if(meshlet_reset&&!meshlet_ready)$fatal(1,"attribute epoch reused before final indexed export");
  if(vwrite&&vfield>=3&&!queue_empty&&export_epoch==vertex_epoch)
   $fatal(1,"V writes an attribute epoch still owned by queued T");
  if(core_read&&core_qvalid&&!(step&&core_uses_q))$fatal(1,"位置Q被未消费的后继读覆盖");
  if(export_read&&!q_can_retire)$fatal(1,"导出Q被未消费的后继读覆盖");
 end
`endif
endmodule

