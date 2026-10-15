// Ordered Context8: a last/empty C capture releases that same context.
// Raw metadata/UV row-zero reservation lives in the existing external leaves.
module gpu_v2_texture_runtime_frontend_prepare_shared #(
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
 localparam IDLE=2'd0,D_FLIGHT=2'd1,DERIVED=2'd2,L_FLIGHT=2'd3;
 reg[2:0]allocate,prepare_head,c_head;
 reg[3:0]occupied,raw_count,lod_count;
 reg[1:0]prepare_stage;
 wire d_valid=prepare_stage==D_FLIGHT,l_valid=prepare_stage==L_FLIGHT;
 wire[2:0]d_head=prepare_head+{2'b0,prepare_stage!=IDLE};
 wire[2:0]l_head=prepare_head+{2'b0,l_valid};
 wire[2:0]d_owner=prepare_head,l_owner=prepare_head;
 wire[3:0]derived_count={3'b0,prepare_stage==DERIVED};
 reg[1:0]head_ordinal;
 // Diagnostic classes are derived from the ordered old-state regions.
 // No per-slot class or validity register is retained.
 wire[7:0]valid;
 wire[15:0]ss;
 generate for(genvar row=0;row<8;row=row+1)begin:diagnostic_regions
  wire[2:0]distance=row-c_head;
  assign valid[row]={1'b0,distance}<occupied;
  assign ss[row*2+:2]=({1'b0,distance}<lod_count)?2'd2:
   ((row==prepare_head)&&(prepare_stage==DERIVED||prepare_stage==L_FLIGHT))?2'd1:2'd0;
 end endgenerate
 function[2:0]next_slot;input[2:0]a;begin next_slot=a+1'b1;end endfunction
 // Parallel rank decode removes the procedural priority/popcount chain.
 wire[1:0]prefix2={1'b0,c_mask[0]}+{1'b0,c_mask[1]};
 wire[2:0]prefix3={1'b0,prefix2}+{2'b0,c_mask[2]};
 wire[2:0]covered=prefix3+{2'b0,c_mask[3]};
 wire[3:0]lane_grant;
 assign lane_grant[0]=c_mask[0]&&(head_ordinal==0);
 assign lane_grant[1]=c_mask[1]&&(head_ordinal=={1'b0,c_mask[0]});
 assign lane_grant[2]=c_mask[2]&&(head_ordinal==prefix2);
 assign lane_grant[3]=c_mask[3]&&({1'b0,head_ordinal}==prefix3);
 wire[1:0]lane={lane_grant[2]||lane_grant[3],lane_grant[1]||lane_grant[3]};
 wire lane_ok=|lane_grant;
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
 wire[5:0]tracked={2'b0,raw_count}+{2'b0,lod_count}+{5'b0,prepare_stage!=IDLE};
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
   if(admit&&!input_ready)bad=1;
   if(di&&(d_valid||uv_d_slot!=d_head||(prepare_stage!=IDLE&&!(l_valid&&l_return))))bad=1;
   if(li&&l_valid)bad=1;
   if(d_return&&!d_valid)bad=1;
   if(l_return&&!l_valid)bad=1;
   if(cr&&(c_mask!=0&&!lane_ok))bad=1;
   if(ct&&(uv_c_slot!=c_head||uv_c_lane!=lane))bad=1;
   if(tracked_bad)bad=1;
  end
 end
 assign input_ready=!reset&&!fault&&occupied<8;
 assign reserve_ready=input_ready;
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
 // The removed generic state is derived wires, not diagnostic-only registers.
 assign state={head_ordinal,ss,8'b0,valid,lod_count,c_head,
  (l_valid?l_owner:3'b0),l_valid,derived_count,l_head,
  (d_valid?d_owner:3'b0),d_valid,raw_count,d_head,occupied,c_head,allocate};
 always@(posedge clk)begin
  if(reset)begin
   allocate<=0;prepare_head<=0;c_head<=0;prepare_stage<=IDLE;
   occupied<=0;raw_count<=0;lod_count<=0;
   fault<=0;head_ordinal<=0;
  end else if(running)begin
   if(bad)fault<=1;
   else if(commit_allow)begin
    case({admit,done})2'b10:occupied<=occupied+1'b1;2'b01:occupied<=occupied-1'b1;default:occupied<=occupied;endcase
    case({admit,di})2'b10:raw_count<=raw_count+1'b1;2'b01:raw_count<=raw_count-1'b1;default:raw_count<=raw_count;endcase
    case({l_return,done})2'b10:lod_count<=lod_count+1'b1;2'b01:lod_count<=lod_count-1'b1;default:lod_count<=lod_count;endcase
    if(l_return)begin prepare_head<=next_slot(prepare_head);prepare_stage<=IDLE;end
    if(di)prepare_stage<=D_FLIGHT;
    if(d_return)prepare_stage<=DERIVED;
    if(li)prepare_stage<=L_FLIGHT;
    if(done)begin c_head<=next_slot(c_head);head_ordinal<=0;end
    else if(ct)head_ordinal<=head_ordinal+1'b1;
    if(admit)allocate<=next_slot(allocate);
   end
  end
 end
endmodule
