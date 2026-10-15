module probe(
input wire [0:0] clk,
input wire [0:0] ce,
input wire [0:0] reset,
input wire [0:0] packet_valid,
input wire [0:0] packet_last,
input wire [35:0] packet_data,
input wire [0:0] result_ready,
output wire [0:0] cfg_ready,
output wire [0:0] pixel_ready,
output wire [0:0] result_valid,
output wire [8:0] result_x,
output wire [7:0] result_y,
output wire [1:0] result_lane,
output wire [3:0] result_mask,
output wire [14:0] result_origin,
output wire [35:0] result_normal,
output wire [31:0] result_ndc,
output wire [35:0] result_uv,
output wire [23:0] result_rgb,
output wire [15:0] result_depth,
output wire [0:0] result_coarse,
output wire [0:0] result_fault,
output wire [0:0] phase,
output wire [3:0] stage_valid,
output wire [17:0] rcp_product,
output wire [35:0] ratio_product,
output wire [35:0] depth_product,
output wire [0:0] raster_busy,
output wire [0:0] raster_done,
output wire [0:0] raster_fault,
output wire [0:0] packet_ready
,input wire vertex_mode,matrix_write,vertex_output_ready,input wire [4:0] matrix_address,input wire [31:0] matrix_data,
output wire vertex_valid,vertex_last,vertex_busy,vertex_fault,output wire [35:0] vertex_data);
wire core_packet_ready,core_raster_busy,core_raster_fault;

