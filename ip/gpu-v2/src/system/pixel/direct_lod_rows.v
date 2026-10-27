// Compatibility observation ports below are tied off; no Raw/Derived/Ready
// payload or controller is instantiated. Four source rows are consumed at II4.
reg[1:0]lane;
wire[1:0]p;wire core_ready,core_advance,core_bad,core_fault,core_issue,core_return;
wire covered=in_raw[21+lane];
wire row_allow=lane!=0||admission_allow;
assign in_ready=!reset&&!fault&&p==0&&row_allow&&(!covered||core_ready);
wire take=advance&&in_valid&&in_ready;
assign advance=core_advance;assign preflight_bad=core_bad;assign fault=core_fault;
assign in_row=lane;assign in_slot=0;assign phase={1'b0,p};
assign raw_write=take&&lane==0;assign admitted=take&&lane==3;
assign reclaim_fire=admitted;assign reclaim_slot=0;
assign d_issue=0;assign l_issue=0;assign d_return=0;assign l_return=0;
assign c_issue=core_issue;assign c_return=core_return;assign c_empty=take&&!covered;
assign d_slot=0;assign l_slot=0;assign c_slot=0;assign d_return_slot=0;assign l_return_slot=0;
assign c_lane=lane;assign context_state=0;
assign uv_write=0;assign uv_read=core_issue;assign uv_write_address=0;assign uv_read_address={3'd0,lane};
assign store_reads=0;assign store_writes=0;
gpu_v2_texture_direct_lod direct(
 .clk(clk),.reset(reset),.ce(ce),.commit_allow(commit_allow),
 .in_valid(in_valid&&in_ready&&covered),.in_uv({in_uv[33:18],in_uv[15:0]}),
 .in_quad(in_raw[20:17]),.in_mask(in_raw[24:21]),.in_slot(in_raw[28:25]),.in_lane(lane),
 .in_lod(in_raw[7:0]),.in_max_n(in_raw[32:29]),.in_has_mip(in_raw[33]),.in_filter(in_raw[35:34]),
 .in_ready(core_ready),.advance(core_advance),.preflight_bad(core_bad),.fault(core_fault),
 .out_ready(out_ready),.out_valid(out_valid),.out_owner(out_owner),.out_result(out_result),
 .phase(p),.issue(core_issue),.returned(core_return),.occupied(c_occupied),.completed(c_completed));
always @(posedge clk)if(reset)lane<=0;else if(take)lane<=lane+1'b1;
