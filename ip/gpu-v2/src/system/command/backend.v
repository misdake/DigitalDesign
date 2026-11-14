module gpu_v2_vliw_backend_three_port(
`undef GPU_V2_COMMAND_JOIN
`define GPU_V2_COMMAND_JOIN
`ifdef GPU_V2_COMMAND_MAINTENANCE
 input ui_active,ui_cmd_valid,ui_cmd_write,ui_lower_ready,input[8:0]ui_tile,input copy_active,input[13:0]copy_address,output ui_cmd_ready,ui_cmd_done,input ui_pixel_valid,ui_pixel_half,input[4:0]ui_pixel_word,input[63:0]ui_pixels,
 input[13:0]color_base,depth_base,
`endif
`ifdef GPU_V2_COMMAND_PORTS
 input[31:0]quad_color,
`endif
 input wire clk,reset,ce,abort,
 input wire front_ce,input wire[1:0]phase,
`ifdef GPU_V2_LIVE_GLOBALS
input wire draw_unlit,
`else
input wire draw_bank,draw_unlit,
`endif

 input wire quad_valid,output wire quad_ready,output wire[2:0]quad_id,
 input wire[7:0]quad_x,input wire[6:0]quad_y,input wire quad_rgb,
 input wire quad_end_valid,input wire[2:0]quad_end_id,input wire[3:0]quad_end_mask,
 input wire result_we,input wire[6:0]result_wa,input wire[35:0]result_wd,
 input wire depth_event_valid,input wire[4:0]depth_event_pid,input wire[15:0]depth_event_z16,input wire depth_event_pass,
 input wire lod_valid,input wire[2:0]lod_id,input wire[7:0]lod_value,
 output wire empty_skipped,output wire[5:0]producer_used,
 input[15:0]texture_base,input[3:0]texture_n,input texture_mip,texture_rgba4444,texture_clamp,input texture_fb400240,input[1:0]texture_filter,input texture_invalidate,input wire[23:0]specular,
`ifndef GPU_V2_COMMAND_PORTS
input wire[7:0]alpha,
`endif

 input wire[2:0]depth_func,input wire depth_write,
`ifdef GPU_V2_LIVE_GLOBALS
blend,
`else
blend,draw_rebind,
`endif

 input wire[15:0]context_epoch,input wire[1:0]context_mode,input wire[4:0]context_code,
 input wire signed[15:0]light_x,light_y,light_z,ray_x,ray_y,ray_k,
 input wire[8:0]ambient,directional,
 input wire flush_request,output wire flush_complete,pipeline_idle,idle,fault,
 output wire published,output wire[4:0]published_slot,
 output wire light_result,sample_result,final_issue,final_result,quad_accepted,commit_valid,
 output wire[8:0]commit_x,output wire[7:0]commit_y,output wire[3:0]commit_mask,
 output wire[5:0]global_used,output wire[4:0]work_reserved,work_count,
 output wire texture_request_valid,output wire[31:0]texture_request_address,
 input wire texture_request_ready,texture_read_valid,input wire[3:0]texture_read_index,
 input wire[63:0]texture_read_data,input wire texture_complete,texture_ok,
 output wire framebuffer_request_valid,framebuffer_request_write,output wire[31:0]framebuffer_request_address,
 input wire framebuffer_request_ready,framebuffer_read_valid,input wire[63:0]framebuffer_read_data,
 output wire framebuffer_write_valid,output wire[63:0]framebuffer_write_data,
 input wire framebuffer_write_ready,framebuffer_complete,framebuffer_ok
);
 wire [53:0] shared_rop_product;wire shared_rop_offer,shared_rop_ce;wire [26:0] shared_rop_a;wire[7:0]shared_rop_b;
 wire [50:0] shared_spec_product;wire shared_spec_ce;wire[8:0]shared_spec_a;wire[23:0]shared_spec_b;
 gpu_v2_final_rop_bank shared_bank(.clk(clk),.reset(reset),.rop_ce(shared_rop_ce),.rop_a(shared_rop_a),.rop_b(shared_rop_b),.rop_product(shared_rop_product),
 .spec_ce(shared_spec_ce),.spec_a(shared_spec_a),.spec_b(shared_spec_b),.spec_product(shared_spec_product));
 wire front_fault,front_bad,advance,public_credit,cache_drained;wire compute_ce;
 reg shade_phase;
 always @(posedge clk)if(reset)shade_phase<=0;else if(advance)shade_phase<=!shade_phase;wire raw_retire_proposal;
 wire sink_fault,sink_idle,final_fault,sink_bad,final_bad;
 wire raw_head_retire;
 wire coalition_allow=compute_ce;
 reg stopped;
 assign fault=abort||stopped;
 assign compute_ce=ce&&!fault;
 wire[22:0]published_begin;
 wire framebuffer_prefetch_valid;wire[9:0]framebuffer_prefetch_tile;
 wire basic_read,basic_collision;
 wire empty_head,empty_skip_ready;
 // No lookahead Basic/Light/Sample read is allowed for a zero-mask slot.
 // The empty head retires only after all previous head tokens have drained.
 wire empty_skip;
 assign empty_skipped=empty_skip;
 wire final_in_ready,final_out_valid;wire[23:0]final_RGB;wire[7:0]final_alpha;

