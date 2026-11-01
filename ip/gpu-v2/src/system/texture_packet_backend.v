// Command Work16 -> two-edge Packet -> Group35/H15. Only explicit abort is sticky.
module gpu_v2_runtime_packet_backend_unique(
 input wire[15:0]texture_base,input wire texture_mip,input wire texture_fb400240,
 input wire clk,reset,proposal_ce,commit_allow,abort,issue_enable,in_valid,
 input wire [75:0] in_member,
 output wire in_ready,
 input wire out_ready,
 output wire out_valid,output wire [34:0] out_payload,
 output wire next_valid,output wire[14:0] next_key,
 output wire fault,preflight_bad,advance,proposal_ready,proposal_ack,
 output wire [5:0] pool_strobes,
 output wire [4:0] producer_count,unwritten_count,
 output wire [5:0] group_count,row_count,
 output wire [4:0] work_count,
 output wire capture_fire,work_ack
);
 reg terminal;
 wire work_ready,work_valid,work_fault,packet_fault,pool_fault;
 wire [75:0] member;wire [1:0] tap;
 wire packet_valid,packet_ready,pool_ready,front_valid,pool_bad;
 wire [34:0] packet,front;
 wire work_read,work_write;
 wire [3:0] work_read_row,work_write_row;
 wire [1:0] work_loaded;
 wire [5:0] allocate_row,write_row,transfer_row,read_row,reclaim_row;
 wire [3:0] owner_address;wire [5:0] owner_value;
 wire [1:0] head_valid_out;wire pending_out;wire [34:0] returned_payload;
 reg owner_e1;
 wire fault_eff=terminal || abort || work_fault || packet_fault || pool_fault;
 // Preflight is independent of the eventual shared commit CE: no fault/CE loop.
 wire attempted=proposal_ce && !reset && !fault_eff;
 wire issue=work_valid && pool_ready && issue_enable;
 wire input_bad=in_valid && work_ready && (in_member[39:36]==0);
 wire capture_bad=issue && !member[36+tap];
 wire write_bad=packet_valid && (unwritten_count==0);
 assign preflight_bad=1'b0;
 assign advance=attempted && !preflight_bad && commit_allow;
 assign fault=fault_eff;
 assign proposal_ready=!reset&&!fault_eff&&work_ready;
 assign proposal_ack=attempted&&issue&&((member[39:36]>>({1'b0,tap}+3'd1))==0);
 assign in_ready=advance && work_ready;
 assign out_valid=!reset && !fault && front_valid;
 assign out_payload=front;
 gpu_v2_texture_work_unique #(.DEPTH(16)) u_work(
  .clk(clk),.reset(reset),.ce(advance),.in_valid(in_valid),.in_member(in_member),
  .in_ready(work_ready),.out_ready(pool_ready && issue_enable),.out_valid(work_valid),
  .out_member(member),.out_tap(tap),.read_fire(work_read),.write_fire(work_write),
  .capture_fire(capture_fire),.ack(work_ack),.read_row(work_read_row),.write_row(work_write_row),
  .materialized(work_count),.loaded(work_loaded),.fault(work_fault));
 packet_leaf_unique #(.SHORT_ALIGNMENT(1)) u_packet(
  .clk(clk),.reset(reset),.ce(advance),.texture_base(texture_base),.texture_mip(texture_mip),.texture_fb400240(texture_fb400240),.in_valid(issue),.in_member(member),.in_tap(tap),
  .in_ready(packet_ready),.out_valid(packet_valid),.out_packet(packet),.fault(packet_fault));
 gpu_v2_packet_store_unique u_pool(
  .clk(clk),.reset(reset),.ce(advance),.reserve_valid(issue),
  .reserve_owner({member[71:68],member[73:72]}),.write_valid(packet_valid),.write_payload(packet),
  .owner_request(owner_e1),.consume(front_valid && out_ready),
  .producer_ready(pool_ready),.front_valid(front_valid),.front_payload(front),
  .next_valid(next_valid),.next_key(next_key),.fault(pool_fault),.bad(pool_bad),
  .producer_count(producer_count),.unwritten_count(unwritten_count),.group_count(group_count),.row_count(row_count),
  .allocate_row(allocate_row),.write_row(write_row),.transfer_row(transfer_row),.read_row(read_row),.reclaim_row(reclaim_row),
  .owner_address(owner_address),.strobes(pool_strobes),.head_valid_out(head_valid_out),.pending_out(pending_out),
  .owner_value(owner_value),.returned_payload(returned_payload));
 always @(posedge clk)begin
  if(reset)begin begin end owner_e1<=0;end
  else begin
   if(abort || preflight_bad || work_fault || packet_fault || pool_fault)begin
`ifndef SYNTHESIS
 if(!reset&&!abort&&((1)!=0))$fatal(1,"GPU contract: terminal");
`endif
 end
   if(advance)owner_e1<=issue;
  end
 end

 always @(posedge clk)if(reset)terminal<=0;else if(abort)terminal<=1;

`ifndef SYNTHESIS
 always @(posedge clk)if(!reset&&((!abort)&&(attempted && (input_bad || capture_bad || write_bad))))$fatal(1,"GPU contract: preflight_bad");
`endif
endmodule
