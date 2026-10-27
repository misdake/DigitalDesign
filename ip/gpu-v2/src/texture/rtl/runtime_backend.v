// C6 keeps its owner/result until actual in_accept. No additional head shadow.
module gpu_v2_texture_runtime_backend(
 input wire clk,reset,ce,commit_allow,abort,
 input wire in_valid,input wire[37:0] in_owner,input wire[35:0] in_result,
 output wire in_ready,in_accept,advance,preflight_bad,fault,
 input wire packet_enable,out_ready,
 output wire out_valid,output wire[71:0] out_payload,
 output wire[4:0] work_reserved,work_count,producer_count,unwritten_count,
 output wire[5:0] group_count,row_count,pool_strobes,
 output wire coefficient_valid,coefficient_consume,member_issue,member_return,
 output wire[1:0] reserved_cost,
 output wire[71:0] coefficient_weights,output wire[35:0] coefficient_meta,
 output wire[75:0] member_payload,
 output wire packet_capture,work_ack
);
 reg stopped,coarse_next;
 reg[4:0] reserved;
 wire coefficient_fault,member_fault,packet_fault;
 wire coefficient_bad,packet_bad,coefficient_ready,work_ready,proposal_ack;
 wire running=ce&&!reset&&!fault;
 assign fault=stopped||abort||coefficient_fault||member_fault||packet_fault;
 wire allow=commit_allow&&!preflight_bad&&!fault;
 assign advance=running&&allow;
 assign work_reserved=reserved;
 wire[7:0] available=8'd16-{3'd0,reserved};
 // Parent preflight sees old raw readiness, independent of joined commit.
 assign in_ready=running&&coefficient_ready;
 wire[8:0] p0=in_owner[8:0],p1=in_owner[17:9];
 wire nearest=in_owner[18];
 wire[3:0] n0=in_owner[22:19],n1=in_owner[26:23];
 wire[9:0] x=in_result[25:16],y=in_result[35:26];
 wire[7:0] fu=in_result[7:0],fv=in_result[15:8];
 wire bx=!x[0]&&!fu[7],by=!y[0]&&!fv[7];
 // Width-bounded mip masks; static comparisons avoid a vendor dynamic-shift bus bug.
 wire[9:0] mask0={(n0>4'd9),(n0>4'd8),(n0>4'd7),(n0>4'd6),(n0>4'd5),(n0>4'd4),(n0>4'd3),(n0>4'd2),(n0>4'd1),1'b1};
 wire[9:0] mask1={(n1>4'd9),(n1>4'd8),(n1>4'd7),(n1>4'd6),(n1>4'd5),(n1>4'd4),(n1>4'd3),(n1>4'd2),(n1>4'd1),1'b1};
 wire[9:0] cx=n0>1?((x>>1)-bx)&mask1:x;
 wire[9:0] cy=n0>1?((y>>1)-by)&mask1:y;
 wire[7:0] cu=n0>1?{x[0]^!fu[7],!fu[7],fu[6:1]}:fu;
 wire[7:0] cv=n0>1?{y[0]^!fv[7],!fv[7],fv[6:1]}:fv;
 wire[9:0] x1=(x+10'd1)&mask0,y1=(y+10'd1)&mask0,cx1=(cx+10'd1)&mask1,cy1=(cy+10'd1)&mask1;
 wire[161:0] coefficient_input={in_owner[37],1'b0,in_owner[36:31],1'b0,in_owner[30:27],n1,n0,
  1'b0,cy1,1'b0,cy,1'b0,cx1,1'b0,cx,1'b0,y1,1'b0,y,1'b0,x1,1'b0,x,cv,cu,fv,fu,1'b0,p1,1'b0,p0,nearest};
 wire fine_active=|coefficient_weights[8:0];
 wire coarse_active=|coefficient_weights[44:36];
 wire selected_coarse=coarse_next||!fine_active;
 wire selected_active=selected_coarse?coarse_active:fine_active;
 wire member_proposal=running&&coefficient_valid;
 assign member_issue=advance&&coefficient_valid;
 wire finish=selected_coarse||!coarse_active;
 assign coefficient_consume=member_issue&&finish;
 wire[35:0] weights=selected_coarse?coefficient_weights[71:36]:coefficient_weights[35:0];
 wire[3:0] level0=coefficient_meta[25:22];
 wire[3:0] level=selected_coarse&&level0!=0?level0-4'd1:level0;
 wire[9:0] base_x=coefficient_meta[9:0],base_y=coefficient_meta[19:10];
 wire[9:0] member_mask={(level>4'd9),(level>4'd8),(level>4'd7),(level>4'd6),(level>4'd5),(level>4'd4),(level>4'd3),(level>4'd2),(level>4'd1),1'b1};
 wire[9:0] mx=selected_coarse&&level0>1?((base_x>>1)-coefficient_meta[20])&member_mask:base_x;
 wire[9:0] my=selected_coarse&&level0>1?((base_y>>1)-coefficient_meta[21])&member_mask:base_y;
 wire[9:0] mx1=(mx+10'd1)&member_mask,my1=(my+10'd1)&member_mask;
 wire credit_bad=running&&((reserved>16)||(work_count>reserved)||(proposal_ack&&reserved==0)||
  (member_return&&!work_ready)||(member_proposal&&!selected_active));
 assign preflight_bad=coefficient_bad||packet_bad||credit_bad;
 gpu_v2_runtime_coefficient coefficient(
 .clk(clk),.reset(reset),.proposal_ce(ce&&!reset&&!fault),.commit_allow(allow),.in_valid(in_valid),.in_payload(coefficient_input),
 .work_available(available),.in_ready(),.in_accept(in_accept),.out_ready(member_proposal&&finish),
 .out_valid(coefficient_valid),.out_weights(coefficient_weights),.out_meta(coefficient_meta),.work_reserved(reserved_cost),
 .fault(coefficient_fault),.proposal_ready(coefficient_ready),.preflight_bad(coefficient_bad));
 membership_leaf #(.SHORT_ALIGNMENT(1)) member(
 .clk(clk),.reset(reset),.ce(advance),.in_valid(member_issue),
 .in_w0(weights[8:0]),.in_w1(weights[17:9]),.in_w2(weights[26:18]),.in_w3(weights[35:27]),
 .in_c0(mx),.in_c1(mx1),.in_c2(my),.in_c3(my1),.in_slot(coefficient_meta[29:26]),.in_level(level),
 .in_key(coefficient_meta[35:30]),.in_fine(!selected_coarse),.in_last_fine(!coarse_active),
 .in_ready(),.out_valid(member_return),.out_member(member_payload),.fault(member_fault));
 gpu_v2_runtime_packet_backend_shared packets(
 .clk(clk),.reset(reset),.proposal_ce(ce&&!stopped),.commit_allow(allow),.abort(abort||stopped),
 .issue_enable(packet_enable),.in_valid(member_return),.in_member(member_payload),.in_ready(),.proposal_ready(work_ready),.proposal_ack(proposal_ack),
 .out_ready(out_ready),.out_valid(out_valid),.out_payload(out_payload),.fault(packet_fault),.preflight_bad(packet_bad),.advance(),
 .pool_strobes(pool_strobes),.producer_count(producer_count),.unwritten_count(unwritten_count),.group_count(group_count),.row_count(row_count),
 .work_count(work_count),.capture_fire(packet_capture),.work_ack(work_ack));
 always @(posedge clk)begin
  if(reset)begin stopped<=0;coarse_next<=0;reserved<=0;end
  else begin
   if(abort||(running&&preflight_bad))stopped<=1;
   if(advance)begin
    reserved<=reserved+{3'b0,reserved_cost}-work_ack;
    if(member_issue)coarse_next<=!finish;
   end
  end
 end
endmodule
