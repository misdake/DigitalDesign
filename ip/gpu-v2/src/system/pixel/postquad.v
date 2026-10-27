// One actual post-quad chain. Draw pins remain immutable until pipeline_idle.
module gpu_v2_postquad(
 input wire clk,reset,ce,abort,
 input wire in_valid,input wire[22:0]in_begin,
 input wire[31:0]in_basic,input wire[35:0]in_light,in_uv,
 output wire in_ready,output wire[2:0]in_row,
 input wire[26:0]draw_config,input wire[23:0]specular,input wire[7:0]alpha,
 input wire[2:0]depth_func,input wire depth_write,blend,draw_rebind,
 input wire[15:0]context_epoch,input wire[1:0]context_mode,input wire[4:0]context_code,
 input wire signed[15:0]light_x,light_y,light_z,ray_x,ray_y,ray_k,
 input wire[8:0]ambient,directional,
 input wire flush_request,output wire flush_complete,pipeline_idle,idle,fault,
 output wire published,output wire[4:0]published_slot,
 output wire light_result,sample_result,final_issue,final_result,quad_accepted,commit_valid,
 output wire[8:0]commit_x,output wire[7:0]commit_y,output wire[3:0]commit_mask,
 output wire[5:0]global_used,output wire[4:0]work_reserved,work_count,
 output wire mem_req_valid,mem_req_write,output wire[31:0]mem_req_addr,
 input wire mem_req_ready,input wire[63:0]mem_read_data,input wire mem_read_valid,
 output wire mem_write_valid,output wire[63:0]mem_write_data,
 input wire mem_write_ready,mem_complete_valid,mem_complete_ok
);
 wire front_fault,front_bad,advance,public_credit,cache_drained;
 wire sink_fault,sink_idle,final_fault;
 reg stopped;
 reg[1:0]arb_state;reg owner;reg[3:0]beat;
 assign fault=abort||stopped||front_fault||sink_fault||final_fault;
 wire compute_ce=ce&&!fault;
 wire[22:0]published_begin;
 wire basic_read,basic_valid,basic_collision;
 wire final_in_ready,final_out_valid;wire[23:0]final_RGB;
 wire[7:0]basic_address;wire[23:0]basic_data;
 wire[4:0]retire_slot;wire[1:0]light_used;wire[3:0]sample_used;
 wire lv,lr,ll;wire[6:0]lk;wire[8:0]lg,lh;
 wire sv,sr,sl,sfire,public_publish;wire[5:0]sk;wire[23:0]sRGB;
 wire[15:0]public_live;wire[10:0]sample_descriptor;
 wire sample_fire;wire[1:0]sample_row;
 wire txreq,txready;wire[31:0]txaddr;
 wire txbeat,txcomplete;wire[3:0]txindex;
 gpu_v2_pixel_sampling_ingress front(
 .clk(clk),.reset(reset),.ce(compute_ce),.abort(abort||sink_fault||stopped||final_fault),
 .in_valid(in_valid),.in_begin(in_begin),.in_basic(in_basic),.in_light(in_light),.in_uv(in_uv),
 .in_ready(in_ready),.in_row(in_row),.advance(advance),.preflight_bad(front_bad),.fault(front_fault),
 .published(published),.published_slot(published_slot),.published_begin(published_begin),
 .basic_read(basic_read),.basic_read_address(basic_address),.basic_return_valid(basic_valid),
 .basic_return_data(basic_data),.basic_collision(basic_collision),
 .rop_row7_accepted(quad_accepted),.rop_slot(retire_slot),.retire_slot(retire_slot),
 .global_used(global_used),.light_used(light_used),.sample_used(sample_used),
 .public_credit(public_credit),.draw_config(draw_config),
 .sample_fire(sample_fire),.sample_row(sample_row),.sample_descriptor(sample_descriptor),
 .packet_enable(1'b1),.result_ready(sr),.result_valid(sv),.result_key(sk),.result_rgb(sRGB),
 .work_reserved(work_reserved),.work_count(work_count),
 .result_quad_last(sl),.result_fire(sfire),.public_publish(public_publish),.public_live(public_live),
 .mem_req_valid(txreq),.mem_req_addr(txaddr),.mem_req_ready(txready),
 .mem_resp_valid(txbeat),.mem_resp_index(txindex),.mem_resp_data(mem_read_data),
 .mem_resp_complete(txcomplete),.mem_resp_ok(mem_complete_ok),.cache_drained(cache_drained),
 .light_bank(),.retire_valid(draw_rebind&&pipeline_idle),.retire_bank(1'b0),
 .context_epoch(context_epoch),.context_mode(context_mode),.context_code(context_code),
 .light_x(light_x),.light_y(light_y),.light_z(light_z),.ambient(ambient),.directional(directional),
 .ray_x(ray_x),.ray_y(ray_y),.ray_k(ray_k),
 .out_valid(lv),.out_ready(lr),.out_key(lk),.out_quad_last(ll),.out_g(lg),.out_h(lh));
 // Public low four slot bits are already in sk; only the global upper bit is missing.
 (* syn_ramstyle="distributed_ram" *)reg public_upper[0:15];
 wire[6:0]sample_address={public_upper[sk[5:2]],sk};
 (* syn_ramstyle="distributed_ram" *)reg[22:0]descriptors[0:31];
 reg[31:0]light_done,sample_done;
 reg[5:0]published_count;
 assign lr=advance;assign sr=advance;
 assign light_result=lv&&lr;assign sample_result=sfire;
 reg active;reg[22:0]header;
 reg[2:0]state;reg[1:0]issue_lane,out_lane;
 reg paid;reg[3:0]depth_valid;
 reg[15:0]depth_rows[0:3];
 localparam J_READ=0,J_WAIT=1,J_ISSUE=2,J_DEPTH=3,J_DEPTH_WAIT=4,J_DRAIN=5;
 wire covered=header[issue_lane];wire out_covered=header[out_lane];
 wire[6:0]read_key={retire_slot,issue_lane};
 wire read_values=active&&state==J_READ;
 assign basic_read=active&&(state==J_READ||state==J_DEPTH);
 assign basic_address={retire_slot,issue_lane,state==J_DEPTH};
 wire light_return,sample_return,light_collision,sample_collision;
 wire[17:0]light_data;wire[23:0]sample_data;
 gpu_v2_postquad_light_bank light_bank(
 .clk(clk),.reset(reset),.read_en(advance&&read_values),.read_address(read_key),
 .write_en(light_result),.write_address(lk),.write_data({lh,lg}),
 .return_valid(light_return),.return_data(light_data),.collision(light_collision));
 gpu_v2_postquad_sample_bank sample_bank(
 .clk(clk),.reset(reset),.read_en(advance&&read_values),.read_address(read_key),
 .write_en(sample_result),.write_address(sample_address),.write_data(sRGB),
 .return_valid(sample_return),.return_data(sample_data),.collision(sample_collision));

 wire final_in_valid=active&&state==J_ISSUE&&covered;
 wire[8:0]final_g=header[5]?9'd256:light_data[8:0];
 wire[8:0]final_h=header[5]?9'd0:light_data[17:9];
 wire[23:0]final_texture=header[6]?24'hffffff:sample_data;
 wire lane_valid=active&&depth_valid[out_lane]&&(!out_covered||final_out_valid);
 wire lane_ready,lane_accept;
 wire final_out_ready=lane_accept&&out_covered;
 gpu_v2_postquad_final final_color(
 .clk(clk),.reset(reset),.ce(advance),.abort(abort||stopped||sink_fault),
 .in_valid(final_in_valid),.in_ready(final_in_ready),
 .in_tint(basic_data),.in_tex(final_texture),.in_spec(specular),.in_g(final_g),.in_h(final_h),
 .out_valid(final_out_valid),.out_ready(final_out_ready),.out_rgb(final_RGB),.fault(final_fault));
 assign final_issue=advance&&final_in_valid&&final_in_ready;
 assign final_result=lane_accept&&out_covered;
 wire fbreq,fbwrite,fbready,fbbeat,fbcomplete,fbwvalid,fbwready;wire[31:0]fbaddr;wire[63:0]fbwdata;
 gpu_v2_pixel_framebuffer_sink sink(
 .clk(clk),.reset(reset),.ce(advance),.lane_valid(lane_valid),.lane(out_lane),
 .rgba({alpha,out_covered?final_RGB:24'd0}),.depth(depth_rows[out_lane]),
 .x({header[15:8],1'b0}),.y({header[22:16],1'b0}),.mask(header[3:0]),
 .depth_func(depth_func),.depth_write(depth_write),.blend(blend),
 .lane_ready(lane_ready),.lane_accept(lane_accept),.quad_accepted(quad_accepted),
 .commit_valid(commit_valid),.commit_x(commit_x),.commit_y(commit_y),.commit_mask(commit_mask),
 .flush_request(flush_request&&pipeline_idle),.flush_complete(flush_complete),.fault(sink_fault),.idle(sink_idle),
 .mem_req_valid(fbreq),.mem_req_write(fbwrite),.mem_req_addr(fbaddr),.mem_req_ready(fbready),
 .mem_read_data(mem_read_data),.mem_read_valid(fbbeat),.mem_write_valid(fbwvalid),.mem_write_data(fbwdata),
 .mem_write_ready(fbwready),.mem_complete_valid(fbcomplete),.mem_complete_ok(mem_complete_ok));
 assign pipeline_idle=!active&&global_used==0&&in_row==0&&cache_drained&&!final_out_valid;
 assign idle=pipeline_idle&&sink_idle&&arb_state==0;
 always @(posedge clk)begin
  if(reset)begin
   stopped<=0;active<=0;state<=J_READ;paid<=0;depth_valid<=0;
   issue_lane<=0;out_lane<=0;light_done<=0;sample_done<=0;published_count<=0;
  end else begin
   if(light_collision||sample_collision||basic_collision||(draw_rebind&&!pipeline_idle))stopped<=1;
   // Paid synchronous read returns are captured regardless of compute CE.
   if(active&&state==J_WAIT&&basic_valid)paid<=1;
   if(active&&state==J_DEPTH_WAIT&&basic_valid)begin
    depth_rows[issue_lane]<=basic_data[15:0];depth_valid[issue_lane]<=1;paid<=1;
   end
   if(public_publish)public_upper[sample_descriptor[3:0]]<=sample_descriptor[4];
   if(advance)begin
    case({published,quad_accepted})
     2'b10:published_count<=published_count+1'b1;
     2'b01:published_count<=published_count-1'b1;
    endcase
    if(published)begin
     descriptors[published_slot]<=published_begin;
     light_done[published_slot]<=published_begin[5];sample_done[published_slot]<=published_begin[6];
    end
    if(light_result&&ll)light_done[lk[6:2]]<=1;
    if(sample_result&&sl)sample_done[sample_address[6:2]]<=1;
    if(!active&&published_count!=0&&light_done[retire_slot]&&sample_done[retire_slot])begin
     active<=1;header<=descriptors[retire_slot];state<=J_READ;issue_lane<=0;out_lane<=0;depth_valid<=0;paid<=0;
    end
    if(active)case(state)
     J_READ:begin state<=J_WAIT;paid<=0;end
     J_WAIT:if(paid||basic_valid)begin state<=J_ISSUE;paid<=0;end
     J_ISSUE:if(!covered||final_in_ready)state<=J_DEPTH;
     J_DEPTH:begin state<=J_DEPTH_WAIT;paid<=0;end
     J_DEPTH_WAIT:if(paid||basic_valid)begin
      paid<=0;
      if(issue_lane==3)state<=J_DRAIN;
      else begin issue_lane<=issue_lane+1'b1;state<=J_READ;end
     end
    endcase
    if(lane_accept)out_lane<=out_lane+1'b1;
    if(quad_accepted)begin active<=0;light_done[retire_slot]<=0;sample_done[retire_slot]<=0;end
   end
  end
 end
 // Fixed-priority single-outstanding transport. The presented owner is locked
 // before request publication and remains locked through the terminal response.
 assign mem_req_valid=arb_state==1&&(owner?fbreq:txreq);
 assign mem_req_addr=owner?fbaddr:txaddr;
 assign mem_req_write=owner&&fbwrite;
 assign txready=arb_state==1&&!owner&&mem_req_ready;
 assign fbready=arb_state==1&&owner&&mem_req_ready;
 assign txbeat=arb_state==2&&!owner&&mem_read_valid;
 assign fbbeat=arb_state==2&&owner&&mem_read_valid;
 assign txindex=beat;
 assign txcomplete=arb_state==2&&!owner&&mem_complete_valid;
 assign fbcomplete=arb_state==2&&owner&&mem_complete_valid;
 assign mem_write_valid=arb_state==2&&owner&&fbwvalid;
 assign mem_write_data=fbwdata;assign fbwready=arb_state==2&&owner&&mem_write_ready;
 always @(posedge clk)begin
  if(reset)begin arb_state<=0;owner<=0;beat<=0;end
  else case(arb_state)
   0:if(fbreq||txreq)begin owner<=fbreq;arb_state<=1;end
   1:if(mem_req_valid&&mem_req_ready)begin arb_state<=2;beat<=0;end
   2:begin
    if(mem_read_valid)beat<=beat+1'b1;
    if(mem_complete_valid)arb_state<=0;
   end
  endcase
 end
endmodule
