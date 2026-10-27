// Context queue only: arithmetic and payload RAM are external live leaves.
module gpu_v2_texture_runtime_frontend #(
    parameter CONTEXTS=8, parameter EARLY=1
)(
    input wire clk,reset,ce,
    input wire admit,d_phase_zero,l_phase_zero,d_return,l_return,
    input wire uv_d_valid,input wire[2:0]uv_d_slot,
    input wire[3:0]c_mask,
    input wire uv_phase_odd,uv_c_empty,uv_c_valid,
    input wire[2:0]uv_c_slot,input wire[1:0]uv_c_lane,
    input wire c_phase_zero,c_credit,
    input wire[7:0]late_retire,
    output wire input_ready,raw_issue,derived_issue,ready_read,
    output wire uv_request,c_take,c_empty,
    output wire[2:0]admit_slot,d_slot,l_slot,c_slot,d_return_slot,l_return_slot,
    output wire[1:0]c_lane,output wire[2:0]c_ordinal,
    output wire reclaim_fire,output wire[2:0]reclaim_slot,
    output reg fault,output wire[72:0]state
);
 reg[2:0]allocate,reclaim,d_head,l_head,c_head,d_owner,l_owner;
 reg[3:0]occupied,raw_count,derived_count,lod_count;
 reg d_valid,l_valid;
 reg[7:0]valid,reclaimable;
 reg[1:0]stage[0:7];reg[1:0]head_ordinal;
 // Classify each owned row once; each control port selects one legality bit.
 // All tag values and invalid rows retain the original fault predicates.
 wire[7:0]raw_rows,derived_rows,ready_rows;
 generate for(genvar row=0;row<8;row=row+1)begin:row_classes
  assign raw_rows[row]=valid[row]&&(stage[row]==0);
  assign derived_rows[row]=valid[row]&&(stage[row]==1);
  assign ready_rows[row]=valid[row]&&(stage[row]==2);
 end endgenerate
 function[2:0]advance;input[2:0]a;begin advance=(a==CONTEXTS-1)?3'd0:a+1'b1;end endfunction
 reg[1:0]lane;reg lane_ok;reg[2:0]covered;
 integer i,ordinal,lane_index;
 always@*begin
  lane=0;lane_ok=0;covered=0;ordinal=0;
  for(lane_index=0;lane_index<4;lane_index=lane_index+1)begin
   if(c_mask[lane_index])begin
    if(ordinal==head_ordinal)begin lane=lane_index;lane_ok=1;end
    ordinal=ordinal+1;covered=covered+1'b1;
   end
  end
 end
 wire running=ce&&!reset&&!fault;
 wire di=running&&raw_count!=0&&d_phase_zero&&uv_d_valid;
 wire li=running&&derived_count!=0&&l_phase_zero;
 wire cr=running&&lod_count!=0;
 wire empty=cr&&c_mask==0;
 wire ct=cr&&c_mask!=0&&c_phase_zero&&c_credit&&uv_c_valid;
 wire done=empty||(ct&&{1'b0,head_ordinal}+3'd1==covered);
 wire early_done=done&&EARLY;
 wire[7:0]c_retire_mask=8'b1<<c_head;
 wire[7:0]retired=late_retire|(early_done?c_retire_mask:8'b0);
 // Decode the old heads/row legality before late C completion arrives.
 // Indexed one-hot preserves unknown-index behavior as well as binary hits.
 wire reclaim_marked=reclaimable[reclaim]|late_retire[reclaim]|
  (early_done&&c_retire_mask[reclaim]);
 wire rf=running&&occupied!=0&&reclaim_marked;
 wire[7:0]invalid_retire_rows=(~valid)|reclaimable;
 wire late_retire_bad=((late_retire>>CONTEXTS)!=0)||
  (|(late_retire&invalid_retire_rows));
 wire early_retire_bad=((c_retire_mask>>CONTEXTS)!=0)||
  (|(c_retire_mask&invalid_retire_rows));
 wire[5:0]tracked={2'b0,raw_count}+{2'b0,derived_count}+{2'b0,lod_count}+{5'b0,d_valid}+{5'b0,l_valid};
 // Count comparisons precede the late C-completion/reclaim controls. The
 // three four-bit counts and two valid bits bound tracked at47, so the
 // original six-bit tracked+rf cannot overflow, including malformed counts.
 wire[5:0]occupied_plus_one={2'b0,occupied}+6'd1;
 wire tracked_over_one=tracked>occupied_plus_one;
 wire tracked_over=tracked>{2'b0,occupied};
 wire tracked_at_least=tracked>={2'b0,occupied};
 reg tracked_bad;
 always@*begin
  case({rf,done})
   2'b00,2'b11:tracked_bad=tracked_over;
   2'b01:tracked_bad=tracked_over_one;
   2'b10:tracked_bad=tracked_at_least;
   default:tracked_bad=1'bx;
  endcase
 end
 reg bad;
 always@*begin
  bad=0;
  if(running)begin
   if(admit&&(!input_ready||valid[allocate]))bad=1;
   if(di&&(!raw_rows[d_head]||d_valid||uv_d_slot!=d_head))bad=1;
   if(li&&(!derived_rows[l_head]||l_valid))bad=1;
   if(d_return&&(!d_valid||!raw_rows[d_owner]))bad=1;
   if(l_return&&(!l_valid||!derived_rows[l_owner]))bad=1;
   if(cr&&(!ready_rows[c_head]||(c_mask!=0&&!lane_ok)))bad=1;
   if(ct&&(uv_c_slot!=c_head||uv_c_lane!=lane))bad=1;
   if(late_retire_bad)bad=1;
   if(early_done&&early_retire_bad)bad=1;
   if(tracked_bad)bad=1;
  end
 end
 assign input_ready=!reset&&!fault&&occupied<CONTEXTS;
 assign raw_issue=di&&!bad;assign derived_issue=li&&!bad;assign ready_read=cr&&!bad;
 assign uv_request=cr&&c_mask!=0&&uv_phase_odd&&uv_c_empty&&!bad;
 assign c_take=ct&&!bad;assign c_empty=empty&&!bad;
 assign admit_slot=allocate;assign d_slot=d_head;assign l_slot=l_head;assign c_slot=c_head;
 assign d_return_slot=d_owner;assign l_return_slot=l_owner;
 assign c_lane=lane;assign c_ordinal={1'b0,head_ordinal};
 assign reclaim_fire=rf&&!bad;assign reclaim_slot=reclaim;
 wire[15:0]ss;
 generate for(genvar b=0;b<8;b=b+1)begin:diagnostic
  assign ss[b*2+:2]=valid[b]?stage[b]:2'b0;
 end endgenerate
 assign state={head_ordinal,ss,reclaimable,valid,lod_count,c_head,
    (l_valid?l_owner:3'b0),l_valid,derived_count,l_head,
    (d_valid?d_owner:3'b0),d_valid,raw_count,d_head,occupied,reclaim,allocate};
 always@(posedge clk)begin
  if(reset)begin
   allocate<=0;reclaim<=0;d_head<=0;l_head<=0;c_head<=0;d_owner<=0;l_owner<=0;
   occupied<=0;raw_count<=0;derived_count<=0;lod_count<=0;d_valid<=0;l_valid<=0;
   valid<=0;reclaimable<=0;fault<=0;head_ordinal<=0;
   for(i=0;i<8;i=i+1)begin stage[i]<=0;end
  end else if(running)begin
   if(bad)fault<=1;
   else begin
    case({admit,rf})2'b10:occupied<=occupied+1'b1;2'b01:occupied<=occupied-1'b1;default:occupied<=occupied;endcase
    case({admit,di})2'b10:raw_count<=raw_count+1'b1;2'b01:raw_count<=raw_count-1'b1;default:raw_count<=raw_count;endcase
    case({d_return,li})2'b10:derived_count<=derived_count+1'b1;2'b01:derived_count<=derived_count-1'b1;default:derived_count<=derived_count;endcase
    case({l_return,done})2'b10:lod_count<=lod_count+1'b1;2'b01:lod_count<=lod_count-1'b1;default:lod_count<=lod_count;endcase
    if(di)begin d_owner<=d_head;d_valid<=1;d_head<=advance(d_head);end
    if(li)begin l_owner<=l_head;l_valid<=1;l_head<=advance(l_head);end
    if(d_return)begin stage[d_owner]<=1;d_valid<=0;end
    if(l_return)begin stage[l_owner]<=2;l_valid<=0;end
    if(done)begin c_head<=advance(c_head);head_ordinal<=0;end
    else if(ct)head_ordinal<=head_ordinal+1'b1;
    for(i=0;i<8;i=i+1)if(retired[i])reclaimable[i]<=1;
    if(rf)begin valid[reclaim]<=0;reclaimable[reclaim]<=0;reclaim<=advance(reclaim);end
    if(admit)begin valid[allocate]<=1;stage[allocate]<=0;allocate<=advance(allocate);end
   end
  end
 end
endmodule