`ifdef GPU_V2_COMMAND_PORTS
wire[23:0]issue_color;
`endif
wire[6:0]basic_address;wire[23:0]basic_data;
`ifdef GPU_V2_COMMAND_PORTS
wire[23:0]depth_data;
`else
wire[15:0]depth_data;
`endif

 wire sink_color_accept;wire[1:0]out_lane;
 reg[4:0]issue_slot;wire[22:0]issue_descriptor,head_descriptor,next_descriptor;wire[8:0]head_tile,next_tile;wire[4:0]retire_slot;wire[5:0]light_used,sample_used;
 wire lv,lr,ll;wire[6:0]lk;wire[8:0]lg,lh;
 wire sv,sr,sl,sfire,public_publish;wire[5:0]sk;wire[23:0]sRGB;wire[7:0]sAlpha;
 wire[15:0]public_live;wire[18:0]sample_descriptor;
 wire sample_fire;wire[1:0]sample_row;
 wire txreq,txready;wire[31:0]txaddr;
 wire txbeat,txcomplete;wire[3:0]txindex;
 gpu_v2_vliw_sampling front(
`ifdef GPU_V2_COMMAND_PORTS
.quad_color(quad_color),.issue_color(issue_color),
`endif

 .prefetch_valid(framebuffer_prefetch_valid),.prefetch_tile(framebuffer_prefetch_tile),
 .clk(clk),.reset(reset),.ce(ce),.light_compute_tick(compute_ce),.coalition_allow(coalition_allow),.abort(abort),
 .front_ce(front_ce&&!fault),.producer_phase(phase),
`ifndef GPU_V2_LIVE_GLOBALS
.draw_bank(draw_bank),
`endif
.draw_unlit(draw_unlit),
 .quad_valid(quad_valid),.quad_ready(quad_ready),.quad_id(quad_id),.quad_x(quad_x),.quad_y(quad_y),.quad_rgb(quad_rgb),
 .quad_end_valid(quad_end_valid),.quad_end_id(quad_end_id),.quad_end_mask(quad_end_mask),
 .result_we(result_we),.result_wa(result_wa),.result_wd(result_wd),
 .depth_event_valid(depth_event_valid),.depth_event_pid(depth_event_pid),.depth_event_z16(depth_event_z16),.depth_event_pass(depth_event_pass),
 .lod_valid(lod_valid),.lod_id(lod_id),.lod_value(lod_value),
 .empty_skip(empty_skip),.empty_head(empty_head),.empty_skip_ready(empty_skip_ready),
 .advance(advance),.preflight_bad(front_bad),.fault(front_fault),
 .published(published),.published_slot(published_slot),.published_begin(published_begin),
 .descriptor_issue_slot(issue_slot),.descriptor_issue(issue_descriptor),.descriptor_head(head_descriptor),.descriptor_next(next_descriptor),.descriptor_head_tile(head_tile),.descriptor_next_tile(next_tile),
 .basic_read(basic_read),.basic_read_address(basic_address),
 .basic_return_data(basic_data),.basic_collision(basic_collision),
 .depth_read(sink_color_accept),.depth_read_address({retire_slot,out_lane}),.depth_data(depth_data),
 .rop_row7_accepted(raw_retire_proposal),.rop_slot(retire_slot),.retire_slot(retire_slot),
 .global_used(global_used),.light_used(light_used),.sample_used(sample_used),.producer_used(producer_used),
 .public_credit(public_credit),.texture_base(texture_base),.texture_n(texture_n),.texture_mip(texture_mip),.texture_rgba4444(texture_rgba4444),.texture_clamp(texture_clamp),.texture_filter(texture_filter),.texture_fb400240(texture_fb400240),.texture_invalidate(texture_invalidate),
 .sample_fire(sample_fire),.sample_row(sample_row),.sample_descriptor(sample_descriptor),
 .packet_enable(1'b1),.result_ready(sr),.result_valid(sv),.result_key(sk),.result_rgb(sRGB),.result_alpha(sAlpha),
 .work_reserved(work_reserved),.work_count(work_count),
 .result_quad_last(sl),.result_fire(sfire),.public_publish(public_publish),.public_live(public_live),
 .mem_req_valid(txreq),.mem_req_addr(txaddr),.mem_req_ready(txready),
 .mem_resp_valid(txbeat),.mem_resp_index(txindex),.mem_resp_data(texture_read_data),
 .mem_resp_complete(txcomplete),.mem_resp_ok(texture_ok),.cache_drained(cache_drained),

`ifndef GPU_V2_LIVE_GLOBALS
 .light_bank(),.retire_valid(draw_rebind&&pipeline_idle),.retire_bank(1'b0),
`endif

 .context_epoch(context_epoch),.context_mode(context_mode),.context_code(context_code),
 .light_x(light_x),.light_y(light_y),.light_z(light_z),.ambient(ambient),.directional(directional),
 .ray_x(ray_x),.ray_y(ray_y),.ray_k(ray_k),
 .out_valid(lv),.out_ready(lr),.out_key(lk),.out_quad_last(ll),.out_g(lg),.out_h(lh),
 // Optional sampling/debug views do not participate in this parent protocol.
 .sample_ready(),.sample_data(),.raw_metadata(),.raw_write(),.committed(),.context_slot(),.phase(),.sample_out_valid(),.sample_out_ready(),.packet_out_ready(),.packet_out_valid(),.coefficient_accept(),.packet_out_payload(),.producer_count(),.unwritten_count(),.group_count(),.row_count(),.sample_out_owner(),.sample_out_result(),.context_state(),.c_occupied(),.c_completed(),.d_issue(),.l_issue(),.c_issue(),.d_return(),.l_return(),.c_return(),.uv_write(),.uv_read(),.uv_write_address(),.uv_read_address(),.pixel_accepted(),.out_epoch(),
`ifndef GPU_V2_LIVE_GLOBALS
 .context_wait(),.context_accepted(),
`endif
 .sample_valid());
 // Public low four slot bits are already in sk; only the global upper bit is missing.
 (* syn_ramstyle="distributed_ram" *)reg public_upper[0:15];
 wire[6:0]sample_address={public_upper[sk[5:2]],sk};

 reg light_complete,sample_complete;reg[4:0]light_last,sample_last;

 assign lr=compute_ce;assign sr=compute_ce;
 assign light_result=advance&&lv&&lr;assign sample_result=sfire;
 reg active;reg[22:0]header;wire[8:0]header_tile={header[22:19],header[15:11]};
 reg state;reg[1:0]issue_lane;
 reg[1:0]input_bypass;
 localparam J_READ=0,J_WAIT=1;
 wire issue_ahead=(issue_slot!=retire_slot);
 wire[22:0]issue_header=issue_descriptor;
 wire issue_exists=issue_slot!=published_slot;
 // For the sole next quad, an ordered completed prefix must extend past
 // the current retirement head. Bypasses need no branch result ownership.
 wire issue_light_ready=issue_header[5]||(light_complete&&(!issue_ahead||light_last!=retire_slot));
 wire issue_sample_ready=issue_header[6]||(sample_complete&&(!issue_ahead||sample_last!=retire_slot));
 wire covered=issue_header[issue_lane];wire out_covered=header[out_lane];
 wire[6:0]read_key={issue_slot,issue_lane};
 wire source_exists=active&&(|issue_header[3:0])&&issue_exists;
 wire issue_room=!issue_ahead||issue_lane!=3;
 wire join_value_read=source_exists&&state==J_READ&&covered&&issue_light_ready&&issue_sample_ready;
 // READ_MODE0 updates all three Qs on the J_READ edge. J_WAIT itself
 // owns complete source Q until Final accepts; CE0 freezes state and Q.
 // The cursor may enter the next quad, but cannot cross its final lane.
 wire join_issue=active&&state==J_WAIT&&final_in_ready&&issue_room;
 // A masked lane owns no arithmetic operands: advance its ordered prefix
 // directly, with no source RAM read, paid return or J_WAIT transition.
 wire skip_issue=source_exists&&state==J_READ&&!covered&&issue_room;
 wire source_advance=join_issue||skip_issue;
 wire read_values=join_value_read;
 assign basic_read=join_value_read;
 assign basic_address=read_key;
 // This ordered prefix also covers lanes which produce no Final token.
 // A lane cannot reach ROP until its real source/issue cursor has passed it.
 wire output_issued=issue_ahead||(issue_lane>out_lane);
 wire light_collision,sample_collision;
 wire[17:0]light_data;wire[31:0]sample_data;
 gpu_v2_postquad_light_bank light_bank(
 .clk(clk),.reset(reset),.read_en(advance&&read_values),.read_address(read_key),
 .write_en(light_result),.write_address(lk),.write_data({lh,lg}),
 .return_data(light_data),.collision(light_collision));
 gpu_v2_postquad_sample_bank sample_bank(
 .clk(clk),.reset(reset),.read_en(advance&&read_values),.read_address(read_key),
 .write_en(sample_result),.write_address(sample_address),.write_data({sAlpha,sRGB}),
 .return_data(sample_data),.collision(sample_collision));

 wire final_in_valid=active&&state==J_WAIT&&issue_room;
 wire[8:0]final_g=input_bypass[0]?9'd256:light_data[8:0];
 wire[8:0]final_h=input_bypass[0]?9'd0:light_data[17:9];

`ifdef GPU_V2_COMMAND_PORTS
wire[23:0]final_texture=input_bypass[1]?basic_data:sample_data[23:0];
`else
wire[23:0]final_texture=input_bypass[1]?24'hffffff:sample_data;
`endif

 wire color_paid;
 assign empty_skip=advance&&!active&&empty_head&&empty_skip_ready&&
  issue_slot==retire_slot&&state==J_READ&&issue_lane==0&&!color_paid&&!final_out_valid;
 wire lane_valid=active&&output_issued&&(!out_covered||final_out_valid||color_paid);
 wire lane_ready,lane_accept,sink_color_ready;assign raw_retire_proposal=ce&&lane_valid&&lane_ready&&out_lane==3;
 wire final_out_ready=lane_valid&&sink_color_ready&&out_covered;
 gpu_v2_postquad_final final_color(
 .shared_rop_offer(shared_rop_offer),.shared_spec_product(shared_spec_product),.shared_spec_ce(shared_spec_ce),.shared_spec_a(shared_spec_a),.shared_spec_b(shared_spec_b),
 .shade_phase(shade_phase),
 .clk(clk),.reset(reset),.ce(advance),.proposal_ce(ce),.commit_allow(coalition_allow),.preflight_bad(final_bad),.abort(abort),
 .in_valid(final_in_valid),.in_ready(final_in_ready),

`ifdef GPU_V2_COMMAND_PORTS
.in_tint(issue_color),
`else
.in_tint(basic_data),
`endif
`ifdef GPU_V2_COMMAND_PORTS
.in_tex_alpha(input_bypass[1]?8'd255:sample_data[31:24]),.out_tex_alpha(final_alpha),
`endif
.in_tex(final_texture),.in_spec(specular),.in_g(final_g),.in_h(final_h),
 .out_valid(final_out_valid),.out_ready(final_out_ready),.out_rgb(final_RGB),.fault(final_fault));
 assign final_issue=advance&&final_in_valid&&final_in_ready;
 assign final_result=sink_color_accept&&out_covered;
 wire fbreq,fbwrite,fbready,fbbeat,fbcomplete,fbwvalid,fbwready;wire[31:0]fbaddr;wire[63:0]fbwdata;
 gpu_v2_pixel_framebuffer_sink sink(
`ifdef GPU_V2_COMMAND_MAINTENANCE
 .ui_active(ui_active),.ui_cmd_valid(ui_cmd_valid),.ui_cmd_write(ui_cmd_write),.ui_lower_ready(ui_lower_ready),.ui_tile(ui_tile),.copy_active(copy_active),.copy_address(copy_address),.ui_cmd_ready(ui_cmd_ready),.ui_cmd_done(ui_cmd_done),.ui_pixel_valid(ui_pixel_valid),.ui_pixel_half(ui_pixel_half),.ui_pixel_word(ui_pixel_word),.ui_pixels(ui_pixels),
 .color_base(color_base),.depth_base(depth_base),
`endif
 .depth_phase(color_paid),
 .prefetch_valid(framebuffer_prefetch_valid),.prefetch_tile(framebuffer_prefetch_tile),.prefetch_abort(abort||stopped),
 .shared_rop_product(shared_rop_product),.shared_rop_offer(shared_rop_offer),.shared_rop_ce(shared_rop_ce),.shared_rop_a(shared_rop_a),.shared_rop_b(shared_rop_b),
 .shade_phase(shade_phase),
 .clk(clk),.reset(reset),.ce(ce),.commit_allow(coalition_allow),.preflight_bad(sink_bad),.lane_valid(lane_valid),.active_lane(out_lane),

`ifdef GPU_V2_COMMAND_PORTS
.rgba({final_alpha,out_covered?final_RGB:24'd0})
`else
.rgba({alpha,out_covered?final_RGB:24'd0})
`endif
,
`ifdef GPU_V2_COMMAND_PORTS
.depth(depth_data[15:0]),.depth_alpha(depth_data[23:16])
`else
.depth(depth_data)
`endif
,
 .sink_tile(header_tile),.x({header[15:8],1'b0}),.y({header[22:16],1'b0}),.mask(header[3:0]),
 .depth_func(depth_func),.depth_write(depth_write),.blend(blend),
 .raw_head_retire(raw_head_retire),.lane_ready(lane_ready),.lane_accept(lane_accept),.color_accept(sink_color_accept),.color_ready(sink_color_ready),.quad_accepted(quad_accepted),
 .commit_valid(commit_valid),.commit_x(commit_x),.commit_y(commit_y),.commit_mask(commit_mask),
 .flush_request(flush_request&&pipeline_idle),.flush_complete(flush_complete),.fault(sink_fault),.idle(sink_idle),
 .mem_req_valid(fbreq),.mem_req_write(fbwrite),.mem_req_addr(fbaddr),.mem_req_ready(fbready),
 .mem_read_data(framebuffer_read_data),.mem_read_valid(fbbeat),.mem_write_valid(fbwvalid),.mem_write_data(fbwdata),
 .mem_write_ready(fbwready),.mem_complete_valid(fbcomplete),.mem_complete_ok(framebuffer_ok));
 assign pipeline_idle=!active&&global_used==0&&cache_drained&&!final_out_valid;
 assign idle=pipeline_idle&&sink_idle;
 always @(posedge clk)begin
  if(reset)begin

`ifdef GPU_V2_COMMAND_JOIN
begin end
`else
stopped<=0;
`endif
active<=0;state<=J_READ;issue_slot<=0;input_bypass<=0;
   issue_lane<=0;light_complete<=0;sample_complete<=0;light_last<=0;sample_last<=0;
  end else begin

`ifdef GPU_V2_LIVE_GLOBALS
if(abort||front_bad||sink_bad||final_bad||front_fault||sink_fault||final_fault||light_collision||sample_collision||basic_collision)
`ifdef GPU_V2_COMMAND_JOIN
begin
`ifndef SYNTHESIS
 if(!reset&&!abort&&((1)!=0))$fatal(1,"GPU contract: stopped");
`endif
 end
`else
stopped<=1;
`endif

`else
if(abort||front_bad||sink_bad||final_bad||front_fault||sink_fault||final_fault||light_collision||sample_collision||basic_collision||(draw_rebind&&!pipeline_idle))
`ifdef GPU_V2_COMMAND_JOIN
begin
`ifndef SYNTHESIS
 if(!reset&&!abort&&((1)!=0))$fatal(1,"GPU contract: stopped");
`endif
 end
`else
stopped<=1;
`endif

`endif

   // J_WAIT owns synchronous RGB/GH/texture Q through CE0 and backpressure.
   if(public_publish)public_upper[sample_descriptor[3:0]]<=sample_descriptor[4];
   if(advance)begin
    if(quad_accepted&&retire_slot==light_last)light_complete<=0;
    if(light_result&&ll)begin light_complete<=1;light_last<=lk[6:2];end
    if(quad_accepted&&retire_slot==sample_last)sample_complete<=0;
    if(sample_result&&sl)begin sample_complete<=1;sample_last<=sample_address[6:2];end
    if(!active&&retire_slot!=published_slot&&!empty_head)begin
     active<=1;header<=head_descriptor;
    end
    if(join_value_read)begin
      state<=J_WAIT;
      // These two controls survive the last g loan even when issue_slot
      // advances at lane3 acceptance. Full origin/header stays canonical.
      input_bypass<=issue_header[6:5];
    end
    if(source_advance)state<=J_READ;
    // Every source prefix step, including a whole empty quad, has one owner.
    if(source_advance||empty_skip)begin
     if(empty_skip||issue_lane==3)begin issue_lane<=0;issue_slot<=issue_slot+5'd1;end
     else issue_lane<=issue_lane+1'b1;
    end
    if(quad_accepted)begin
     if((retire_slot+5'd1)!=published_slot)begin active<=|next_descriptor[3:0];header<=next_descriptor;end
     else active<=0;
    end
   end
  end
 end
 // Explicit abort is sticky on a wall edge, independently of compute CE.
`ifdef GPU_V2_COMMAND_JOIN
 always @(posedge clk)if(reset)stopped<=0;else if(abort)stopped<=1;
`endif
`undef GPU_V2_COMMAND_JOIN
 // Independent client transactions; no texture/framebuffer arbitration here.
 assign texture_request_valid=txreq;assign texture_request_address=txaddr;
 assign txready=texture_request_ready;assign txbeat=texture_read_valid;
 assign txindex=texture_read_index;assign txcomplete=texture_complete;
 assign framebuffer_request_valid=fbreq;assign framebuffer_request_write=fbwrite;
 assign framebuffer_request_address=fbaddr;assign fbready=framebuffer_request_ready;
 assign fbbeat=framebuffer_read_valid;assign fbcomplete=framebuffer_complete;
 assign framebuffer_write_valid=fbwvalid;assign framebuffer_write_data=fbwdata;
 assign fbwready=framebuffer_write_ready;
endmodule
