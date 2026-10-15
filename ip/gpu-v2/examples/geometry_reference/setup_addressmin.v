module probe(input wire clk,
input wire [0:0] ce,
input wire [0:0] reset,
input wire [0:0] kick,
input wire [0:0] metadata_valid,
input wire [0:0] metadata_address,
input wire [63:0] metadata_data,
input wire [0:0] grouped,
input wire [0:0] write_ready,
input wire [0:0] out_ready,
input wire [0:0] cp_color,
input wire [0:0] dma_valid,
input wire [9:0] dma_address,
input wire [63:0] dma_data,
output wire [0:0] busy,
output wire [0:0] done,
output wire [10:0] pc,
output wire [0:0] flag,
output wire [6:0] norm_shift,
output wire [1:0] whole,
output wire [5:0] residual,
output wire [8:0] pointer,
output wire [0:0] guard,
output wire [0:0] sticky,
output wire [0:0] bad,
output wire [0:0] orientation,
output wire [6:0] scan_candidate,
output wire [0:0] scan_nonzero,
output wire [35:0] code,
output wire [71:0] w_product,
output wire [71:0] w_acc,
output wire [71:0] w_buffer,
output wire [35:0] n_product0,
output wire [35:0] n_product1,
output wire [35:0] n_acc,
output wire [35:0] n_buffer,
output wire [35:0] alu_buffer,
output wire [35:0] write_data,
output wire [143:0] read_data,
output wire [35:0] output_read,
output wire [0:0] carry,
output wire [6:0] vertex_id,
output wire [1:0] decode_axis,
output wire [35:0] normal_pack,
output wire [4:0] grid_shift,
output wire [63:0] front_do,
output wire [0:0] front_pending,
output wire [0:0] return_half,
output wire [0:0] return_signed,
output wire [0:0] rcp_busy,
output wire [5:0] rcp_count,
output wire [35:0] rcp_q,
output wire [32:0] rcp_rem,
output wire [31:0] rcp_w,
output wire [0:0] deferred,
output wire [0:0] far_clamped,
output wire [31:0] depth_value,
output wire [6:0] published,
output wire [4:0] rcp_exp,
output wire [17:0] rcp_fraction,
output wire [35:0] rcp_table,
output wire [6:0] rcp_index,
output wire [0:0] t_mode,
output wire [0:0] out_valid,
output wire [0:0] out_last,
output wire [31:0] compact_position,
output wire [31:0] compact_normal,
output wire [31:0] compact_attribute,
output wire [35:0] out_data,
output wire [2:0] clip_plane,
output wire [3:0] clip_count,
output wire [2:0] clip_index,
output wire [3:0] clip_out_count,
output wire [0:0] clip_buffer,
output wire [0:0] clip_inside,
output wire [0:0] fan_active,
output wire [2:0] fan_index,
output wire [0:0] clip_all_inside,
output wire [31:0] partner_position,
output wire [31:0] partner_normal,
output wire [31:0] partner_attribute,
output wire [3:0] schedule_state,
output wire [6:0] tag_position,
output wire [6:0] triangle_count,
output wire [6:0] group_end,
output wire [53:0] index_word,
output wire [1:0] index_lane,
output wire [5:0] triangle_group,
output wire [63:0] tags_left,
output wire [5:0] id0,
output wire [5:0] id1,
output wire [5:0] id2,
output wire [0:0] schedule_done,
output wire [0:0] schedule_fault
);
wire [8:0] query_cold_base;
wire [1:0] query_cold_mode;
wire [0:0] query_cr;
wire [2:0] query_im;
wire [17:0] query_transport18;
wire [0:0] query_prefetch_next;

