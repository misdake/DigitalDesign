// Complete selected TDP sink; source lifetime ends at D, cache lifetime at C.
module gpu_v2_pixel_framebuffer_sink #(
 parameter [31:0] COLOR_BASE=32'd@COLOR@,DEPTH_BASE=32'd@DEPTH@,
 parameter [9:0] WIDTH=10'd@WIDTH@,
 parameter [8:0] HEIGHT=9'd@HEIGHT@,
 parameter PREFETCH_ENABLE=@PREFETCH@,parameter PREFETCH_DIRTY=@PREFETCH_DIRTY@
)(
 input wire shade_phase,
 input wire [53:0] shared_rop_product, output wire shared_rop_offer,shared_rop_ce, output wire [26:0] shared_rop_a, output wire [7:0] shared_rop_b,
 input wire clk,reset,ce,commit_allow,
 input wire prefetch_abort,prefetch_valid,input wire[9:0]prefetch_tile,
 output wire preflight_bad,
 input wire lane_valid,input wire[1:0]lane,
 input wire[31:0]rgba,input wire[15:0]depth,
 input wire[8:0]sink_tile,x,input wire[7:0]y,input wire[3:0]mask,
 input wire[2:0]depth_func,input wire depth_write,blend,
 output wire raw_head_retire,
 output wire lane_ready,lane_accept,color_accept,color_ready,quad_accepted,
 output wire commit_valid,output wire[8:0]commit_x,
 output wire[7:0]commit_y,output wire[3:0]commit_mask,
 input wire flush_request,output wire flush_complete,fault,idle,
 output wire mem_req_valid,mem_req_write,output wire[31:0]mem_req_addr,
 input wire mem_req_ready,input wire[63:0]mem_read_data,input wire mem_read_valid,
 output wire mem_write_valid,output wire[63:0]mem_write_data,
 input wire mem_write_ready,mem_complete_valid,mem_complete_ok
);
 reg phase;
 reg[1:0]expected_lane;
 wire row_accept,row_ready,cache_idle;
 wire lane_matches=(lane==expected_lane);
 wire[2:0]row_index={lane,phase};
 wire[31:0]row_data=phase?{16'd0,depth}:rgba;
 // A color ACK reserves its existing cache tail until the paired depth ACK.
 // Flush is only forwarded between whole quads, so it cannot block that ACK.
 assign lane_ready=!reset&&!fault&&phase&&lane_matches;
 assign lane_accept=row_accept&&phase&&lane_matches;
 assign color_accept=row_accept&&!phase&&lane_matches;
 assign color_ready=!shade_phase&&!reset&&!phase&&lane_matches&&row_ready;
 assign quad_accepted=lane_accept&&(expected_lane==2'd3);
 assign raw_head_retire=ce&&!reset&&!fault&&lane_valid&&phase&&lane_matches&&(phase==shade_phase)&&row_ready&&(expected_lane==2'd3);
 wire flush_boundary=!lane_valid&&!phase&&(expected_lane==2'd0);
 assign idle=cache_idle&&!phase&&(expected_lane==2'd0);
 always @(posedge clk)begin
  if(reset||fault)begin phase<=1'b0;expected_lane<=2'd0;end
  else if(row_accept&&lane_matches)begin
   phase<=!phase;
   if(phase)expected_lane<=expected_lane+2'd1;
  end
 end
 gpu_v2_framebuffer_tdp #(.COLOR_BASE(COLOR_BASE),.DEPTH_BASE(DEPTH_BASE),
  .WIDTH(WIDTH),.HEIGHT(HEIGHT),.PREFETCH_ENABLE(PREFETCH_ENABLE),.PREFETCH_DIRTY(PREFETCH_DIRTY)) cache(
  .shared_rop_product(shared_rop_product),.shared_rop_offer(shared_rop_offer),.shared_rop_ce(shared_rop_ce),.shared_rop_a(shared_rop_a),.shared_rop_b(shared_rop_b),
  .prefetch_abort(prefetch_abort),.prefetch_valid(prefetch_valid),.prefetch_tile(prefetch_tile),
  .shade_phase(shade_phase),.clk(clk),.reset(reset),.ce(ce&&!reset),.commit_allow(commit_allow),.preflight_bad(preflight_bad),
  .row_valid(lane_valid&&!reset&&(phase==shade_phase)),.row_tile(sink_tile),.row_x(x),.row_y(y),.row_mask(mask),
  .row_index(row_index),.row_data(row_data),.row_accept(row_accept),.row_ready(row_ready),
  .depth_func(depth_func),.depth_write(depth_write),.blend(blend),
  .commit_valid(commit_valid),.commit_x(commit_x),.commit_y(commit_y),.commit_mask(commit_mask),
  .flush_request(flush_request&&flush_boundary),.flush_complete(flush_complete),
  .fault(fault),.idle(cache_idle),
  .mem_req_valid(mem_req_valid),.mem_req_write(mem_req_write),.mem_req_addr(mem_req_addr),
  .mem_req_ready(mem_req_ready),.mem_read_data(mem_read_data),.mem_read_valid(mem_read_valid),
  .mem_write_valid(mem_write_valid),.mem_write_data(mem_write_data),
  .mem_write_ready(mem_write_ready),.mem_complete_valid(mem_complete_valid),
  .mem_complete_ok(mem_complete_ok));
endmodule