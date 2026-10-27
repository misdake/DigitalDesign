module tb;
reg clk=0;always #5 clk=~clk;
GSR GSR(.GSRI(1'b1));
reg [0:0] ce;
reg [0:0] reset;
reg [0:0] kick;
reg [0:0] metadata_valid;
reg [0:0] metadata_address;
reg [63:0] metadata_data;
reg [0:0] grouped;
reg [0:0] write_ready;
reg [0:0] out_ready;
reg [0:0] cp_color;
reg [0:0] dma_valid;
reg [9:0] dma_address;
reg [63:0] dma_data;
wire [0:0] busy;reg [0:0] expected_busy;
wire [0:0] done;reg [0:0] expected_done;
wire [10:0] pc;reg [10:0] expected_pc;
wire [0:0] flag;reg [0:0] expected_flag;
wire [6:0] norm_shift;reg [6:0] expected_norm_shift;
wire [1:0] whole;reg [1:0] expected_whole;
wire [5:0] residual;reg [5:0] expected_residual;
wire [8:0] pointer;reg [8:0] expected_pointer;
wire [0:0] guard;reg [0:0] expected_guard;
wire [0:0] sticky;reg [0:0] expected_sticky;
wire [0:0] bad;reg [0:0] expected_bad;
wire [0:0] orientation;reg [0:0] expected_orientation;
wire [6:0] scan_candidate;reg [6:0] expected_scan_candidate;
wire [0:0] scan_nonzero;reg [0:0] expected_scan_nonzero;
wire [35:0] code;reg [35:0] expected_code;
wire [71:0] w_product;reg [71:0] expected_w_product;
wire [71:0] w_acc;reg [71:0] expected_w_acc;
wire [71:0] w_buffer;reg [71:0] expected_w_buffer;
wire [35:0] n_product0;reg [35:0] expected_n_product0;
wire [35:0] n_product1;reg [35:0] expected_n_product1;
wire [35:0] n_acc;reg [35:0] expected_n_acc;
wire [35:0] n_buffer;reg [35:0] expected_n_buffer;
wire [35:0] alu_buffer;reg [35:0] expected_alu_buffer;
wire [35:0] write_data;reg [35:0] expected_write_data;
wire [143:0] read_data;reg [143:0] expected_read_data;
wire [35:0] output_read;reg [35:0] expected_output_read;
wire [0:0] carry;reg [0:0] expected_carry;
wire [6:0] vertex_id;reg [6:0] expected_vertex_id;
wire [1:0] decode_axis;reg [1:0] expected_decode_axis;
wire [35:0] normal_pack;reg [35:0] expected_normal_pack;
wire [4:0] grid_shift;reg [4:0] expected_grid_shift;
wire [63:0] front_do;reg [63:0] expected_front_do;
wire [0:0] front_pending;reg [0:0] expected_front_pending;
wire [0:0] return_half;reg [0:0] expected_return_half;
wire [0:0] return_signed;reg [0:0] expected_return_signed;
wire [0:0] rcp_busy;reg [0:0] expected_rcp_busy;
wire [5:0] rcp_count;reg [5:0] expected_rcp_count;
wire [35:0] rcp_q;reg [35:0] expected_rcp_q;
wire [32:0] rcp_rem;reg [32:0] expected_rcp_rem;
wire [31:0] rcp_w;reg [31:0] expected_rcp_w;
wire [0:0] deferred;reg [0:0] expected_deferred;
wire [0:0] far_clamped;reg [0:0] expected_far_clamped;
wire [31:0] depth_value;reg [31:0] expected_depth_value;
wire [6:0] published;reg [6:0] expected_published;
wire [4:0] rcp_exp;reg [4:0] expected_rcp_exp;
wire [17:0] rcp_fraction;reg [17:0] expected_rcp_fraction;
wire [35:0] rcp_table;reg [35:0] expected_rcp_table;
wire [6:0] rcp_index;reg [6:0] expected_rcp_index;
wire [0:0] t_mode;reg [0:0] expected_t_mode;
wire [0:0] out_valid;reg [0:0] expected_out_valid;
wire [0:0] out_last;reg [0:0] expected_out_last;
wire [31:0] compact_position;reg [31:0] expected_compact_position;
wire [31:0] compact_normal;reg [31:0] expected_compact_normal;
wire [31:0] compact_attribute;reg [31:0] expected_compact_attribute;
wire [35:0] out_data;reg [35:0] expected_out_data;
wire [2:0] clip_plane;reg [2:0] expected_clip_plane;
wire [3:0] clip_count;reg [3:0] expected_clip_count;
wire [2:0] clip_index;reg [2:0] expected_clip_index;
wire [3:0] clip_out_count;reg [3:0] expected_clip_out_count;
wire [0:0] clip_buffer;reg [0:0] expected_clip_buffer;
wire [0:0] clip_inside;reg [0:0] expected_clip_inside;
wire [0:0] fan_active;reg [0:0] expected_fan_active;
wire [2:0] fan_index;reg [2:0] expected_fan_index;
wire [0:0] clip_all_inside;reg [0:0] expected_clip_all_inside;
wire [31:0] partner_position;reg [31:0] expected_partner_position;
wire [31:0] partner_normal;reg [31:0] expected_partner_normal;
wire [31:0] partner_attribute;reg [31:0] expected_partner_attribute;
wire [3:0] schedule_state;reg [3:0] expected_schedule_state;
wire [6:0] tag_position;reg [6:0] expected_tag_position;
wire [6:0] triangle_count;reg [6:0] expected_triangle_count;
wire [6:0] group_end;reg [6:0] expected_group_end;
wire [53:0] index_word;reg [53:0] expected_index_word;
wire [1:0] index_lane;reg [1:0] expected_index_lane;
wire [5:0] triangle_group;reg [5:0] expected_triangle_group;
wire [63:0] tags_left;reg [63:0] expected_tags_left;
wire [5:0] id0;reg [5:0] expected_id0;
wire [5:0] id1;reg [5:0] expected_id1;
wire [5:0] id2;reg [5:0] expected_id2;
wire [0:0] schedule_done;reg [0:0] expected_schedule_done;
wire [0:0] schedule_fault;reg [0:0] expected_schedule_fault;
probe dut(.clk(clk),.ce(ce),.reset(reset),.kick(kick),.metadata_valid(metadata_valid),.metadata_address(metadata_address),.metadata_data(metadata_data),.grouped(grouped),.write_ready(write_ready),.out_ready(out_ready),.cp_color(cp_color),.dma_valid(dma_valid),.dma_address(dma_address),.dma_data(dma_data),.busy(busy),.done(done),.pc(pc),.flag(flag),.norm_shift(norm_shift),.whole(whole),.residual(residual),.pointer(pointer),.guard(guard),.sticky(sticky),.bad(bad),.orientation(orientation),.scan_candidate(scan_candidate),.scan_nonzero(scan_nonzero),.code(code),.w_product(w_product),.w_acc(w_acc),.w_buffer(w_buffer),.n_product0(n_product0),.n_product1(n_product1),.n_acc(n_acc),.n_buffer(n_buffer),.alu_buffer(alu_buffer),.write_data(write_data),.read_data(read_data),.output_read(output_read),.carry(carry),.vertex_id(vertex_id),.decode_axis(decode_axis),.normal_pack(normal_pack),.grid_shift(grid_shift),.front_do(front_do),.front_pending(front_pending),.return_half(return_half),.return_signed(return_signed),.rcp_busy(rcp_busy),.rcp_count(rcp_count),.rcp_q(rcp_q),.rcp_rem(rcp_rem),.rcp_w(rcp_w),.deferred(deferred),.far_clamped(far_clamped),.depth_value(depth_value),.published(published),.rcp_exp(rcp_exp),.rcp_fraction(rcp_fraction),.rcp_table(rcp_table),.rcp_index(rcp_index),.t_mode(t_mode),.out_valid(out_valid),.out_last(out_last),.compact_position(compact_position),.compact_normal(compact_normal),.compact_attribute(compact_attribute),.out_data(out_data),.clip_plane(clip_plane),.clip_count(clip_count),.clip_index(clip_index),.clip_out_count(clip_out_count),.clip_buffer(clip_buffer),.clip_inside(clip_inside),.fan_active(fan_active),.fan_index(fan_index),.clip_all_inside(clip_all_inside),.partner_position(partner_position),.partner_normal(partner_normal),.partner_attribute(partner_attribute),.schedule_state(schedule_state),.tag_position(tag_position),.triangle_count(triangle_count),.group_end(group_end),.index_word(index_word),.index_lane(index_lane),.triangle_group(triangle_group),.tags_left(tags_left),.id0(id0),.id1(id1),.id2(id2),.schedule_done(schedule_done),.schedule_fault(schedule_fault));
integer f,scan_count,cycle;
initial begin
cycle=0;f=$fopen("vectors.txt","r");if(!f)$fatal;
while(!$feof(f)&&cycle<2680)begin
@(negedge clk);scan_count=$fscanf(f,"%h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h %h\n",ce,reset,kick,metadata_valid,metadata_address,metadata_data,grouped,write_ready,out_ready,cp_color,dma_valid,dma_address,dma_data,expected_busy,expected_done,expected_pc,expected_flag,expected_norm_shift,expected_whole,expected_residual,expected_pointer,expected_guard,expected_sticky,expected_bad,expected_orientation,expected_scan_candidate,expected_scan_nonzero,expected_code,expected_w_product,expected_w_acc,expected_w_buffer,expected_n_product0,expected_n_product1,expected_n_acc,expected_n_buffer,expected_alu_buffer,expected_write_data,expected_read_data,expected_output_read,expected_carry,expected_vertex_id,expected_decode_axis,expected_normal_pack,expected_grid_shift,expected_front_do,expected_front_pending,expected_return_half,expected_return_signed,expected_rcp_busy,expected_rcp_count,expected_rcp_q,expected_rcp_rem,expected_rcp_w,expected_deferred,expected_far_clamped,expected_depth_value,expected_published,expected_rcp_exp,expected_rcp_fraction,expected_rcp_table,expected_rcp_index,expected_t_mode,expected_out_valid,expected_out_last,expected_compact_position,expected_compact_normal,expected_compact_attribute,expected_out_data,expected_clip_plane,expected_clip_count,expected_clip_index,expected_clip_out_count,expected_clip_buffer,expected_clip_inside,expected_fan_active,expected_fan_index,expected_clip_all_inside,expected_partner_position,expected_partner_normal,expected_partner_attribute,expected_schedule_state,expected_tag_position,expected_triangle_count,expected_group_end,expected_index_word,expected_index_lane,expected_triangle_group,expected_tags_left,expected_id0,expected_id1,expected_id2,expected_schedule_done,expected_schedule_fault);
if(scan_count==93)begin
@(posedge clk);#1;
if(busy!==expected_busy)begin $display("FAIL cycle %0d busy: %h != %h",cycle,busy,expected_busy);$fatal;end
if(done!==expected_done)begin $display("FAIL cycle %0d done: %h != %h",cycle,done,expected_done);$fatal;end
if(pc!==expected_pc)begin $display("FAIL cycle %0d pc: %h != %h",cycle,pc,expected_pc);$fatal;end
if(flag!==expected_flag)begin $display("FAIL cycle %0d flag: %h != %h",cycle,flag,expected_flag);$fatal;end
if(norm_shift!==expected_norm_shift)begin $display("FAIL cycle %0d norm_shift: %h != %h",cycle,norm_shift,expected_norm_shift);$fatal;end
if(whole!==expected_whole)begin $display("FAIL cycle %0d whole: %h != %h",cycle,whole,expected_whole);$fatal;end
if(residual!==expected_residual)begin $display("FAIL cycle %0d residual: %h != %h",cycle,residual,expected_residual);$fatal;end
if(pointer!==expected_pointer)begin $display("FAIL cycle %0d pointer: %h != %h",cycle,pointer,expected_pointer);$fatal;end
if(guard!==expected_guard)begin $display("FAIL cycle %0d guard: %h != %h",cycle,guard,expected_guard);$fatal;end
if(sticky!==expected_sticky)begin $display("FAIL cycle %0d sticky: %h != %h",cycle,sticky,expected_sticky);$fatal;end
if(bad!==expected_bad)begin $display("FAIL cycle %0d bad: %h != %h",cycle,bad,expected_bad);$fatal;end
if(orientation!==expected_orientation)begin $display("FAIL cycle %0d orientation: %h != %h",cycle,orientation,expected_orientation);$fatal;end
if(scan_candidate!==expected_scan_candidate)begin $display("FAIL cycle %0d scan_candidate: %h != %h",cycle,scan_candidate,expected_scan_candidate);$fatal;end
if(scan_nonzero!==expected_scan_nonzero)begin $display("FAIL cycle %0d scan_nonzero: %h != %h",cycle,scan_nonzero,expected_scan_nonzero);$fatal;end
if(code!==expected_code)begin $display("FAIL cycle %0d code: %h != %h",cycle,code,expected_code);$fatal;end
if(w_product!==expected_w_product)begin $display("FAIL cycle %0d w_product: %h != %h",cycle,w_product,expected_w_product);$fatal;end
if(w_acc!==expected_w_acc)begin $display("FAIL cycle %0d w_acc: %h != %h",cycle,w_acc,expected_w_acc);$fatal;end
if(w_buffer!==expected_w_buffer)begin $display("FAIL cycle %0d w_buffer: %h != %h",cycle,w_buffer,expected_w_buffer);$fatal;end
if(n_product0!==expected_n_product0)begin $display("FAIL cycle %0d n_product0: %h != %h",cycle,n_product0,expected_n_product0);$fatal;end
if(n_product1!==expected_n_product1)begin $display("FAIL cycle %0d n_product1: %h != %h",cycle,n_product1,expected_n_product1);$fatal;end
if(n_acc!==expected_n_acc)begin $display("FAIL cycle %0d n_acc: %h != %h",cycle,n_acc,expected_n_acc);$fatal;end
if(n_buffer!==expected_n_buffer)begin $display("FAIL cycle %0d n_buffer: %h != %h",cycle,n_buffer,expected_n_buffer);$fatal;end
if(alu_buffer!==expected_alu_buffer)begin $display("FAIL cycle %0d alu_buffer: %h != %h",cycle,alu_buffer,expected_alu_buffer);$fatal;end
if(write_data!==expected_write_data)begin $display("FAIL cycle %0d write_data: %h != %h",cycle,write_data,expected_write_data);$fatal;end
if(read_data!==expected_read_data)begin $display("FAIL cycle %0d read_data: %h != %h",cycle,read_data,expected_read_data);$fatal;end
if(output_read!==expected_output_read)begin $display("FAIL cycle %0d output_read: %h != %h",cycle,output_read,expected_output_read);$fatal;end
if(carry!==expected_carry)begin $display("FAIL cycle %0d carry: %h != %h",cycle,carry,expected_carry);$fatal;end
if(vertex_id!==expected_vertex_id)begin $display("FAIL cycle %0d vertex_id: %h != %h",cycle,vertex_id,expected_vertex_id);$fatal;end
if(decode_axis!==expected_decode_axis)begin $display("FAIL cycle %0d decode_axis: %h != %h",cycle,decode_axis,expected_decode_axis);$fatal;end
if(normal_pack!==expected_normal_pack)begin $display("FAIL cycle %0d normal_pack: %h != %h",cycle,normal_pack,expected_normal_pack);$fatal;end
if(grid_shift!==expected_grid_shift)begin $display("FAIL cycle %0d grid_shift: %h != %h",cycle,grid_shift,expected_grid_shift);$fatal;end
if(front_do!==expected_front_do)begin $display("FAIL cycle %0d front_do: %h != %h",cycle,front_do,expected_front_do);$fatal;end
if(front_pending!==expected_front_pending)begin $display("FAIL cycle %0d front_pending: %h != %h",cycle,front_pending,expected_front_pending);$fatal;end
if(return_half!==expected_return_half)begin $display("FAIL cycle %0d return_half: %h != %h",cycle,return_half,expected_return_half);$fatal;end
if(return_signed!==expected_return_signed)begin $display("FAIL cycle %0d return_signed: %h != %h",cycle,return_signed,expected_return_signed);$fatal;end
if(rcp_busy!==expected_rcp_busy)begin $display("FAIL cycle %0d rcp_busy: %h != %h",cycle,rcp_busy,expected_rcp_busy);$fatal;end
if(rcp_count!==expected_rcp_count)begin $display("FAIL cycle %0d rcp_count: %h != %h",cycle,rcp_count,expected_rcp_count);$fatal;end
if(rcp_q!==expected_rcp_q)begin $display("FAIL cycle %0d rcp_q: %h != %h",cycle,rcp_q,expected_rcp_q);$fatal;end
if(rcp_rem!==expected_rcp_rem)begin $display("FAIL cycle %0d rcp_rem: %h != %h",cycle,rcp_rem,expected_rcp_rem);$fatal;end
if(rcp_w!==expected_rcp_w)begin $display("FAIL cycle %0d rcp_w: %h != %h",cycle,rcp_w,expected_rcp_w);$fatal;end
if(deferred!==expected_deferred)begin $display("FAIL cycle %0d deferred: %h != %h",cycle,deferred,expected_deferred);$fatal;end
if(far_clamped!==expected_far_clamped)begin $display("FAIL cycle %0d far_clamped: %h != %h",cycle,far_clamped,expected_far_clamped);$fatal;end
if(depth_value!==expected_depth_value)begin $display("FAIL cycle %0d depth_value: %h != %h",cycle,depth_value,expected_depth_value);$fatal;end
if(published!==expected_published)begin $display("FAIL cycle %0d published: %h != %h",cycle,published,expected_published);$fatal;end
if(rcp_exp!==expected_rcp_exp)begin $display("FAIL cycle %0d rcp_exp: %h != %h",cycle,rcp_exp,expected_rcp_exp);$fatal;end
if(rcp_fraction!==expected_rcp_fraction)begin $display("FAIL cycle %0d rcp_fraction: %h != %h",cycle,rcp_fraction,expected_rcp_fraction);$fatal;end
if(rcp_table!==expected_rcp_table)begin $display("FAIL cycle %0d rcp_table: %h != %h",cycle,rcp_table,expected_rcp_table);$fatal;end
if(rcp_index!==expected_rcp_index)begin $display("FAIL cycle %0d rcp_index: %h != %h",cycle,rcp_index,expected_rcp_index);$fatal;end
if(t_mode!==expected_t_mode)begin $display("FAIL cycle %0d t_mode: %h != %h",cycle,t_mode,expected_t_mode);$fatal;end
if(out_valid!==expected_out_valid)begin $display("FAIL cycle %0d out_valid: %h != %h",cycle,out_valid,expected_out_valid);$fatal;end
if(out_last!==expected_out_last)begin $display("FAIL cycle %0d out_last: %h != %h",cycle,out_last,expected_out_last);$fatal;end
if(compact_position!==expected_compact_position)begin $display("FAIL cycle %0d compact_position: %h != %h",cycle,compact_position,expected_compact_position);$fatal;end
if(compact_normal!==expected_compact_normal)begin $display("FAIL cycle %0d compact_normal: %h != %h",cycle,compact_normal,expected_compact_normal);$fatal;end
if(compact_attribute!==expected_compact_attribute)begin $display("FAIL cycle %0d compact_attribute: %h != %h",cycle,compact_attribute,expected_compact_attribute);$fatal;end
if(out_data!==expected_out_data)begin $display("FAIL cycle %0d out_data: %h != %h",cycle,out_data,expected_out_data);$fatal;end
if(clip_plane!==expected_clip_plane)begin $display("FAIL cycle %0d clip_plane: %h != %h",cycle,clip_plane,expected_clip_plane);$fatal;end
if(clip_count!==expected_clip_count)begin $display("FAIL cycle %0d clip_count: %h != %h",cycle,clip_count,expected_clip_count);$fatal;end
if(clip_index!==expected_clip_index)begin $display("FAIL cycle %0d clip_index: %h != %h",cycle,clip_index,expected_clip_index);$fatal;end
if(clip_out_count!==expected_clip_out_count)begin $display("FAIL cycle %0d clip_out_count: %h != %h",cycle,clip_out_count,expected_clip_out_count);$fatal;end
if(clip_buffer!==expected_clip_buffer)begin $display("FAIL cycle %0d clip_buffer: %h != %h",cycle,clip_buffer,expected_clip_buffer);$fatal;end
if(clip_inside!==expected_clip_inside)begin $display("FAIL cycle %0d clip_inside: %h != %h",cycle,clip_inside,expected_clip_inside);$fatal;end
if(fan_active!==expected_fan_active)begin $display("FAIL cycle %0d fan_active: %h != %h",cycle,fan_active,expected_fan_active);$fatal;end
if(fan_index!==expected_fan_index)begin $display("FAIL cycle %0d fan_index: %h != %h",cycle,fan_index,expected_fan_index);$fatal;end
if(clip_all_inside!==expected_clip_all_inside)begin $display("FAIL cycle %0d clip_all_inside: %h != %h",cycle,clip_all_inside,expected_clip_all_inside);$fatal;end
if(partner_position!==expected_partner_position)begin $display("FAIL cycle %0d partner_position: %h != %h",cycle,partner_position,expected_partner_position);$fatal;end
if(partner_normal!==expected_partner_normal)begin $display("FAIL cycle %0d partner_normal: %h != %h",cycle,partner_normal,expected_partner_normal);$fatal;end
if(partner_attribute!==expected_partner_attribute)begin $display("FAIL cycle %0d partner_attribute: %h != %h",cycle,partner_attribute,expected_partner_attribute);$fatal;end
if(schedule_state!==expected_schedule_state)begin $display("FAIL cycle %0d schedule_state: %h != %h",cycle,schedule_state,expected_schedule_state);$fatal;end
if(tag_position!==expected_tag_position)begin $display("FAIL cycle %0d tag_position: %h != %h",cycle,tag_position,expected_tag_position);$fatal;end
if(triangle_count!==expected_triangle_count)begin $display("FAIL cycle %0d triangle_count: %h != %h",cycle,triangle_count,expected_triangle_count);$fatal;end
if(group_end!==expected_group_end)begin $display("FAIL cycle %0d group_end: %h != %h",cycle,group_end,expected_group_end);$fatal;end
if(index_word!==expected_index_word)begin $display("FAIL cycle %0d index_word: %h != %h",cycle,index_word,expected_index_word);$fatal;end
if(index_lane!==expected_index_lane)begin $display("FAIL cycle %0d index_lane: %h != %h",cycle,index_lane,expected_index_lane);$fatal;end
if(triangle_group!==expected_triangle_group)begin $display("FAIL cycle %0d triangle_group: %h != %h",cycle,triangle_group,expected_triangle_group);$fatal;end
if(tags_left!==expected_tags_left)begin $display("FAIL cycle %0d tags_left: %h != %h",cycle,tags_left,expected_tags_left);$fatal;end
if(id0!==expected_id0)begin $display("FAIL cycle %0d id0: %h != %h",cycle,id0,expected_id0);$fatal;end
if(id1!==expected_id1)begin $display("FAIL cycle %0d id1: %h != %h",cycle,id1,expected_id1);$fatal;end
if(id2!==expected_id2)begin $display("FAIL cycle %0d id2: %h != %h",cycle,id2,expected_id2);$fatal;end
if(schedule_done!==expected_schedule_done)begin $display("FAIL cycle %0d schedule_done: %h != %h",cycle,schedule_done,expected_schedule_done);$fatal;end
if(schedule_fault!==expected_schedule_fault)begin $display("FAIL cycle %0d schedule_fault: %h != %h",cycle,schedule_fault,expected_schedule_fault);$fatal;end
cycle=cycle+1;
end
end
if(cycle!=2679)$fatal;
$display("PASS tight-translation: %0d edges",cycle);$finish;
end
initial begin #30000;$fatal;end
endmodule