wire core_start,index_read;wire [10:0] core_entry;wire [6:0] vertex_limit;wire [9:0] cp_address;wire cp_half;
reg [95:0] metadata_hold;always @(posedge clk)if(reset)metadata_hold<=0;else if(metadata_valid)begin if(!metadata_address)metadata_hold[63:0]<=metadata_data;else metadata_hold[95:64]<=metadata_data[31:0];end
wire [6:0] vertex_count=metadata_hold[14:8],total_triangles=metadata_hold[22:16],tag_count=metadata_hold[30:24];wire [63:0] metadata=metadata_hold[95:32];
tag_controller tags(.clk(clk),.ce(ce),.reset(reset),.kick(kick),.vertex_count(vertex_count),.total_triangles(total_triangles),.tag_count(tag_count),.metadata(metadata),.grouped(grouped),.core_busy(busy),.core_out_valid(out_valid),.out_ready(out_ready),.core_t_mode(t_mode),.published(published),.front_do(front_do),.core_start(core_start),.core_entry(core_entry),.vertex_limit(vertex_limit),.index_read(index_read),.schedule_state(schedule_state),.tag_position(tag_position),.triangle_count(triangle_count),.group_end(group_end),.index_word(index_word),.index_lane(index_lane),.triangle_group(triangle_group),.tags_left(tags_left),.id0(id0),.id1(id1),.id2(id2),.schedule_done(schedule_done),.schedule_fault(schedule_fault));
wire [6:0] plane_rows=(vertex_count+1)>>1;wire [2:0] vf=query_cold_base[2:0];wire [2:0] vi={1'b0,query_cold_base[4:3]};
wire [5:0] source_id=(vi==0)?id0:((vi==1)?id1:id2);wire [6:0] next_v=vertex_id+1'b1;
wire [2:0] result_field=vf;
assign cp_address=index_read?(3*plane_rows+triangle_group):
(query_transport18[0] && query_prefetch_next)?(((query_transport18[8:7]==0)?10'd0:((query_transport18[8:7]==1)?{3'b0,plane_rows}:((query_transport18[8:7]==2)?{2'b0,plane_rows,1'b0}:({3'b0,plane_rows}+{2'b0,plane_rows,1'b0}))))+(next_v>>1)):
(query_cold_base==54 && (query_im==5 || query_im==6))?(384+(rcp_index>>1)):
(query_cold_mode==3 && query_cr)?({1'b0,source_id,result_field}):
(query_cold_base<3)?(((query_cold_base==0)?10'd0:((query_cold_base==1)?{3'b0,plane_rows}:{2'b0,plane_rows,1'b0}))+(vertex_id>>1)):(256+((query_cold_base-32)>>1));
assign cp_half=index_read?1'b0:((query_transport18[0] && query_prefetch_next)?next_v[0]:((query_cold_base==54)?rcp_index[0]:((query_cold_base<3)?vertex_id[0]:query_cold_base[0])));
geometry_core core(.clk(clk),.ce(ce),.reset(reset),.start(core_start),.entry(core_entry),.load(1'b0),.load_address(9'd0),.load_data(36'd0),.inspect_enable(1'b0),.inspect_address(9'd0),.write_ready(write_ready),.cp_address(cp_address),.cp_half(cp_half),.cp_color(cp_color),.dma_valid(dma_valid),.dma_address(dma_address),.dma_data(dma_data),.out_ready(out_ready),.cp_projected(1'b0),.cp_vertex_count(vertex_count),.cp_vertex_limit(vertex_limit),.cp_grouped(grouped),.cp_index_read(index_read),.query_cold_base(query_cold_base),.query_cold_mode(query_cold_mode),.query_cr(query_cr),.query_im(query_im),.query_transport18(query_transport18),.query_prefetch_next(query_prefetch_next),.busy(busy),.done(done),.pc(pc),.flag(flag),.norm_shift(norm_shift),.whole(whole),.residual(residual),.pointer(pointer),.guard(guard),.sticky(sticky),.bad(bad),.orientation(orientation),.scan_candidate(scan_candidate),.scan_nonzero(scan_nonzero),.code(code),.w_product(w_product),.w_acc(w_acc),.w_buffer(w_buffer),.n_product0(n_product0),.n_product1(n_product1),.n_acc(n_acc),.n_buffer(n_buffer),.alu_buffer(alu_buffer),.write_data(write_data),.read_data(read_data),.output_read(output_read),.carry(carry),.vertex_id(vertex_id),.decode_axis(decode_axis),.normal_pack(normal_pack),.grid_shift(grid_shift),.front_do(front_do),.front_pending(front_pending),.return_half(return_half),.return_signed(return_signed),.rcp_busy(rcp_busy),.rcp_count(rcp_count),.rcp_q(rcp_q),.rcp_rem(rcp_rem),.rcp_w(rcp_w),.deferred(deferred),.far_clamped(far_clamped),.depth_value(depth_value),.published(published),.rcp_exp(rcp_exp),.rcp_fraction(rcp_fraction),.rcp_table(rcp_table),.rcp_index(rcp_index),.t_mode(t_mode),.out_valid(out_valid),.out_last(out_last),.compact_position(compact_position),.compact_normal(compact_normal),.compact_attribute(compact_attribute),.out_data(out_data),.clip_plane(clip_plane),.clip_count(clip_count),.clip_index(clip_index),.clip_out_count(clip_out_count),.clip_buffer(clip_buffer),.clip_inside(clip_inside),.fan_active(fan_active),.fan_index(fan_index),.clip_all_inside(clip_all_inside),.partner_position(partner_position),.partner_normal(partner_normal),.partner_attribute(partner_attribute));
endmodule
module tag_controller(input wire clk,ce,reset,kick,input wire [6:0] vertex_count,total_triangles,tag_count,input wire [63:0] metadata,input wire grouped,core_busy,core_out_valid,out_ready,core_t_mode,input wire [6:0] published,input wire [63:0] front_do,
output reg [3:0] schedule_state,output reg [6:0] tag_position,triangle_count,group_end,output reg [53:0] index_word,output reg [1:0] index_lane,output reg [5:0] triangle_group,output reg [63:0] tags_left,output reg [5:0] id0,id1,id2,output reg schedule_done,schedule_fault,
output wire core_start,output reg [10:0] core_entry,output wire [6:0] vertex_limit,output wire index_read);
reg [6:0] nv,nt,count;
wire run=ce && (!core_out_valid || out_ready);
wire tag=tags_left[0];wire [6:0] vend={published[6:1],1'b0}+7'd2;
wire [17:0] tuple=index_word[17:0];
assign core_start=schedule_state==1 || schedule_state==4 || schedule_state==9;
always @*begin core_entry=0;if(schedule_state==4)begin if(core_t_mode)core_entry=81;else core_entry=94;end if(schedule_state==9)core_entry=181;end
assign vertex_limit=grouped?group_end:published+1'b1;assign index_read=schedule_state==6;
always @(posedge clk)begin
if(reset)begin schedule_state<=0;tag_position<=0;triangle_count<=0;group_end<=0;index_word<=0;index_lane<=0;triangle_group<=0;tags_left<=0;id0<=0;id1<=0;id2<=0;schedule_done<=0;schedule_fault<=0;nv<=0;nt<=0;count<=0;end
else if(run)case(schedule_state)
0:if(kick)begin nv<=vertex_count;nt<=total_triangles;count<=tag_count;tags_left<=metadata;tag_position<=0;triangle_count<=0;group_end<=0;index_word<=0;index_lane<=0;triangle_group<=0;id0<=0;id1<=0;id2<=0;schedule_done<=0;schedule_fault<=0;if(vertex_count>64 || total_triangles>96 || tag_count>64)schedule_fault<=1;else schedule_state<=1;end
1:schedule_state<=2;
2:if(!core_busy && !core_out_valid)schedule_state<=3;
3:if(tag_position==count)begin schedule_done<=1;if(triangle_count!=nt || published!=nv)schedule_fault<=1;schedule_state<=0;end
else if(tag)begin if(triangle_count>=nt)begin schedule_fault<=1;schedule_state<=0;end else schedule_state<=6;end
else if(published>=nv)begin schedule_fault<=1;schedule_state<=0;end
else begin group_end<=(vend>nv)?nv:vend;schedule_state<=4;end
4:schedule_state<=5;
5:if(!core_busy && !core_out_valid)begin if(published<group_end)schedule_state<=4;else begin tag_position<=tag_position+1'b1;tags_left<=tags_left>>1;schedule_state<=3;end end
6:schedule_state<=7;
7:begin index_word<=front_do[53:0];schedule_state<=8;end
8:begin id0<=tuple[5:0];id1<=tuple[11:6];id2<=tuple[17:12];
if(tuple[5:0]>=nv || tuple[11:6]>=nv || tuple[17:12]>=nv || tuple[5:0]>=published || tuple[11:6]>=published || tuple[17:12]>=published)begin schedule_fault<=1;schedule_state<=0;end else schedule_state<=9;end
9:schedule_state<=10;
10:if(!core_busy && !core_out_valid)begin triangle_count<=triangle_count+1'b1;if(triangle_count+1==nt || index_lane==2)begin tag_position<=tag_position+1'b1;tags_left<=tags_left>>1;index_lane<=0;triangle_group<=triangle_group+1'b1;schedule_state<=3;end else begin index_word<=index_word>>18;index_lane<=index_lane+1'b1;schedule_state<=8;end end
default:schedule_state<=0;endcase end endmodule
module geometry_core(
input wire clk,
input wire [0:0] ce,
input wire [0:0] reset,
input wire [0:0] start,
input wire [10:0] entry,
input wire [0:0] load,
input wire [8:0] load_address,
input wire [35:0] load_data,
input wire [0:0] inspect_enable,
input wire [8:0] inspect_address,
input wire [0:0] write_ready,
input wire [9:0] cp_address,
input wire [0:0] cp_half,
input wire [0:0] cp_color,
input wire [0:0] dma_valid,
input wire [9:0] dma_address,
input wire [63:0] dma_data,
input wire [0:0] out_ready,
input wire [0:0] cp_projected,
output reg [0:0] busy,
output reg [0:0] done,
output reg [10:0] pc,
output reg [0:0] flag,
output reg [6:0] norm_shift,
output reg [1:0] whole,
output reg [5:0] residual,
output reg [8:0] pointer,
output reg [0:0] guard,
output reg [0:0] sticky,
output reg [0:0] bad,
output reg [0:0] orientation,
output reg [6:0] scan_candidate,
output reg [0:0] scan_nonzero,
output wire [35:0] code,
output wire [71:0] w_product,
output wire [71:0] w_acc,
output wire [71:0] w_buffer,
output wire [35:0] n_product0,
output wire [35:0] n_product1,
output wire [35:0] n_acc,
output wire [35:0] n_buffer,
output wire [35:0] alu_buffer,
output wire [35:0] write_data,
output wire [143:0] read_data,
output wire [35:0] output_read,
output wire [0:0] carry,
output reg [6:0] vertex_id,
output reg [1:0] decode_axis,
output reg [35:0] normal_pack,
output reg [4:0] grid_shift,
output wire [63:0] front_do,
output reg [0:0] front_pending,
output reg [0:0] return_half,
output reg [0:0] return_signed,
output reg [0:0] rcp_busy,
output reg [5:0] rcp_count,
output reg [35:0] rcp_q,
output reg [32:0] rcp_rem,
output reg [31:0] rcp_w,
output reg [0:0] deferred,
output reg [0:0] far_clamped,
output reg [31:0] depth_value,
output reg [6:0] published,
output reg [4:0] rcp_exp,
output reg [17:0] rcp_fraction,
output wire [35:0] rcp_table,
output reg [6:0] rcp_index,
output reg [0:0] t_mode,
output reg [0:0] out_valid,
output reg [0:0] out_last,
output reg [31:0] compact_position,
output reg [31:0] compact_normal,
output reg [31:0] compact_attribute,
output wire [35:0] out_data,
output reg [2:0] clip_plane,
output reg [3:0] clip_count,
output reg [2:0] clip_index,
output reg [3:0] clip_out_count,
output reg [0:0] clip_buffer,
output reg [0:0] clip_inside,
output reg [0:0] fan_active,
output reg [2:0] fan_index,
output reg [0:0] clip_all_inside
,
input wire [6:0] cp_vertex_count,
input wire [6:0] cp_vertex_limit,
input wire [0:0] cp_grouped,
input wire [0:0] cp_index_read,
output reg [31:0] partner_position,
output reg [31:0] partner_normal,
output reg [31:0] partner_attribute,
output wire [8:0] query_cold_base,
output wire [1:0] query_cold_mode,
output wire [0:0] query_cr,
output wire [2:0] query_im,
output wire [17:0] query_transport18,
output wire [0:0] query_prefetch_next);
wire [35:0] alu_input_a;
wire run_ce;wire [35:0] compact_ingress;
wire [35:0] transport_bus;wire [17:0] transport18=transport_bus[17:0];
wire prefetch_next=vertex_id+1<cp_vertex_limit;
wire partner_next=cp_grouped && !vertex_id[0] && prefetch_next;
wire prefetch_request=busy && transport18[0] && prefetch_next && !partner_next;
wire prefetch_ingress=busy && transport18[1] && prefetch_next;
wire transport_publish=busy && transport18[2];
wire [1:0] transport_latch=transport18[4:3];
reg position_ahead,compact_ahead,clip_ahead,lookup_ahead;
always @(posedge clk)begin
if(reset)begin position_ahead<=0;compact_ahead<=0;clip_ahead<=0;lookup_ahead<=0;end
else if(run_ce && busy)begin position_ahead<=transport18[12];compact_ahead<=transport18[13];clip_ahead<=transport18[14];lookup_ahead<=transport18[15];end
end
wire compact_alu=busy && compact_ahead;
wire [35:0] compact_attribute_value={4'b0,compact_attribute[31:24]&{8{!cp_color}},compact_attribute[23:0]};
always @(posedge clk)begin
if(reset)begin compact_position<=0;compact_normal<=0;compact_attribute<=0;end
else if(run_ce && busy && (transport18[5] || prefetch_next))case(transport_latch)
1:compact_position<=compact_ingress[31:0];
2:compact_normal<=compact_ingress[31:0];
3:compact_attribute<=compact_ingress[31:0];
default:begin end endcase end

wire [1:0] fmt=code[1:0];
wire [3:0] dst=code[5:2];wire wb=code[6];wire [1:0] sel=code[8:7];wire hi=code[9];
wire [2:0] wo=code[12:10];wire [5:0] alu=code[18:13];
wire compute=(fmt==0 || fmt==3),transfer=(fmt==1),control=(fmt==2);
wire [3:0] ar=code[22:19],br=code[26:23];
wire ra=compute && code[27],rb=compute && code[28];
wire [3:0] rf_a=ar+((fmt==0 && code[35]) ? {2'b0,whole} : 4'b0);
wire [8:0] cold_base=code[27:19];wire [1:0] cold_mode=code[29:28];
reg [9:0] cold_address;
always @* begin cold_address=(t_mode && cold_base>=384)?cold_base+10'd128:cold_base;case(cold_mode)
1:cold_address=cold_base+pointer+10'd128;2:begin if(t_mode)cold_address=cold_base+residual+10'd128;else cold_address=cold_base+{vertex_id,3'b0};end
default:begin end
endcase end
wire cr=transfer && code[30],cw=(transfer && code[31]) || transport_publish,ig=transfer && code[32];
wire [2:0] im=code[35:33];wire [2:0] kind=code[21:19];wire [3:0] sub=code[25:22];wire [9:0] imm=code[35:26];
wire [4:0] aux=code[33:29];wire [1:0] arg=code[35:34];
assign run_ce=ce && (!busy || !cw || write_ready) && (!out_valid || out_ready);
wire [35:0] issue_bus;wire [17:0] issue18=issue_bus[17:0];
wire dual_issue=busy && issue18[17];
wire normal_issue=dual_issue && imm[4];
wire normal_capture=dual_issue && imm[7];
wire [1:0] normal_pack_select=dual_issue?imm[9:8]:2'd0;

wire uniform_slot=busy && issue18[11];
wire uniform_store=busy && issue18[12];
wire uniform_issue=busy && issue18[13];
wire uniform_n=busy && issue18[14];
wire uniform_capture=busy && issue18[15];
wire [4:0] uniform_address=dual_issue?{1'b0,sub}:{imm[4],sub};
wire uniform_publish=busy && issue18[16];
wire normal_publish=dual_issue && uniform_publish && !wb;
wire [1:0] uniform_field=normal_publish?2'd3:((dst==13)?2'd0:((dst==10)?2'd1:2'd2));
wire [9:0] uniform_result={vertex_id,3'b0}+{7'b0,uniform_field};
wire [35:0] uniform_value;
wire [35:0] qa=read_data[35:0],qb=read_data[71:36];
reg [5:0] bit_length;integer j;
always @* begin bit_length=0;for(j=0;j<9;j=j+1)if(qa[j])bit_length=j+1;end
reg [6:0] scan_offset;always @* begin scan_offset=0;case(arg)1:scan_offset=36;2:scan_offset=72;default:begin end endcase end
wire [7:0] scan_bits=bit_length+scan_offset;
wire [6:0] candidate=(scan_bits>23)?scan_bits-23:0;
reg [10:0] next_pc;
always @* begin next_pc=pc+11'd1;if(control)begin
if(kind==2 && !flag)next_pc={sub[3],imm};else if(kind==3)next_pc={sub[3],imm};
end end
wire [10:0] fetch_pc=busy ? next_pc : entry;
reg load_valid;reg [8:0] saved_address;
always @(posedge clk)begin
if(reset)begin clip_plane<=0;clip_count<=0;clip_index<=0;clip_out_count<=0;clip_buffer<=0;clip_inside<=0;fan_active<=0;fan_index<=0;clip_all_inside<=0;busy<=0;done<=0;pc<=0;flag<=0;norm_shift<=0;whole<=0;residual<=0;pointer<=0;guard<=0;sticky<=0;bad<=0;orientation<=0;scan_candidate<=0;scan_nonzero<=0;load_valid<=0;saved_address<=0;end
else if(run_ce)begin
load_valid<=load && !busy;if(load && !busy)saved_address<=load_address;
if(start && !busy)begin busy<=1;done<=0;pc<=entry;end
if(busy)begin
pc<=next_pc;
if(control)case(kind)
0:case(sub)
0:norm_shift<=0;1:residual<=pointer[3:2];13:begin pointer<=pointer+1;flag<=pointer<imm[8:0];end
2:begin
if(norm_shift>=72)begin whole<=2;residual<=norm_shift-72;end
else if(norm_shift>=36)begin whole<=1;residual<=norm_shift-36;end
else begin whole<=0;residual<=norm_shift;end
end
3:flag<=norm_shift!=0;4:flag<=residual!=0;5:flag<=whole>=1;6:flag<=whole>=2;
7:begin guard<=0;sticky<=0;end
10:bad<=0;11:flag<=!orientation;
12:begin pointer<=pointer+4;flag<=pointer<imm[8:0];end
14:pointer<=0;15:orientation<=qa[35];default:begin end
endcase
1:begin
if(sub==0)flag<=!qa[35];
else begin flag<=qa[35:23]=={13{qa[23]}};if(qa[35:23]!={13{qa[23]}})bad<=1;end
end
4:begin busy<=0;done<=1;end
5:begin if(!uniform_slot)case(sub)0:flag<=!bad;1:norm_shift<=norm_shift+1;default:begin end endcase end
6:case(sub)0:flag<=!rcp_busy;1:flag<=!deferred;default:begin end endcase
7:case(sub)1:flag<=vertex_id+1<cp_vertex_limit;5:flag<=cp_color;
6:begin if(imm[0])flag<=qa==0;else flag<=qa!=0;end
10:begin case(imm[1:0])0:flag<=clip_plane==0;1:flag<=clip_plane>=2;2:flag<=clip_plane<5;3:flag<=clip_all_inside;default:flag<=0;endcase end
11:flag<=!(cp_projected || fan_active);
12:clip_inside<=!qa[35];13:flag<=clip_inside;14:flag<=!fan_active;15:flag<=clip_plane<3;
7:clip_all_inside<=clip_all_inside && !qa[35];default:begin end endcase
default:begin end
endcase
if(fmt==1 && cw && im==2 && qa[35:23]!={13{qa[23]}})norm_shift<=5;
if(fmt==3)case(aux)







default:begin end
16:begin clip_plane<=0;clip_count<=3;clip_index<=0;clip_out_count<=0;clip_buffer<=0;fan_active<=0;end
17:begin clip_all_inside<=1;clip_index<=0;clip_out_count<=0;if(clip_plane==0)whole<=2;else if(clip_plane<3)whole<=0;else whole<=1;end
18:residual<={clip_buffer,(clip_count[2:0]-3'd1),2'b00};
19:residual<={clip_buffer,clip_index,2'b00};
20:begin if(arg[0])residual<={!clip_buffer,clip_out_count[2:0],2'b00};else flag<=!qa[35];end
21:clip_out_count<=clip_out_count+4'd1;
22:begin clip_index<=clip_index+3'd1;flag<=({1'b0,clip_index}+4'd1)<clip_count;end
23:begin clip_plane<=clip_plane+3'd1;if(arg[0])flag<=clip_count>=3;else begin clip_count<=clip_out_count;clip_buffer<=!clip_buffer;flag<=clip_out_count>=3;end end
24:residual<={clip_buffer,clip_index,2'b00};
25:begin clip_index<=clip_index+3'd1;flag<=({1'b0,clip_index}+4'd1)<clip_count;end
29:begin fan_active<=!arg[0];fan_index<=1;end
30:begin fan_index<=fan_index+3'd1;flag<=({1'b0,fan_index}+4'd2)<clip_count;end
31:flag<=!qa[17] && !qa[35];
endcase
end end end


wire signed [36:0] axis_neg=-$signed({qa[35],qa});
wire signed [36:0] axis_pos=$signed({1'b0,qa});
reg signed [36:0] abs_axis;
always @* begin if(qa[35])abs_axis=axis_neg;else abs_axis=axis_pos;end
wire signed [38:0] guard_limit=$signed(qb)*39'sd8;
reg [4:0] lookup_exp;integer lookup_j;
always @* begin lookup_exp=0;for(lookup_j=0;lookup_j<32;lookup_j=lookup_j+1)if(alu_input_a[lookup_j])lookup_exp=lookup_j;end
wire [31:0] lookup_normalized;
wire lookup_issue=run_ce && busy && fmt==3 && aux==28;
assign rcp_table=0;

assign out_data=output_read;
always @(posedge clk)begin
if(reset)begin t_mode<=0;out_valid<=0;out_last<=0;end
else begin
if(ce && out_valid && out_ready)out_valid<=0;
if(run_ce && busy)begin
if(fmt==2 && kind==7)begin
if(sub==0)t_mode<=0;
if(sub==9)t_mode<=imm[0];

end
if(fmt==1 && cr && im==7)begin out_valid<=1;out_last<=hi;end
end end end
wire front_read=(busy && cr && (im==5 || im==6)) || prefetch_request || cp_index_read;
wire [31:0] selected32=return_half ? front_do[63:32] : front_do[31:0];
reg [35:0] front_extended;
always @* begin front_extended={4'b0,selected32};
if(return_signed)front_extended={{4{selected32[31]}},selected32};end
wire [1:0] partner_field=transport18[10:9];
wire [35:0] partner_value={4'b0,(partner_field==0)?partner_position:((partner_field==1)?partner_normal:partner_attribute)};
wire partner_ingress=prefetch_ingress && partner_next;
wire [31:0] partner_capture=return_half?front_do[31:0]:front_do[63:32];
always @(posedge clk)begin
if(reset)begin partner_position<=0;partner_normal<=0;partner_attribute<=0;end
else if(run_ce && busy)begin
if(pc==96)partner_position<=partner_capture;
if(pc==97)partner_normal<=partner_capture;
if(pc==98)partner_attribute<=partner_capture;
if(prefetch_ingress && !partner_next)case(partner_field)
0:partner_position<=front_do[63:32];1:partner_normal<=front_do[63:32];2:partner_attribute<=front_do[63:32];default:begin end endcase
end end
wire [35:0] ingress_word=partner_ingress ? partner_value : (im==5 || im==6 || prefetch_ingress) ? front_extended : output_read;
always @(posedge clk)begin
if(reset)begin vertex_id<=0;decode_axis<=0;normal_pack<=0;grid_shift<=0;
front_pending<=0;return_half<=0;return_signed<=0;published<=0;
rcp_busy<=0;rcp_count<=0;rcp_q<=0;rcp_rem<=0;rcp_w<=0;rcp_exp<=0;rcp_fraction<=0;rcp_index<=0;deferred<=0;far_clamped<=0;depth_value<=0;
end else if(run_ce)begin
front_pending<=front_read;
if(front_read)begin return_half<=cp_half;return_signed<=!prefetch_request && im==6;end
if(busy)begin
case(normal_pack_select)
1:normal_pack[11:0]<=n_buffer[11:0];
2:normal_pack[23:12]<=n_buffer[11:0];
3:normal_pack[35:24]<=n_buffer[11:0];
default:begin end endcase
if(fmt==2 && kind==7)case(sub)
0:begin vertex_id<=0;normal_pack<=0;published<=0;end
1:begin vertex_id<=vertex_id+1'b1;published<=published+1'b1;normal_pack<=0;end
2:grid_shift<=qa[4:0];8:begin if(qa<=2)grid_shift<=2;else grid_shift<=bit_length+1;end
3:decode_axis<=imm[1:0];
4:case(imm[1:0])0:normal_pack[11:0]<=qa[11:0];1:normal_pack[23:12]<=qa[11:0];2:normal_pack[35:24]<=qa[11:0];default:begin end endcase
default:begin end endcase
if(fmt==3)case(aux)
26:begin deferred<=($signed(qa)<36'sd8199);far_clamped<=($signed(qa)>36'sd13041664);
if(qa[35])depth_value<=0;else if($signed(qa)>36'sd13041664)depth_value<=32'd13041664;else depth_value<=qa[31:0];end
27:if(abs_axis>guard_limit)deferred<=1;
28:begin rcp_index<=lookup_normalized[30:24];rcp_exp<=lookup_exp;rcp_fraction<=lookup_normalized[23:6];end
default:begin end endcase
end end end
// Input capacity contract: DMA and compute addresses are below512.
wire [35:0] compact_front_do0;
assign front_do[0 +: 32]=compact_front_do0[31:0];
SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(0),.BLK_SEL_0(0),.BLK_SEL_1(0),.RESET_MODE("SYNC")) scratch0(
.DO(compact_front_do0),.DI({4'b0,dma_data[0 +: 32]}),
.ADA({dma_address[8:0],5'b01111}),.ADB({cp_address[8:0],5'b00000}),.BLKSELA(3'b000),.BLKSELB(3'b000),
.CLKA(clk),.CLKB(clk),.CEA(dma_valid && !reset && !dma_address[9]),.CEB(run_ce && front_read && !reset && !cp_address[9]),
.OCE(1'b1),.RESETA(reset),.RESETB(reset));
wire [35:0] compact_front_do1;
assign front_do[32 +: 32]=compact_front_do1[31:0];
SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(0),.BLK_SEL_0(0),.BLK_SEL_1(0),.RESET_MODE("SYNC")) scratch1(
.DO(compact_front_do1),.DI({4'b0,dma_data[32 +: 32]}),
.ADA({dma_address[8:0],5'b01111}),.ADB({cp_address[8:0],5'b00000}),.BLKSELA(3'b000),.BLKSELB(3'b000),
.CLKA(clk),.CLKB(clk),.CEA(dma_valid && !reset && !dma_address[9]),.CEB(run_ce && front_read && !reset && !cp_address[9]),
.OCE(1'b1),.RESETA(reset),.RESETB(reset));
wire direct_store=busy && fmt==1 && cw && (im==2 || im==3);
wire dp_ce=run_ce && (busy || load || load_valid || inspect_enable);
wire [4:0] write_enable=busy ? {cw || uniform_publish,2'b0,{2{wb}}} : {load_valid,4'b0};
wire [9:0] ca=busy ? (transport_publish ? {vertex_id,3'b0}+9'd4 : (uniform_publish ? uniform_result : cold_address)) : saved_address;
wire [2:0] logical_vertex={1'b0,cold_base[4:3]};
wire [2:0] logical_field=cold_base[2:0];
wire [2:0] fan_vertex=(logical_vertex==0)?3'd0:((logical_vertex==1)?fan_index:fan_index+3'd1);
wire [1:0] fan_field=(logical_field==5)?2'd0:logical_field[1:0]+2'd1;
wire local_fan_read=fan_active && cold_mode==3 && (logical_field<3 || logical_field==5);
wire [8:0] fan_address={3'b110,clip_buffer,fan_vertex,fan_field};
wire [9:0] sr=busy ? ((cold_mode==3 && cr) ? (local_fan_read ? fan_address+10'd128 : cp_address) : cold_address) : inspect_address;
reg [2:0] no,nc;reg ns,ne;
always @* begin no=0;nc=0;ns=0;ne=0;
if(fmt==3 && busy)case(aux)
1:no=3;2:no=2;3:no=1;9:begin no=1;ns=1;end
4:begin ne=1;nc=0;end 5:begin ne=1;nc=1;end 6:begin ne=1;nc=2;end
11:begin ne=1;nc=7;end 7:begin ne=1;nc=4;end 8:begin ne=1;nc=3;end default:begin end
endcase
if(uniform_n)no=wo;
if(dual_issue)begin no={1'b0,imm[6:5]};ne=normal_capture;nc=2;end
end
wire prepare_alu=fmt==0 && code[35];
wire [5:0] prepared_mode=code[34:29];
wire [1:0] axis_slot=whole+2'd1;
wire [3:0] issued_a={issue18[3:2],(issue18[10]?axis_slot:issue18[1:0])};
(* syn_preserve=1, syn_keep=1 *) reg [5:0] alu_flags;
wire prepare_sub=(prepared_mode==1 || prepared_mode==4 || prepared_mode==9 || prepared_mode==13 || prepared_mode==28 || prepared_mode==31 || prepared_mode==39 || prepared_mode==40 || prepared_mode==62) ||
(prepared_mode==60 && orientation) ||
(prepared_mode==6 && !clip_plane[0]) || (prepared_mode==7 && clip_plane[0]);
wire prepare_packed=(prepared_mode==2 || prepared_mode==28 || prepared_mode==31 || prepared_mode==39 || prepared_mode==40 || prepared_mode==62);
always @(posedge clk)begin
if(reset)alu_flags<=0;
else if(run_ce && busy && prepare_alu)alu_flags<={prepared_mode==7,prepared_mode==6,(prepared_mode==13 || prepared_mode==40),prepared_mode==38,prepare_packed,prepare_sub};
else if(run_ce && busy && fmt==3 && aux==1 && code[35])alu_flags<=6'd1;
end
pROMX9 #(.BIT_WIDTH(18),.READ_MODE(0),.INIT_RAM_00(288'h000000000000000000000740000000000000000000000000000000000000000000000000),.INIT_RAM_01(288'h060000000000001800000000000060000000000001800000000000060000000000001800),.INIT_RAM_02(288'h000001800000000000060000000000001800000000000060000000000001800000000000),.INIT_RAM_03(288'h000000000060000000000001800000000000060000000000001800000000000060000000),.INIT_RAM_04(288'h060000000000001800000000000060000000000001800000000000060000000000001800),.INIT_RAM_05(288'h00000000000000001d000000000000000000000000000000000000000000000000000000),.INIT_RAM_06(288'h8acd000000a04000000000000000a04000000000000000a0400000000000000000000000),.INIT_RAM_07(288'h0004f0960a2482096082483297caa4db29758a4d3297caa4da2b758acd22b7c8acda2b75),.INIT_RAM_08(288'h00000001b00c5c00000000000000006c00000000000000004c0013000000000009e8027d),.INIT_RAM_09(288'h0000002370000000000000002da00b740000000000000000780000000000016000000000),.INIT_RAM_0A(288'h00000000000d64000000000001000040000000000000000c4802f1000000000009600000),.INIT_RAM_0B(288'h000000019000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_0C(288'h00070000000000001b00000000000000001c000000000000680000000000000000700000),.INIT_RAM_0D(288'h0000002d400000000000ac00000000000233008c800000006c001a000640000000000000),.INIT_RAM_0E(288'h00944025100000001800000000000fdc00000000000000000002f0000000017000000000),.INIT_RAM_0F(288'h00000000000b5400000000003550000002140005400000005002d5000000274009d40000),.INIT_RAM_10(288'h0000000180000000000005c000000000035700000000000ad000000005c0000000000295),.INIT_RAM_11(288'h0000002d10000002950000002f3000540354008c002f000ad00233009c80250000000000),.INIT_RAM_12(288'h00de8001e00e40037000060000000a60000000dc8001800944027000c4002d000bc0001a),.INIT_RAM_13(288'h00070001b0007c001a00b6c000000070001b00fdc001d00000029c00cec001700bf8001b),.INIT_RAM_14(288'h0005400000007c00150005403d600070001500070001b00060001a00058001e00060001a),.INIT_RAM_15(288'h00000000000000001700060001a0007c00140007000190006c001d00058001500000001e),.INIT_RAM_16(288'h00fe000170000000000000003f7000000000000000000000000000000000000000000000),.INIT_RAM_17(288'h0000000000000000000000000000000000190000000000000003f9000600000000000000),.INIT_RAM_18(288'h000000000000000000000000000000580015000500013000480011000000000000000000),.INIT_RAM_19(288'h0000000000000000000000000000000000160005400140004c0012000440000000000000),.INIT_RAM_1A(288'h00cf0031b00be8033600c5402f400ccc031200bc40000000000000000000000000000000),.INIT_RAM_1B(288'h00000000000000000000000000000000800020000800020cf0833620ccc031b00c540312),.INIT_RAM_1C(288'h00000000000000000000cf0031b00be80000000000000000000000000000000000000000),.INIT_RAM_1D(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_1E(288'h00000000000000033c00c6c02fa00000000000000000000000000000000000000c680000),.INIT_RAM_1F(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_20(288'h00000000000cf0031b00be8000000000000000000000000000000000000033a000000000),.INIT_RAM_21(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_22(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_23(288'h0000000000000000000000000000007c0000000000000000000000000000000000000000),.INIT_RAM_24(288'h000500013000480011000000000000000000000000000000000000000000000000000000),.INIT_RAM_25(288'h0000000000000000000000000140004c0012000440000000000000000000000000000000),.INIT_RAM_26(288'h00000000001bd40000000000000000000000000000000000500013000480011000000000),.INIT_RAM_27(288'h00000000000000000000000000000000000000000000000000000000000000000000001a),.INIT_RAM_28(288'h00000001a00000000001bd40000000000671000000000000000000000000000000000000),.INIT_RAM_29(288'h000000256000000000000740000000740000000000235000000000000740000000000359),.INIT_RAM_2A(288'h000440000000000000000740000000740000000000277000000000000740000000740000),.INIT_RAM_2B(288'h00064001d00000001800050001d0000000170004c001d00000001600048001d000000015),.INIT_RAM_2C(288'h00000000000000001b0006c000000000037000000033000d64000000000001d00000001a),.INIT_RAM_2D(288'h00f64000000000001900e78000000000000000efc000000000000000000001d00074001d),.INIT_RAM_2E(288'h0000000000095803b100000000000000000000e740000000000235000700000000000000),.INIT_RAM_2F(288'h00a6003b300000000000000000000e74000000000027700ec8000000000000000000039d),.INIT_RAM_30(288'h00000000000000000000000000000000000000ed0000000000000000f74000000000039c),.INIT_RAM_31(288'h00000000000000000000f400000000000013000000000009c00000000000000009c00000),.INIT_RAM_32(288'h0000000000000000000000000140004c001200044000000000000000f4400000000003f1),.INIT_RAM_33(288'h0000000180005c0016000540000000600017000580015000000000000000000000000000),.INIT_RAM_34(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_35(288'h00000000000efc000000000000000000001d00074001d0000000000000000130004c0000),.INIT_RAM_36(288'h00000000000b44000000000000000000000000058000000000001600000000000dd80000),.INIT_RAM_37(288'h00000000000f6000000000000000000002d200000031d0000000000000003b8000000000),.INIT_RAM_38(288'h0006c00000000000000000002f20000000000000002f100000035900050001400c780000),.INIT_RAM_39(288'h000000000000000000000000000000000000000000000000000000000000019000600017),.INIT_RAM_3A(288'h000000000000000000000000000000000000000000000000000000000740000000002810),.INIT_RAM_3B(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3C(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3D(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3E(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3F(288'h000000000000000000000000000000000000000000000000000000000000000000000000)) issue_rom(.DO(issue_bus),.AD({fetch_pc[9:0],4'b0}),.CLK(clk),.CE(run_ce && (busy || start)),.OCE(1'b1),.RESET(reset));
pROMX9 #(.BIT_WIDTH(18),.READ_MODE(0),.INIT_RAM_00(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_01(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_02(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_03(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_04(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_05(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_06(288'h0000400400c00000400800000400c00000000010020400c0000000000e00030000a00000),.INIT_RAM_07(288'h00000000000000000000000000000000000000000000000000000000060041200c2c0087),.INIT_RAM_08(288'h000000000000000000000000000000000000000000000000008000000000000000000000),.INIT_RAM_09(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_0A(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_0B(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_0C(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_0D(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_0E(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_0F(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_10(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_11(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_12(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_13(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_14(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_15(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_16(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_17(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_18(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_19(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_1A(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_1B(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_1C(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_1D(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_1E(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_1F(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_20(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_21(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_22(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_23(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_24(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_25(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_26(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_27(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_28(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_29(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_2A(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_2B(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_2C(288'h000000000000000000200000000000000000000000000000000000000000000000000000),.INIT_RAM_2D(288'h000000000000004000000000000000000000000000000000000000000000000000000000),.INIT_RAM_2E(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_2F(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_30(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_31(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_32(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_33(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_34(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_35(288'h000000000000000000000000000000000000000000000000000000000000000200000000),.INIT_RAM_36(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_37(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_38(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_39(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3A(288'h000000000000000000000000000000000000000000000000000000000000000000001000),.INIT_RAM_3B(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3C(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3D(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3E(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3F(288'h000000000000000000000000000000000000000000000000000000000000000000000000)) transport_rom(.DO(transport_bus),.AD({fetch_pc[9:0],4'b0}),.CLK(clk),.CE(run_ce && (busy || start)),.OCE(1'b1),.RESET(reset));
wire [35:0] code0;
pROMX9 #(.BIT_WIDTH(36),.READ_MODE(0),.INIT_RAM_00(288'h0000001f000003c0000000001c80000240000000001c40000220000000001c000002e000),.INIT_RAM_01(288'hd00000001c41000001000380002000b80002008680000a00000075b00000001a41a80001),.INIT_RAM_02(288'hc41180001900a80002d00000001c41100001900680002d00000001c41080001900280002),.INIT_RAM_03(288'h901680002d00000001c41280001901280002d00000001c41200001900e80002d00000001),.INIT_RAM_04(288'hd00000001c41400001901e80002d00000001c41380001901a80002d00000001c41300001),.INIT_RAM_05(288'hc41580001902a80002d00000001c41500001902680002d00000001c41480001902280002),.INIT_RAM_06(288'hd03680002d00000001c41680001d03280002b00000001a41600001902e80002d00000001),.INIT_RAM_07(288'hb00000001a41800001d03e80002d00000001c41780001d03a80002b00000001a41700001),.INIT_RAM_08(288'hc41980001940a80002d00000001c41900001d40680002d00000001c41880001d40280002),.INIT_RAM_09(288'h941680002b00000001a41a80001941280002d00000001c41a00001940e80002d00000001),.INIT_RAM_0A(288'h00003c0000000001c80000240000000001c40000220000000001c000002e000000200002),.INIT_RAM_0B(288'ha40000001002780002000b80002008680000a00000075b00000001a41a800010000001f0),.INIT_RAM_0C(288'h0000341d0000032000a40a80002000f80002a00000001b00000001b40100001b40080001),.INIT_RAM_0D(288'h0402801da000032000a412801ee000034000008f801d6000032000a40e80002004f801e6),.INIT_RAM_0E(288'hc02280802201e808f6901a810023d56808025512808023cce80802148a810021c4680002),.INIT_RAM_0F(288'h3481800030002800ce000280002000280802000280802002e808ea002a81002002680802),.INIT_RAM_10(288'hb00000001a41b00001388180003008180000224100002000700002379d00003379e80003),.INIT_RAM_11(288'h0c000000342858000341c3800030000001e000004a1dc000048000008580000a0000006d),.INIT_RAM_12(288'h0007000020a03001810087000000000001f800005e0000083000000000001d800000c000),.INIT_RAM_13(288'h020000003018b800e37000000dc70000100000000100001b50000001b6800002a4100002),.INIT_RAM_14(288'h0000001e80000041e483c10200081b88016016000000302000000301940015f160000003),.INIT_RAM_15(288'h0000001f400002a00001d4800000000001e80000081e40080080000080000002b4180002),.INIT_RAM_16(288'h0000001c000002e0000067800020002000021901800022d01000020007800020a0280181),.INIT_RAM_17(288'h00007400000848000080000006d900000069970a80065970680001870280001003800002),.INIT_RAM_18(288'h0000001f00000740000085000008f8180002314100002001b800020086000000000001f0),.INIT_RAM_19(288'h0086000000000001f00000740000085800008f8180002330100002001b80002008600000),.INIT_RAM_1A(288'hbf89001cc0000421c80085c21c40085420000084800008f818000234c100002001b80002),.INIT_RAM_1B(288'h02000000301b2000000000001d800007e000fda8000000000001d40000401d0bf89c0000),.INIT_RAM_1C(288'h00006800008ec801dd00004e000cdb800000003c0000200838000000000015c080000003),.INIT_RAM_1D(288'hcf90d21d8d190ec0008dc1000022884000030000001e000000400083fb800000000001fc),.INIT_RAM_1E(288'h0082ee1d00000541d400820600085b2801d00000501d4cf9a521e8d19aea1d00000501d4),.INIT_RAM_1F(288'h0000001d4000050000cfb2800000000001d4000040000bfd2801d00000521d4d182541d8),.INIT_RAM_20(288'h08e08018100006000001aa000008dc1000023e83800030000001dc000040000bfa280000),.INIT_RAM_21(288'h0023800020084000000000001e00000620000083800000000001dc00000600085d380000),.INIT_RAM_22(288'hfd883a1d0b9b8061e085aa401dcbf89c01d8bf997e000fd9000000003800002000000002),.INIT_RAM_23(288'h02000016b09b08000302000015f09a2800030200001e701b9f81d00082861dc85d27e1d4),.INIT_RAM_24(288'h01d9441f00084401ecbf90fe1e0fd983a1d8b9c07a1dcf9b07a1e8f9b85c158088500003),.INIT_RAM_25(288'h81dd5c1f000873a1ecb9e07a1fcf9d87817808840000302000016f09a4000030200001f7),.INIT_RAM_26(288'h83fbda16c0886800030200001e701a6041dc83cdd81f80083821ec81bf581e80085821f8),.INIT_RAM_27(288'h0086001e848f35a1ad0087801ec48ef021a981b5801fc0000701ec00865c1dc0085841f4),.INIT_RAM_28(288'h00865a1e00085d61e800845a1d80085580000083001e848f1701b90084001ec48f2c41a9),.INIT_RAM_29(288'h0082801fc00005a1d40087c41f80082d61d80082841f083f3561fc0086701f80082f81ec),.INIT_RAM_2A(288'h68f3801b168f4001a548f2001ad48f0001b568f80019948ee001950000001d468efda1b9),.INIT_RAM_2B(288'h08ed0018108e1f218100007c00068f60019d68f1801a148f0801a948ee801bd68e100191),.INIT_RAM_2C(288'h95ef0005d95ee8000185ee000010038000020000001fc0000640005f0100002000c00002),.INIT_RAM_2D(288'h01fc0000069ee0019d0000000dc78000000000000000001fb80000800000065900000061),.INIT_RAM_2E(288'h0000000e478000000000000000001fc8000069ee801a10000000e0780000000000000000),.INIT_RAM_2F(288'h94f40004994f38004594f18000184ef800015941800025f010000202300000269ef001a5),.INIT_RAM_30(288'h68eb0019168ea8018d68ea0018968e98018580000005990000005594f80005194f60004d),.INIT_RAM_31(288'h97098004d97060004997058004597020000187018000100380000268ec0019968eb80195),.INIT_RAM_32(288'h68e88019568e80019168e78018d68e70018968e680185800000059900000055970a00051),.INIT_RAM_33(288'h94f08004d94ee8004994f20004594f00000184ee000016ec100002002f8000268e900199),.INIT_RAM_34(288'h900000061970b0005d97070007197030006d94f30006994f10005994ef0005594f280051),.INIT_RAM_35(288'h49ce0100001c58080008e20088149cb0100001c28080001ba0080001c981000800000065),.INIT_RAM_36(288'h08e50108108e48108108e40108108e38108108e30108149c58100001c28080008e280881),.INIT_RAM_37(288'h97040000187000000170c1800026f8100002002f8000285418000208e60008108e580081),.INIT_RAM_38(288'h90000006d95f20006995f00000185ee0000100380000280000006590000006197080005d),.INIT_RAM_39(288'h09e20008148000000000000080000000080001ce0100001c58000001bd00000800000071),.INIT_RAM_3A(288'h97048000187008000176c180002758100002002f8000271018000274c10000200b400002),.INIT_RAM_3B(288'h90000000185f000001794100002002f8000200380000280000006590000006197088005d),.INIT_RAM_3C(288'h95f20006995f00000185ee000017bc18000200000100000000000001c500000800000069),.INIT_RAM_3D(288'h48000000000000080000000080001ce0100001c58000001bd0000080000007190000006d),.INIT_RAM_3E(288'h8701000017f01800027dc100002002f800027701800027d010000200b40000209e380081),.INIT_RAM_3F(288'h85f200001818100002002f8000200380000280000006590000006197090005d970500001)) code_rom0(.DO(code0),.AD({fetch_pc[8:0],5'b0}),.CLK(clk),.CE(run_ce && (busy || start)),.OCE(1'b1),.RESET(reset));
wire [35:0] code1;
pROMX9 #(.BIT_WIDTH(36),.READ_MODE(0),.INIT_RAM_00(288'h95f00000185ee0000184018000200000100000000000001cd00000800000069900000001),.INIT_RAM_01(288'h00000080000000080001ce0100001c58000001bd0000080000007190000006d95f200069),.INIT_RAM_02(288'h08ed8018108e02e1810000660007f418000285410000200b40000209e500081480000000),.INIT_RAM_03(288'he4e380001e4e300001e4e280001e4e200001e4e180001e4e100001e4e080001e4e000001),.INIT_RAM_04(288'he4e780001e4e700001e4e680001e4e600001e4e580001e4e500001e4e480001e4e400001),.INIT_RAM_05(288'he4eb80001e4eb00001e4ea80001e4ea00001e4e980001e4e900001e4e880001e4e800001),.INIT_RAM_06(288'h00002e0000002000028fc100002003b80002e4ed80201e4ed00001e4ec80001e4ec00001),.INIT_RAM_07(288'h3c00000039101800020002000028fc100002003b80002e4e00020168e0001bd0000001fc),.INIT_RAM_08(288'h9701000459700800018700000012000000030002000027a00000032d4180002908100002),.INIT_RAM_09(288'h68c18019168c10018d68c08018968c0001850000001d000002e00080000004d900000049),.INIT_RAM_0A(288'h68c2001850000001d000003c00080000004d900000049970500045970480001870400001),.INIT_RAM_0B(288'h80000004d90000004997090004597088000187080000168c38019168c30018d68c280189),.INIT_RAM_0C(288'h26000000322000000368c58019168c50018d68c48018968c4001850000001d0000020000),.INIT_RAM_0D(288'h0000001e800000e0008dba8000080000005d90000005996c10005596c08000186c000001),.INIT_RAM_0E(288'h6e00000039e410000200eb8000299c1800029d41000022c0000003001f80002008500000),.INIT_RAM_0F(288'h90000004d96c18004996c10004596c08000186c000001240000003220000003d18180002),.INIT_RAM_10(288'h80000006190000005d96c18005996c10005596c08000186c000001260000003800000051),.INIT_RAM_11(288'h0033800020085000000000001e800000e0008dba800000000001e400000e0008d9880000),.INIT_RAM_12(288'h000004000838a80000ccc100002001b800022886800030000001f400002800001d480000),.INIT_RAM_13(288'h000004000839300000b14100002005b80002008680000abc1000022886800030000001f4),.INIT_RAM_14(288'h000004000839b80000b14100002005b80002008680000abc1000022886800030000001f4),.INIT_RAM_15(288'h008080000b14180002b14100002005b80002008680000abc1000022886800030000001f4),.INIT_RAM_16(288'h0081880000086801c80000081f40083081d40081080000086801c40000081f4008288000),.INIT_RAM_17(288'h0084880000086801d00000081f40084081dc0082080000086801cc0000081f40083881d8),.INIT_RAM_18(288'h00004e1eccdc80400083d4800000000001e80000080000086801e40000081f40085081e0),.INIT_RAM_19(288'ha00000075b00000001a41b000013885800030085800000000001ec00004e000cdd8001e4),.INIT_RAM_1A(288'h41ef800030000001f400004a0000000001fc0000481f80086c6000008680000008680000),.INIT_RAM_1B(288'h01f4800000000001e40000301f800848400083e7000000000001700c0000003420000003),.INIT_RAM_1C(288'h01e6800000000001f40000041f0838a9e0000086000000000000f0580000000000000000),.INIT_RAM_1D(288'h0000001f40000041c483930200081e8800000000000f4500000000000001000000000000),.INIT_RAM_1E(288'h0000041c8839b8200081e9000000000000f450000000000000100000000000001e680000),.INIT_RAM_1F(288'hbfa40200081e9800000000000f450000000000000100000000000001e6800000000001f4),.INIT_RAM_20(288'h85ea0000000000017410000000342000000341f6800030000001f80000121f401e6401cc),.INIT_RAM_21(288'h0000001cc00001a000c44100002002b80002c54100002003f800020000001d0000006000),.INIT_RAM_22(288'h0000001c80000100008f9800000c601800020000001c40000100008f9800000c60180002),.INIT_RAM_23(288'hc98100002006b800020000001fc00000a00089f0000000000001f8000016000008180000),.INIT_RAM_24(288'h6ac0001856800000030000001c400001400093f0800000000001c400001c0009bf880000),.INIT_RAM_25(288'h96c10005596c08000186c0000012600000032a00000036ac1801916ac10018d6ac080189),.INIT_RAM_26(288'h6ac0801996ac000195680000003cec10000200378000280000006190000005d96c180059),.INIT_RAM_27(288'h0000081cc0084081c80083881c40083080000082800002a00000036ac1801a16ac10019d),.INIT_RAM_28(288'hd2410000200ab800028dc1000022e0000003a04180002d101000022c00000030000001d0),.INIT_RAM_29(288'h90000004d96c18004996c10004596c08000186c000001300000003620000003998180002),.INIT_RAM_2A(288'h008680000008680000a00000075b00000001a41b00001388180003008180000800000051),.INIT_RAM_2B(288'h0c000000342000000341ef800030000001f400004a0000000001fc0000481d80086c6000),.INIT_RAM_2C(288'h0083000000000001d800004c0000083000000000001d800000400083db0000000000016c),.INIT_RAM_2D(288'h00000100000000000001b0801f80000240000000001f40000220000000001dc00005e000),.INIT_RAM_2E(288'h00000200081c68000000000016016000000302000000301ec000000000000e0700000000),.INIT_RAM_2F(288'h16000000302000000301f4000000000000e070000000000000100000000000001b1001e4),.INIT_RAM_30(288'h00000000001b8801ec00002c1c801d4f01c40082441e800820400083c700000000000160),.INIT_RAM_31(288'h6ac0001ad0000000e448000000000000100000000000001b9000e0480000000000001000),.INIT_RAM_32(288'h2d41800023a0000003d28180002e581000023200000036ac1801a56ac1001a16ac08019d),.INIT_RAM_33(288'h0000001f000003c0000000001c80000240000000001c40000220000000001c000002e000),.INIT_RAM_34(288'h00000000017c180002000380002000b800020086800000000001f4000032000a41680002),.INIT_RAM_35(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_36(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_37(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_38(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_39(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3A(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3B(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3C(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3D(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3E(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3F(288'h000000000000000000000000000000000000000000000000000000000000000000000000)) code_rom1(.DO(code1),.AD({fetch_pc[8:0],5'b0}),.CLK(clk),.CE(run_ce && (busy || start)),.OCE(1'b1),.RESET(reset));
reg [35:0] selected_code;always @* begin selected_code=code0;case(pc[10:9])
0:selected_code=code0;
1:selected_code=code1;
default:begin end endcase end
assign code=selected_code;


geometry_data data(.position_prepare(busy && transport18[12]),.compact_prepare(transport18[13]),.position_alu(position_ahead),.clip_normalize_ahead(clip_ahead),.normalize_exp(lookup_exp),.normalize_lookup(lookup_ahead),.lookup_normalized(lookup_normalized),.compact_ingress(compact_ingress),.transport_publish(transport_publish),.compact_attribute_value(compact_attribute_value),.compact_alu(compact_alu),.compact_position(compact_position),.compact_normal(compact_normal),.normal_issue(normal_issue),.normal_publish(normal_publish),.normal_address(imm[3:0]),.uniform_slot(uniform_slot),.uniform_issue(uniform_issue),.uniform_n(uniform_n),.uniform_store(uniform_store),.uniform_pack(imm[8]),.uniform_address(uniform_address),.uniform_value(uniform_value),.alu_flags(alu_flags),.clip_plane(clip_plane),.direct_store(direct_store),.edge_orientation(orientation),.color_mode(cp_color),.rcp_table(rcp_table),.rcp_fraction(rcp_fraction),.rcp_exponent(rcp_exp),.vertex_grid(grid_shift),.vertex_axis(decode_axis),.vertex_normal(normal_pack),.alu_input_a(alu_input_a),.projection_reciprocal(rcp_q),.projection_deferred(deferred),.projection_far(far_clamped),.clk(clk),.ce(dp_ce),.reset(reset),.write_enable(write_enable),.write_address({5'b0,dst}),.cold_write_address(ca),.ingress_data(busy ? ingress_word : load_data),.write_select(busy ? {1'b0,sel} : 3'b0),.write_high(hi),.read_enable({1'b0,(busy && cr && (im<5 || im==7)) || (!busy && inspect_enable),busy && issue18[9],busy && issue18[4]}),.read_a({5'b0,issued_a}),.read_b({5'b0,issue18[8:5]}),.read_n_a(sr),.read_n_b(9'b0),.w_op((busy && !uniform_n) ? wo : 3'b0),.n_op(no),.n_unsigned(fmt==3 && aux<16 && arg[0]),.n_seed_f16(ns),.n_capture(nc),.alu_mode(alu),.alu_ce(busy && alu!=0),.w_capture_enable((busy && fmt==0 && !prepare_alu && code[34]) || uniform_capture),.n_capture_enable(ne),.w_unsigned(fmt==0 && !prepare_alu ? code[30:29] : 2'b0),.ingress_enable((busy && ig) || prefetch_ingress || (!busy && load)),.ingress_mode(3'd4),.ingress_channel(2'b0),.w_capture_mode(fmt==0 ? code[33:31] : 3'b0),.normal_pack_enable(1'b0),.normal_pack_channel(2'b0),.round_up(guard && (sticky || alu_input_a[0])),.exponent(norm_shift),.w_product(w_product),.w_acc(w_acc),.w_buffer(w_buffer),.n_product0(n_product0),.n_product1(n_product1),.n_acc(n_acc),.n_buffer(n_buffer),.alu_buffer(alu_buffer),.write_data(write_data),.read_data(read_data),.output_read(output_read),.carry(carry));
assign query_cold_base=cold_base;
assign query_cold_mode=cold_mode;
assign query_cr=cr;
assign query_im=im;
assign query_transport18=transport18;
assign query_prefetch_next=prefetch_next;

endmodule
/* synthesis syn_dspstyle = "logic" */
module geometry_data(
input wire position_prepare,compact_prepare,
input wire position_alu,clip_normalize_ahead,
input wire [4:0] normalize_exp,
input wire normalize_lookup,
output wire [31:0] lookup_normalized,
output wire [35:0] compact_ingress,
input wire transport_publish,compact_alu,
input wire [35:0] compact_attribute_value,
input wire [31:0] compact_position,compact_normal,
input wire normal_issue,normal_publish,
input wire [3:0] normal_address,
input wire uniform_slot,uniform_issue,uniform_n,uniform_store,uniform_pack,
input wire [4:0] uniform_address,
output wire [35:0] uniform_value,
input wire [5:0] alu_flags,
input wire [2:0] clip_plane,
input wire direct_store,
input wire edge_orientation,color_mode,
input wire [35:0] rcp_table,
input wire [17:0] rcp_fraction,
input wire [4:0] rcp_exponent,
input wire [4:0] vertex_grid,
input wire [1:0] vertex_axis,
input wire [35:0] vertex_normal,
output wire [35:0] alu_input_a,
input wire [35:0] projection_reciprocal,
input wire projection_deferred,projection_far,
input wire clk,
input wire ce,
input wire reset,
input wire [4:0] write_enable,
input wire [8:0] write_address,
input wire [9:0] cold_write_address,
input wire [35:0] ingress_data,
input wire ingress_enable,
input wire [2:0] ingress_mode,
input wire [1:0] ingress_channel,
input wire [2:0] write_select,
input wire write_high,
input wire [3:0] read_enable,
input wire [8:0] read_a,
input wire [8:0] read_b,
input wire [9:0] read_n_a,
input wire [8:0] read_n_b,
input wire [2:0] w_op,
input wire [2:0] n_op,
input wire n_unsigned,
input wire n_seed_f16,
input wire [2:0] n_capture,
input wire [5:0] alu_mode,
input wire round_up,
input wire [6:0] exponent,
input wire [1:0] w_unsigned,
input wire alu_ce,
input wire w_capture_enable,
input wire [2:0] w_capture_mode,
input wire normal_pack_enable,
input wire [1:0] normal_pack_channel,
input wire n_capture_enable,
output wire [71:0] w_product,
(* syn_preserve=1, syn_keep=1 *) output wire [71:0] w_acc,
(* syn_preserve=1, syn_keep=1 *) output wire [71:0] w_buffer,
output wire [35:0] n_product0,
output wire [35:0] n_product1,
output reg [35:0] n_acc,
output reg [35:0] n_buffer,
output reg [35:0] alu_buffer,
output reg carry,
output wire [35:0] write_data,
output wire [143:0] read_data,
output wire [35:0] output_read
);
reg [53:0] w_state;reg [35:0] w_bufstate;
assign w_acc={{18{w_state[53]}},w_state};
assign w_buffer={36'b0,w_bufstate};

wire [35:0] qa,qb,qna,qnb;
assign qna=qa;assign qnb=qb;
reg [3:0] rf_a_held;
wire [3:0] rf_a_rad=read_enable[0] ? read_a[3:0] : rf_a_held;
always @(posedge clk)begin if(reset)rf_a_held<=0;else if(ce && read_enable[0])rf_a_held<=read_a[3:0];end
wire [35:0] rf_a_do;
wire [35:0] rf_a_bank0;
RAM16SDP4 r_a_0_0(.DO(rf_a_bank0[0 +: 4]),.DI(write_data[0 +: 4]),.WAD(write_address[3:0]),.RAD(rf_a_rad),.WRE(ce && !reset && write_enable[0] && 1'b1),.CLK(clk));
RAM16SDP4 r_a_0_1(.DO(rf_a_bank0[4 +: 4]),.DI(write_data[4 +: 4]),.WAD(write_address[3:0]),.RAD(rf_a_rad),.WRE(ce && !reset && write_enable[0] && 1'b1),.CLK(clk));
RAM16SDP4 r_a_0_2(.DO(rf_a_bank0[8 +: 4]),.DI(write_data[8 +: 4]),.WAD(write_address[3:0]),.RAD(rf_a_rad),.WRE(ce && !reset && write_enable[0] && 1'b1),.CLK(clk));
RAM16SDP4 r_a_0_3(.DO(rf_a_bank0[12 +: 4]),.DI(write_data[12 +: 4]),.WAD(write_address[3:0]),.RAD(rf_a_rad),.WRE(ce && !reset && write_enable[0] && 1'b1),.CLK(clk));
RAM16SDP4 r_a_0_4(.DO(rf_a_bank0[16 +: 4]),.DI(write_data[16 +: 4]),.WAD(write_address[3:0]),.RAD(rf_a_rad),.WRE(ce && !reset && write_enable[0] && 1'b1),.CLK(clk));
RAM16SDP4 r_a_0_5(.DO(rf_a_bank0[20 +: 4]),.DI(write_data[20 +: 4]),.WAD(write_address[3:0]),.RAD(rf_a_rad),.WRE(ce && !reset && write_enable[0] && 1'b1),.CLK(clk));
RAM16SDP4 r_a_0_6(.DO(rf_a_bank0[24 +: 4]),.DI(write_data[24 +: 4]),.WAD(write_address[3:0]),.RAD(rf_a_rad),.WRE(ce && !reset && write_enable[0] && 1'b1),.CLK(clk));
RAM16SDP4 r_a_0_7(.DO(rf_a_bank0[28 +: 4]),.DI(write_data[28 +: 4]),.WAD(write_address[3:0]),.RAD(rf_a_rad),.WRE(ce && !reset && write_enable[0] && 1'b1),.CLK(clk));
RAM16SDP4 r_a_0_8(.DO(rf_a_bank0[32 +: 4]),.DI(write_data[32 +: 4]),.WAD(write_address[3:0]),.RAD(rf_a_rad),.WRE(ce && !reset && write_enable[0] && 1'b1),.CLK(clk));
assign rf_a_do=rf_a_bank0;
assign qa=rf_a_do;
reg [3:0] rf_b_held;
wire [3:0] rf_b_rad=read_enable[1] ? read_b[3:0] : rf_b_held;
always @(posedge clk)begin if(reset)rf_b_held<=0;else if(ce && read_enable[1])rf_b_held<=read_b[3:0];end
wire [35:0] rf_b_do;
wire [35:0] rf_b_bank0;
RAM16SDP4 r_b_0_0(.DO(rf_b_bank0[0 +: 4]),.DI(write_data[0 +: 4]),.WAD(write_address[3:0]),.RAD(rf_b_rad),.WRE(ce && !reset && write_enable[1] && 1'b1),.CLK(clk));
RAM16SDP4 r_b_0_1(.DO(rf_b_bank0[4 +: 4]),.DI(write_data[4 +: 4]),.WAD(write_address[3:0]),.RAD(rf_b_rad),.WRE(ce && !reset && write_enable[1] && 1'b1),.CLK(clk));
RAM16SDP4 r_b_0_2(.DO(rf_b_bank0[8 +: 4]),.DI(write_data[8 +: 4]),.WAD(write_address[3:0]),.RAD(rf_b_rad),.WRE(ce && !reset && write_enable[1] && 1'b1),.CLK(clk));
RAM16SDP4 r_b_0_3(.DO(rf_b_bank0[12 +: 4]),.DI(write_data[12 +: 4]),.WAD(write_address[3:0]),.RAD(rf_b_rad),.WRE(ce && !reset && write_enable[1] && 1'b1),.CLK(clk));
RAM16SDP4 r_b_0_4(.DO(rf_b_bank0[16 +: 4]),.DI(write_data[16 +: 4]),.WAD(write_address[3:0]),.RAD(rf_b_rad),.WRE(ce && !reset && write_enable[1] && 1'b1),.CLK(clk));
RAM16SDP4 r_b_0_5(.DO(rf_b_bank0[20 +: 4]),.DI(write_data[20 +: 4]),.WAD(write_address[3:0]),.RAD(rf_b_rad),.WRE(ce && !reset && write_enable[1] && 1'b1),.CLK(clk));
RAM16SDP4 r_b_0_6(.DO(rf_b_bank0[24 +: 4]),.DI(write_data[24 +: 4]),.WAD(write_address[3:0]),.RAD(rf_b_rad),.WRE(ce && !reset && write_enable[1] && 1'b1),.CLK(clk));
RAM16SDP4 r_b_0_7(.DO(rf_b_bank0[28 +: 4]),.DI(write_data[28 +: 4]),.WAD(write_address[3:0]),.RAD(rf_b_rad),.WRE(ce && !reset && write_enable[1] && 1'b1),.CLK(clk));
RAM16SDP4 r_b_0_8(.DO(rf_b_bank0[32 +: 4]),.DI(write_data[32 +: 4]),.WAD(write_address[3:0]),.RAD(rf_b_rad),.WRE(ce && !reset && write_enable[1] && 1'b1),.CLK(clk));
assign rf_b_do=rf_b_bank0;
assign qb=rf_b_do;
wire [35:0] result_bank0,result_bank1;reg result_read_bank;
assign output_read=result_read_bank?result_bank1:result_bank0;
always @(posedge clk)if(reset)result_read_bank<=0;else if(ce && read_enable[2])result_read_bank<=read_n_a[9];
SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(0),.BLK_SEL_0(0),.BLK_SEL_1(0),.RESET_MODE("SYNC")) output_bank0(
.DO(result_bank0),.DI(write_data),.ADA({cold_write_address[8:0],1'b0,4'b1111}),.ADB({read_n_a[8:0],5'b00000}),
.BLKSELA(3'b000),.BLKSELB(3'b000),.CLKA(clk),.CLKB(clk),.CEA(ce && !reset && write_enable[4] && cold_write_address[9]==1'b0),.CEB(ce && read_enable[2] && read_n_a[9]==1'b0),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));
SDPX9B #(.BIT_WIDTH_0(36),.BIT_WIDTH_1(36),.READ_MODE(0),.BLK_SEL_0(0),.BLK_SEL_1(0),.RESET_MODE("SYNC")) output_bank1(
.DO(result_bank1),.DI(write_data),.ADA({cold_write_address[8:0],1'b0,4'b1111}),.ADB({read_n_a[8:0],5'b00000}),
.BLKSELA(3'b000),.BLKSELB(3'b000),.CLKA(clk),.CLKB(clk),.CEA(ce && !reset && write_enable[4] && cold_write_address[9]==1'b1),.CEB(ce && read_enable[2] && read_n_a[9]==1'b1),.OCE(1'b1),.RESETA(1'b0),.RESETB(reset));

wire [35:0] uniform_low,uniform_high;
wire [35:0] w_b=uniform_issue?uniform_value:qb;
wire [35:0] n_b=normal_issue?uniform_high:(uniform_issue?uniform_value:qnb);
wire [35:0] n_a=normal_issue?qb:qna;
reg [35:0] uniform_input;
always @* begin
uniform_input=write_data;
if(uniform_pack)begin
if(uniform_address[0])uniform_input={18'b0,{2{write_data[15]}},write_data[15:0]};
else uniform_input={{2{write_data[31]}},write_data[31:16],{2{write_data[15]}},write_data[15:0]};
end
end

wire [3:0] uniform_w_address=uniform_address[4]?(uniform_address[3:0]-4'd6):uniform_address[3:0];
wire [3:0] uniform_n_write_address=uniform_address[3:0]-4'd12;
wire normal_store=uniform_address>=12 && uniform_address<18;

RAM16SDP4 u_0_0(.DO(uniform_low[0 +: 4]),.DI(uniform_input[0 +: 4]),.WAD(uniform_w_address),.RAD(uniform_w_address),.WRE(ce && !reset && uniform_store && !normal_store),.CLK(clk));
RAM16SDP4 u_0_1(.DO(uniform_low[4 +: 4]),.DI(uniform_input[4 +: 4]),.WAD(uniform_w_address),.RAD(uniform_w_address),.WRE(ce && !reset && uniform_store && !normal_store),.CLK(clk));
RAM16SDP4 u_0_2(.DO(uniform_low[8 +: 4]),.DI(uniform_input[8 +: 4]),.WAD(uniform_w_address),.RAD(uniform_w_address),.WRE(ce && !reset && uniform_store && !normal_store),.CLK(clk));
RAM16SDP4 u_0_3(.DO(uniform_low[12 +: 4]),.DI(uniform_input[12 +: 4]),.WAD(uniform_w_address),.RAD(uniform_w_address),.WRE(ce && !reset && uniform_store && !normal_store),.CLK(clk));
RAM16SDP4 u_0_4(.DO(uniform_low[16 +: 4]),.DI(uniform_input[16 +: 4]),.WAD(uniform_w_address),.RAD(uniform_w_address),.WRE(ce && !reset && uniform_store && !normal_store),.CLK(clk));
RAM16SDP4 u_0_5(.DO(uniform_low[20 +: 4]),.DI(uniform_input[20 +: 4]),.WAD(uniform_w_address),.RAD(uniform_w_address),.WRE(ce && !reset && uniform_store && !normal_store),.CLK(clk));
RAM16SDP4 u_0_6(.DO(uniform_low[24 +: 4]),.DI(uniform_input[24 +: 4]),.WAD(uniform_w_address),.RAD(uniform_w_address),.WRE(ce && !reset && uniform_store && !normal_store),.CLK(clk));
RAM16SDP4 u_0_7(.DO(uniform_low[28 +: 4]),.DI(uniform_input[28 +: 4]),.WAD(uniform_w_address),.RAD(uniform_w_address),.WRE(ce && !reset && uniform_store && !normal_store),.CLK(clk));
RAM16SDP4 u_0_8(.DO(uniform_low[32 +: 4]),.DI(uniform_input[32 +: 4]),.WAD(uniform_w_address),.RAD(uniform_w_address),.WRE(ce && !reset && uniform_store && !normal_store),.CLK(clk));
RAM16SDP4 u_1_0(.DO(uniform_high[0 +: 4]),.DI(uniform_input[0 +: 4]),.WAD(uniform_n_write_address),.RAD(normal_address),.WRE(ce && !reset && uniform_store && normal_store),.CLK(clk));
RAM16SDP4 u_1_1(.DO(uniform_high[4 +: 4]),.DI(uniform_input[4 +: 4]),.WAD(uniform_n_write_address),.RAD(normal_address),.WRE(ce && !reset && uniform_store && normal_store),.CLK(clk));
RAM16SDP4 u_1_2(.DO(uniform_high[8 +: 4]),.DI(uniform_input[8 +: 4]),.WAD(uniform_n_write_address),.RAD(normal_address),.WRE(ce && !reset && uniform_store && normal_store),.CLK(clk));
RAM16SDP4 u_1_3(.DO(uniform_high[12 +: 4]),.DI(uniform_input[12 +: 4]),.WAD(uniform_n_write_address),.RAD(normal_address),.WRE(ce && !reset && uniform_store && normal_store),.CLK(clk));
RAM16SDP4 u_1_4(.DO(uniform_high[16 +: 4]),.DI(uniform_input[16 +: 4]),.WAD(uniform_n_write_address),.RAD(normal_address),.WRE(ce && !reset && uniform_store && normal_store),.CLK(clk));
RAM16SDP4 u_1_5(.DO(uniform_high[20 +: 4]),.DI(uniform_input[20 +: 4]),.WAD(uniform_n_write_address),.RAD(normal_address),.WRE(ce && !reset && uniform_store && normal_store),.CLK(clk));
RAM16SDP4 u_1_6(.DO(uniform_high[24 +: 4]),.DI(uniform_input[24 +: 4]),.WAD(uniform_n_write_address),.RAD(normal_address),.WRE(ce && !reset && uniform_store && normal_store),.CLK(clk));
RAM16SDP4 u_1_7(.DO(uniform_high[28 +: 4]),.DI(uniform_input[28 +: 4]),.WAD(uniform_n_write_address),.RAD(normal_address),.WRE(ce && !reset && uniform_store && normal_store),.CLK(clk));
RAM16SDP4 u_1_8(.DO(uniform_high[32 +: 4]),.DI(uniform_input[32 +: 4]),.WAD(uniform_n_write_address),.RAD(normal_address),.WRE(ce && !reset && uniform_store && normal_store),.CLK(clk));
assign uniform_value=uniform_low;
MULT36X36 #(.AREG(0),.BREG(0),.ASIGN_REG(0),.BSIGN_REG(0),.PIPE_REG(1),.OUT0_REG(1),.OUT1_REG(0),.MULT_RESET_MODE("SYNC")) wide(
.A(qa),.B(w_b),.ASIGN(!w_unsigned[0]),.BSIGN(!w_unsigned[1]),.CE(ce),.CLK(clk),.RESET(reset),.DOUT(w_product));
MULT18X18 #(.AREG(0),.BREG(0),.ASIGN_REG(0),.BSIGN_REG(0),.PIPE_REG(0),.OUT_REG(1),.MULT_RESET_MODE("SYNC")) narrow0(
.A(n_a[17:0]),.B(n_b[17:0]),.SIA(18'b0),.SIB(18'b0),.ASEL(1'b0),.BSEL(1'b0),.SOA(),.SOB(),.ASIGN(1'b1),.BSIGN(!n_unsigned),.CE(ce),.CLK(clk),.RESET(reset),.DOUT(n_product0));
MULT18X18 #(.AREG(0),.BREG(0),.ASIGN_REG(0),.BSIGN_REG(0),.PIPE_REG(0),.OUT_REG(1),.MULT_RESET_MODE("SYNC")) narrow1(
.A(n_a[35:18]),.B(n_b[35:18]),.SIA(18'b0),.SIB(18'b0),.ASEL(1'b0),.BSEL(1'b0),.SOA(),.SOB(),.ASIGN(1'b1),.BSIGN(!n_unsigned),.CE(ce),.CLK(clk),.RESET(reset),.DOUT(n_product1));
wire signed [71:0] w_add=$signed(w_acc)+$signed(w_product);

wire signed [36:0] n_sum=$signed(n_product0)+$signed(n_product1);
wire signed [36:0] n_add=$signed(n_acc)+n_sum;
wire signed [35:0] n_seed0={{18{qna[17]}},qna[17:0]};
wire signed [35:0] n_seed16={{2{qna[17]}},qna[17:0],16'b0};
(* syn_preserve=1, syn_keep=1 *) reg [35:0] alu_qa,alu_qb;
wire [35:0] grid_shifted;
assign alu_input_a=alu_qa;
always @(posedge clk)begin
if(reset)begin alu_qa<=0;alu_qb<=0;end
else if(ce)begin if(read_enable[0])alu_qa<=qa;if(position_prepare)alu_qa<=grid_shifted;if(uniform_issue)alu_qb<=uniform_value;else if(read_enable[1])alu_qb<=qb;end
end
wire packed_math=(!position_alu) && alu_flags[1];
wire alu_sub=(!position_alu) && (alu_flags[0] || (alu_flags[2] && alu_qb[35]));
reg [35:0] math_a,math_b;
always @* begin math_a=alu_qa;math_b=alu_qb;
if(alu_flags[4])begin if(clip_plane==0)begin math_a=alu_qa;math_b=36'd8199;end else begin math_a=alu_qb<<3;math_b=alu_qa;end end
if(alu_flags[5])begin math_a=0;math_b=alu_qb<<3;end
if(position_alu)begin math_a=alu_qa;math_b=alu_qb;end
end
wire [35:0] alu_rhs=alu_sub ? ~math_b : math_b;
wire alu_cin=alu_sub;
wire [18:0] lo_sum={1'b0,math_a[17:0]}+{1'b0,alu_rhs[17:0]}+alu_cin;
wire hi_cin=packed_math ? alu_sub : lo_sum[18];
wire [18:0] hi_sum={1'b0,math_a[35:18]}+{1'b0,alu_rhs[35:18]}+hi_cin;
wire [36:0] alu_sum={hi_sum,lo_sum[17:0]};
wire lt_lo=(alu_qa[17]!=alu_qb[17]) ? alu_qa[17] : lo_sum[17];
wire lt_hi=(alu_qa[35]!=alu_qb[35]) ? alu_qa[35] : hi_sum[17];
wire signed [17:0] packed_lo_floor=$signed(alu_qa[17:0])>>>2;
wire signed [17:0] packed_hi_floor=$signed(alu_qa[35:18])>>>2;
wire [31:0] compact_decode_input=compact_alu?compact_normal:alu_qa[31:0];
reg [9:0] vertex10;
always @* begin vertex10=compact_decode_input[9:0];case(vertex_axis)
1:vertex10=compact_decode_input[19:10];2:vertex10=compact_decode_input[29:20];default:begin end endcase end
reg [9:0] position10;
always @*begin position10=compact_position[9:0];case(vertex_axis)
1:position10=compact_position[19:10];2:position10=compact_position[29:20];default:begin end endcase end
wire [9:0] position_input10=compact_alu?position10:vertex10;
wire [9:0] prepare_position10=compact_prepare?position10:vertex10;
wire [35:0] grid_operand=position_prepare?{26'b0,prepare_position10}:alu_qa;
assign grid_shifted=grid_operand<<vertex_grid;
wire [17:0] normal_low={{3{vertex10[9]}},vertex10,5'b0};
wire [17:0] normal_y={{3{compact_decode_input[19]}},compact_decode_input[19:10],5'b0};
wire [35:0] normal_pair={normal_y,normal_low};
wire [35:0] normal_z={18'b0,normal_low};
wire signed [35:0] low_quarter=$signed({{20{alu_qa[15]}},alu_qa[15:0]})>>>2;
wire signed [35:0] high_quarter=$signed({{20{alu_qa[31]}},alu_qa[31:16]})>>>2;
wire signed [35:0] pixel_floor=$signed(alu_qa)>>>2;
wire signed [35:0] edge_floor=$signed(alu_qa)>>>exponent[3:0];
wire signed [35:0] neg_qa=-$signed(alu_qa);
wire signed [8:0] depth_exponent=$signed({4'b0,vertex_grid})+9'sd3-$signed({2'b0,exponent});
wire [31:0] clip_num_normalized;
wire [18:0] clip_num_rounded={1'b0,clip_num_normalized[31:14]}+(clip_num_normalized[13] && ((|clip_num_normalized[12:0]) || clip_num_normalized[14]));
wire scalar_lt=(alu_qa[35]!=alu_qb[35])?alu_qa[35]:alu_sum[35];
wire minmax_lo=(packed_math?lt_lo:scalar_lt)^alu_flags[3];
wire minmax_hi=(packed_math?lt_hi:scalar_lt)^alu_flags[3];
wire [35:0] minmax_result={minmax_hi?alu_qa[35:18]:alu_qb[35:18],minmax_lo?alu_qa[17:0]:alu_qb[17:0]};
wire [17:0] rcp_lerp18=alu_qa[17:0]-n_buffer[17:0];
wire [17:0] rcp_scale_input=(alu_mode==6)?rcp_lerp18:alu_qa[17:0];
wire clip_normalize=clip_normalize_ahead;
wire [35:0] normalize_operand=(normalize_lookup || clip_normalize)?{4'b0,alu_qa[31:0]}:{18'b0,rcp_scale_input};
wire [4:0] normalize_exponent=normalize_lookup?normalize_exp:rcp_exponent;
wire [4:0] normalize_shift=(normalize_lookup || clip_normalize)?5'd31-normalize_exponent:5'd30-normalize_exponent;
wire [35:0] normalize_result=normalize_operand<<normalize_shift;
assign lookup_normalized=normalize_result[31:0];
assign clip_num_normalized=normalize_result[31:0];
wire [35:0] rcp_scaled=normalize_result;
wire alu_select_0=alu_mode==6'd0 || alu_mode==6'd1 || alu_mode==6'd2 || alu_mode==6'd3 || alu_mode==6'd5 || alu_mode==6'd7 || alu_mode==6'd8 || alu_mode==6'd12 || alu_mode==6'd25 || alu_mode==6'd32 || alu_mode==6'd39 || alu_mode==6'd61;
wire [35:0] alu_value_0=alu_sum[35:0];
wire alu_select_1=alu_mode==6'd4;
wire [35:0] alu_value_1=alu_qa;
wire alu_select_2=alu_mode==6'd6 || alu_mode==6'd38;
wire [35:0] alu_value_2=rcp_scaled;
wire alu_select_3=alu_mode==6'd9;
wire [35:0] alu_value_3={alu_qb[17:0],alu_qa[17:0]};
wire alu_select_4=alu_mode==6'd16;
wire [35:0] alu_value_4=36'h400000000;
wire alu_select_5=alu_mode==6'd17;
wire [35:0] alu_value_5=36'd3200;
wire alu_select_6=alu_mode==6'd18;
wire [35:0] alu_value_6=36'd1920;
wire alu_select_7=alu_mode==6'd19;
wire [35:0] alu_value_7=projection_reciprocal;
wire alu_select_8=alu_mode==6'd20;
wire [35:0] alu_value_8={35'b0,alu_qa[35]^alu_qb[35]};
wire alu_select_9=alu_mode==6'd21;
wire [35:0] alu_value_9={2'b0,projection_far,projection_deferred,alu_qb[15:0],alu_qa[15:0]};
wire alu_select_10=alu_mode==6'd23;
wire [35:0] alu_value_10=0;
wire alu_select_11=alu_mode==6'd26;
reg [35:0] alu_value_11;always @* begin if(vertex_axis==2)alu_value_11=normal_z;else alu_value_11=normal_pair;end
wire alu_select_12=alu_mode==6'd27;
wire [35:0] alu_value_12={12'b0,alu_qa[23:0]};
wire alu_select_13=alu_mode==6'd28;
wire [35:0] alu_value_13=vertex_normal;
wire alu_select_14=alu_mode==6'd30;
wire [35:0] alu_value_14=36'd65536;
wire alu_select_15=alu_mode==6'd31;
wire [35:0] alu_value_15={{2{alu_qa[31]}},alu_qa[31:16],{2{alu_qa[15]}},alu_qa[15:0]};
wire alu_select_16=alu_mode==6'd33;
wire [35:0] alu_value_16={high_quarter[17:0],low_quarter[17:0]};
wire alu_select_17=alu_mode==6'd34;
wire [35:0] alu_value_17={{18{alu_qa[17]}},alu_qa[17:0]};
wire alu_select_18=alu_mode==6'd35;
wire [35:0] alu_value_18={18'b0,alu_qa[17:0]};
wire alu_select_19=alu_mode==6'd36;
wire [35:0] alu_value_19={26'b0,alu_qa[27:18]};
wire alu_select_20=alu_mode==6'd37;
wire [35:0] alu_value_20={18'b0,rcp_fraction};
wire alu_select_21=alu_mode==6'd40;
wire [35:0] alu_value_21=minmax_result;
wire alu_select_22=alu_mode==6'd41;
wire [35:0] alu_value_22=minmax_result;
wire alu_select_23=alu_mode==6'd42;
wire [35:0] alu_value_23={packed_hi_floor,packed_lo_floor};
wire alu_select_24=alu_mode==6'd43;
wire [35:0] alu_value_24={alu_qa[33:18],2'b0,alu_qa[15:0],2'b0};
wire alu_select_25=alu_mode==6'd44;
wire [35:0] alu_value_25=alu_qa<<4;
wire alu_select_26=alu_mode==6'd45;
wire [35:0] alu_value_26=grid_shifted;
wire alu_select_27=alu_mode==6'd46;
wire [35:0] alu_value_27=alu_qa<<1;
wire alu_select_28=alu_mode==6'd47;
wire [35:0] alu_value_28=alu_qa>>11;
wire alu_select_29=alu_mode==6'd48;
wire [35:0] alu_value_29={2'b0,alu_qb[25:18],alu_qa[25:18],alu_qb[8:0],alu_qa[8:0]};
wire alu_select_30=alu_mode==6'd49;
reg [35:0] alu_value_30;always @* begin if(alu_qa[17:0]>alu_qa[35:18])alu_value_30={18'b0,alu_qa[17:0]};else alu_value_30={18'b0,alu_qa[35:18]};end
wire alu_select_31=alu_mode==6'd50;
wire [35:0] alu_value_31=36'h080000000;
wire alu_select_32=alu_mode==6'd51;
wire [35:0] alu_value_32=36'd9 | {34'b0,edge_orientation,1'b0} | {30'b0,color_mode,5'b0};
wire alu_select_33=alu_mode==6'd52;
wire [35:0] alu_value_33=36'd33;
wire alu_select_34=alu_mode==6'd53;
wire [35:0] alu_value_34=36'h000040001;
wire alu_select_35=alu_mode==6'd54;
wire [35:0] alu_value_35=36'h0000C0003;
wire alu_select_36=alu_mode==6'd55;
wire [35:0] alu_value_36=(36'd239<<18)|36'd399;
wire alu_select_37=alu_mode==6'd56;
wire [35:0] alu_value_37={{18{alu_qa[35]}},alu_qa[35:18]};
wire alu_select_38=alu_mode==6'd57;
wire [35:0] alu_value_38={{27{depth_exponent[8]}},depth_exponent};
wire alu_select_39=alu_mode==6'd58;
wire [35:0] alu_value_39={35'b0,alu_qa[32]};
wire alu_select_40=alu_mode==6'd59;
wire [35:0] alu_value_40=36'd2;
wire alu_select_41=alu_mode==6'd60;
reg [35:0] alu_value_41;always @* begin if((!alu_qa[17] && alu_qa[17:0]!=0) || (alu_qa[17:0]==0 && !alu_qa[35] && alu_qa[35:18]!=0))alu_value_41=0;else alu_value_41=1;end
wire alu_select_42=alu_mode==6'd62;
wire [35:0] alu_value_42=({31'b0,vertex_grid}-36'd2) | {25'b0,exponent,4'b0} | 36'd16384;
wire alu_select_43=alu_mode==6'd29;
reg [35:0] alu_value_43;always @* begin if(edge_orientation)alu_value_43={alu_qb[17:0],alu_sum[35:18]};else alu_value_43={alu_sum[17:0],alu_qb[35:18]};end
wire alu_select_44=alu_mode==6'd63;
wire [35:0] alu_value_44={alu_sum[17:0],alu_qb[35:18]};
wire alu_select_45=alu_mode==6'd10;
wire [35:0] alu_value_45=minmax_result;
wire alu_select_46=alu_mode==6'd11;
wire [35:0] alu_value_46=alu_qa<<3;
wire alu_select_47=alu_mode==6'd14;
wire [35:0] alu_value_47=minmax_result;
wire alu_select_48=alu_mode==6'd15;
reg [35:0] alu_value_48;always @* begin if(alu_qa>36'd131072)alu_value_48=36'd131072;else alu_value_48=alu_qa;end
wire alu_select_49=alu_mode==6'd22;
wire [35:0] alu_value_49={4'b0,alu_qb[15:0],alu_qa[15:0]};
wire alu_select_50=alu_mode==6'd24;
wire [35:0] alu_value_50={17'b0,clip_num_rounded};
wire alu_select_51=alu_mode==6'd13;
wire [35:0] alu_value_51=36'd8199;
wire [35:0] alu_result=
({36{alu_select_0}}&alu_value_0) |
({36{alu_select_1}}&alu_value_1) |
({36{alu_select_2}}&alu_value_2) |
({36{alu_select_3}}&alu_value_3) |
({36{alu_select_4}}&alu_value_4) |
({36{alu_select_5}}&alu_value_5) |
({36{alu_select_6}}&alu_value_6) |
({36{alu_select_7}}&alu_value_7) |
({36{alu_select_8}}&alu_value_8) |
({36{alu_select_9}}&alu_value_9) |
({36{alu_select_10}}&alu_value_10) |
({36{alu_select_11}}&alu_value_11) |
({36{alu_select_12}}&alu_value_12) |
({36{alu_select_13}}&alu_value_13) |
({36{alu_select_14}}&alu_value_14) |
({36{alu_select_15}}&alu_value_15) |
({36{alu_select_16}}&alu_value_16) |
({36{alu_select_17}}&alu_value_17) |
({36{alu_select_18}}&alu_value_18) |
({36{alu_select_19}}&alu_value_19) |
({36{alu_select_20}}&alu_value_20) |
({36{alu_select_21}}&alu_value_21) |
({36{alu_select_22}}&alu_value_22) |
({36{alu_select_23}}&alu_value_23) |
({36{alu_select_24}}&alu_value_24) |
({36{alu_select_25}}&alu_value_25) |
({36{alu_select_26}}&alu_value_26) |
({36{alu_select_27}}&alu_value_27) |
({36{alu_select_28}}&alu_value_28) |
({36{alu_select_29}}&alu_value_29) |
({36{alu_select_30}}&alu_value_30) |
({36{alu_select_31}}&alu_value_31) |
({36{alu_select_32}}&alu_value_32) |
({36{alu_select_33}}&alu_value_33) |
({36{alu_select_34}}&alu_value_34) |
({36{alu_select_35}}&alu_value_35) |
({36{alu_select_36}}&alu_value_36) |
({36{alu_select_37}}&alu_value_37) |
({36{alu_select_38}}&alu_value_38) |
({36{alu_select_39}}&alu_value_39) |
({36{alu_select_40}}&alu_value_40) |
({36{alu_select_41}}&alu_value_41) |
({36{alu_select_42}}&alu_value_42) |
({36{alu_select_43}}&alu_value_43) |
({36{alu_select_44}}&alu_value_44) |
({36{alu_select_45}}&alu_value_45) |
({36{alu_select_46}}&alu_value_46) |
({36{alu_select_47}}&alu_value_47) |
({36{alu_select_48}}&alu_value_48) |
({36{alu_select_49}}&alu_value_49) |
({36{alu_select_50}}&alu_value_50) |
({36{alu_select_51}}&alu_value_51);
wire signed [35:0] nr13=$signed(n_acc)>>>13;
wire signed [35:0] n_round13=nr13+(n_acc[12] && ((|n_acc[11:0]) || nr13[0]));
wire signed [35:0] nr16=$signed(n_acc)>>>16;
wire signed [35:0] nr18=$signed(n_acc)>>>18;
wire signed [35:0] np0floor=$signed(n_product0)>>>16;
wire signed [35:0] np1floor=$signed(n_product1)>>>16;
wire signed [35:0] n_round16=nr16+(n_acc[15] && ((|n_acc[14:0])||nr16[0]));
wire signed [35:0] beta_floor0=$signed(n_product0)>>>17,beta_floor1=$signed(n_product1)>>>17;
wire signed [35:0] beta17_0=beta_floor0+(n_product0[16] && ((|n_product0[15:0]) || n_product0[17]));
wire signed [35:0] beta17_1=beta_floor1+(n_product1[16] && ((|n_product1[15:0]) || n_product1[17]));
wire signed [35:0] n_round18=nr18+(n_acc[17] && ((|n_acc[16:0])||nr18[0]));
wire signed [35:0] beta0=np0floor+(n_product0[15] && ((|n_product0[14:0])||np0floor[0]));
wire signed [35:0] beta1=np1floor+(n_product1[15] && ((|n_product1[14:0])||np1floor[0]));
reg [35:0] w_floor;reg w_guard,w_sticky;
always @* begin
w_floor=w_acc[51:16];w_guard=w_acc[15];w_sticky=|w_acc[14:0];
case(w_capture_mode)
2:begin w_floor=w_acc[52:17];w_guard=w_acc[16];w_sticky=|w_acc[15:0];end
3:begin w_floor=w_product[52:17];w_guard=w_product[16];w_sticky=|w_product[15:0];end
7:begin w_floor=w_product[71:36];w_guard=w_product[35];w_sticky=|w_product[34:0];end
6:begin w_floor=w_acc[69:34];w_guard=w_acc[33];w_sticky=|w_acc[32:0];end
endcase
end
wire [35:0] w_rounded=w_floor+(w_guard && (w_sticky || w_floor[0]));
wire signed [35:0] n_floor12=$signed(n_acc)>>>12;
wire [35:0] n_round12=n_floor12+(n_acc[11] && ((|n_acc[10:0]) || n_floor12[0]));
wire [35:0] n_shared_floor=(n_capture==2)?nr18:nr13;
wire n_shared_increment=(n_capture==2)?(n_acc[17] && ((|n_acc[16:0]) || n_shared_floor[0])):(n_acc[12] && ((|n_acc[11:0]) || n_shared_floor[0]));
wire [35:0] n_shared_round=n_shared_floor+n_shared_increment;
assign read_data={qnb,qna,qb,qa};
(* syn_preserve=1, syn_keep=1 *) reg [35:0] ingress_buffer;
reg [11:0] selected12;
reg [7:0] selected8;
always @* begin
selected12=ingress_data[11:0];selected8=ingress_data[7:0];
case(ingress_channel)
1:begin selected12=ingress_data[23:12];selected8=ingress_data[15:8];end
2:begin selected12=ingress_data[35:24];selected8=ingress_data[23:16];end
default:begin end
endcase
end
wire [12:0] uv_correction={1'b0,selected12}+13'd128;
wire [16:0] uv_f16={1'b0,selected12,4'b0}+uv_correction[12:8];
assign compact_ingress=ingress_buffer;
reg [35:0] ingress_decoded;
always @* begin
ingress_decoded=ingress_data;
case(ingress_mode)
0:ingress_decoded={{4{ingress_data[31]}},ingress_data[31:0]};
1:ingress_decoded={{24{selected12[11]}},selected12};
2:ingress_decoded={19'b0,uv_f16};
3:ingress_decoded={28'b0,selected8};
default:begin end
endcase
end
always @(posedge clk)begin
if(reset)ingress_buffer<=0;
else if(ce && ingress_enable)ingress_buffer<=ingress_decoded;
end
reg [35:0] bus;
always @* begin
bus=0;
case(write_select)
0:bus=ingress_buffer;
1:begin if(write_high)bus=w_buffer[71:36];else bus=w_buffer[35:0];end
2:bus=n_buffer;
3:begin if(direct_store)bus=qa;else bus=alu_buffer;end
default:bus=0;
endcase
end
assign write_data=transport_publish?compact_attribute_value:(normal_publish?vertex_normal:bus);
always @(posedge clk)begin
if(reset)begin w_state<=0;w_bufstate<=0;n_acc<=0;n_buffer<=0;alu_buffer<=0;carry<=0;end
else if(ce)begin
case(w_op)

2:w_state<=w_add;

4:w_state<=w_product;
endcase
if(w_capture_enable || w_op==5)begin
w_bufstate<=w_rounded;end
case(n_op)
1:begin if(n_seed_f16)n_acc<=n_seed16;else n_acc<=n_seed0;end
2:n_acc<=n_add[35:0];
3:n_acc<=n_sum[35:0];
endcase
if(n_capture_enable || n_op==4)begin case(n_capture)
7:n_buffer<=n_shared_round;
0:n_buffer<=n_acc;

2:n_buffer<=n_shared_round;
3:n_buffer<={beta17_1[17:0],beta17_0[17:0]};


endcase end
if(normal_pack_enable)begin
case(normal_pack_channel)
0:n_buffer[11:0]<=qa[11:0];
1:n_buffer[23:12]<=qa[11:0];
2:n_buffer[35:24]<=qa[11:0];
default:begin end
endcase
end
if(alu_ce)begin alu_buffer<=alu_result;
end
end
end
endmodule
