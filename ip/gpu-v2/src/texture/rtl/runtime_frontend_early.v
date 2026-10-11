// Ordered Context8: a last/empty C capture releases that same context.
// Raw metadata/UV row-zero reservation lives in the existing external leaves.
module gpu_v2_texture_runtime_frontend_early_shared #(
 parameter CONTEXTS=8, parameter EARLY=1
)(
 input wire clk,reset,proposal_ce,commit_allow,reserve_check,
 input wire admit,d_phase_zero,l_phase_zero,d_return,l_return,
 input wire uv_d_valid,input wire[2:0]uv_d_slot,
 input wire[3:0]c_mask,
 input wire uv_phase_odd,uv_c_empty,uv_c_valid,
 input wire[2:0]uv_c_slot,input wire[1:0]uv_c_lane,
 input wire c_phase_zero,c_credit,
 input wire[7:0]late_retire,
 output wire preflight_bad,advance,reserve_ready,
 output wire[6:0]proposal_cmds,
 output wire input_ready,raw_issue,derived_issue,ready_read,
 output wire uv_request,c_take,c_empty,
 output wire[2:0]admit_slot,d_slot,l_slot,c_slot,d_return_slot,l_return_slot,
 output wire[1:0]c_lane,output wire[2:0]c_ordinal,
 output wire reclaim_fire,output wire[2:0]reclaim_slot,
 output reg fault,output wire[72:0]state
);
 reg[2:0]allocate,d_head,l_head,c_head,d_owner,l_owner;
 reg[3:0]occupied,raw_count,derived_count,lod_count;
 reg d_valid,l_valid;
 reg[7:0]valid;
 reg[1:0]stage[0:7];reg[1:0]head_ordinal;
 wire[7:0]raw_rows,derived_rows,ready_rows;
 generate for(genvar row=0;row<8;row=row+1)begin:row_classes
  assign raw_rows[row]=valid[row]&&(stage[row]==0);
  assign derived_rows[row]=valid[row]&&(stage[row]==1);
  assign ready_rows[row]=valid[row]&&(stage[row]==2);
 end endgenerate
 function[2:0]next_slot;input[2:0]a;begin next_slot=a+1'b1;end endfunction
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
 wire running=proposal_ce&&!reset&&!fault;
 wire di=running&&raw_count!=0&&d_phase_zero&&uv_d_valid;
 wire li=running&&derived_count!=0&&l_phase_zero;
 wire cr=running&&lod_count!=0;
 wire empty=cr&&c_mask==0;
 wire ct=cr&&c_mask!=0&&c_phase_zero&&c_credit&&uv_c_valid;
 wire done=empty||(ct&&{1'b0,head_ordinal}+3'd1==covered);
 // Reset and every healthy ordered last-C edge keep generic reclaim=C-head
 // and reclaimable=0. The generic mark/clear is this direct same-head release.
 wire rf=done;
 wire[5:0]tracked={2'b0,raw_count}+{2'b0,derived_count}+{2'b0,lod_count}+{5'b0,d_valid}+{5'b0,l_valid};
 reg tracked_bad;
 always@*begin
  case(done)
   1'b0,1'b1:tracked_bad=tracked>{2'b0,occupied};
   default:tracked_bad=1'bx;
  endcase
 end
 reg bad;
 always@*begin
  bad=0;
  if(running)begin
   if(CONTEXTS!=8||EARLY!=1)bad=1;
   if(late_retire!=0)bad=1;
   if(reserve_check&&!reserve_ready)bad=1;
   if(admit&&(!input_ready||valid[allocate]))bad=1;
   if(di&&(!raw_rows[d_head]||d_valid||uv_d_slot!=d_head))bad=1;
   if(li&&(!derived_rows[l_head]||l_valid))bad=1;
   if(d_return&&(!d_valid||!raw_rows[d_owner]))bad=1;
   if(l_return&&(!l_valid||!derived_rows[l_owner]))bad=1;
   if(cr&&(!ready_rows[c_head]||(c_mask!=0&&!lane_ok)))bad=1;
   if(ct&&(uv_c_slot!=c_head||uv_c_lane!=lane))bad=1;
   if(tracked_bad)bad=1;
  end
 end
 assign input_ready=!reset&&!fault&&occupied<8;
 assign reserve_ready=input_ready&&!valid[allocate];
 assign preflight_bad=bad;
 assign advance=running&&!bad&&commit_allow;
 assign proposal_cmds={rf,empty,ct,(cr&&c_mask!=0&&uv_phase_odd&&uv_c_empty),cr,li,di};
 assign raw_issue=di&&advance;assign derived_issue=li&&advance;assign ready_read=cr&&advance;
 assign uv_request=cr&&c_mask!=0&&uv_phase_odd&&uv_c_empty&&advance;
 assign c_take=ct&&advance;assign c_empty=empty&&advance;
 assign admit_slot=allocate;assign d_slot=d_head;assign l_slot=l_head;assign c_slot=c_head;
 assign d_return_slot=d_owner;assign l_return_slot=l_owner;
 assign c_lane=lane;assign c_ordinal={1'b0,head_ordinal};
 assign reclaim_fire=rf&&advance;assign reclaim_slot=c_head;
 wire[15:0]ss;
 generate for(genvar b=0;b<8;b=b+1)begin:diagnostic
  assign ss[b*2+:2]=valid[b]?stage[b]:2'b0;
 end endgenerate
 // The removed generic state is derived wires, not diagnostic-only registers.
 assign state={head_ordinal,ss,8'b0,valid,lod_count,c_head,
  (l_valid?l_owner:3'b0),l_valid,derived_count,l_head,
  (d_valid?d_owner:3'b0),d_valid,raw_count,d_head,occupied,c_head,allocate};
 always@(posedge clk)begin
  if(reset)begin
   allocate<=0;d_head<=0;l_head<=0;c_head<=0;d_owner<=0;l_owner<=0;
   occupied<=0;raw_count<=0;derived_count<=0;lod_count<=0;d_valid<=0;l_valid<=0;
   valid<=0;fault<=0;head_ordinal<=0;
   for(i=0;i<8;i=i+1)begin stage[i]<=0;end
  end else if(running)begin
   if(bad)fault<=1;
   else if(commit_allow)begin
    case({admit,done})2'b10:occupied<=occupied+1'b1;2'b01:occupied<=occupied-1'b1;default:occupied<=occupied;endcase
    case({admit,di})2'b10:raw_count<=raw_count+1'b1;2'b01:raw_count<=raw_count-1'b1;default:raw_count<=raw_count;endcase
    case({d_return,li})2'b10:derived_count<=derived_count+1'b1;2'b01:derived_count<=derived_count-1'b1;default:derived_count<=derived_count;endcase
    case({l_return,done})2'b10:lod_count<=lod_count+1'b1;2'b01:lod_count<=lod_count-1'b1;default:lod_count<=lod_count;endcase
    if(di)begin d_owner<=d_head;d_valid<=1;d_head<=next_slot(d_head);end
    if(li)begin l_owner<=l_head;l_valid<=1;l_head<=next_slot(l_head);end
    if(d_return)begin stage[d_owner]<=1;d_valid<=0;end
    if(l_return)begin stage[l_owner]<=2;l_valid<=0;end
    if(done)begin valid[c_head]<=0;c_head<=next_slot(c_head);head_ordinal<=0;end
    else if(ct)head_ordinal<=head_ordinal+1'b1;
    if(admit)begin valid[allocate]<=1;stage[allocate]<=0;allocate<=next_slot(allocate);end
   end
  end
 end
endmodule