localparam IDLE=0,READ=1,SEND=2;
reg [1:0] cache_state;reg [3:0] word_index;reg [5:0] reference0,reference1,reference2;reg reference_color;
reg [8:0] write_address;reg [6:0] loaded_vertices;reg previous_mode,cache_fault;
reg [5:0] selected;reg [2:0] field;reg [8:0] read_address;
wire [35:0] memory_do;
wire vertex_write=vertex_mode && vertex_valid && vertex_output_ready;
// SPX9 x36 uses AD[3:0] as four9-bit write enables.
wire [13:0] memory_address=vertex_mode?{write_address,5'b01111}:{read_address,5'b0};
wire memory_enable=ce && (vertex_write || (!vertex_mode && cache_state==READ));
SPX9 #(.BIT_WIDTH(36),.READ_MODE(0),.WRITE_MODE(0),.RESET_MODE("SYNC")) transformed_cache(
.DO(memory_do),.DI(vertex_data),.BLKSEL(3'b0),.AD(memory_address),.WRE(vertex_write),.CLK(clk),.CE(memory_enable),.OCE(1'b1),.RESET(reset));
always @*begin
selected=reference0;field=0;
if(word_index<9)begin
case(word_index/3)0:selected=reference0;1:selected=reference1;default:selected=reference2;endcase
field=word_index%3;
end else begin
case((word_index-9)/2)0:selected=reference0;1:selected=reference1;default:selected=reference2;endcase
field=3+(word_index-9)%2;
end
read_address={3'b0,selected}+({3'b0,selected}<<2)+field;
end
reg [35:0] cached_word;
always @*begin cached_word=memory_do;
if(word_index<9)begin cached_word={6'b0,memory_do[29:0]};if(word_index==0)cached_word[35]=reference_color;end end
wire core_packet_valid=vertex_mode?packet_valid:(cache_state==SEND);
wire core_packet_last=vertex_mode?packet_last:(word_index==14);
wire [35:0] core_packet_data=vertex_mode?packet_data:cached_word;
assign packet_ready=ce && (vertex_mode?core_packet_ready:(cache_state==IDLE && !vertex_busy));
assign raster_busy=core_raster_busy || cache_state!=IDLE;
assign raster_fault=core_raster_fault || cache_fault;
always @(posedge clk)begin
if(reset)begin cache_state<=IDLE;word_index<=0;write_address<=0;loaded_vertices<=0;previous_mode<=0;cache_fault<=0;end
else if(ce)begin
previous_mode<=vertex_mode;
if(previous_mode!=vertex_mode && cache_state!=IDLE)cache_fault<=1;
if(vertex_mode && !previous_mode)begin write_address<=0;loaded_vertices<=0;end
if(vertex_write)begin
if(write_address>=320)cache_fault<=1;write_address<=write_address+1;
if(vertex_last)loaded_vertices<=loaded_vertices+1;
end
if(!vertex_mode)case(cache_state)
IDLE:if(packet_valid && packet_ready)begin
reference0<=packet_data[5:0];reference1<=packet_data[11:6];reference2<=packet_data[17:12];reference_color<=packet_data[18];
if(!packet_last || packet_data[5:0]>=loaded_vertices || packet_data[11:6]>=loaded_vertices || packet_data[17:12]>=loaded_vertices)cache_fault<=1;
word_index<=0;cache_state<=READ;
end
READ:cache_state<=SEND;
SEND:if(core_packet_ready)begin if(word_index==14)cache_state<=IDLE;else begin word_index<=word_index+1;cache_state<=READ;end end
default:begin cache_fault<=1;cache_state<=IDLE;end endcase
end end
owner_core core(.clk(clk),.ce(ce),.reset(reset),.packet_valid(core_packet_valid),.packet_last(core_packet_last),.packet_data(core_packet_data),.result_ready(result_ready),.cfg_ready(cfg_ready),.pixel_ready(pixel_ready),.result_valid(result_valid),.result_x(result_x),.result_y(result_y),.result_lane(result_lane),.result_mask(result_mask),.result_origin(result_origin),.result_normal(result_normal),.result_ndc(result_ndc),.result_uv(result_uv),.result_rgb(result_rgb),.result_depth(result_depth),.result_coarse(result_coarse),.result_fault(result_fault),.phase(phase),.stage_valid(stage_valid),.rcp_product(rcp_product),.ratio_product(ratio_product),.depth_product(depth_product),.raster_busy(core_raster_busy),.raster_done(raster_done),.raster_fault(core_raster_fault),.packet_ready(core_packet_ready),.vertex_mode(vertex_mode),.matrix_write(matrix_write),.vertex_output_ready(vertex_output_ready),.matrix_address(matrix_address),.matrix_data(matrix_data),.vertex_valid(vertex_valid),.vertex_last(vertex_last),.vertex_busy(vertex_busy),.vertex_fault(vertex_fault),.vertex_data(vertex_data));
endmodule
module owner_core(
input wire [0:0] clk,
input wire [0:0] ce,
input wire [0:0] reset,
input wire [0:0] packet_valid,
input wire [0:0] packet_last,
input wire [35:0] packet_data,
input wire [0:0] result_ready,
output wire [0:0] cfg_ready,
output wire [0:0] pixel_ready,
output wire [0:0] result_valid,
output wire [8:0] result_x,
output wire [7:0] result_y,
output wire [1:0] result_lane,
output wire [3:0] result_mask,
output wire [14:0] result_origin,
output wire [35:0] result_normal,
output wire [31:0] result_ndc,
output wire [35:0] result_uv,
output wire [23:0] result_rgb,
output wire [15:0] result_depth,
output wire [0:0] result_coarse,
output wire [0:0] result_fault,
output wire [0:0] phase,
output wire [3:0] stage_valid,
output wire [17:0] rcp_product,
output wire [35:0] ratio_product,
output wire [35:0] depth_product,
output wire [0:0] raster_busy,
output wire [0:0] raster_done,
output wire [0:0] raster_fault,
output wire [0:0] packet_ready
,input wire vertex_mode,matrix_write,vertex_output_ready,input wire [4:0] matrix_address,input wire [31:0] matrix_data,
output wire vertex_valid,vertex_last,vertex_busy,vertex_fault,output wire [35:0] vertex_data);
wire [71:0] shared_product;wire [35:0] vertex_a,vertex_b,triangle_a,triangle_b;wire vertex_asign,vertex_bsign,triangle_asign,triangle_bsign,vertex_input_ready,triangle_input_ready;
wire [35:0] owner_a=vertex_mode?vertex_a:triangle_a,owner_b=vertex_mode?vertex_b:triangle_b;
wire owner_asign=vertex_mode?vertex_asign:triangle_asign,owner_bsign=vertex_mode?vertex_bsign:triangle_bsign;
reg previous_mode,owner_fault;
assign packet_ready=vertex_mode?vertex_input_ready:triangle_input_ready;
MULT36X36 #(.AREG(0),.BREG(0),.OUT0_REG(1),.OUT1_REG(0),.PIPE_REG(1),.ASIGN_REG(0),.BSIGN_REG(0),.MULT_RESET_MODE("SYNC")) geometry_multiplier(.A(owner_a),.B(owner_b),.ASIGN(owner_asign),.BSIGN(owner_bsign),.CLK(clk),.CE(ce),.RESET(reset),.DOUT(shared_product));
vertex_preparer vertex(.clk(clk),.ce(ce && vertex_mode),.reset(reset),.input_valid(packet_valid && vertex_mode),.input_last(packet_last),.input_data(packet_data),.output_ready(vertex_output_ready),.input_ready(vertex_input_ready),.output_valid(vertex_valid),.output_last(vertex_last),.output_data(vertex_data),.busy(vertex_busy),.fault(vertex_fault),.matrix_write(matrix_write),.matrix_address(matrix_address),.matrix_data(matrix_data),.shared_product(shared_product),.shared_a(vertex_a),.shared_b(vertex_b),.shared_asign(vertex_asign),.shared_bsign(vertex_bsign));
wire prepared_valid,prepared_last,prepared_ready,preparer_busy,preparer_fault,walker_busy,walker_fault;
wire [35:0] prepared_data;
assign raster_busy=walker_busy || preparer_busy || vertex_busy;
always @(posedge clk)begin if(reset)begin previous_mode<=0;owner_fault<=0;end else if(ce)begin previous_mode<=vertex_mode;if(previous_mode!=vertex_mode && (preparer_busy || vertex_busy))owner_fault<=1;end end
assign raster_fault=walker_fault || preparer_fault || vertex_fault || owner_fault;
triangle_preparer setup(.clk(clk),.ce(ce && !vertex_mode),.reset(reset),.input_valid(packet_valid && !vertex_mode),.input_last(packet_last),.input_data(packet_data),.input_color(packet_data[35]),.input_ready(triangle_input_ready),.output_valid(prepared_valid),.output_last(prepared_last),.output_data(prepared_data),.output_ready(prepared_ready),.busy(preparer_busy),.fault(preparer_fault),.shared_product(shared_product),.shared_a(triangle_a),.shared_b(triangle_b),.shared_asign(triangle_asign),.shared_bsign(triangle_bsign));
wire [0:0] pixel_valid;
wire [0:0] cfg_write;
wire [0:0] cfg_commit;
wire [0:0] cfg_color;
wire [53:0] pixel_d;
wire [53:0] pixel_n1;
wire [53:0] pixel_n2;
wire [8:0] pixel_x;
wire [7:0] pixel_y;
wire [1:0] pixel_lane;
wire [3:0] pixel_mask;
wire [14:0] pixel_origin;
wire [2:0] cfg_index;
wire [35:0] cfg_data;
wire [35:0] cfg_area;
wire [7:0] cfg_depth_exp;
reg queue_valid;wire raw_pixel_valid;wire raw_pixel_ready=!queue_valid || pixel_ready;
assign pixel_valid=queue_valid;
wire [53:0] raw_pixel_d;reg [53:0] queue_d;assign pixel_d=queue_d;
wire [53:0] raw_pixel_n1;reg [53:0] queue_n1;assign pixel_n1=queue_n1;
wire [53:0] raw_pixel_n2;reg [53:0] queue_n2;assign pixel_n2=queue_n2;
wire [8:0] raw_pixel_x;reg [8:0] queue_x;assign pixel_x=queue_x;
wire [7:0] raw_pixel_y;reg [7:0] queue_y;assign pixel_y=queue_y;
wire [1:0] raw_pixel_lane;reg [1:0] queue_lane;assign pixel_lane=queue_lane;
wire [3:0] raw_pixel_mask;reg [3:0] queue_mask;assign pixel_mask=queue_mask;
wire [14:0] raw_pixel_origin;reg [14:0] queue_origin;assign pixel_origin=queue_origin;
always @(posedge clk)begin if(reset)queue_valid<=0;else if(ce && raw_pixel_ready)begin queue_valid<=raw_pixel_valid;if(raw_pixel_valid)begin
queue_d<=raw_pixel_d;
queue_n1<=raw_pixel_n1;
queue_n2<=raw_pixel_n2;
queue_x<=raw_pixel_x;
queue_y<=raw_pixel_y;
queue_lane<=raw_pixel_lane;
queue_mask<=raw_pixel_mask;
queue_origin<=raw_pixel_origin;
end end end
raster_core raster(.clk(clk),.ce(ce),.reset(reset),.packet_valid(prepared_valid),.packet_last(prepared_last),.pixel_ready(raw_pixel_ready),.packet_data(prepared_data),.packet_ready(prepared_ready),.pixel_valid(raw_pixel_valid),.cfg_write(cfg_write),.cfg_commit(cfg_commit),.cfg_color(cfg_color),.pixel_d(raw_pixel_d),.pixel_n1(raw_pixel_n1),.pixel_n2(raw_pixel_n2),.pixel_x(raw_pixel_x),.pixel_y(raw_pixel_y),.pixel_lane(raw_pixel_lane),.pixel_mask(raw_pixel_mask),.pixel_origin(raw_pixel_origin),.cfg_index(cfg_index),.cfg_data(cfg_data),.cfg_area(cfg_area),.cfg_depth_exp(cfg_depth_exp),.busy(walker_busy),.done(raster_done),.fault(walker_fault),.downstream_idle(!(stage_valid!=0 || result_valid || queue_valid)));
interpolator_core interpolation(.clk(clk),.ce(ce),.reset(reset),.cfg_write(cfg_write),.cfg_index(cfg_index),.cfg_data(cfg_data),.cfg_commit(cfg_commit),.cfg_color(cfg_color),.cfg_area(cfg_area),.cfg_depth_exp(cfg_depth_exp),.pixel_valid(pixel_valid),.pixel_d(pixel_d),.pixel_n1(pixel_n1),.pixel_n2(pixel_n2),.pixel_x(pixel_x),.pixel_y(pixel_y),.pixel_lane(pixel_lane),.pixel_mask(pixel_mask),.pixel_origin(pixel_origin),.result_ready(result_ready),.cfg_ready(cfg_ready),.pixel_ready(pixel_ready),.result_valid(result_valid),.result_x(result_x),.result_y(result_y),.result_lane(result_lane),.result_mask(result_mask),.result_origin(result_origin),.result_normal(result_normal),.result_ndc(result_ndc),.result_uv(result_uv),.result_rgb(result_rgb),.result_depth(result_depth),.result_coarse(result_coarse),.result_fault(result_fault),.phase(phase),.stage_valid(stage_valid),.rcp_product(rcp_product),.ratio_product(ratio_product),.depth_product(depth_product));
endmodule
/* synthesis syn_dspstyle = "logic" */
/* synthesis syn_dspstyle = "logic" */
// Independent scalar-edge tile walker. Same packet and 12-field pixel semantics.
// Independent scalar-edge tile walker. Same packet and 12-field pixel semantics.
// Independent scalar-edge tile walker. Same packet and 12-field pixel semantics.
module raster_core(
input wire clk,ce,reset,packet_valid,packet_last,pixel_ready,downstream_idle,
input wire [35:0] packet_data,
output wire packet_ready,pixel_valid,cfg_write,cfg_commit,cfg_color,busy,
output wire [53:0] pixel_d,pixel_n1,pixel_n2,
output wire [8:0] pixel_x,output wire [7:0] pixel_y,output wire [1:0] pixel_lane,
output wire [3:0] pixel_mask,output wire [14:0] pixel_origin,
output wire [2:0] cfg_index,output wire [35:0] cfg_data,cfg_area,
output wire [7:0] cfg_depth_exp,output reg done,fault);
localparam LOAD=0,COMMIT=1,INIT=2,SEED=3,COPYT=4,COPYS=5,
 MAXX=6,MAXY=7,MAXTEST=8,MINX=9,MINY=10,MINTEST=11,
 QUAD=12,MASK=13,MASKEND=14,EVALSEND=15,SEND=16,
 NEXTQ=17,TILEADV=18,DRAIN=19,DECIDE=20;
reg [4:0] decision_for;
reg [4:0] state,cursor,seedbit;
reg [1:0] edge_number,lane;
reg [5:0] align_shift[0:2];
reg [35:0] header,area;reg [7:0] dexp;
reg [37:0] near_limit,max_den;
// These asynchronous-read, single-write arrays can use RAM16SDP.
reg [27:0] coefficient[0:15];
reg [35:0] work[0:15];
reg [35:0] seed[0:3];
// Prepare once while receiving each edge's coefficients. No BSRAM or DSP.
reg [35:0] tile_scale[0:7];
reg [29:0] diagonal_scale[0:3];
reg signed [27:0] previous_fx;
reg prepared_write;reg [2:0] prepared_address;
reg diagonal_write;reg [1:0] diagonal_address;
always @*begin
 prepared_write=1;prepared_address=0;
 case(cursor)
 4:prepared_address=0;5:prepared_address=1;
 7:prepared_address=2;8:prepared_address=3;
 10:prepared_address=4;11:prepared_address=5;
 default:prepared_write=0;
 endcase
 diagonal_write=1;diagonal_address=0;
 case(cursor)
 5:diagonal_address=0;8:diagonal_address=1;11:diagonal_address=2;
 default:diagonal_write=0;
 endcase
end

wire signed [27:0] incoming_coefficient=packet_data[27:0];
wire signed [35:0] incoming_times30=($signed(incoming_coefficient)<<<5)-($signed(incoming_coefficient)<<<1);
wire signed [28:0] incoming_difference=$signed(incoming_coefficient)-$signed(previous_fx);
wire signed [29:0] incoming_diagonal={incoming_difference,1'b0};
wire [2:0] tile_scale_address={edge_number,(state==MAXY || state==MINY)};
wire signed [35:0] prepared_tile=tile_scale[tile_scale_address];
wire signed [29:0] prepared_diagonal=diagonal_scale[edge_number];

reg [8:0] tx,qx;reg [7:0] ty,qy;
reg direction,full_tile,copy_strip;
reg [3:0] held_mask;
reg signed [37:0] sum_acc,out_d;
reg signed [35:0] out_n1,out_n2;
reg positive_acc,nonnegative_acc,inside_acc;
reg [3:0] parameter_x,parameter_y,parameter_b;
reg [3:0] q_address,p_address,t_address,s_address;
wire signed [27:0] fx=coefficient[parameter_x];
wire signed [27:0] fy=coefficient[parameter_y];
wire signed [27:0] fb=coefficient[parameter_b];
reg [3:0] read_address,write_address;
reg work_write;
reg signed [35:0] write_value,operand_b;
reg subtract;
wire signed [35:0] operand_a=work[read_address];
wire signed [35:0] seed_value=seed[edge_number];
wire signed [35:0] add_result=operand_a+(operand_b^{36{subtract}})+subtract;
wire signed [35:0] aligned_value=operand_a>>>align_shift[edge_number];
wire signed [37:0] aligned_extended={{2{aligned_value[35]}},aligned_value};
wire signed [37:0] accumulated=sum_acc+aligned_extended;
reg signed [37:0] sum_next;
always @*begin
 sum_next=accumulated;
 if(edge_number==0)sum_next=aligned_extended;
end
wire positive_now=operand_a>0;
wire nonnegative_now=operand_a>=0;
wire inside_now=positive_now || (operand_a==0 && header[6+edge_number]);
wire all_positive=positive_now && (edge_number==0 || positive_acc);
wire all_nonnegative=nonnegative_now && (edge_number==0 || nonnegative_acc);
wire all_inside=inside_now && (edge_number==0 || inside_acc);
wire signed [38:0] signed_near={1'b0,near_limit};
wire passes_near=sum_acc>0 && sum_acc<=signed_near;
wire row_end=(!direction && qx==tx+14)||(direction && qx==tx);
wire tile_end=row_end && qy==ty+14;
wire take=ce && packet_valid && packet_ready;
assign packet_ready=ce && state==LOAD;
assign cfg_write=take && cursor>=13 && cursor<=18;
assign cfg_index=cursor-13;assign cfg_data=packet_data;
assign cfg_commit=ce && state==COMMIT;
assign cfg_color=header[5];assign cfg_area=area;assign cfg_depth_exp=dexp;
assign busy=state!=LOAD || cursor!=0;
assign pixel_valid=state==SEND;
assign pixel_d={{16{out_d[37]}},out_d};
assign pixel_n1={{18{out_n1[35]}},out_n1};
assign pixel_n2={{18{out_n2[35]}},out_n2};
assign pixel_x=qx+lane[0];assign pixel_y=qy+lane[1];
assign pixel_lane=lane;assign pixel_mask=held_mask;
assign pixel_origin={qy[7:1],qx[8:1]};

always @*begin
 read_address=q_address;write_address=read_address;
 operand_b=0;subtract=0;work_write=0;write_value=add_result;
 case(state)
 INIT:begin work_write=1;write_value=$signed(fb)<<<10;end
 SEED:begin work_write=1;
   if(seedbit<9)begin if((9'd399>>seedbit)&1)operand_b=seed_value;end
   else begin if((9'd239>>(seedbit-9))&1)operand_b=seed_value;end
 end
 COPYT:begin write_address=t_address;work_write=1;write_value=operand_a;end
 COPYS:begin write_address=s_address;work_write=1;write_value=operand_a;end
 MAXX,MINX:begin write_address=p_address;work_write=1;
   if((state==MAXX && !fx[27])||(state==MINX && fx<0))operand_b=prepared_tile;
 end
 MAXY,MINY:begin read_address=p_address;write_address=read_address;work_write=1;
   if((state==MAXY && !fy[27])||(state==MINY && fy<0))operand_b=prepared_tile;
 end
 MAXTEST,MINTEST:read_address=p_address;
 QUAD,MASKEND:begin write_address=p_address;work_write=1;write_value=operand_a;end
 MASK,EVALSEND:begin
   read_address=p_address;write_address=read_address;
   work_write=lane!=3;
   if(lane==1)operand_b={{6{prepared_diagonal[29]}},prepared_diagonal};
   else operand_b=$signed(fx)<<<1;
 end
 NEXTQ:begin work_write=1;
   if(row_end)operand_b=$signed(fy)<<<2;
   else begin subtract=direction;operand_b=$signed(fx)<<<2;end
 end
 TILEADV:begin work_write=1;
   if(tx==384)begin read_address=s_address;operand_b=$signed(fy)<<<5;end
   else begin read_address=t_address;operand_b=$signed(fx)<<<5;end
   write_address=q_address;
 end
 default:begin end
 endcase
end

always @(posedge clk)begin
 if(ce && !reset)begin
  if(state==LOAD && take && cursor>=4 && cursor<=12)
    coefficient[cursor-4]<=packet_data[27:0];
  
if(state==LOAD && take)begin
 case(cursor)
 4,7,10:previous_fx<=incoming_coefficient;
 default:begin end
 endcase
 if(prepared_write)tile_scale[prepared_address]<=incoming_times30;
 if(diagonal_write)diagonal_scale[diagonal_address]<=incoming_diagonal;
end
if(work_write)work[write_address]<=write_value;
  if(state==INIT)seed[edge_number]<=-$signed(fx);
  else if(state==SEED)begin
   if(seedbit==8)seed[edge_number]<=-$signed(fy);
   else seed[edge_number]<=seed_value<<<1;
  end
 end
end

always @(posedge clk)begin
 if(reset)begin
  state<=LOAD;cursor<=0;edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;done<=0;fault<=0;
  tx<=0;ty<=0;qx<=0;qy<=0;direction<=0;lane<=0;held_mask<=0;full_tile<=0;
 end else if(ce)begin
  done<=0;
  case(state)
  LOAD:if(take)begin
   if(cursor==0)begin
    header<=packet_data;
    if(!packet_data[0])begin cursor<=0;done<=1;if(!packet_last)fault<=1;end
    else begin cursor<=1;if(packet_last)fault<=1;end
   end else begin
    case(cursor)
    1:if(packet_data!=((36'd399<<9)+(36'd239<<26)))fault<=1;
    2:if(packet_data!=(36'd400+(36'd240<<18)))fault<=1;
    3:if(packet_data!=9)fault<=1;
    22:near_limit[35:0]<=packet_data;
    23:near_limit[37:36]<=packet_data[1:0];
    25:area<=packet_data;
    26:dexp<=packet_data[7:0];
    27:begin align_shift[0]<=packet_data[5:0];align_shift[1]<=packet_data[11:6];align_shift[2]<=packet_data[17:12];end
    default:begin end
    endcase
    if(cursor==27)begin cursor<=0;state<=COMMIT;if(!packet_last)fault<=1;end
    else begin cursor<=cursor+1;if(packet_last)fault<=1;end
   end
  end
  COMMIT:begin state<=INIT;edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;end
  INIT:begin
   tx<=0;ty<=0;qx<=0;qy<=0;direction<=0;seedbit<=0;copy_strip<=1;
   if(edge_number==2)begin edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;state<=SEED;end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  end
  SEED:begin
   if(edge_number==2)begin
    edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;
    if(seedbit==16)state<=COPYT;else seedbit<=seedbit+1;
   end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  end
  COPYT:if(edge_number==2)begin edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;if(copy_strip)state<=COPYS;else state<=MAXX;end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  COPYS:if(edge_number==2)begin edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;state<=MAXX;end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  MAXX,MAXY,MINX,MINY:begin
   if(edge_number==2)begin edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;
    case(state)MAXX:state<=MAXY;MAXY:state<=MAXTEST;MINX:state<=MINY;MINY:state<=MINTEST;endcase
   end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  end
  MAXTEST,MINTEST,MASK,EVALSEND:begin
   sum_acc<=sum_next;positive_acc<=all_positive;
   nonnegative_acc<=all_nonnegative;inside_acc<=all_inside;
   if(state==EVALSEND)begin
    if(edge_number==1)out_n1<=aligned_value;
    if(edge_number==2)out_n2<=aligned_value;
   end
   if(edge_number==2)begin
    edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;

    if(state==EVALSEND)begin out_d<=sum_next;state<=SEND;end
    else begin decision_for<=state;state<=DECIDE;end

   end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  end

  DECIDE:case(decision_for)
   MAXTEST:begin
    if(!nonnegative_acc)state<=TILEADV;
    else begin max_den<=sum_acc;state<=MINX;end
   end
   MINTEST:begin full_tile<=positive_acc && sum_acc>0 && max_den<=near_limit;state<=QUAD;end
   MASK:begin
    held_mask[lane]<=inside_acc && passes_near;
    if(lane==3)state<=MASKEND;
    else begin lane<=lane+1;state<=MASK;end
   end
   default:begin fault<=1;state<=LOAD;end
  endcase
  QUAD:if(edge_number==2)begin
   edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;lane<=0;
   if(full_tile)begin held_mask<=15;state<=EVALSEND;end
   else begin held_mask<=0;state<=MASK;end
  end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  MASKEND:if(edge_number==2)begin
   edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;lane<=0;
   if(held_mask==0)begin if(tile_end)state<=TILEADV;else state<=NEXTQ;end
   else state<=EVALSEND;
  end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  SEND:if(pixel_ready)begin
   if(lane==3)begin if(tile_end)state<=TILEADV;else state<=NEXTQ;end
   else begin lane<=lane+1;state<=EVALSEND;end
  end
  NEXTQ:if(edge_number==2)begin
   edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;
   if(row_end)begin qy<=qy+2;direction<=!direction;end
   else if(direction)qx<=qx-2;else qx<=qx+2;
   state<=QUAD;
  end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  TILEADV:begin
   direction<=0;
   if(tx==384 && ty==224)begin state<=DRAIN;edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;end
   else if(edge_number==2)begin
    edge_number<=0;{parameter_x,parameter_y,parameter_b}<=12'h012;{q_address,p_address,t_address,s_address}<=16'h0369;copy_strip<=tx==384;
    if(tx==384)begin tx<=0;qx<=0;ty<=ty+16;qy<=ty+16;end
    else begin tx<=tx+16;qx<=tx+16;qy<=ty;end
    state<=COPYT;
   end else begin edge_number<=edge_number+1;
if(edge_number==0)begin
{parameter_x,parameter_y,parameter_b}<=12'h345;
{q_address,p_address,t_address,s_address}<=16'h147a;
end else begin
{parameter_x,parameter_y,parameter_b}<=12'h678;
{q_address,p_address,t_address,s_address}<=16'h258b;
end end
  end
  DRAIN:if(downstream_idle)begin state<=LOAD;done<=1;end
  default:begin state<=LOAD;fault<=1;end
  endcase
 end
end
endmodule




/* synthesis syn_dspstyle = "logic" */
/* synthesis syn_dspstyle = "logic" */
module interpolator_core(
input wire clk,
input wire [0:0] ce,
input wire [0:0] reset,
input wire [0:0] cfg_write,
input wire [2:0] cfg_index,
input wire [35:0] cfg_data,
input wire [0:0] cfg_commit,
input wire [0:0] cfg_color,
input wire [35:0] cfg_area,
input wire [7:0] cfg_depth_exp,
input wire [0:0] pixel_valid,
input wire [53:0] pixel_d,
input wire [53:0] pixel_n1,
input wire [53:0] pixel_n2,
input wire [8:0] pixel_x,
input wire [7:0] pixel_y,
input wire [1:0] pixel_lane,
input wire [3:0] pixel_mask,
input wire [14:0] pixel_origin,
input wire [0:0] result_ready,
output wire [0:0] cfg_ready,
output wire [0:0] pixel_ready,
output reg [0:0] result_valid,
output reg [8:0] result_x,
output reg [7:0] result_y,
output reg [1:0] result_lane,
output reg [3:0] result_mask,
output reg [14:0] result_origin,
output reg [35:0] result_normal,
output reg [31:0] result_ndc,
output reg [35:0] result_uv,
output reg [23:0] result_rgb,
output reg [15:0] result_depth,
output reg [0:0] result_coarse,
output reg [0:0] result_fault,
output wire [0:0] phase,
output wire [3:0] stage_valid,
output wire [17:0] rcp_product,
output wire [35:0] ratio_product,
output wire [35:0] depth_product
);
reg [1:0] step;assign phase=step[0];
reg signed [17:0] beta1_hold;reg beta1_bad;reg [23:0] world_hold;reg [15:0] depth3;
wire run_ce=ce && (!result_valid || result_ready);
reg loaded,color;reg [35:0] source[0:5];
reg signed [8:0] normal_delta0[0:2],normal_delta1[0:2];
reg signed [17:0] abase[0:5],delta0[0:5],delta1[0:5];
reg [8:0] dm0[0:5],dm1[0:5];reg [3:0] de0[0:5],de1[0:5];reg dn0[0:5],dn1[0:5];
reg [17:0] area_m;reg [5:0] area_e;reg signed [7:0] depth_exp;
reg v0,v1,v2,v3;assign stage_valid={v3,v2,v1,v0};
assign cfg_ready=!(v0||v1||v2||v3||result_valid);
assign pixel_ready=loaded && step==0 && run_ce && !cfg_write && !cfg_commit;
wire accept=pixel_valid && pixel_ready;
function [5:0] exponent54(input [53:0] a);integer k;begin exponent54=0;for(k=0;k<54;k=k+1)if(a[k])exponent54=k;end endfunction
function [17:0] mantissa54(input [53:0] a,input [5:0] e);begin if(e>=17)mantissa54=a>>(e-17);else mantissa54=a<<(17-e);end endfunction
function [53:0] magnitude54(input signed [53:0] a);begin if(a<0)magnitude54=-a;else magnitude54=a;end endfunction
function signed [47:0] round48(input signed [47:0] a,input signed [7:0] shift);
reg signed [47:0] q;reg [47:0] rem,mask,half;begin
if(shift<=0)round48=a<<(-shift);
else if(shift>=48)round48=0;
else begin q=a>>>shift;mask=(48'd1<<shift)-1;rem=a&mask;half=48'd1<<(shift-1);round48=q;if(rem>half || (rem==half && q[0]))round48=q+1;end
end endfunction
function [13:0] narrow9(input signed [17:0] a);
reg [17:0] mag;reg [5:0] e;reg signed [47:0] m;begin
if(a<0)mag=-a;else mag=a;e=exponent54({36'b0,mag});if(e>=9)e=e-8;else e=0;
m=round48({30'b0,mag},$signed({2'b0,e}));if(m==512)begin m=256;e=e+1;end
narrow9={a[17],e[3:0],m[8:0]};end endfunction
function signed [27:0] ratio_code(input [35:0] prod,input signed [7:0] diff,input neg);
reg signed [47:0] z;begin
if(diff>6)z=262144;else if(diff < -30)z=0;else z=round48({12'b0,prod},8'sd22-diff);
if(neg)ratio_code=-z;else ratio_code=z;
end endfunction
function signed [17:0] beta_limit(input signed [27:0] a);begin if(a>131071)beta_limit=131071;else if(a < -131072)beta_limit=-131072;else beta_limit=a;end endfunction
function [23:0] world_code(input [35:0] prod,input signed [7:0] shift);
reg [36:0] extended,rounded;reg [35:0] lowmask;reg sticky;
begin
extended={prod,1'b0}>>shift;
lowmask=~(36'hfffffffff<<(shift-1));sticky=|(prod&lowmask);
rounded={1'b0,extended[36:1]}+(extended[0] && (sticky || extended[1]));
if(shift<=0)world_code=24'd13041664;
else if(shift>36)world_code=0;
else if(rounded>13041664)world_code=24'd13041664;else world_code=rounded[23:0];
end endfunction
function [11:0] normal_code(input signed [47:0] a);reg signed [47:0] z;begin z=round48(a,12);if(z>2047)normal_code=2047;else if(z < -2048)normal_code=12'h800;else normal_code=z[11:0];end endfunction
function [7:0] color_code(input signed [47:0] a);reg signed [47:0] z;begin z=round48(a,12);if(z>255)color_code=255;else if(z<0)color_code=0;else color_code=z[7:0];end endfunction

// Packet field stepping is proved signed32 by the tight viewport scaler.
// The N1 and N2 normalizers share alternate enabled edges at the II2 rate.
reg [5:0] ed0;reg signed [35:0] raw_n1;
wire signed [35:0] normalize_n;
assign normalize_n=phase?raw_n1:$signed(pixel_n2[35:0]);
function [35:0] magnitude32(input signed [35:0] a);begin if(a<0)magnitude32=-a;else magnitude32=a;end endfunction
function [5:0] exponent32(input [35:0] a);integer j;begin exponent32=0;for(j=0;j<36;j=j+1)if(a[j])exponent32=j;end endfunction
function [17:0] mantissa32(input [35:0] a,input [5:0] e);begin if(e>=17)mantissa32=a>>(e-17);else mantissa32=a<<(17-e);end endfunction
wire [35:0] d_mag=pixel_d[53]?36'd1:pixel_d[35:0];
wire [5:0] in_de=exponent32(d_mag);
wire [35:0] normalize_mag=magnitude32(normalize_n);
wire [5:0] normalize_e=(phase?ed0:in_de)+6'd5;
wire [52:0] common_wide={normalize_mag,17'b0}>>normalize_e;
wire common_overflow=|common_wide[52:18];
wire [17:0] normalize_m=common_overflow?18'h3ffff:common_wide[17:0];
reg common_bad1;
wire [17:0] in_dm=mantissa32(d_mag,in_de);
wire [6:0] table_index=in_dm[16:10];
reg [8:0] table_slope;
always @*begin table_slope=0;case(table_index)
7'd0:table_slope=9'd508;
7'd1:table_slope=9'd500;
7'd2:table_slope=9'd493;
7'd3:table_slope=9'd485;
7'd4:table_slope=9'd478;
7'd5:table_slope=9'd470;
7'd6:table_slope=9'd464;
7'd7:table_slope=9'd457;
7'd8:table_slope=9'd450;
7'd9:table_slope=9'd444;
7'd10:table_slope=9'd438;
7'd11:table_slope=9'd431;
7'd12:table_slope=9'd425;
7'd13:table_slope=9'd419;
7'd14:table_slope=9'd413;
7'd15:table_slope=9'd408;
7'd16:table_slope=9'd402;
7'd17:table_slope=9'd396;
7'd18:table_slope=9'd390;
7'd19:table_slope=9'd386;
7'd20:table_slope=9'd380;
7'd21:table_slope=9'd376;
7'd22:table_slope=9'd370;
7'd23:table_slope=9'd366;
7'd24:table_slope=9'd360;
7'd25:table_slope=9'd356;
7'd26:table_slope=9'd352;
7'd27:table_slope=9'd347;
7'd28:table_slope=9'd342;
7'd29:table_slope=9'd338;
7'd30:table_slope=9'd334;
7'd31:table_slope=9'd330;
7'd32:table_slope=9'd326;
7'd33:table_slope=9'd322;
7'd34:table_slope=9'd318;
7'd35:table_slope=9'd314;
7'd36:table_slope=9'd310;
7'd37:table_slope=9'd306;
7'd38:table_slope=9'd303;
7'd39:table_slope=9'd299;
7'd40:table_slope=9'd296;
7'd41:table_slope=9'd292;
7'd42:table_slope=9'd289;
7'd43:table_slope=9'd285;
7'd44:table_slope=9'd282;
7'd45:table_slope=9'd278;
7'd46:table_slope=9'd276;
7'd47:table_slope=9'd272;
7'd48:table_slope=9'd269;
7'd49:table_slope=9'd266;
7'd50:table_slope=9'd264;
7'd51:table_slope=9'd260;
7'd52:table_slope=9'd258;
7'd53:table_slope=9'd254;
7'd54:table_slope=9'd252;
7'd55:table_slope=9'd249;
7'd56:table_slope=9'd246;
7'd57:table_slope=9'd244;
7'd58:table_slope=9'd241;
7'd59:table_slope=9'd238;
7'd60:table_slope=9'd236;
7'd61:table_slope=9'd234;
7'd62:table_slope=9'd231;
7'd63:table_slope=9'd229;
7'd64:table_slope=9'd226;
7'd65:table_slope=9'd224;
7'd66:table_slope=9'd222;
7'd67:table_slope=9'd220;
7'd68:table_slope=9'd217;
7'd69:table_slope=9'd216;
7'd70:table_slope=9'd212;
7'd71:table_slope=9'd211;
7'd72:table_slope=9'd208;
7'd73:table_slope=9'd206;
7'd74:table_slope=9'd205;
7'd75:table_slope=9'd202;
7'd76:table_slope=9'd200;
7'd77:table_slope=9'd198;
7'd78:table_slope=9'd197;
7'd79:table_slope=9'd194;
7'd80:table_slope=9'd193;
7'd81:table_slope=9'd191;
7'd82:table_slope=9'd190;
7'd83:table_slope=9'd188;
7'd84:table_slope=9'd186;
7'd85:table_slope=9'd184;
7'd86:table_slope=9'd182;
7'd87:table_slope=9'd181;
7'd88:table_slope=9'd179;
7'd89:table_slope=9'd177;
7'd90:table_slope=9'd176;
7'd91:table_slope=9'd174;
7'd92:table_slope=9'd172;
7'd93:table_slope=9'd171;
7'd94:table_slope=9'd170;
7'd95:table_slope=9'd168;
7'd96:table_slope=9'd166;
7'd97:table_slope=9'd165;
7'd98:table_slope=9'd164;
7'd99:table_slope=9'd162;
7'd100:table_slope=9'd160;
7'd101:table_slope=9'd160;
7'd102:table_slope=9'd158;
7'd103:table_slope=9'd156;
7'd104:table_slope=9'd156;
7'd105:table_slope=9'd154;
7'd106:table_slope=9'd153;
7'd107:table_slope=9'd151;
7'd108:table_slope=9'd150;
7'd109:table_slope=9'd148;
7'd110:table_slope=9'd148;
7'd111:table_slope=9'd146;
7'd112:table_slope=9'd145;
7'd113:table_slope=9'd144;
7'd114:table_slope=9'd142;
7'd115:table_slope=9'd142;
7'd116:table_slope=9'd140;
7'd117:table_slope=9'd139;
7'd118:table_slope=9'd138;
7'd119:table_slope=9'd137;
7'd120:table_slope=9'd136;
7'd121:table_slope=9'd134;
7'd122:table_slope=9'd134;
7'd123:table_slope=9'd132;
7'd124:table_slope=9'd132;
7'd125:table_slope=9'd130;
7'd126:table_slope=9'd130;
7'd127:table_slope=9'd128;
default:begin end endcase end
reg [8:0] x0,x1,x2,x3;reg [7:0] y0,y1,y2,y3;
reg [1:0] lane0,lane1,lane2,lane3;reg [3:0] mask0,mask1,mask2,mask3;reg [14:0] origin0,origin1,origin2,origin3;
reg [5:0] ed1,ed2;reg [5:0] e10,e11,e12,e20,e21,e22;
reg [17:0] nm10,nm11,nm20,nm21,R1;
reg ns10,ns11,ns12,ns20,ns21,ns22;reg bad0,bad1,bad2,bad3;
reg [8:0] fraction0,slope0;reg signed [15:0] ndcx0,ndcx1,ndcx2,ndcx3,ndcy1,ndcy2,ndcy3;
reg signed [17:0] beta1_2,beta1_3,beta2_3,b2hold;reg b2bad;reg [23:0] w2,w3;
wire [35:0] table_do,ndc_do;
wire [9:0] ndc_address=(step==1)?(10'd528+{2'b0,y0}):(10'd128+{1'b0,pixel_x});
wire ndc_read=(step==1 && v0)||(step==0 && accept);
DPX9B #(.BIT_WIDTH_0(18),.BIT_WIDTH_1(18),.READ_MODE0(0),.READ_MODE1(0),.WRITE_MODE0(0),.WRITE_MODE1(0),.RESET_MODE("SYNC"),.INIT_RAM_00(288'h7292dcd857432dd41d75deddae677975e1e2795d1e9137b301f07c7d119f8207f0220000),.INIT_RAM_01(288'h670b59ec9685b5a41a69b41a98f6b15daf286c80db4e86df5dbad06f74dc0e070fe5c71c),.INIT_RAM_02(288'h5d9f978a55eb497d065fd01818260f258618621b98acc634c18f9c64841948b65c39999a),.INIT_RAM_03(288'h55c7d58ed56b015c99579d9605858901642d5987d68175a8516c175b87d702e5c90d745d),.INIT_RAM_04(288'h4f2653e234fec1414150b5944705183547ae525514afd532b14e5e5405551d054e455555),.INIT_RAM_05(288'h4978927354a22d29e44ad012ca04b8092f684c349323e4ceb935224da6538144e6493b14),.INIT_RAM_06(288'h448d9135d4521915b245b811812465151a7c46ed51cf0478bd1f70482d121fb48d152492),.INIT_RAM_07(288'h40405020440c25041041465062541cc90842425510a6842df90c97436c90ecf43fbd1111),.INIT_RAM_08(288'h313d8c4a431148c40030eb8c35c30c28c2b830998c2143070cc1713047cc0cd301ecc029),.INIT_RAM_09(288'h32850c9c3325c4c91f32334c87b320a4c7d731e14c73331b84c68f318f4c5ec31668c548),.INIT_RAM_0A(288'h33ccccee133a3cce3d337b0cd9a33520ccf633290cc5233000cbae32d70cb0a32ae0ca66),.INIT_RAM_0B(288'h35148d40034eb8d35c34c28d2b834998d2143470cd1713447cd0cd341ecd02933f5ccf85),.INIT_RAM_0C(288'h365c4d91f36334d87b360a4d7d735e14d73335b84d68f358f4d5ec35668d548353d8d4a4),.INIT_RAM_0D(288'h37a3cde3d377b0dd9a37520dcf637290dc5237000dbae36d70db0a36ae0da6636850d9c3),.INIT_RAM_0E(288'h38eb8e35c38c28e2b838998e2143870ce1713847ce0cd381ece02937f5cdf8537cccdee1),.INIT_RAM_0F(288'h3a334e87b3a0a4e7d739e14e73339b84e68f398f4e5ec39668e548393d8e4a439148e400),.INIT_RAM_10(288'h3b7b0ed9a3b520ecf63b290ec523b000ebae3ad70eb0a3aae0ea663a850e9c33a5c4e91f),.INIT_RAM_11(288'h3cc28f2b83c998f2143c70cf1713c47cf0cd3c1ecf0293bf5cef853bccceee13ba3cee3d),.INIT_RAM_12(288'h3e0a4f7d73de14f7333db84f68f3d8f4f5ec3d668f5483d3d8f4a43d148f4003ceb8f35c),.INIT_RAM_13(288'h3f520fcf63f290fc523f000fbae3ed70fb0a3eae0fa663e850f9c33e5c4f91f3e334f87b),.INIT_RAM_14(288'h0099802140070c01710047c00cd001ec00293ff5cff853fcccfee13fa3cfe3d3f7b0fd9a),.INIT_RAM_15(288'h01e14073301b84068f018f405ec016680548013d804a401148040000eb8035c00c2802b8),.INIT_RAM_16(288'h032900c52030000bae02d700b0a02ae00a660285009c3025c4091f02334087b020a407d7),.INIT_RAM_17(288'h0470c11710447c10cd041ec102903f5c0f8503ccc0ee103a3c0e3d037b00d9a035200cf6),.INIT_RAM_18(288'h05b84168f058f415ec056681548053d814a405148140004eb8135c04c2812b8049981214),.INIT_RAM_19(288'h070001bae06d701b0a06ae01a660685019c3065c4191f06334187b060a417d705e141733),.INIT_RAM_1A(288'h0847c20cd081ec202907f5c1f8507ccc1ee107a3c1e3d077b01d9a075201cf6072901c52),.INIT_RAM_1B(288'h098f425ec096682548093d824a409148240008eb8235c08c2822b80899822140870c2171),.INIT_RAM_1C(288'h0ad702b0a0aae02a660a85029c30a5c4291f0a334287b0a0a427d709e14273309b84268f),.INIT_RAM_1D(288'h0c1ec30290bf5c2f850bccc2ee10ba3c2e3d0b7b02d9a0b5202cf60b2902c520b0002bae),.INIT_RAM_1E(288'h0d66835480d3d834a40d14834000ceb8335c0cc2832b80c99832140c70c31710c47c30cd),.INIT_RAM_1F(288'h0eae03a660e85039c30e5c4391f0e334387b0e0a437d70de1437330db84368f0d8f435ec),.INIT_RAM_20(288'h0ff5c3f850fccc3ee10fa3c3e3d0f7b03d9a0f5203cf60f2903c520f0003bae0ed703b0a),.INIT_RAM_21(288'h0def038440e33439550e7783a660ebbc3b770f0003c890f4443d9a0f8883eab0fccc3fbc),.INIT_RAM_22(288'h0bccc2fbc0c11030cd0c55431de0c99832ef0cddc34000d22435110d66836220daac3733),.INIT_RAM_23(288'h09aac273309ef028440a33429550a7782a660abbc2b770b0002c890b4442d9a0b8882eab),.INIT_RAM_24(288'h078881eab07ccc1fbc0811020cd0855421de0899822ef08ddc2400092242511096682622),.INIT_RAM_25(288'h05668162205aac173305ef01844063341955067781a6606bbc1b77070001c89074441d9a),.INIT_RAM_26(288'h034440d9a038880eab03ccc0fbc0411010cd0455411de0499812ef04ddc1400052241511),.INIT_RAM_27(288'h01224051101668062201aac073301ef00844023340955027780a6602bbc0b77030000c89),.INIT_RAM_28(288'h3f000fc893f444fd9a3f888feab3fcccffbc0011000cd0055401de0099802ef00ddc0400),.INIT_RAM_29(288'h3cddcf4003d224f5113d668f6223daacf7333def0f8443e334f9553e778fa663ebbcfb77),.INIT_RAM_2A(288'h3abbceb773b000ec893b444ed9a3b888eeab3bcccefbc3c110f0cd3c554f1de3c998f2ef),.INIT_RAM_2B(288'h38998e2ef38ddce40039224e51139668e62239aace73339ef0e8443a334e9553a778ea66),.INIT_RAM_2C(288'h36778da6636bbcdb7737000dc8937444dd9a37888deab37cccdfbc38110e0cd38554e1de),.INIT_RAM_2D(288'h34554d1de34998d2ef34ddcd40035224d51135668d62235aacd73335ef0d84436334d955),.INIT_RAM_2E(288'h32334c95532778ca6632bbccb7733000cc8933444cd9a33888ceab33ccccfbc34110d0cd),.INIT_RAM_2F(288'h30110c0cd30554c1de30998c2ef30ddcc40031224c51131668c62231aacc73331ef0c844),.INIT_RAM_30(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_31(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_32(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_33(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_34(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_35(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_36(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_37(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_38(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_39(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3A(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3B(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3C(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3D(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3E(288'h000000000000000000000000000000000000000000000000000000000000000000000000),.INIT_RAM_3F(288'h000000000000000000000000000000000000000000000000000000000000000000000000)) shared_table(.DOA(table_do),.DOB(ndc_do),.DIA(36'b0),.DIB(36'b0),.ADA({3'b0,table_index,4'b0}),.ADB({ndc_address,4'b0}),.BLKSELA(3'b0),.BLKSELB(3'b0),.CLKA(clk),.CLKB(clk),.CEA(run_ce && accept),.CEB(run_ce && ndc_read),.OCEA(1'b1),.OCEB(1'b1),.WREA(1'b0),.WREB(1'b0),.RESETA(reset),.RESETB(reset));
MULT9X9 #(.AREG(0),.BREG(0),.OUT_REG(1),.PIPE_REG(0),.ASIGN_REG(0),.BSIGN_REG(0),.SOA_REG(0),.MULT_RESET_MODE("SYNC")) reciprocal(.A(slope0),.B(fraction0),.SIA(9'b0),.SIB(9'b0),.ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(run_ce && step==1 && v0),.RESET(reset),.DOUT(rcp_product),.SOA(),.SOB());

wire [15:0] depth_delta;reg [17:0] arithmetic_a,arithmetic_b;
always @*begin
arithmetic_a=nm21;arithmetic_b=R1;
case(step)
1:arithmetic_a=nm11;
2:arithmetic_a=area_m;
3:begin arithmetic_a={2'b0,depth_delta};arithmetic_b=18'd84359;end
default:begin end endcase end
wire [35:0] arithmetic_product;
assign ratio_product=arithmetic_product;assign depth_product=arithmetic_product;
MULT18X18 #(.AREG(0),.BREG(0),.OUT_REG(1),.PIPE_REG(0),.ASIGN_REG(0),.BSIGN_REG(0),.SOA_REG(0),.MULT_RESET_MODE("SYNC")) arithmetic(
.A(arithmetic_a),.B(arithmetic_b),.SIA(18'b0),.SIB(18'b0),.ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b0),.BSIGN(1'b0),.CLK(clk),.CE(run_ce && ((step==3 && v2)||(step!=3 && v1))),.RESET(reset),.DOUT(arithmetic_product),.SOA(),.SOB());
wire signed [27:0] b1raw=ratio_code(ratio_product,8'sd5,ns11);
wire signed [27:0] b2raw=ratio_code(ratio_product,8'sd5,ns22);
wire signed [17:0] b2limited=beta_limit(b2raw);
wire signed [17:0] current_beta=phase?beta1_2:b2hold;
// Deliberate normal-only precision trade. UV/RGB/depth remain unchanged.
function signed [8:0] normal_delta(input signed [17:0] a);
reg signed [17:0] q;begin q=a>>>4;if(a[3] && ((|a[2:0]) || q[0]))q=q+1;
if(q>255)normal_delta=255;else if(q < -256)normal_delta=-256;else normal_delta=q[8:0];end endfunction
wire signed [17:0] normal_weight_floor=$signed(current_beta)>>>4;
wire signed [17:0] normal_weight_rounded=normal_weight_floor+$signed({17'b0,(current_beta[3] && ((|current_beta[2:0]) || current_beta[4]))});
reg [8:0] normal_weight;
always @*begin if(normal_weight_rounded<0)normal_weight=0;else if(normal_weight_rounded>511)normal_weight=511;else normal_weight=normal_weight_rounded[8:0];end

reg signed [35:0] acc[0:5],finished[0:5];
wire side_first=step[0];wire channel_last=(step==0 || step==3);
wire signed [17:0] attr_a=channel_last?(side_first?delta0[5]:delta1[5]):(side_first?delta0[3]:delta1[3]);
wire signed [17:0] attr_b=side_first?delta0[4]:delta1[4];
wire signed [8:0] normal_a=channel_last?(side_first?normal_delta0[2]:normal_delta1[2]):(side_first?normal_delta0[0]:normal_delta1[0]);
wire signed [8:0] normal_b=side_first?normal_delta0[1]:normal_delta1[1];
wire [35:0] attr_product_a,attr_product_b;wire [17:0] normal_product_a,normal_product_b;
wire issue_attr=run_ce && v2;
MULT18X18 #(.AREG(0),.BREG(0),.OUT_REG(1),.PIPE_REG(0),.ASIGN_REG(0),.BSIGN_REG(0),.SOA_REG(0),.MULT_RESET_MODE("SYNC")) attribute_a(
.A(attr_a),.B(current_beta),.SIA(18'b0),.SIB(18'b0),.ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b1),.BSIGN(1'b1),.CLK(clk),.CE(issue_attr),.RESET(reset),.DOUT(attr_product_a),.SOA(),.SOB());
MULT18X18 #(.AREG(0),.BREG(0),.OUT_REG(1),.PIPE_REG(0),.ASIGN_REG(0),.BSIGN_REG(0),.SOA_REG(0),.MULT_RESET_MODE("SYNC")) attribute_b(
.A(attr_b),.B(current_beta),.SIA(18'b0),.SIB(18'b0),.ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b1),.BSIGN(1'b1),.CLK(clk),.CE(issue_attr && !channel_last),.RESET(reset),.DOUT(attr_product_b),.SOA(),.SOB());
MULT9X9 #(.AREG(0),.BREG(0),.OUT_REG(1),.PIPE_REG(0),.ASIGN_REG(0),.BSIGN_REG(0),.SOA_REG(0),.MULT_RESET_MODE("SYNC")) normal_a_mul(
.A(normal_a),.B(normal_weight),.SIA(9'b0),.SIB(9'b0),.ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b1),.BSIGN(1'b0),.CLK(clk),.CE(issue_attr),.RESET(reset),.DOUT(normal_product_a),.SOA(),.SOB());
MULT9X9 #(.AREG(0),.BREG(0),.OUT_REG(1),.PIPE_REG(0),.ASIGN_REG(0),.BSIGN_REG(0),.SOA_REG(0),.MULT_RESET_MODE("SYNC")) normal_b_mul(
.A(normal_b),.B(normal_weight),.SIA(9'b0),.SIB(9'b0),.ASEL(1'b0),.BSEL(1'b0),.ASIGN(1'b1),.BSIGN(1'b0),.CLK(clk),.CE(issue_attr && !channel_last),.RESET(reset),.DOUT(normal_product_b),.SOA(),.SOB());
wire signed [35:0] normal_piece_a=$signed({{18{normal_product_a[17]}},normal_product_a})<<<8;
wire signed [35:0] normal_piece_b=$signed({{18{normal_product_b[17]}},normal_product_b})<<<8;
wire signed [35:0] final_value[0:5];
assign final_value[0]=finished[0];assign final_value[1]=finished[1];assign final_value[2]=acc[2]+normal_piece_a;
assign final_value[3]=finished[3];assign final_value[4]=finished[4];assign final_value[5]=acc[5]+$signed(attr_product_a);
wire signed [47:0] uq=round48(final_value[3],12),vq=round48(final_value[4],12);
wire uv_bad=!color && (uq < -131072 || uq>131071 || vq < -131072 || vq>131071);
wire covered3=mask3[lane3];
wire [15:0] w8=w2[23:8]+(w2[7] && (|w2[6:0] || w2[8]));
assign depth_delta=w8<32?16'd0:w8-16'd32;
wire [20:0] depth_rounded={1'b0,depth_product[35:16]}+(depth_product[15] && (|depth_product[14:0] || depth_product[16]));
integer i;reg signed [17:0] src0,src1,src2;reg [13:0] nn;
always @(posedge clk)begin
if(reset)begin loaded<=0;color<=0;step<=0;v0<=0;v1<=0;v2<=0;v3<=0;result_valid<=0;result_x<=0;result_y<=0;result_lane<=0;result_mask<=0;result_origin<=0;result_normal<=0;result_ndc<=0;result_uv<=0;result_rgb<=0;result_depth<=0;result_coarse<=0;result_fault<=0;beta1_hold<=0;beta1_bad<=0;world_hold<=0;depth3<=0;
for(i=0;i<6;i=i+1)begin source[i]<=0;abase[i]<=0;delta0[i]<=0;delta1[i]<=0;dm0[i]<=0;dm1[i]<=0;de0[i]<=0;de1[i]<=0;dn0[i]<=0;dn1[i]<=0;acc[i]<=0;finished[i]<=0;end
end else if(run_ce)begin
if(result_valid && result_ready)result_valid<=0;
if(cfg_write && cfg_ready)source[cfg_index]<=cfg_data;
if(cfg_commit && cfg_ready)begin
loaded<=1;color<=cfg_color;step<=0;depth_exp<=cfg_depth_exp;
area_e<=exponent54({18'b0,cfg_area});area_m<=mantissa54({18'b0,cfg_area},exponent54({18'b0,cfg_area}));
for(i=0;i<6;i=i+1)begin
if(i<3)begin src0=$signed(source[0]>>(12*i));src0={{6{src0[11]}},src0[11:0]};src1=$signed(source[2]>>(12*i));src1={{6{src1[11]}},src1[11:0]};src2=$signed(source[4]>>(12*i));src2={{6{src2[11]}},src2[11:0]};end
else if(cfg_color)begin src0=(source[1]>>(8*(i-3)))&255;src1=(source[3]>>(8*(i-3)))&255;src2=(source[5]>>(8*(i-3)))&255;end
else if(i<5)begin src0=(source[1]>>(16*(i-3)))&65535;src1=(source[3]>>(16*(i-3)))&65535;src2=(source[5]>>(16*(i-3)))&65535;end
else begin src0=0;src1=0;src2=0;end
abase[i]<=src0;delta0[i]<=src1-src0;delta1[i]<=src2-src0;if(i<3)begin normal_delta0[i]<=normal_delta(src1-src0);normal_delta1[i]<=normal_delta(src2-src0);end
nn=narrow9(src1-src0);dm0[i]<=nn[8:0];de0[i]<=nn[12:9];dn0[i]<=nn[13];
nn=narrow9(src2-src0);dm1[i]<=nn[8:0];de1[i]<=nn[12:9];dn1[i]<=nn[13];end
end else begin
step<=step+1;
if(step==0)begin
v0<=accept;v1<=v0;v2<=v1;v3<=v2;
if(accept)begin x0<=pixel_x;y0<=pixel_y;lane0<=pixel_lane;mask0<=pixel_mask;origin0<=pixel_origin;ed0<=in_de;raw_n1<=pixel_n1[35:0];e20<=normalize_e;nm20<=normalize_m;ns10<=pixel_n1[53];ns20<=pixel_n2[53];bad0<=pixel_d[53] || pixel_d==0 || common_overflow;slope0<=table_slope;fraction0<=in_dm[9:1];end
if(v0)begin x1<=x0;y1<=y0;lane1<=lane0;mask1<=mask0;origin1<=origin0;ed1<=ed0;e11<=e10;e21<=e20;nm11<=nm10;nm21<=nm20;ns11<=ns10;ns21<=ns20;bad1<=bad0 || common_bad1;R1<=table_do[17:0]-round48({30'b0,rcp_product},8);ndcx1<=ndcx0;ndcy1<=ndc_do[15:0];end
if(v1)begin x2<=x1;y2<=y1;lane2<=lane1;mask2<=mask1;origin2<=origin1;ed2<=ed1;e12<=e11;e22<=e21;ns12<=ns11;ns22<=ns21;beta1_2<=beta1_hold;bad2<=bad1 || beta1_bad;w2<=world_hold;ndcx2<=ndcx1;ndcy2<=ndcy1;end
if(v2)begin x3<=x2;y3<=y2;lane3<=lane2;mask3<=mask2;origin3<=origin2;beta1_3<=beta1_2;beta2_3<=b2hold;bad3<=bad2 || b2bad;w3<=w2;ndcx3<=ndcx2;ndcy3<=ndcy2;acc[2]<=($signed(abase[2])<<<12)+normal_piece_a;acc[5]<=($signed(abase[5])<<<12)+$signed(attr_product_a);if(depth_rounded>65535)depth3<=65535;else depth3<=depth_rounded[15:0];end
end else if(step==1)begin
if(v0)begin ndcx0<=ndc_do[15:0];e10<=normalize_e;nm10<=normalize_m;common_bad1<=common_overflow;end
if(v2)begin b2hold<=b2limited;b2bad<=b2raw>131071 || b2raw < -131072;end
if(v3)begin
result_valid<=1;result_x<=x3;result_y<=y3;result_lane<=lane3;result_mask<=mask3;result_origin<=origin3;
result_normal<={normal_code(final_value[2]),normal_code(final_value[1]),normal_code(final_value[0])};result_ndc<={ndcy3,ndcx3};
result_uv<=0;if(!color && !bad3 && !uv_bad)result_uv<={vq[17:0],uq[17:0]};
if(color)result_rgb<={color_code(final_value[5]),color_code(final_value[4]),color_code(final_value[3])};else result_rgb<=24'hffffff;
result_depth<=depth3;
result_coarse<=!covered3 && (bad3 || uv_bad);result_fault<=covered3 && (bad3 || uv_bad);
end end
if(step==2)begin
if(v1)begin beta1_hold<=beta_limit(b1raw);beta1_bad<=b1raw>131071 || b1raw < -131072;end
if(v2)begin
acc[0]<=($signed(abase[0])<<<12)+normal_piece_a;acc[1]<=($signed(abase[1])<<<12)+normal_piece_b;
acc[3]<=($signed(abase[3])<<<12)+$signed(attr_product_a);acc[4]<=($signed(abase[4])<<<12)+$signed(attr_product_b);
end end
if(step==3)begin
if(v1)world_hold<=world_code(arithmetic_product,8'sd18-depth_exp-($signed({2'b0,area_e})-$signed({2'b0,ed1})));
if(v2)begin
finished[0]<=acc[0]+normal_piece_a;finished[1]<=acc[1]+normal_piece_b;
finished[3]<=acc[3]+$signed(attr_product_a);finished[4]<=acc[4]+$signed(attr_product_b);
end end
end end end
endmodule

/* synthesis syn_dspstyle = "logic" */
/* synthesis syn_dspstyle = "logic" */
/* synthesis syn_dspstyle = "logic" */
/* synthesis syn_dspstyle = "logic" */
module triangle_preparer(input wire clk,ce,reset,input_valid,input_last,output_ready,
input wire [35:0] input_data,input wire input_color,
output wire input_ready,output_valid,output_last,busy,
output reg [35:0] output_data,output reg fault,input wire [71:0] shared_product,output wire [35:0] shared_a,shared_b,output wire shared_asign,shared_bsign);
localparam LOAD=0,ISSUE=1,CROSS=2,DETISSUE=3,DET=4,EMIT=5,WAITCROSS=6,WAITDET=7;
localparam STREAM=27,DSTREAM=28;
reg stream_v0,stream_v1;reg [4:0] stream_op0,stream_op1;
reg [4:0] state;reg [4:0] cursor;reg [4:0] op;reg [2:0] detop;
reg [3:0] vertex_a,vertex_b;wire [31:0] va,vb;wire [63:0] cr;
reg signed [59:0] first_product;
reg signed [90:0] determinant;
reg [35:0] ma,mb;reg asign,bsign;
assign shared_asign=asign;assign shared_bsign=bsign;
wire [71:0] product=shared_product;assign shared_a=ma;assign shared_b=mb;
localparam CINIT=8,CADD=9,CSTORE=10,FLOAD=11,FSHIFT=12,FSTORE=13,
AREA_INIT=14,AREA_SHIFT=15,AREA_ISSUE=16,AREA_WAIT=17,AREA_CAPTURE=18,
FACTOR_SHIFT=19,NEAR_ISSUE=20,NEAR_WAIT=21,NEAR_CAPTURE=22,NEAR_SHIFT=23,FINAL=24,CABS=25,CLENGTH=26;
reg [5:0] edge_shift[0:2],common_shift;
reg color_mode,degenerate;reg [1:0] term_index,add_index;
reg signed [68:0] coefficient_acc,quant_serial;reg [5:0] field_shift,quant_left;
reg quant_guard,quant_sticky;reg [6:0] peak,det_shift,factor_shift;
reg [2:0] edge_a_pos,edge_a_zero,edge_inclusive;
reg [90:0] area_work;reg area_guard,area_sticky;reg [35:0] area_rounded,area_value;
reg signed [7:0] depth_exp,near_shift;reg [71:0] near_work;reg near_guard,near_sticky;
reg [36:0] near_limit;
wire [31:0] vertex_di={{2{input_data[29]}},input_data[29:0]};
wire [71:0] field_do;wire [35:0] attr_do;
wire [3:0] field_rad=state==EMIT?cursor-4:cursor[3:0];
wire [3:0] attr_wad=cursor-9,attr_rad=cursor-13;
wire attr_write=ce && !reset && state==LOAD && input_valid && cursor>=9;
wire field_write=ce && !reset && (state==CSTORE || state==FSTORE);
wire signed [27:0] quant_rounded=quant_serial[27:0]+(quant_guard && (quant_sticky || quant_serial[0]));
wire [71:0] field_di=state==CSTORE?{{3{coefficient_acc[68]}},coefficient_acc}:{{44{quant_rounded[27]}},quant_rounded};
reg signed [59:0] oriented_cofactor;
always @*begin oriented_cofactor=$signed(cr[59:0]);
if(determinant[90] ^ (term_index==1))oriented_cofactor=-$signed(cr[59:0]);end
reg signed [59:0] oriented_hold;
wire signed [68:0] extended_cofactor={{9{oriented_hold[59]}},oriented_hold};
reg signed [68:0] coefficient_term;
always @*begin coefficient_term=extended_cofactor;
if(term_index==0)begin if(add_index==0)coefficient_term=extended_cofactor<<<7;else coefficient_term=extended_cofactor<<<6;end
else if(term_index==1)begin if(add_index==0)coefficient_term=extended_cofactor<<<8;else coefficient_term=extended_cofactor<<<6;end
else begin case(add_index)0:coefficient_term=extended_cofactor<<<6;1:coefficient_term=extended_cofactor<<<3;2:coefficient_term=extended_cofactor<<<1;default:coefficient_term=extended_cofactor;endcase end end
reg [68:0] coefficient_magnitude;reg [6:0] coefficient_peak;
function [6:0] bitlength72(input [71:0] value);
reg [63:0] r;reg [6:0] n;
begin
if(|value[71:64])begin r={56'b0,value[71:64]};n=64;end else begin r=value[63:0];n=0;end
if(|r[63:32])begin r={32'b0,r[63:32]};n=n+32;end
if(|r[31:16])begin r={48'b0,r[31:16]};n=n+16;end
if(|r[15:8])begin r={56'b0,r[15:8]};n=n+8;end
if(|r[7:4])begin r={60'b0,r[7:4]};n=n+4;end
if(|r[3:2])begin r={62'b0,r[3:2]};n=n+2;end
if(r[1])begin r={63'b0,r[1]};n=n+1;end
bitlength72=n+r[0];
end endfunction
wire [6:0] final_peak=coefficient_peak>peak?coefficient_peak:peak;
wire [5:0] row_shift=final_peak>33?final_peak-33:0;
wire [35:0] rounded_area={1'b0,area_work[34:0]}+(area_guard && (area_sticky || area_work[0]));
wire [37:0] rounded_near={1'b0,near_work[36:0]}+(near_guard && (near_sticky || near_work[0]));
always @*begin output_data=0;
if(!degenerate)case(cursor)
0:output_data={27'b0,edge_inclusive,color_mode,4'b0,1'b1};
1:output_data=36'd0+(36'd399<<9)+(36'd239<<26);
2:output_data=36'd400+(36'd240<<18);3:output_data=36'd9;
4,5,6,7,8,9,10,11,12:output_data=field_do[35:0];
13,14,15,16,17,18:output_data=attr_do;
22:output_data=near_limit[35:0];23:output_data={35'b0,near_limit[36]};
25:output_data=area_value;26:output_data={28'b0,depth_exp};27:output_data={18'b0,(field_shift-edge_shift[2]),(field_shift-edge_shift[1]),(field_shift-edge_shift[0])};default:begin end endcase end
assign input_ready=ce && state==LOAD;
assign output_valid=state==EMIT;
assign output_last=degenerate || cursor==27;
assign busy=state!=LOAD || cursor!=0;
always @*begin vertex_a=0;vertex_b=0;
if(state==STREAM)begin case(op)
5'd0:begin vertex_a=4'd4;vertex_b=4'd8;end
5'd1:begin vertex_a=4'd5;vertex_b=4'd7;end
5'd2:begin vertex_a=4'd5;vertex_b=4'd6;end
5'd3:begin vertex_a=4'd3;vertex_b=4'd8;end
5'd4:begin vertex_a=4'd3;vertex_b=4'd7;end
5'd5:begin vertex_a=4'd4;vertex_b=4'd6;end
5'd6:begin vertex_a=4'd7;vertex_b=4'd2;end
5'd7:begin vertex_a=4'd8;vertex_b=4'd1;end
5'd8:begin vertex_a=4'd8;vertex_b=4'd0;end
5'd9:begin vertex_a=4'd6;vertex_b=4'd2;end
5'd10:begin vertex_a=4'd6;vertex_b=4'd1;end
5'd11:begin vertex_a=4'd7;vertex_b=4'd0;end
5'd12:begin vertex_a=4'd1;vertex_b=4'd5;end
5'd13:begin vertex_a=4'd2;vertex_b=4'd4;end
5'd14:begin vertex_a=4'd2;vertex_b=4'd3;end
5'd15:begin vertex_a=4'd0;vertex_b=4'd5;end
5'd16:begin vertex_a=4'd0;vertex_b=4'd4;end
5'd17:begin vertex_a=4'd1;vertex_b=4'd3;end
default:begin end endcase end
else if(state==DSTREAM)vertex_a={2'b0,detop[2:1]};
ma={{6{va[29]}},va[29:0]};mb={{6{vb[29]}},vb[29:0]};asign=1;bsign=1;
if(state==DSTREAM)begin mb={{6{va[29]}},va[29:0]};
if(detop[0])ma={{12{cr[59]}},cr[59:36]};else begin ma=cr[35:0];asign=0;end end
if(state==AREA_ISSUE)begin ma=area_rounded;mb=36'd75;asign=0;bsign=0;end
if(state==NEAR_ISSUE)begin ma=area_value;mb=36'd2095362;asign=0;bsign=0;end end
wire vertex_write=ce && !reset && state==LOAD && input_valid && cursor<9;
wire cofactor_write=ce && !reset && (state==STREAM && stream_v1 && stream_op1[0]);
wire [3:0] cofactor_wad=stream_op1[4:1];
wire [3:0] cofactor_rad=(state==DSTREAM)?{2'b0,detop[2:1]}:cursor[3:0];
wire signed [59:0] cofactor_next=first_product-$signed(product[59:0]);
wire [63:0] cofactor_di={{4{cofactor_next[59]}},cofactor_next};
RAM16SDP4 va0(.DO(va[0 +: 4]),.DI(vertex_di[0 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_a),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 vb0(.DO(vb[0 +: 4]),.DI(vertex_di[0 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_b),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 va1(.DO(va[4 +: 4]),.DI(vertex_di[4 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_a),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 vb1(.DO(vb[4 +: 4]),.DI(vertex_di[4 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_b),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 va2(.DO(va[8 +: 4]),.DI(vertex_di[8 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_a),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 vb2(.DO(vb[8 +: 4]),.DI(vertex_di[8 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_b),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 va3(.DO(va[12 +: 4]),.DI(vertex_di[12 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_a),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 vb3(.DO(vb[12 +: 4]),.DI(vertex_di[12 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_b),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 va4(.DO(va[16 +: 4]),.DI(vertex_di[16 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_a),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 vb4(.DO(vb[16 +: 4]),.DI(vertex_di[16 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_b),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 va5(.DO(va[20 +: 4]),.DI(vertex_di[20 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_a),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 vb5(.DO(vb[20 +: 4]),.DI(vertex_di[20 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_b),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 va6(.DO(va[24 +: 4]),.DI(vertex_di[24 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_a),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 vb6(.DO(vb[24 +: 4]),.DI(vertex_di[24 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_b),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 va7(.DO(va[28 +: 4]),.DI(vertex_di[28 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_a),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 vb7(.DO(vb[28 +: 4]),.DI(vertex_di[28 +: 4]),.WAD(cursor[3:0]),.RAD(vertex_b),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 cofactor0(.DO(cr[0 +: 4]),.DI(cofactor_di[0 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor1(.DO(cr[4 +: 4]),.DI(cofactor_di[4 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor2(.DO(cr[8 +: 4]),.DI(cofactor_di[8 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor3(.DO(cr[12 +: 4]),.DI(cofactor_di[12 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor4(.DO(cr[16 +: 4]),.DI(cofactor_di[16 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor5(.DO(cr[20 +: 4]),.DI(cofactor_di[20 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor6(.DO(cr[24 +: 4]),.DI(cofactor_di[24 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor7(.DO(cr[28 +: 4]),.DI(cofactor_di[28 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor8(.DO(cr[32 +: 4]),.DI(cofactor_di[32 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor9(.DO(cr[36 +: 4]),.DI(cofactor_di[36 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor10(.DO(cr[40 +: 4]),.DI(cofactor_di[40 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor11(.DO(cr[44 +: 4]),.DI(cofactor_di[44 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor12(.DO(cr[48 +: 4]),.DI(cofactor_di[48 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor13(.DO(cr[52 +: 4]),.DI(cofactor_di[52 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor14(.DO(cr[56 +: 4]),.DI(cofactor_di[56 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 cofactor15(.DO(cr[60 +: 4]),.DI(cofactor_di[60 +: 4]),.WAD(cofactor_wad),.RAD(cofactor_rad),.WRE(cofactor_write),.CLK(clk));
RAM16SDP4 fields0(.DO(field_do[0 +: 4]),.DI(field_di[0 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields1(.DO(field_do[4 +: 4]),.DI(field_di[4 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields2(.DO(field_do[8 +: 4]),.DI(field_di[8 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields3(.DO(field_do[12 +: 4]),.DI(field_di[12 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields4(.DO(field_do[16 +: 4]),.DI(field_di[16 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields5(.DO(field_do[20 +: 4]),.DI(field_di[20 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields6(.DO(field_do[24 +: 4]),.DI(field_di[24 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields7(.DO(field_do[28 +: 4]),.DI(field_di[28 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields8(.DO(field_do[32 +: 4]),.DI(field_di[32 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields9(.DO(field_do[36 +: 4]),.DI(field_di[36 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields10(.DO(field_do[40 +: 4]),.DI(field_di[40 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields11(.DO(field_do[44 +: 4]),.DI(field_di[44 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields12(.DO(field_do[48 +: 4]),.DI(field_di[48 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields13(.DO(field_do[52 +: 4]),.DI(field_di[52 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields14(.DO(field_do[56 +: 4]),.DI(field_di[56 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields15(.DO(field_do[60 +: 4]),.DI(field_di[60 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields16(.DO(field_do[64 +: 4]),.DI(field_di[64 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 fields17(.DO(field_do[68 +: 4]),.DI(field_di[68 +: 4]),.WAD(cursor[3:0]),.RAD(field_rad),.WRE(field_write),.CLK(clk));
RAM16SDP4 attributes0(.DO(attr_do[0 +: 4]),.DI(input_data[0 +: 4]),.WAD(attr_wad),.RAD(attr_rad),.WRE(attr_write),.CLK(clk));
RAM16SDP4 attributes1(.DO(attr_do[4 +: 4]),.DI(input_data[4 +: 4]),.WAD(attr_wad),.RAD(attr_rad),.WRE(attr_write),.CLK(clk));
RAM16SDP4 attributes2(.DO(attr_do[8 +: 4]),.DI(input_data[8 +: 4]),.WAD(attr_wad),.RAD(attr_rad),.WRE(attr_write),.CLK(clk));
RAM16SDP4 attributes3(.DO(attr_do[12 +: 4]),.DI(input_data[12 +: 4]),.WAD(attr_wad),.RAD(attr_rad),.WRE(attr_write),.CLK(clk));
RAM16SDP4 attributes4(.DO(attr_do[16 +: 4]),.DI(input_data[16 +: 4]),.WAD(attr_wad),.RAD(attr_rad),.WRE(attr_write),.CLK(clk));
RAM16SDP4 attributes5(.DO(attr_do[20 +: 4]),.DI(input_data[20 +: 4]),.WAD(attr_wad),.RAD(attr_rad),.WRE(attr_write),.CLK(clk));
RAM16SDP4 attributes6(.DO(attr_do[24 +: 4]),.DI(input_data[24 +: 4]),.WAD(attr_wad),.RAD(attr_rad),.WRE(attr_write),.CLK(clk));
RAM16SDP4 attributes7(.DO(attr_do[28 +: 4]),.DI(input_data[28 +: 4]),.WAD(attr_wad),.RAD(attr_rad),.WRE(attr_write),.CLK(clk));
RAM16SDP4 attributes8(.DO(attr_do[32 +: 4]),.DI(input_data[32 +: 4]),.WAD(attr_wad),.RAD(attr_rad),.WRE(attr_write),.CLK(clk));

wire signed [90:0] high_term=$signed({{19{product[71]}},product})<<<36;
wire signed [90:0] low_term={{19{product[71]}},product};
wire det_high=stream_op1[0];
wire signed [90:0] det_term=det_high?high_term:low_term;
integer i;
always @(posedge clk)begin
if(reset)begin stream_v0<=0;stream_v1<=0;stream_op0<=0;stream_op1<=0;state<=LOAD;cursor<=0;op<=0;detop<=0;determinant<=0;fault<=0;degenerate<=0;color_mode<=0;term_index<=0;edge_inclusive<=0;end
else if(ce)begin
case(state)
LOAD:if(input_valid)begin if(cursor==0)begin color_mode<=input_color;degenerate<=0;end
if(cursor==14)begin cursor<=0;op<=0;stream_v0<=0;stream_v1<=0;state<=STREAM;if(!input_last)fault<=1;end
else begin cursor<=cursor+1;if(input_last)fault<=1;end end
STREAM:begin
stream_v0<=op<18;stream_v1<=stream_v0;stream_op0<=op;stream_op1<=stream_op0;
if(op<18)op<=op+1;
if(stream_v1)begin if(!stream_op1[0])first_product<=product[59:0];
if(stream_op1==17)begin detop<=0;determinant<=0;stream_v0<=0;stream_v1<=0;state<=DSTREAM;end end end
DSTREAM:begin
stream_v0<=detop<6;stream_v1<=stream_v0;stream_op0<={2'b0,detop};stream_op1<=stream_op0;
if(detop<6)detop<=detop+1;
if(stream_v1)begin determinant<=determinant+det_term;
if(stream_op1==5)begin cursor<=0;term_index<=0;peak<=0;common_shift<=0;stream_v0<=0;stream_v1<=0;state<=CINIT;end end end
CINIT:begin oriented_hold<=oriented_cofactor;if(term_index==0)peak<=0;coefficient_acc<=0;add_index<=0;state<=CADD;
if(cursor==0 && determinant==0)begin degenerate<=1;state<=EMIT;end end
CADD:begin coefficient_acc<=coefficient_acc+coefficient_term;
if((term_index!=2 && add_index==1)||(term_index==2 && add_index==3))state<=CABS;else add_index<=add_index+1;end
CABS:begin if(coefficient_acc<0)coefficient_magnitude<=-coefficient_acc;else coefficient_magnitude<=coefficient_acc;state<=CLENGTH;end
CLENGTH:begin coefficient_peak<=bitlength72(coefficient_magnitude)+(term_index==0?7'd9:term_index==1?7'd8:7'd10)+7'd2;state<=CSTORE;end
CSTORE:begin peak<=final_peak;if(term_index==2)begin edge_shift[cursor/3]<=row_shift;if(row_shift>common_shift)common_shift<=row_shift;end
if(cursor==8)begin if(row_shift>common_shift)field_shift<=row_shift;else field_shift<=common_shift;cursor<=0;term_index<=0;state<=FLOAD;end
else begin cursor<=cursor+1;if(term_index==2)term_index<=0;else term_index<=term_index+1;state<=CINIT;end end
FLOAD:begin quant_serial<=field_do;quant_left<=edge_shift[cursor/3];quant_guard<=0;quant_sticky<=0;
if(edge_shift[cursor/3]==0)state<=FSTORE;else state<=FSHIFT;end
FSHIFT:begin
if(quant_left>=8)begin quant_serial<=quant_serial>>>8;quant_guard<=quant_serial[7];quant_sticky<=quant_sticky || quant_guard || (|quant_serial[6:0]);quant_left<=quant_left-8;if(quant_left==8)state<=FSTORE;end
else if(1'b1)begin quant_serial<=quant_serial>>>1;quant_guard<=quant_serial[0];quant_sticky<=quant_sticky || quant_guard;quant_left<=quant_left-1;if(quant_left==1)state<=FSTORE;end
end
FSTORE:begin
if(term_index==0)begin edge_a_pos[cursor/3]<=!quant_rounded[27] && quant_rounded!=0;edge_a_zero[cursor/3]<=quant_rounded==0;end
if(term_index==1)edge_inclusive[cursor/3]<=edge_a_pos[cursor/3] || (edge_a_zero[cursor/3] && !quant_rounded[27] && quant_rounded!=0);
if(cursor==8)state<=AREA_INIT;else begin cursor<=cursor+1;if(term_index==2)term_index<=0;else term_index<=term_index+1;state<=FLOAD;end end
AREA_INIT:begin if(determinant[90])area_work<=-determinant;else area_work<=determinant;det_shift<=0;area_guard<=0;area_sticky<=0;state<=AREA_SHIFT;end
AREA_SHIFT:begin if(|area_work[90:35])begin
if((|area_work[90:42]))begin area_work<=area_work>>8;area_guard<=area_work[7];area_sticky<=area_sticky || area_guard || (|area_work[6:0]);det_shift<=det_shift+8;end
else if(1'b1)begin area_work<=area_work>>1;area_guard<=area_work[0];area_sticky<=area_sticky || area_guard;det_shift<=det_shift+1;end
end
else begin area_rounded<=rounded_area;state<=AREA_ISSUE;end end
AREA_ISSUE:state<=AREA_WAIT;AREA_WAIT:state<=AREA_CAPTURE;
AREA_CAPTURE:begin area_work<={19'b0,product};factor_shift<=0;area_guard<=0;area_sticky<=0;state<=FACTOR_SHIFT;end
FACTOR_SHIFT:begin if(|area_work[90:35])begin
if((|area_work[90:42]))begin area_work<=area_work>>8;area_guard<=area_work[7];area_sticky<=area_sticky || area_guard || (|area_work[6:0]);factor_shift<=factor_shift+8;end
else if(1'b1)begin area_work<=area_work>>1;area_guard<=area_work[0];area_sticky<=area_sticky || area_guard;factor_shift<=factor_shift+1;end
end
else begin area_value<=rounded_area;depth_exp<=$signed({1'b0,det_shift})+$signed({1'b0,factor_shift})-$signed({2'b0,field_shift})-8'sd10;state<=NEAR_ISSUE;end end
NEAR_ISSUE:state<=NEAR_WAIT;NEAR_WAIT:state<=NEAR_CAPTURE;
NEAR_CAPTURE:begin near_work<=product;near_shift<=8'sd18-depth_exp;near_guard<=0;near_sticky<=0;state<=NEAR_SHIFT;end
NEAR_SHIFT:begin
if(near_shift>0)begin
if(near_shift>=8)begin near_work<=near_work>>8;near_guard<=near_work[7];near_sticky<=near_sticky || near_guard || (|near_work[6:0]);near_shift<=near_shift-8;end
else if(1'b1)begin near_work<=near_work>>1;near_guard<=near_work[0];near_sticky<=near_sticky || near_guard;near_shift<=near_shift-1;end
end
else if(near_shift<0 && !(|near_work[71:36]))begin near_work<=near_work<<1;near_shift<=near_shift+1;end
else begin if((|near_work[71:37]) || near_shift<0 || rounded_near[37])near_limit<=37'h1fffffffff;else near_limit<=rounded_near[36:0];state<=FINAL;end end
FINAL:begin cursor<=0;state<=EMIT;end
EMIT:if(output_ready)begin if(degenerate || cursor==27)begin cursor<=0;state<=LOAD;end else cursor<=cursor+1;end
default:begin state<=LOAD;fault<=1;end
endcase end end
endmodule



/* synthesis syn_dspstyle = "logic" */
module vertex_preparer(input wire clk,ce,reset,input_valid,input_last,output_ready,
input wire [35:0] input_data,input wire matrix_write,input wire [4:0] matrix_address,input wire [31:0] matrix_data,
output wire input_ready,output_valid,output_last,busy,output reg [35:0] output_data,output reg fault,input wire [71:0] shared_product,output wire [35:0] shared_a,shared_b,output wire shared_asign,shared_bsign);
localparam LOAD=0,NSTORE=1,STREAM=2,EMIT=3;
reg [1:0] state;reg [2:0] cursor;reg [1:0] ni;reg [4:0] op,op0,op1;reg valid0,valid1;
reg [35:0] normal_source,attribute;reg signed [29:0] pos0,pos1,pos2;reg signed [11:0] norm0,norm1,norm2;
reg signed [64:0] acc;reg [3:0] vertex_address,write_address;wire [31:0] vertex_do,coeff0,coeff1;
reg [31:0] vertex_di;wire vertex_write=ce && !reset && ((state==LOAD && input_valid && cursor<3)||state==NSTORE);
wire coefficient_write=ce && !reset && matrix_write && !busy && matrix_address<21;
wire [31:0] coefficient=op[4]?coeff1:coeff0;
wire [35:0] ma={{4{coefficient[31]}},coefficient};
wire [35:0] mb=vertex_address==6?36'd1048576:{{4{vertex_do[31]}},vertex_do};
assign shared_asign=1'b1;assign shared_bsign=1'b1;
wire [71:0] product=shared_product;assign shared_a=ma;assign shared_b=mb;wire signed [64:0] signed_product=product[64:0];
wire first_term=op1==0 || op1==4 || op1==8 || op1==12 || op1==15 || op1==18;
wire signed [64:0] sum=acc+signed_product;
wire signed [44:0] rounded_floor=sum[64:20];
wire signed [44:0] rounded=rounded_floor+$signed({44'b0,(sum[19] && ((|sum[18:0]) || sum[20]))});
reg signed [29:0] position_code;reg signed [11:0] normal_code;
always @*begin
if(rounded>45'sd536870911)position_code=30'sd536870911;else if(rounded < -45'sd536870912)position_code=-30'sd536870912;else position_code=rounded[29:0];
if(rounded>2047)normal_code=2047;else if(rounded < -2048)normal_code=-2048;else normal_code=rounded[11:0];
end
assign input_ready=ce && state==LOAD;assign output_valid=state==EMIT;assign output_last=cursor==4;assign busy=state!=LOAD || cursor!=0;
always @*begin vertex_address=0;case(op)
5'd0:vertex_address=4'd0;
5'd1:vertex_address=4'd1;
5'd2:vertex_address=4'd2;
5'd3:vertex_address=4'd6;
5'd4:vertex_address=4'd0;
5'd5:vertex_address=4'd1;
5'd6:vertex_address=4'd2;
5'd7:vertex_address=4'd6;
5'd8:vertex_address=4'd0;
5'd9:vertex_address=4'd1;
5'd10:vertex_address=4'd2;
5'd11:vertex_address=4'd6;
5'd12:vertex_address=4'd3;
5'd13:vertex_address=4'd4;
5'd14:vertex_address=4'd5;
5'd15:vertex_address=4'd3;
5'd16:vertex_address=4'd4;
5'd17:vertex_address=4'd5;
5'd18:vertex_address=4'd3;
5'd19:vertex_address=4'd4;
5'd20:vertex_address=4'd5;
default:begin end endcase
write_address={1'b0,cursor};vertex_di={{2{input_data[29]}},input_data[29:0]};
if(state==NSTORE)begin write_address=3+ni;
case(ni)0:vertex_di={{20{normal_source[11]}},normal_source[11:0]};1:vertex_di={{20{normal_source[23]}},normal_source[23:12]};default:vertex_di={{20{normal_source[35]}},normal_source[35:24]};endcase end
output_data=attribute;case(cursor)0:output_data={{6{pos0[29]}},pos0};1:output_data={{6{pos1[29]}},pos1};2:output_data={{6{pos2[29]}},pos2};3:output_data={norm2,norm1,norm0};default:begin end endcase end
RAM16SDP4 vertex0(.DO(vertex_do[0 +: 4]),.DI(vertex_di[0 +: 4]),.WAD(write_address),.RAD(vertex_address),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 coefficient0_0(.DO(coeff0[0 +: 4]),.DI(matrix_data[0 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b0),.CLK(clk));
RAM16SDP4 coefficient1_0(.DO(coeff1[0 +: 4]),.DI(matrix_data[0 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b1),.CLK(clk));
RAM16SDP4 vertex1(.DO(vertex_do[4 +: 4]),.DI(vertex_di[4 +: 4]),.WAD(write_address),.RAD(vertex_address),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 coefficient0_1(.DO(coeff0[4 +: 4]),.DI(matrix_data[4 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b0),.CLK(clk));
RAM16SDP4 coefficient1_1(.DO(coeff1[4 +: 4]),.DI(matrix_data[4 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b1),.CLK(clk));
RAM16SDP4 vertex2(.DO(vertex_do[8 +: 4]),.DI(vertex_di[8 +: 4]),.WAD(write_address),.RAD(vertex_address),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 coefficient0_2(.DO(coeff0[8 +: 4]),.DI(matrix_data[8 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b0),.CLK(clk));
RAM16SDP4 coefficient1_2(.DO(coeff1[8 +: 4]),.DI(matrix_data[8 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b1),.CLK(clk));
RAM16SDP4 vertex3(.DO(vertex_do[12 +: 4]),.DI(vertex_di[12 +: 4]),.WAD(write_address),.RAD(vertex_address),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 coefficient0_3(.DO(coeff0[12 +: 4]),.DI(matrix_data[12 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b0),.CLK(clk));
RAM16SDP4 coefficient1_3(.DO(coeff1[12 +: 4]),.DI(matrix_data[12 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b1),.CLK(clk));
RAM16SDP4 vertex4(.DO(vertex_do[16 +: 4]),.DI(vertex_di[16 +: 4]),.WAD(write_address),.RAD(vertex_address),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 coefficient0_4(.DO(coeff0[16 +: 4]),.DI(matrix_data[16 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b0),.CLK(clk));
RAM16SDP4 coefficient1_4(.DO(coeff1[16 +: 4]),.DI(matrix_data[16 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b1),.CLK(clk));
RAM16SDP4 vertex5(.DO(vertex_do[20 +: 4]),.DI(vertex_di[20 +: 4]),.WAD(write_address),.RAD(vertex_address),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 coefficient0_5(.DO(coeff0[20 +: 4]),.DI(matrix_data[20 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b0),.CLK(clk));
RAM16SDP4 coefficient1_5(.DO(coeff1[20 +: 4]),.DI(matrix_data[20 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b1),.CLK(clk));
RAM16SDP4 vertex6(.DO(vertex_do[24 +: 4]),.DI(vertex_di[24 +: 4]),.WAD(write_address),.RAD(vertex_address),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 coefficient0_6(.DO(coeff0[24 +: 4]),.DI(matrix_data[24 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b0),.CLK(clk));
RAM16SDP4 coefficient1_6(.DO(coeff1[24 +: 4]),.DI(matrix_data[24 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b1),.CLK(clk));
RAM16SDP4 vertex7(.DO(vertex_do[28 +: 4]),.DI(vertex_di[28 +: 4]),.WAD(write_address),.RAD(vertex_address),.WRE(vertex_write),.CLK(clk));
RAM16SDP4 coefficient0_7(.DO(coeff0[28 +: 4]),.DI(matrix_data[28 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b0),.CLK(clk));
RAM16SDP4 coefficient1_7(.DO(coeff1[28 +: 4]),.DI(matrix_data[28 +: 4]),.WAD(matrix_address[3:0]),.RAD(op[3:0]),.WRE(coefficient_write && matrix_address[4]==1'b1),.CLK(clk));

always @(posedge clk)begin
if(reset)begin state<=LOAD;cursor<=0;ni<=0;op<=0;valid0<=0;valid1<=0;op0<=0;op1<=0;acc<=0;fault<=0;end
else if(ce)case(state)
LOAD:if(input_valid)begin
if(cursor==3)normal_source<=input_data;
if(cursor==4)begin attribute<=input_data;cursor<=0;ni<=0;state<=NSTORE;if(!input_last)fault<=1;end
else begin cursor<=cursor+1;if(input_last)fault<=1;end end
NSTORE:if(ni==2)begin op<=0;valid0<=0;valid1<=0;state<=STREAM;end else ni<=ni+1;
STREAM:begin valid0<=op<21;valid1<=valid0;op0<=op;op1<=op0;if(op<21)op<=op+1;
if(valid1)begin if(first_term)acc<=signed_product;else acc<=sum;
case(op1)
3:begin pos0<=position_code;if(rounded>45'sd536870911 || rounded < -45'sd536870912)fault<=1;end
7:begin pos1<=position_code;if(rounded>45'sd536870911 || rounded < -45'sd536870912)fault<=1;end
11:begin pos2<=position_code;if(rounded>45'sd536870911 || rounded < -45'sd536870912)fault<=1;end
14:norm0<=normal_code;17:norm1<=normal_code;
20:begin norm2<=normal_code;cursor<=0;state<=EMIT;end
default:begin end endcase end end
EMIT:if(output_ready)begin if(cursor==4)begin cursor<=0;state<=LOAD;end else cursor<=cursor+1;end
default:begin state<=LOAD;fault<=1;end endcase end endmodule
